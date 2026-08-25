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

const TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const PROGRAM_BASE: u16 = 0x0100;
const RX_CAPTURE: u16 = 0x0400;

struct TempConfig {
    path: PathBuf,
    simh_debug_path: PathBuf,
}

impl TempConfig {
    fn create(port0: u16, port1: u16) -> Result<Self, Box<dyn Error>> {
        // FrontPanel uses Open-SIMH Remote Console in MASTER mode. Open-SIMH
        // explicitly requires the simulator console to be Telnet or Serial in
        // that mode, even though the FrontPanel command channel itself is
        // REM-CON. Keep a private buffered listener for that requirement.
        let console_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let console_port = console_listener.local_addr()?.port();
        drop(console_listener);

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-m2sio-diagnostic-{}-{nonce}.ini",
            std::process::id()
        ));
        let simh_debug_path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-m2sio-simh-{}-{nonce}.log",
            std::process::id()
        ));
        // SET DEBUG is parsed with get_glyph_nc(), which does not strip quotes.
        // Use an unquoted, slash-normalized path so Windows opens the intended file.
        let simh_debug_name = simh_debug_path.to_string_lossy().replace('\\', "/");

        let contents = format!(
            "set debug -n -a -p {simh_debug_name}\n\
set cpu 8080\n\
set cpu 64kb\n\
set console telnet=buffered\n\
set console -u telnet={console_port}\n\
set m2sio0 enabled\n\
set m2sio1 enabled\n\
set m2sio0 debug=STATUS;VERBOSE;ERROR\n\
set m2sio1 debug=STATUS;VERBOSE;ERROR\n\
set m2sio0 noconsole\n\
set m2sio1 noconsole\n\
set m2sio0 dtr\n\
set m2sio1 dtr\n\
set m2sio0 dcd\n\
set m2sio0 cts\n\
set m2sio1 dcd\n\
set m2sio1 cts\n\
attach m2sio0 Connect=127.0.0.1:{port0};notelnet\n\
attach m2sio1 Connect=127.0.0.1:{port1};notelnet\n\
reset m2sio0\n\
reset m2sio1\n"
        );
        fs::write(&path, contents)?;
        Ok(Self {
            path,
            simh_debug_path,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn simh_debug_path(&self) -> &Path {
        &self.simh_debug_path
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&self.simh_debug_path);
    }
}

struct TempDebugLog {
    path: PathBuf,
}

