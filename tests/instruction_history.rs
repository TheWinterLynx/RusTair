use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamInit, RamSize, SerialBoard};
use rustair::trace8080::InstructionEffect8080;

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

fn exercise_effects(engine: EmulationEngine) {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.configure_serial_board(SerialBoard::Sio88);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(
        0,
        &[
            0x21, 0x80, 0x00, // LXI H,0080h
            0x3e, 0x5a,       // MVI A,5Ah
            0x77,             // MOV M,A
            0x7e,             // MOV A,M
            0xd3, 0x01,       // OUT 01h (88-SIO data)
            0xdb, 0x00,       // IN 00h (88-SIO status)
            0x76,             // HLT
        ],
    );
    host.clear_instruction_trace();
    host.set_instruction_trace_enabled(true);
    host.set_running(true);
    host.run_cycles(256);

    let history = host.instruction_trace_snapshot();
    assert!(history.len() >= 7, "{engine:?}: missing effect history: {history:?}");

    assert_eq!(
        history[2].effects,
        vec![InstructionEffect8080::MemoryWrite { address: 0x0080, value: 0x5a }],
        "{engine:?}: MOV M,A write",
    );
    assert_eq!(
        history[3].effects,
        vec![InstructionEffect8080::MemoryRead { address: 0x0080, value: 0x5a }],
        "{engine:?}: MOV A,M read",
    );
    assert_eq!(
        history[4].effects,
        vec![InstructionEffect8080::IoWrite { port: 0x01, value: 0x5a }],
        "{engine:?}: OUT effect",
    );
    assert_eq!(history[5].effects.len(), 1, "{engine:?}: IN effect count");
    match history[5].effects[0] {
        InstructionEffect8080::IoRead { port: 0x00, value } => {
            assert_eq!(value, history[5].after.a, "{engine:?}: IN value must match A");
        }
        ref other => panic!("{engine:?}: expected IN effect, got {other:?}"),
    }
}

#[test]
fn fast_core_records_instruction_history() {
    exercise_history(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_core_records_instruction_history() {
    exercise_history(EmulationEngine::RustCycleAccurate8080);
}

#[test]
fn fast_core_records_memory_and_io_effects() {
    exercise_effects(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_core_records_memory_and_io_effects() {
    exercise_effects(EmulationEngine::RustCycleAccurate8080);
}
