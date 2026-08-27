use rustair::backend::{BackendHost, DebugStopReason, EmulationEngine};
use rustair::config::{RamInit, RamSize};

fn exercise_live_breakpoint(engine: EmulationEngine) {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    // 0000 NOP
    // 0001 JMP 0000
    host.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]);

    host.set_running(true);
    host.run_cycles(128);
    assert!(host.running(), "{engine:?}: loop should still be running before breakpoint is armed");

    // This is the exact UI use case: the debugger window is opened while RUN
    // is already active and the operator arms an execute breakpoint live.
    host.debugger_set_breakpoint(0x0000, true);
    assert_eq!(host.debugger_breakpoints(), vec![0x0000], "{engine:?}");

    host.run_cycles(256);
    assert!(!host.running(), "{engine:?}: live-armed breakpoint must stop the running loop");
    assert_eq!(host.intel8080_state().pc, 0x0000, "{engine:?}: stop must occur before fetching opcode at 0000h");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::ExecuteBreakpoint(0x0000)),
        "{engine:?}",
    );
}

fn exercise_operand_address_never_masquerades_as_execute_boundary(engine: EmulationEngine) {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    // 0000 JMP 0000. Addresses 0001/0002 are operands, never opcode fetches.
    host.load_bytes(0, &[0xc3, 0x00, 0x00]);
    host.set_running(true);
    host.run_cycles(64);

    host.debugger_set_breakpoint(0x0001, true);
    host.run_cycles(256);

    assert!(host.running(), "{engine:?}: an operand address must not trigger an execute breakpoint");
    assert_eq!(host.debugger_stop_reason(), None, "{engine:?}");
}

#[test]
fn fast_breakpoint_can_be_armed_while_loop_is_already_running() {
    exercise_live_breakpoint(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_breakpoint_can_be_armed_while_loop_is_already_running() {
    exercise_live_breakpoint(EmulationEngine::RustCycleAccurate8080);
}

#[test]
fn fast_execute_breakpoint_requires_opcode_boundary() {
    exercise_operand_address_never_masquerades_as_execute_boundary(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_execute_breakpoint_requires_opcode_boundary() {
    exercise_operand_address_never_masquerades_as_execute_boundary(EmulationEngine::RustCycleAccurate8080);
}
