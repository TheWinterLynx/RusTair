use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamInit, RamSize};

fn exercise_history(engine: EmulationEngine) {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, &[0x3e, 0x42, 0x3c, 0x76]); // MVI A,42 / INR A / HLT

    // RESET establishes the execution entry point, but the Intel 8080 general
    // registers are not specified to become zero. Preserve the actual state
    // presented by each backend and require the trace to capture that exact
    // pre-instruction snapshot rather than inventing a debugger-friendly one.
    let initial = host.intel8080_state();
    assert_eq!(initial.pc, 0x0000, "{engine:?}");

    host.clear_instruction_trace();
    host.set_instruction_trace_enabled(true);
    host.set_running(true);
    host.run_cycles(128);

    let history = host.instruction_trace_snapshot();
    assert!(history.len() >= 3, "{engine:?}: missing history entries: {history:?}");

    let mvi = &history[0];
    assert_eq!(mvi.address, 0x0000, "{engine:?}");
    assert_eq!(&mvi.bytes[..2], &[0x3e, 0x42], "{engine:?}");
    assert_eq!(mvi.length, 2, "{engine:?}");
    assert_eq!(mvi.before.pc, initial.pc, "{engine:?}");
    assert_eq!(mvi.before.a, initial.a, "{engine:?}");
    assert_eq!(mvi.before.b, initial.b, "{engine:?}");
    assert_eq!(mvi.before.c, initial.c, "{engine:?}");
    assert_eq!(mvi.before.d, initial.d, "{engine:?}");
    assert_eq!(mvi.before.e, initial.e, "{engine:?}");
    assert_eq!(mvi.before.h, initial.h, "{engine:?}");
    assert_eq!(mvi.before.l, initial.l, "{engine:?}");
    assert_eq!(mvi.before.flags, initial.flags, "{engine:?}");
    assert_eq!(mvi.before.sp, initial.sp, "{engine:?}");
    assert_eq!(mvi.before.inte, initial.inte, "{engine:?}");
    assert_eq!(mvi.before.halted, initial.halted.unwrap_or(false), "{engine:?}");
    assert_eq!(mvi.after.a, 0x42, "{engine:?}");
    assert_eq!(mvi.after.pc, 0x0002, "{engine:?}");

    let inr = &history[1];
    assert_eq!(inr.address, 0x0002, "{engine:?}");
    assert_eq!(inr.bytes[0], 0x3c, "{engine:?}");
    assert_eq!(inr.before.a, 0x42, "{engine:?}");
    assert_eq!(inr.after.a, 0x43, "{engine:?}");

    let hlt = &history[2];
    assert_eq!(hlt.address, 0x0003, "{engine:?}");
    assert_eq!(hlt.bytes[0], 0x76, "{engine:?}");
    assert!(hlt.after.halted, "{engine:?}");

    host.set_instruction_trace_enabled(false);
    host.clear_instruction_trace();
    assert!(host.instruction_trace_snapshot().is_empty(), "{engine:?}");
}

#[test]
fn fast_core_records_instruction_history() {
    exercise_history(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_core_records_instruction_history() {
    exercise_history(EmulationEngine::RustCycleAccurate8080);
}
