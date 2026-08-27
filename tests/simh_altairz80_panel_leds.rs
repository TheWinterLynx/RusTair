#![cfg(feature = "simh-ffi")]

use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::thread;
use std::time::{Duration, Instant};

use rustair::backend::simh::{SimhThreadedBackend, active_console_snapshot};
use rustair::backend::{CpuState, EmulationEngine, FrontPanelState, MachineBackend};

const WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const DATA_PROBE_ADDRESS: u16 = 0x0200;

fn test_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(IoError::new(ErrorKind::Other, message.into()))
}

fn bits16(value: u16) -> String {
    (0..16)
        .rev()
        .map(|bit| if value & (1u16 << bit) != 0 { '1' } else { '0' })
        .collect()
}

fn bits8(value: u8) -> String {
    (0..8)
        .rev()
        .map(|bit| if value & (1u8 << bit) != 0 { '1' } else { '0' })
        .collect()
}

fn assert_binary_lamps<const N: usize>(label: &str, lamps: &[f32; N], expected: u64) {
    for (bit, intensity) in lamps.iter().copied().enumerate() {
        let should_be_on = expected & (1u64 << bit) != 0;
        if should_be_on {
            assert!(
                intensity >= 0.99,
                "{label} bit {bit} should be ON, got intensity {intensity}"
            );
        } else {
            assert!(
                intensity <= 0.01,
                "{label} bit {bit} should be OFF, got intensity {intensity}"
            );
        }
    }
}

fn assert_status_lamps_dark(panel: &FrontPanelState) {
    let lamps = panel.lamps;
    for (name, value) in [
        ("INTE", lamps.inte),
        ("PROT", lamps.prot),
        ("MEMR", lamps.memr),
        ("INP", lamps.inp),
        ("M1", lamps.m1),
        ("OUT", lamps.out),
        ("HLTA", lamps.hlta),
        ("STACK", lamps.stack),
        ("WO", lamps.wo),
        ("INT", lamps.int_ack),
        ("WAIT", lamps.wait),
        ("HLDA", lamps.hlda),
    ] {
        assert!(
            value <= 0.01,
            "SIMH status lamp {name} must remain dark until a real S-100 bus feed exists; got {value}"
        );
    }
}

fn worker_error() -> Option<String> {
    active_console_snapshot().and_then(|snapshot| snapshot.last_error)
}

fn worker_tail() -> String {
    let Some(snapshot) = active_console_snapshot() else {
        return "<no active worker console>".into();
    };
    let start = snapshot.lines.len().saturating_sub(16);
    snapshot.lines[start..].join("\n")
}

