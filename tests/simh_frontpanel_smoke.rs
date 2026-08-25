#![cfg(feature = "simh-ffi")]

use std::env;
use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustair::backend::simh::{SimhAltairBackend, SimhLaunchConfig, SimhTarget};
use rustair::backend::{CpuState, MachineBackend};

const STATE_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct TempSimhConfig {
    path: PathBuf,
}

impl TempSimhConfig {
    fn create() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-altair-smoke-{}-{nonce}.ini",
            std::process::id()
        ));

        // FrontPanel appends SET REMOTE ... / SET REMOTE MASTER to this file.
        // Open-SIMH only permits Master Remote Console mode when the simulator's
        // primary console is itself Telnet or Serial. Use a buffered Telnet
        // console on a locally selected free port; no terminal client needs to
        // connect for CPU execution to proceed.
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let console_port = listener.local_addr()?.port();
        drop(listener);

        // Do not depend on Open-SIMH's target defaults here. The backend contract
        // being exercised is explicitly the classic 8080 Altair with writable
        // RAM, so select the CPU mode and a full 64 KiB address space before
        // FrontPanel takes control of execution.
        let contents = format!(
            "set cpu 8080\nset cpu 64k\nset console telnet=buffered\nset console -u telnet={console_port}\n"
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

fn altair_executable() -> PathBuf {
    env::var_os("RUSTAIR_SIMH_ALTAIR_EXE")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| {
            panic!(
                "RUSTAIR_SIMH_ALTAIR_EXE must point at the x64 Open-SIMH altair.exe; run tools/simh/build-simh-x64.ps1 first"
            )
        })
}

fn test_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(IoError::new(ErrorKind::Other, message.into()))
}

fn intel8080_pc(backend: &mut SimhAltairBackend) -> Result<u16, Box<dyn Error>> {
    match backend.cpu_state()? {
        CpuState::Intel8080(state) => Ok(state.pc),
        CpuState::Z80(_) => Err(test_error(
            "classic Open-SIMH Altair unexpectedly reported Z80 state",
        )),
    }
}

fn wait_for_running(
    backend: &mut SimhAltairBackend,
    expected: bool,
) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + STATE_TIMEOUT;
    loop {
        let state = backend.front_panel_state()?;
        if state.running == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(test_error(format!(
                "timed out waiting for SIMH running={expected}; last state was running={}",
                state.running
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[test]
#[ignore = "requires the local x64 Open-SIMH stack built by tools/simh/build-simh-x64.ps1"]
fn classic_altair_frontpanel_round_trip() -> Result<(), Box<dyn Error>> {
    let executable = altair_executable();
    let config = TempSimhConfig::create()?;
    let launch = SimhLaunchConfig::new(SimhTarget::Altair, executable, config.path());
    let mut backend = SimhAltairBackend::launch(launch)?;

    wait_for_running(&mut backend, false)?;

    // Memory examine/deposit through the real FrontPanel connection.
    assert!(backend.write_memory(0x0200, 0xa5, false)?);
    assert_eq!(backend.peek_memory(0x0200)?, Some(0xa5));

    // Front-panel switch register + EXAMINE must update SIMH's PC.
    backend.write_memory(0x0100, 0x00, false)?; // NOP
    backend.set_switch_register(0x0100)?;
    assert_eq!(backend.switch_register()?, 0x0100);
    backend.panel_examine(false)?;
    assert_eq!(intel8080_pc(&mut backend)?, 0x0100);

    // One FrontPanel STEP executes exactly the NOP and returns to Halt.
    backend.step()?;
    wait_for_running(&mut backend, false)?;
    assert_eq!(intel8080_pc(&mut backend)?, 0x0101);

    // RUN/HALT against an intentional infinite loop. This proves that RusTair
    // can transition a live external SIMH process in both directions.
    backend.load_bytes(0x0300, &[0xc3, 0x00, 0x03])?; // JMP 0300h
    backend.set_switch_register(0x0300)?;
    backend.panel_examine(false)?;
    assert_eq!(intel8080_pc(&mut backend)?, 0x0300);
    backend.run()?;
    wait_for_running(&mut backend, true)?;
    backend.halt()?;
    wait_for_running(&mut backend, false)?;

    // Exercise the backend's real process lifecycle. power(false) drops the
    // FrontPanel session/process; power(true) must create a fresh usable one.
    backend.power(false)?;
    backend.power(true)?;
    wait_for_running(&mut backend, false)?;
    let _ = backend.cpu_state()?;

    println!("RusTair -> FrontPanel -> Open-SIMH classic Altair smoke test passed");
    Ok(())
}
