#![cfg(feature = "simh-ffi")]

use std::env;
use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustair::backend::simh::{
    AltairZ80CpuMode, SimhAltairZ80Backend, SimhLaunchConfig, SimhTarget,
};
use rustair::backend::{BackendSerialPort, MachineBackend};

const STATE_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const PROGRAM_BASE: u16 = 0x0100;
const RX_CAPTURE: u16 = 0x0400;

struct TempSimhConfig {
    path: PathBuf,
}

impl TempSimhConfig {
    fn create() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-serial-{}-{nonce}.ini",
            std::process::id()
        ));

        // FrontPanel master mode requires the primary console itself to be a
        // Telnet/serial console. The M2SIO bridge is separate raw TCP traffic.
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let console_port = listener.local_addr()?.port();
        drop(listener);

        let contents = format!(
            "set cpu 8080\nset cpu 64kb\nset console telnet=buffered\nset console -u telnet={console_port}\n"
        );
        fs::write(&path, contents)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path { &self.path }
}

impl Drop for TempSimhConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn altairz80_executable() -> PathBuf {
    env::var_os("RUSTAIR_SIMH_ALTAIRZ80_EXE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| {
            panic!(
                "RUSTAIR_SIMH_ALTAIRZ80_EXE must point at the x64 Open-SIMH altairz80.exe; run tools/simh/build-simh-x64.ps1 first"
            )
        })
}

fn test_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(IoError::new(ErrorKind::Other, message.into()))
}

