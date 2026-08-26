#![cfg(feature = "simh-ffi")]

use std::env;
use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustair::backend::simh::{SimhOperationalState, SimhSession};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RX_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const CONNECTION_SETTLE: Duration = Duration::from_millis(150);
const PROGRAM_BASE: u16 = 0x0100;
const RX_CAPTURE: u16 = 0x0400;

struct TempConfig {
    path: PathBuf,
    simh_debug_path: PathBuf,
}

impl TempConfig {
    fn create(port0: u16) -> Result<Self, Box<dyn Error>> {
        // FrontPanel MASTER mode requires the simulator console itself to be
        // Telnet/Serial. Keep that unrelated console on a private port.
        let console_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let console_port = console_listener.local_addr()?.port();
        drop(console_listener);

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-m2sio-rx-focused-{}-{nonce}.ini",
            std::process::id()
        ));
        let simh_debug_path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-m2sio-rx-focused-{}-{nonce}.log",
            std::process::id()
        ));
        let simh_debug_name = simh_debug_path.to_string_lossy().replace('\\', "/");

        // Deliberately configure only M2SIO0.  Port 1 is irrelevant to this
        // receive probe and removing it also removes a second TMXR connection
        // lifecycle from the diagnosis.
        let contents = format!(
            "set debug -n -a -p {simh_debug_name}\n\
set cpu 8080\n\
set cpu 64kb\n\
set console telnet=buffered\n\
set console -u telnet={console_port}\n\
set m2sio0 enabled\n\
set m2sio0 debug=STATUS;VERBOSE;ERROR\n\
set m2sio0 noconsole\n\
set m2sio0 dtr\n\
set m2sio0 dcd\n\
set m2sio0 cts\n\
attach m2sio0 Connect=127.0.0.1:{port0};notelnet\n\
reset m2sio0\n"
        );
        fs::write(&path, contents)?;
        Ok(Self {
            path,
            simh_debug_path,
        })
    }

    fn path(&self) -> &Path { &self.path }
    fn simh_debug_path(&self) -> &Path { &self.simh_debug_path }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&self.simh_debug_path);
    }
}

fn simulator() -> PathBuf {
    env::var_os("RUSTAIR_SIMH_ALTAIRZ80_EXE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| panic!("RUSTAIR_SIMH_ALTAIRZ80_EXE must point at altairz80.exe"))
}

fn test_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(IoError::new(ErrorKind::Other, message.into()))
}

fn accept_once(listener: &TcpListener, label: &str) -> Result<TcpStream, Box<dyn Error>> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nodelay(true)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(test_error(format!("timed out accepting {label}")));
                }
                thread::sleep(POLL_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Accept the post-RUN connection but reject any additional short-lived TMXR
/// probe socket.  A real outgoing M2SIO connection remains open for at least
/// CONNECTION_SETTLE while an ATTACH validation socket closes promptly.
fn accept_stable_connection(
    listener: &TcpListener,
    label: &str,
) -> Result<(TcpStream, usize), Box<dyn Error>> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut rejected = 0usize;

    while Instant::now() < deadline {
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(POLL_INTERVAL);
                continue;
            }
            Err(error) => return Err(error.into()),
        };

        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let settle_deadline = Instant::now() + CONNECTION_SETTLE;
        let mut closed = false;
        let mut probe = [0u8; 1];

        while Instant::now() < settle_deadline {
            match stream.peek(&mut probe) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(_) => break,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(POLL_INTERVAL);
                }
                Err(_) => {
                    closed = true;
                    break;
                }
            }
        }

        if closed {
            rejected += 1;
            continue;
        }

        stream.set_nonblocking(false)?;
        return Ok((stream, rejected));
    }

    Err(test_error(format!("timed out accepting stable {label}")))
}

