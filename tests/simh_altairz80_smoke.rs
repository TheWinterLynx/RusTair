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
use rustair::backend::{CpuState, MachineBackend};

const STATE_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct TempSimhConfig {
    path: PathBuf,
}

impl TempSimhConfig {
    fn create(mode: AltairZ80CpuMode) -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir().join(format!(
            "rustair-simh-altairz80-{}-{}-{nonce}.ini",
            match mode {
                AltairZ80CpuMode::Intel8080 => "8080",
                AltairZ80CpuMode::Z80 => "z80",
            },
            std::process::id()
        ));

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let console_port = listener.local_addr()?.port();
        drop(listener);

        let contents = format!(
            "set cpu {}\nset cpu 64kb\nset console telnet=buffered\nset console -u telnet={console_port}\n",
            mode.simh_modifier()
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

fn pc(
    backend: &mut SimhAltairZ80Backend,
    expected_mode: AltairZ80CpuMode,
) -> Result<u16, Box<dyn Error>> {
    match (expected_mode, backend.cpu_state()?) {
        (AltairZ80CpuMode::Intel8080, CpuState::Intel8080(state)) => Ok(state.pc),
        (AltairZ80CpuMode::Z80, CpuState::Z80(state)) => Ok(state.pc),
        (AltairZ80CpuMode::Intel8080, CpuState::Z80(_)) => {
            Err(test_error("AltairZ80 8080 mode returned a Z80 CpuState"))
        }
        (AltairZ80CpuMode::Z80, CpuState::Intel8080(_)) => {
            Err(test_error("AltairZ80 Z80 mode returned an Intel8080 CpuState"))
        }
    }
}

fn run_mode(mode: AltairZ80CpuMode) -> Result<(), Box<dyn Error>> {
    let executable = altairz80_executable();
    let config = TempSimhConfig::create(mode)?;
    let launch = SimhLaunchConfig::new(SimhTarget::AltairZ80, executable, config.path());
    let mut backend = SimhAltairZ80Backend::launch(launch, mode)?;

    wait_for_running(&mut backend, false)?;
    let _ = pc(&mut backend, mode)?;

    assert!(backend.write_memory(0x0200, 0xa5, false)?);
    assert_eq!(backend.peek_memory(0x0200)?, Some(0xa5));

    // AltairZ80 only exposes SR as 8 bits. The backend must still retain the
    // physical 16-bit RusTair switch register locally.
    backend.set_switch_register(0xabcd)?;
    assert_eq!(backend.switch_register()?, 0xabcd);

    // STEP is validated by architecturally visible state. Open-SIMH's raw
    // TSTATES pseudo-register is per sim_instr() invocation rather than a
    // cumulative counter, and on the pinned a1f57fa3 build a FrontPanel STEP
    // reports zero there despite advancing the PC. It therefore must not be
    // used as the backend-neutral total_t_states contract.
    backend.write_memory(0x0100, 0x00, false)?; // NOP in both 8080 and Z80
    backend.set_switch_register(0x0100)?;
    backend.panel_examine(false)?;
    assert_eq!(pc(&mut backend, mode)?, 0x0100);

    backend.step()?;
    wait_for_running(&mut backend, false)?;
    assert_eq!(pc(&mut backend, mode)?, 0x0101);

    // JP/JMP 0300h is C3 00 03 in both personalities and gives us a stable
    // running loop until FrontPanel HALT is requested.
    backend.load_bytes(0x0300, &[0xc3, 0x00, 0x03])?;
    backend.set_switch_register(0x0300)?;
    backend.panel_examine(false)?;
    assert_eq!(pc(&mut backend, mode)?, 0x0300);

    backend.run()?;
    wait_for_running(&mut backend, true)?;
    backend.halt()?;
    wait_for_running(&mut backend, false)?;
    assert_eq!(pc(&mut backend, mode)?, 0x0300);

    backend.power(false)?;
    backend.power(true)?;
    wait_for_running(&mut backend, false)?;
    let _ = pc(&mut backend, mode)?;

    println!("RusTair -> FrontPanel -> Open-SIMH AltairZ80 {mode:?} smoke test passed");
    Ok(())
}

#[test]
#[ignore = "requires the local x64 Open-SIMH stack built by tools/simh/build-simh-x64.ps1"]
fn altairz80_frontpanel_round_trip_in_z80_and_8080_modes() -> Result<(), Box<dyn Error>> {
    run_mode(AltairZ80CpuMode::Z80)?;
    run_mode(AltairZ80CpuMode::Intel8080)?;
    Ok(())
}