fn wait_for_running(
    backend: &mut SimhAltairZ80Backend,
    expected: bool,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + STATE_TIMEOUT;
    loop {
        // External-process backends use this hook to service host-side serial
        // transport while SIMH owns instruction execution.
        backend.service_execution(0)?;
        let state = backend.front_panel_state()?;
        if state.running == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(test_error(format!(
                "timed out waiting for AltairZ80 running={expected}; last state was running={}",
                state.running
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn set_program_counter(
    backend: &mut SimhAltairZ80Backend,
    address: u16,
) -> Result<(), Box<dyn Error>> {
    backend.set_switch_register(address)?;
    backend.panel_examine(false)?;
    Ok(())
}

fn receiver_program(status_port: u8, data_port: u8, destination: u16) -> Vec<u8> {
    let [dest_lo, dest_hi] = destination.to_le_bytes();
    vec![
        0x3e, 0x03,             // MVI A,03h  - ACIA master reset
        0xd3, status_port,      // OUT status
        0x3e, 0x14,             // MVI A,14h  - 8N1, RTS low, TX IRQ disabled
        0xd3, status_port,      // OUT status
        0xdb, status_port,      // 0108: IN status
        0xe6, 0x01,             // ANI 01h    - RDRF
        0xca, 0x08, 0x01,       // JZ 0108h
        0xdb, data_port,        // IN data
        0x32, dest_lo, dest_hi, // STA destination
        0x76,                   // HLT (STOPONHALT is AltairZ80 default)
    ]
}

fn transmitter_program(status_port: u8, data_port: u8, byte: u8) -> Vec<u8> {
    vec![
        0x3e, 0x03,        // MVI A,03h  - ACIA master reset
        0xd3, status_port, // OUT status
        0x3e, 0x14,        // MVI A,14h  - 8N1, RTS low, TX IRQ disabled
        0xd3, status_port, // OUT status
        0xdb, status_port, // 0108: IN status
        0xe6, 0x02,        // ANI 02h    - TDRE
        0xca, 0x08, 0x01,  // JZ 0108h
        0x3e, byte,        // MVI A,byte
        0xd3, data_port,   // OUT data
        0x76,              // HLT
    ]
}

fn exercise_host_to_guest(
    backend: &mut SimhAltairZ80Backend,
    logical_port: BackendSerialPort,
    status_port: u8,
    data_port: u8,
    byte: u8,
) -> Result<(), Box<dyn Error>> {
    backend.write_memory(RX_CAPTURE, 0x00, false)?;
    let program = receiver_program(status_port, data_port, RX_CAPTURE);
    backend.load_bytes(PROGRAM_BASE, &program)?;
    set_program_counter(backend, PROGRAM_BASE)?;

    backend.run()?;
    // The receive program deliberately waits for RDRF, so seeing RUN here is
    // meaningful and ensures the guest is ready before the host injects data.
    wait_for_running(backend, true)?;
    backend.serial_receive(logical_port, byte)?;
    wait_for_running(backend, false)?;

    assert_eq!(
        backend.peek_memory(RX_CAPTURE)?,
        Some(byte),
        "guest did not receive byte {byte:02X} through {logical_port:?}"
    );
    Ok(())
}

fn exercise_guest_to_host(
    backend: &mut SimhAltairZ80Backend,
    logical_port: BackendSerialPort,
    status_port: u8,
    data_port: u8,
    byte: u8,
) -> Result<(), Box<dyn Error>> {
    backend.clear_serial()?;
    let program = transmitter_program(status_port, data_port, byte);
    backend.load_bytes(PROGRAM_BASE, &program)?;
    set_program_counter(backend, PROGRAM_BASE)?;

    backend.run()?;
    // This program can complete before the caller observes an intermediate RUN
    // state, so only require the architecturally relevant final HALT.
    wait_for_running(backend, false)?;

    let deadline = Instant::now() + STATE_TIMEOUT;
    loop {
        if let Some(received) = backend.serial_tx_complete(logical_port)? {
            assert_eq!(
                received, byte,
                "host received wrong byte through {logical_port:?}"
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(test_error(format!(
                "timed out waiting for guest TX byte {byte:02X} through {logical_port:?}"
            )));
        }
        backend.service_execution(0)?;
        thread::sleep(POLL_INTERVAL);
    }
}

#[test]
#[ignore = "requires the local x64 Open-SIMH stack built by tools/simh/build-simh-x64.ps1"]
fn altairz80_m2sio_raw_tcp_round_trip() -> Result<(), Box<dyn Error>> {
    let executable = altairz80_executable();
    let config = TempSimhConfig::create()?;
    let launch = SimhLaunchConfig::new(SimhTarget::AltairZ80, executable, config.path());
    let mut backend = SimhAltairZ80Backend::launch_with_serial(
        launch,
        AltairZ80CpuMode::Intel8080,
    )?;

    assert!(backend.capabilities().serial_routing);
    assert!(backend.serial_connected(BackendSerialPort::Port0));
    assert!(backend.serial_connected(BackendSerialPort::Port1));
    wait_for_running(&mut backend, false)?;

    // M2SIO0: status 10h, data 11h.
    exercise_host_to_guest(
        &mut backend,
        BackendSerialPort::Port0,
        0x10,
        0x11,
        0x41,
    )?;
    exercise_guest_to_host(
        &mut backend,
        BackendSerialPort::Port0,
        0x10,
        0x11,
        0x51,
    )?;

    // M2SIO1: status 12h, data 13h.
    exercise_host_to_guest(
        &mut backend,
        BackendSerialPort::Port1,
        0x12,
        0x13,
        0x42,
    )?;
    exercise_guest_to_host(
        &mut backend,
        BackendSerialPort::Port1,
        0x12,
        0x13,
        0x52,
    )?;

    // The private listeners and temporary overlay survive a simulator process
    // power-cycle; the new altairz80.exe must reconnect both M2SIO channels.
    backend.power(false)?;
    backend.power(true)?;
    assert!(backend.serial_connected(BackendSerialPort::Port0));
    assert!(backend.serial_connected(BackendSerialPort::Port1));
    wait_for_running(&mut backend, false)?;

    println!("RusTair -> raw TCP -> Open-SIMH M2SIO0/M2SIO1 serial smoke test passed");
    Ok(())
}
