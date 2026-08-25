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
    SimhAltairBackend, SimhLaunchConfig, SimhOperationalState, SimhSession, SimhTarget,
};
use rustair::backend::{CpuState, MachineBackend};

const STATE_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct TempSimhConfig {
    path: PathBuf,
    debug_path: PathBuf,
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
        let debug_path = path.with_extension("frontpanel.log");

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let console_port = listener.local_addr()?.port();
        drop(listener);

        let contents = format!(
            "set cpu 8080\nset cpu 64k\nset console telnet=buffered\nset console -u telnet={console_port}\n"
        );
        fs::write(&path, contents)?;
        Ok(Self { path, debug_path })
    }

    fn path(&self) -> &Path { &self.path }
    fn debug_path(&self) -> &Path { &self.debug_path }
}

impl Drop for TempSimhConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&self.debug_path);
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

fn diagnose_frontpanel_memory(
    executable: &Path,
    config: &TempSimhConfig,
) -> Result<(), Box<dyn Error>> {
    let mut session = SimhSession::start_debug(executable, config.path(), 0, config.debug_path())?;
    if session.state() != SimhOperationalState::Halted {
        session.halt()?;
    }

    session.deposit_u32("A", 0x5a)?;
    let after_register_api = session.examine_u32("A")? as u8;

    let before = session.read_byte(0x0200)?;
    session.write_byte(0x0200, 0xa5)?;
    let after_mem_api = session.read_byte(0x0200)?;

    session.deposit_u32("1001", 0x5a)?;
    let after_generic_api = session.examine_u32("1001")? as u8;

    println!(
        "FrontPanel diagnostic: register_A={after_register_api:02X}, before={before:02X}, mem_api_after={after_mem_api:02X}, generic_mem_after={after_generic_api:02X}"
    );

    drop(session);

    if after_register_api != 0x5a || after_mem_api != 0xa5 || after_generic_api != 0x5a {
        let debug = match fs::read(config.debug_path()) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => format!("<unable to read FrontPanel debug log: {error}>"),
        };
        println!("\n===== Open-SIMH FrontPanel wire log =====\n{debug}\n===== end FrontPanel wire log =====\n");
        return Err(test_error(format!(
            "FrontPanel round-trip mismatch: register A wrote 5A/read {after_register_api:02X}; mem API wrote A5/read {after_mem_api:02X}; generic memory wrote 5A/read {after_generic_api:02X}"
        )));
    }

    Ok(())
}

#[test]
#[ignore = "requires the local x64 Open-SIMH stack built by tools/simh/build-simh-x64.ps1"]
fn classic_altair_frontpanel_round_trip() -> Result<(), Box<dyn Error>> {
    let executable = altair_executable();
    let config = TempSimhConfig::create()?;

    diagnose_frontpanel_memory(&executable, &config)?;

    let launch = SimhLaunchConfig::new(SimhTarget::Altair, executable.clone(), config.path());
    let mut backend = SimhAltairBackend::launch(launch)?;

    wait_for_running(&mut backend, false)?;

    assert!(backend.write_memory(0x0200, 0xa5, false)?);
    assert_eq!(backend.peek_memory(0x0200)?, Some(0xa5));

    backend.write_memory(0x0100, 0x00, false)?; // NOP
    backend.set_switch_register(0x0100)?;
    assert_eq!(backend.switch_register()?, 0x0100);
    backend.panel_examine(false)?;
    assert_eq!(intel8080_pc(&mut backend)?, 0x0100);

    backend.step()?;
    wait_for_running(&mut backend, false)?;
    assert_eq!(intel8080_pc(&mut backend)?, 0x0101);

    backend.load_bytes(0x0300, &[0xc3, 0x00, 0x03])?; // JMP 0300h
    backend.set_switch_register(0x0300)?;
    backend.panel_examine(false)?;
    assert_eq!(intel8080_pc(&mut backend)?, 0x0300);
    backend.run()?;
    wait_for_running(&mut backend, true)?;
    backend.halt()?;
    wait_for_running(&mut backend, false)?;

    backend.power(false)?;
    backend.power(true)?;
    wait_for_running(&mut backend, false)?;
    let _ = backend.cpu_state()?;

    println!("RusTair -> FrontPanel -> Open-SIMH classic Altair smoke test passed");
    Ok(())
}
