use rustair::backend::{
    BackendHost, DebugStopReason, EmulationEngine, MemoryWatchAccess,
};
use rustair::config::{RamInit, RamSize};

fn prepared_host(engine: EmulationEngine, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

fn exercise_breakpoint(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x00, 0x00, 0x76]); // NOP / NOP / HLT
    host.debugger_set_breakpoint(0x0001, true);
    assert_eq!(host.debugger_breakpoints(), vec![0x0001], "{engine:?}");

    host.set_running(true);
    host.run_cycles(128);
    let stopped = host.intel8080_state();
    assert_eq!(stopped.pc, 0x0001, "{engine:?}: breakpoint must stop before opcode fetch");
    assert!(!host.running(), "{engine:?}: breakpoint must drop execution to STOP");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::ExecuteBreakpoint(0x0001)),
        "{engine:?}",
    );

    host.set_running(true);
    host.run_cycles(128);
    let after_resume = host.intel8080_state();
    assert!(after_resume.pc >= 0x0002, "{engine:?}: resume did not pass breakpoint");
}

fn exercise_fresh_breakpoint_at_current_pc(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x00, 0x76]);
    host.debugger_set_breakpoint(0x0000, true);
    host.set_running(true);
    host.run_cycles(64);
    let cpu = host.intel8080_state();
    assert_eq!(cpu.pc, 0x0000, "{engine:?}: fresh RUN must not silently skip current breakpoint");
    assert!(!host.running(), "{engine:?}");
}

fn exercise_run_to(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x00, 0x00, 0x76]);
    host.debugger_run_to(0x0002);
    assert_eq!(host.debugger_run_to_target(), Some(0x0002), "{engine:?}");
    host.run_cycles(128);

    let cpu = host.intel8080_state();
    assert_eq!(cpu.pc, 0x0002, "{engine:?}: run-to must stop before target executes");
    assert!(!host.running(), "{engine:?}");
    assert_eq!(host.debugger_run_to_target(), None, "{engine:?}: run-to must be one-shot");
    assert_eq!(host.debugger_stop_reason(), Some(DebugStopReason::RunTo(0x0002)), "{engine:?}");
}

fn exercise_run_to_from_breakpoint(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x00, 0x00, 0x76]);
    host.debugger_set_breakpoint(0x0000, true);
    host.set_running(true);
    host.run_cycles(64);
    assert_eq!(host.intel8080_state().pc, 0x0000, "{engine:?}");
    assert_eq!(host.debugger_stop_reason(), Some(DebugStopReason::ExecuteBreakpoint(0x0000)), "{engine:?}");

    host.debugger_run_to(0x0002);
    host.run_cycles(128);
    assert_eq!(host.intel8080_state().pc, 0x0002, "{engine:?}: run-to must resume past the triggered breakpoint");
    assert_eq!(host.debugger_stop_reason(), Some(DebugStopReason::RunTo(0x0002)), "{engine:?}");
}

fn exercise_debugger_instruction_step(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x3e, 0x42, 0x76]); // MVI A,42 / HLT
    host.set_instruction_trace_enabled(true);
    host.debugger_step_instruction();

    let cpu = host.intel8080_state();
    assert_eq!(cpu.pc, 0x0002, "{engine:?}: debugger step must complete whole MVI instruction");
    assert_eq!(cpu.a, 0x42, "{engine:?}");
    assert!(!host.running(), "{engine:?}: debugger step must remain stopped");

    let history = host.instruction_trace_snapshot();
    assert_eq!(history.len(), 1, "{engine:?}: debugger step should produce one history entry");
    assert_eq!(history[0].address, 0x0000, "{engine:?}");
    assert_eq!(history[0].after.pc, 0x0002, "{engine:?}");
}

fn exercise_memory_read_watchpoint_without_history(engine: EmulationEngine) {
    // LXI H,0080 / MOV A,M / HLT
    let mut host = prepared_host(engine, &[0x21, 0x80, 0x00, 0x7e, 0x76]);
    host.load_bytes(0x0080, &[0xa5]);
    host.set_instruction_trace_enabled(false);
    host.debugger_set_watchpoint(0x0080, Some(MemoryWatchAccess::Read));
    assert_eq!(
        host.debugger_watchpoints(),
        vec![(0x0080, MemoryWatchAccess::Read)],
        "{engine:?}",
    );

    host.set_running(true);
    host.run_cycles(256);

    let cpu = host.intel8080_state();
    assert!(!host.running(), "{engine:?}: read watchpoint must stop execution");
    assert_eq!(cpu.a, 0xa5, "{engine:?}: watched read must have completed");
    assert_eq!(cpu.pc, 0x0004, "{engine:?}: stop must be after MOV A,M");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::MemoryReadWatchpoint {
            instruction_pc: 0x0003,
            address: 0x0080,
            value: 0xa5,
        }),
        "{engine:?}",
    );
    assert!(host.instruction_trace_snapshot().is_empty(), "{engine:?}: watchpoints must not force history retention");
}

fn exercise_memory_write_watchpoint_without_history(engine: EmulationEngine) {
    // LXI H,0080 / MVI M,5A / HLT
    let mut host = prepared_host(engine, &[0x21, 0x80, 0x00, 0x36, 0x5a, 0x76]);
    host.set_instruction_trace_enabled(false);
    host.debugger_set_watchpoint(0x0080, Some(MemoryWatchAccess::Write));

    host.set_running(true);
    host.run_cycles(256);

    let cpu = host.intel8080_state();
    assert!(!host.running(), "{engine:?}: write watchpoint must stop execution");
    assert_eq!(host.peek_memory(0x0080), Some(0x5a), "{engine:?}: watched write must have completed");
    assert_eq!(cpu.pc, 0x0005, "{engine:?}: stop must be after MVI M,5A");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::MemoryWriteWatchpoint {
            instruction_pc: 0x0003,
            address: 0x0080,
            value: 0x5a,
        }),
        "{engine:?}",
    );
    assert!(host.instruction_trace_snapshot().is_empty(), "{engine:?}: watchpoints must not force history retention");
}

#[test]
fn fast_debugger_execution_control() {
    exercise_breakpoint(EmulationEngine::RustFast8080);
    exercise_fresh_breakpoint_at_current_pc(EmulationEngine::RustFast8080);
    exercise_run_to(EmulationEngine::RustFast8080);
    exercise_run_to_from_breakpoint(EmulationEngine::RustFast8080);
    exercise_debugger_instruction_step(EmulationEngine::RustFast8080);
    exercise_memory_read_watchpoint_without_history(EmulationEngine::RustFast8080);
    exercise_memory_write_watchpoint_without_history(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_debugger_execution_control() {
    exercise_breakpoint(EmulationEngine::RustCycleAccurate8080);
    exercise_fresh_breakpoint_at_current_pc(EmulationEngine::RustCycleAccurate8080);
    exercise_run_to(EmulationEngine::RustCycleAccurate8080);
    exercise_run_to_from_breakpoint(EmulationEngine::RustCycleAccurate8080);
    exercise_debugger_instruction_step(EmulationEngine::RustCycleAccurate8080);
    exercise_memory_read_watchpoint_without_history(EmulationEngine::RustCycleAccurate8080);
    exercise_memory_write_watchpoint_without_history(EmulationEngine::RustCycleAccurate8080);
}