impl TempDebugLog {
    fn create() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-m2sio-frontpanel-{}-{nonce}.log",
            std::process::id()
        ));
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDebugLog {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
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

fn accept_one(listener: &TcpListener, label: &str) -> Result<TcpStream, Box<dyn Error>> {
    let deadline = Instant::now() + TIMEOUT;
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

fn receiver_program() -> Vec<u8> {
    vec![
        0x3e, 0x03,       // 0100: MVI A,03h - ACIA reset
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

fn focused_simh_debug(log: &str) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.contains("continue_cmd executing"))
        .unwrap_or_else(|| lines.len().saturating_sub(120));
    let stop = lines[start..]
        .iter()
        .position(|line| line.contains("Master Session Returned: Status -"))
        .map(|relative| start + relative + 1)
        .unwrap_or(lines.len());

    let focused: Vec<&str> = lines[start..stop]
        .iter()
        .copied()
        .filter(|line| {
            line.contains("SCP-PROCESS EVENT")
                || line.contains("RUSTAIR STOP TRACE")
                || line.contains("REM-CON CMD: continue_cmd")
                || line.contains("REM-CON MODE:")
                || line.contains("CON-TELNET")
                || line.contains("M2SIO0 STATUS")
                || line.contains("M2SIO1 STATUS")
                || line.contains("Simulation stopped")
                || line.contains("Simulator Running")
                || line.contains("stop_cpu")
        })
        .collect();

    if focused.is_empty() {
        lines[start..stop].join("\n")
    } else {
        focused.join("\n")
    }
}

fn focused_frontpanel_wire(log: &str) -> String {
    log.lines()
        .filter(|line| {
            line.contains("CONT")
                || line.contains("Simulator Running")
                || line.contains("Simulation stopped")
                || line.contains("Status - 77")
                || line.contains("State transitioning")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "direct diagnostic requiring the local x64 Open-SIMH stack"]
fn direct_open_simh_m2sio_receive_probe() -> Result<(), Box<dyn Error>> {
    let listener0 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let listener1 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener0.set_nonblocking(true)?;
    listener1.set_nonblocking(true)?;
    let port0 = listener0.local_addr()?.port();
    let port1 = listener1.local_addr()?.port();

    let config = TempConfig::create(port0, port1)?;
    let debug = TempDebugLog::create()?;
    let mut session = SimhSession::start_debug(&simulator(), config.path(), 0, debug.path())?;

    // REM-CON is an internal device registered by the FrontPanel-appended
    // `set remote ...` configuration, so enable its tracing only after startup.
    session.set_device_debug_mode("REM-CON", true, "")?;

    // Open-SIMH's own sim_process_event() already traces the exact UNIT it is
    // about to dispatch through SCP-PROCESS/EVENT. This is more authoritative
    // than trying to infer the culprit from CPU state after SCPE_STOP: the
    // scheduler can return SCPE_STOP either from a UNIT service or from the
    // global stop_cpu flag before/after dispatch.
    session.set_device_debug_mode("SCP-PROCESS", true, "EVENT")?;

    // MASTER mode requires a Telnet/Serial simulator console. Trace that
    // otherwise-unused private listener as well, since it is the next queued
    // unit immediately before the unexplained SCPE_STOP in the failing probe.
    session.set_device_debug_mode("CON-TELNET", true, "")?;

    // ATTACH validates each Connect= destination with one disposable TCP
    // connection. Consume those explicitly before starting guest execution.
    drop(accept_one(&listener0, "M2SIO0 validation connection")?);
    drop(accept_one(&listener1, "M2SIO1 validation connection")?);

    session.write_byte(RX_CAPTURE, 0xee)?;
    session.load_bytes(PROGRAM_BASE, &receiver_program())?;
    session.deposit_register_u32("PC", u32::from(PROGRAM_BASE))?;
    session.run()?;
    let state_after_run = session.state();

    // The guest writes 14h to M2SIO0, raising RTS/DTR. m2sio_svc then asks
    // TMXR to establish the persistent outgoing raw TCP connection.
    let mut persistent0 = accept_one(&listener0, "M2SIO0 persistent connection")?;
    let state_after_persistent_accept = session.state();
    persistent0.write_all(&[0x41])?;
    persistent0.flush()?;
    let state_after_write = session.state();

    let deadline = Instant::now() + TIMEOUT;
    while session.state() == SimhOperationalState::Running && Instant::now() < deadline {
        thread::sleep(POLL_INTERVAL);
    }

    let final_state_before_forced_halt = session.state();
    let stopped_naturally = final_state_before_forced_halt == SimhOperationalState::Halted;
    if !stopped_naturally {
        session.halt()?;
    }

    let pc = session.examine_register_u32("PC")?;
    let captured = session.read_byte(RX_CAPTURE)?;
    let halt = session.halt_text();

    println!(
        "DIRECT M2SIO diagnostic: state_after_run={state_after_run:?}, state_after_persistent_accept={state_after_persistent_accept:?}, state_after_write={state_after_write:?}, final_state_before_forced_halt={final_state_before_forced_halt:?}, stopped_naturally={stopped_naturally}, halt_text={halt:?}, pc={pc:04X}, captured={captured:02X}, M2STA0={}, M2CTL0={}, M2CON0={}, M2RTS0={}, M2DCD0={}, M2CTS0={}, M2RDRF0={}, M2RXD0={}, M2WAIT0={}",
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

    if captured != 0x41 {
        drop(session);
        thread::sleep(Duration::from_millis(100));

        let simh_debug = match fs::read(config.simh_debug_path()) {
            Ok(bytes) => focused_simh_debug(&String::from_utf8_lossy(&bytes)),
            Err(error) => format!("<unable to read Open-SIMH debug log: {error}>"),
        };
        eprintln!(
            "--- focused Open-SIMH diagnostic ---\n{simh_debug}\n--- end focused Open-SIMH diagnostic ---"
        );

        let wire = match fs::read(debug.path()) {
            Ok(bytes) => focused_frontpanel_wire(&String::from_utf8_lossy(&bytes)),
            Err(error) => format!("<unable to read FrontPanel wire log: {error}>"),
        };
        eprintln!(
            "--- focused FrontPanel wire ---\n{wire}\n--- end focused FrontPanel wire ---"
        );
        return Err(test_error(format!(
            "direct SIMH M2SIO receive path did not deliver 41h (captured {captured:02X})"
        )));
    }

    Ok(())
}
