use rustair::backend::{BackendHost, BackendSerialPort, EmulationEngine};
use rustair::config::{RamInit, RamSize, SerialBoard};

const BASIC32_IMAGE: &[u8; 4096] = include_bytes!("../assets/4kbas32.bin");
const PORT0: BackendSerialPort = BackendSerialPort::Port0;

fn drain_tx(machine: &mut BackendHost, output: &mut Vec<u8>) {
    while machine.serial_tx_front(PORT0).is_some() {
        if let Some(byte) = machine.serial_tx_complete(PORT0) {
            output.push(byte & 0x7f);
        } else {
            break;
        }
    }
}

fn output_contains(output: &[u8], needle: &str) -> bool {
    String::from_utf8_lossy(output)
        .to_ascii_uppercase()
        .contains(&needle.to_ascii_uppercase())
}

fn run_until_output(
    machine: &mut BackendHost,
    output: &mut Vec<u8>,
    needle: &str,
    max_t_states: u64,
    context: &str,
) {
    let start = machine.intel8080_state().total_t_states.unwrap_or(0);
    loop {
        machine.run_cycles(2_048);
        drain_tx(machine, output);
        if output_contains(output, needle) {
            return;
        }

        let cpu = machine.intel8080_state();
        let elapsed = cpu.total_t_states.unwrap_or(start).saturating_sub(start);
        if elapsed >= max_t_states || !machine.running() {
            panic!(
                "{context}: BASIC did not reach {needle:?}; PC={:04X} SP={:04X} running={} elapsed={}T output={:?}",
                cpu.pc,
                cpu.sp,
                machine.running(),
                elapsed,
                String::from_utf8_lossy(output),
            );
        }
    }
}

fn start_quick_basic(engine: EmulationEngine, ram: RamSize, trace: bool) -> BackendHost {
    let mut machine = BackendHost::from_engine(engine).expect("built-in Rust backend");
    machine.configure_memory(ram, RamInit::Zeroed);
    machine.configure_serial_board(SerialBoard::Sio88);
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    machine.clear_memory_protection();
    machine.load_bytes(0, BASIC32_IMAGE);
    machine.set_switch_register(0x0000); // 88-SIO console selection.

    if ram == RamSize::K64 {
        assert!(
            machine.arm_basic32_full_memory_probe_guard(),
            "64 KiB Quick Load regression requires the explicit BASIC 3.2 workaround"
        );
    }

    machine.set_instruction_trace_enabled(trace);
    machine.set_running(true);
    machine
}

fn exercise_memory_size_return(
    engine: EmulationEngine,
    ram: RamSize,
    trace: bool,
    max_after_cr_t_states: u64,
) {
    let context = format!("{engine:?} / {ram:?} / trace={trace}");
    let mut machine = start_quick_basic(engine, ram, trace);
    let mut output = Vec::new();

    run_until_output(
        &mut machine,
        &mut output,
        "MEMORY SIZE",
        4_000_000,
        &context,
    );
    assert!(machine.serial_rx_empty(PORT0), "{context}: RX should be empty before RETURN");

    machine.serial_receive(PORT0, b'\r');
    run_until_output(
        &mut machine,
        &mut output,
        "TERMINAL WIDTH",
        max_after_cr_t_states,
        &context,
    );

    assert!(
        output_contains(&output, "TERMINAL WIDTH"),
        "{context}: RETURN at MEMORY SIZE must advance BASIC startup"
    );
}

#[test]
fn bundled_basic_quick_load_accepts_memory_size_return_on_both_cores() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        exercise_memory_size_return(engine, RamSize::K8, false, 6_000_000);
    }
}

#[test]
fn bundled_basic_quick_load_still_works_while_instruction_trace_is_active() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        exercise_memory_size_return(engine, RamSize::K8, true, 6_000_000);
    }
}

#[test]
fn bundled_basic_64k_probe_workaround_reaches_terminal_width_on_both_cores() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        exercise_memory_size_return(engine, RamSize::K64, false, 30_000_000);
    }
}