fn wait_for(
    backend: &mut SimhThreadedBackend,
    description: &str,
    mut predicate: impl FnMut(&FrontPanelState) -> bool,
) -> Result<FrontPanelState, Box<dyn Error>> {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        if let Some(error) = worker_error() {
            return Err(test_error(format!(
                "worker failed while waiting for {description}: {error}\n--- worker tail ---\n{}",
                worker_tail()
            )));
        }

        let panel = backend.front_panel_state()?;
        if predicate(&panel) {
            return Ok(panel);
        }
        if Instant::now() >= deadline {
            let worker = active_console_snapshot();
            return Err(test_error(format!(
                "timed out waiting for {description}; last panel: powered={} running={} address={:04X} data={:02X}; worker busy={}\n--- worker tail ---\n{}",
                panel.powered,
                panel.running,
                panel.address,
                panel.data,
                worker.as_ref().is_some_and(|snapshot| snapshot.busy),
                worker_tail()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_power_ready(backend: &mut SimhThreadedBackend) -> Result<FrontPanelState, Box<dyn Error>> {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        if let Some(error) = worker_error() {
            return Err(test_error(format!(
                "SIMH POWER ON failed: {error}\n--- worker tail ---\n{}",
                worker_tail()
            )));
        }

        let panel = backend.front_panel_state()?;
        let worker = active_console_snapshot();
        let ready = worker.as_ref().is_some_and(|snapshot| {
            !snapshot.busy
                && snapshot.powered
                && snapshot.lines.iter().any(|line| line.starts_with("POWER ON complete"))
        });
        if ready && panel.powered && !panel.running {
            return Ok(panel);
        }

        if Instant::now() >= deadline {
            return Err(test_error(format!(
                "timed out waiting for SIMH POWER ON completion; panel powered={} running={}; worker={worker:?}\n--- worker tail ---\n{}",
                panel.powered,
                panel.running,
                worker_tail()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn probe_data(
    backend: &mut SimhThreadedBackend,
    data: u8,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    assert!(backend.write_memory(DATA_PROBE_ADDRESS, data, false)?);
    backend.set_switch_register(DATA_PROBE_ADDRESS)?;
    backend.panel_examine(false)?;

    let panel = wait_for(backend, name, |panel| {
        panel.powered
            && !panel.running
            && panel.address == DATA_PROBE_ADDRESS
            && panel.data == data
    })?;

    assert_eq!(panel.address, DATA_PROBE_ADDRESS);
    assert_eq!(panel.data, data);
    assert_binary_lamps("DATA", &panel.lamps.data, u64::from(data));
    assert_status_lamps_dark(&panel);
    println!(
        "PASS DATA {name:<12} @0200h = {} ({data:02X}h)",
        bits8(data)
    );
    Ok(())
}

fn probe_address(
    backend: &mut SimhThreadedBackend,
    address: u16,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    backend.set_switch_register(address)?;
    backend.panel_examine(false)?;

    let panel = wait_for(backend, name, |panel| {
        panel.powered && !panel.running && panel.address == address
    })?;

    assert_eq!(panel.address, address);
    assert_binary_lamps("ADDRESS", &panel.lamps.address, u64::from(address));
    assert_status_lamps_dark(&panel);
    println!(
        "PASS ADDRESS {name:<9} = {} ({address:04X}h)  examined DATA={:02X}h",
        bits16(address),
        panel.data
    );
    Ok(())
}

fn assert_panel_pattern(panel: &FrontPanelState, address: u16, data: u8) {
    assert!(panel.powered, "panel unexpectedly reports POWER OFF");
    assert!(!panel.running, "deterministic LED checks require STOP");
    assert_eq!(panel.address, address, "wrong panel ADDRESS value");
    assert_eq!(panel.data, data, "wrong panel DATA value");
    assert_binary_lamps("ADDRESS", &panel.lamps.address, u64::from(address));
    assert_binary_lamps("DATA", &panel.lamps.data, u64::from(data));
    assert_status_lamps_dark(panel);
}

fn wait_live_cpu_pattern(
    backend: &mut SimhThreadedBackend,
    address: u16,
    accumulator: u8,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let panel = wait_for(backend, name, |panel| {
        panel.powered
            && !panel.running
            && panel.address == address
            && panel.data == accumulator
    })?;
    assert_panel_pattern(&panel, address, accumulator);

    match backend.cpu_state()? {
        CpuState::Intel8080(cpu) => {
            assert_eq!(cpu.pc, address, "{name}: CPU PC disagrees with panel");
            assert_eq!(cpu.a, accumulator, "{name}: CPU A disagrees with panel");
        }
        CpuState::Z80(_) => {
            return Err(test_error(
                "product AltairZ80 backend unexpectedly exposed Z80 CpuState instead of its configured Intel 8080 personality",
            ));
        }
    }

    println!(
        "PASS {name:<18} ADDRESS {} ({address:04X}h)  A/DATA {} ({accumulator:02X}h)",
        bits16(address),
        bits8(accumulator)
    );
    Ok(())
}

#[test]
#[ignore = "starts the embedded Open-SIMH AltairZ80 process; run explicitly with --ignored --nocapture"]
fn altairz80_product_panel_leds_are_deterministic() -> Result<(), Box<dyn Error>> {
    println!("SIMH deterministic LED harness v2 — ADDRESS and DATA are probed independently");

    let mut backend = SimhThreadedBackend::new(EmulationEngine::SimhAltairZ80)?;
    backend.power(true)?;
    let power_on = wait_power_ready(&mut backend)?;
    println!(
        "POWER READY            ADDRESS {} ({:04X}h)  DATA {} ({:02X}h)",
        bits16(power_on.address),
        power_on.address,
        bits8(power_on.data),
        power_on.data
    );

    // DATA path first, always at a known-safe RAM address already exercised by
    // the older AltairZ80 smoke test. This isolates memory deposit/examine,
    // stopped-panel latch ownership, and DATA bit ordering from ADDRESS tests.
    probe_data(&mut backend, 0x00, "all off")?;
    probe_data(&mut backend, 0xff, "all on")?;
    probe_data(&mut backend, 0x55, "alternating")?;
    probe_data(&mut backend, 0xaa, "inverse alt")?;

    // ADDRESS path independently. The DATA read at these locations is printed
    // but deliberately not asserted; an edge location such as FFFFh must not be
    // allowed to hide an ADDRESS wiring/bit-order bug.
    probe_address(&mut backend, 0x0000, "all off")?;
    probe_address(&mut backend, 0xffff, "all on")?;
    probe_address(&mut backend, 0xaaaa, "alternating")?;
    probe_address(&mut backend, 0x5555, "inverse")?;

    // Real 8080 execution through the exact product backend used by the GUI:
    //   4000: MVI A,A5h   -> PC=4002, A=A5
    //   4002: MVI A,5Ah   -> PC=4004, A=5A
    //   4004: HLT
    // One-byte writes keep this independent of the bulk Quick Load path.
    for (address, value) in [
        (0x4000, 0x3e),
        (0x4001, 0xa5),
        (0x4002, 0x3e),
        (0x4003, 0x5a),
        (0x4004, 0x76),
    ] {
        assert!(backend.write_memory(address, value, false)?);
    }

    backend.set_switch_register(0x4000)?;
    backend.panel_examine(false)?;
    let before_step = wait_for(&mut backend, "program entry EXAMINE", |panel| {
        panel.powered && !panel.running && panel.address == 0x4000 && panel.data == 0x3e
    })?;
    assert_panel_pattern(&before_step, 0x4000, 0x3e);
    println!(
        "PASS program entry      ADDRESS {} (4000h)  MEMORY {} (3Eh)",
        bits16(0x4000),
        bits8(0x3e)
    );

    backend.step()?;
    wait_live_cpu_pattern(&mut backend, 0x4002, 0xa5, "STEP MVI A,A5")?;

    backend.step()?;
    wait_live_cpu_pattern(&mut backend, 0x4004, 0x5a, "STEP MVI A,5A")?;

    backend.power(false)?;
    println!("PASS deterministic AltairZ80 product-panel LED diagnostic");
    Ok(())
}