fn receiver_program() -> Vec<u8> {
    vec![
        0x3e, 0x03,       // 0100: MVI A,03h - ACIA master reset
        0xd3, 0x10,       // 0102: OUT 10h
        0x3e, 0x14,       // 0104: MVI A,14h - 8N1, RTS active
        0xd3, 0x10,       // 0106: OUT 10h
        0xdb, 0x10,       // 0108: IN 10h
        0xe6, 0x01,       // 010A: ANI 01h - RDRF
        0xca, 0x08, 0x01, // 010C: JZ 0108h
        0xdb, 0x11,       // 010F: IN 11h
        0x32, 0x00, 0x04, // 0111: STA 0400h
        0x76,             // 0114: HLT
    ]
}

fn examine(session: &SimhSession, name: &str) -> String {
    match session.examine_u32(name) {
        Ok(value) => format!("{value:02X}"),
        Err(error) => format!("<error: {error}>"),
    }
}

#[test]
#[ignore = "focused diagnostic requiring the local x64 Open-SIMH stack"]
fn focused_open_simh_m2sio_receive_probe() -> Result<(), Box<dyn Error>> {
    let listener0 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener0.set_nonblocking(true)?;
    let port0 = listener0.local_addr()?.port();

    let config = TempConfig::create(port0)?;
    let mut session = SimhSession::start(&simulator(), config.path(), 0)?;

    // ATTACH performs a disposable destination-validation connection before
    // guest execution. Consume it, but do not assume it is the only short-lived
    // connection TMXR may create.
    drop(accept_once(&listener0, "M2SIO0 ATTACH validation connection")?);

    session.write_byte(RX_CAPTURE, 0xee)?;
    session.load_bytes(PROGRAM_BASE, &receiver_program())?;
    session.deposit_register_u32("PC", u32::from(PROGRAM_BASE))?;
    session.run()?;

    let (mut persistent0, rejected_after_run) =
        accept_stable_connection(&listener0, "M2SIO0 persistent connection")?;
    persistent0.write_all(&[0x41])?;
    persistent0.flush()?;

    let deadline = Instant::now() + RX_TIMEOUT;
    while session.state() == SimhOperationalState::Running && Instant::now() < deadline {
        thread::sleep(POLL_INTERVAL);
    }

    let stopped_naturally = session.state() == SimhOperationalState::Halted;
    if !stopped_naturally {
        session.halt()?;
    }

    let pc = session.examine_register_u32("PC")?;
    let captured = session.read_byte(RX_CAPTURE)?;
    println!(
        "FOCUSED M2SIO RX: rejected_short_lived_after_run={rejected_after_run}, stopped_naturally={stopped_naturally}, pc={pc:04X}, captured={captured:02X}, M2STA0={}, M2CTL0={}, M2CON0={}, M2RTS0={}, M2DCD0={}, M2CTS0={}, M2RDRF0={}, M2RXD0={}, M2WAIT0={}",
        examine(&session, "M2STA0"),
        examine(&session, "M2CTL0"),
        examine(&session, "M2CON0"),
        examine(&session, "M2RTS0"),
        examine(&session, "M2DCD0"),
        examine(&session, "M2CTS0"),
        examine(&session, "M2RDRF0"),
        examine(&session, "M2RXD0"),
        examine(&session, "M2WAIT0"),
    );

    drop(session);
    thread::sleep(Duration::from_millis(100));

    let log = fs::read(config.simh_debug_path())?;
    let text = String::from_utf8_lossy(&log);
    let focused: Vec<&str> = text
        .lines()
        .filter(|line| {
            line.contains("RUSTAIR RX TRACE")
                || (line.contains("M2SIO0 STATUS")
                    && (line.contains("new connection")
                        || line.contains("lost connection")
                        || line.contains("RTS state changed")
                        || line.contains("DCD state changed")
                        || line.contains("CTS state changed")))
        })
        .collect();

    println!("--- focused M2SIO/TMXR RX trace ---");
    if focused.is_empty() {
        println!("<no focused M2SIO RX trace lines were emitted>");
    } else {
        for line in focused {
            println!("{line}");
        }
    }
    println!("--- end focused M2SIO/TMXR RX trace ---");

    if captured != 0x41 {
        return Err(test_error(format!(
            "focused M2SIO receive path did not deliver 41h (captured {captured:02X})"
        )));
    }

    Ok(())
}
