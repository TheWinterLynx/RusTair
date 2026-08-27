use rustair::backend::{
    BackendHost, DebugStopReason, EmulationEngine, MemoryWatchAccess,
};
use rustair::config::{RamInit, RamSize};

fn prepared_host_with_ram(
    engine: EmulationEngine,
    ram_size: RamSize,
    program: &[u8],
) -> BackendHost {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(ram_size, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

fn prepared_host(engine: EmulationEngine, program: &[u8]) -> BackendHost {
    prepared_host_with_ram(engine, RamSize::K1, program)
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

fn exercise_breakpoint_waits_until_reset_is_released(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x00, 0x76]); // NOP / HLT
    host.set_running(true);
    host.assert_front_panel_reset(); // physical RESET preserves the RUN latch
    assert!(host.running(), "{engine:?}: RESET must preserve RUN latch");
    host.debugger_set_breakpoint(0x0000, true);

    host.run_cycles(64);
    assert!(host.running(), "{engine:?}: debugger must not consume RUN while RESET is held");
    assert_eq!(host.debugger_stop_reason(), None, "{engine:?}: RESET is not an instruction boundary");

    host.release_front_panel_reset();
    host.run_cycles(64);
    assert!(!host.running(), "{engine:?}: breakpoint should trigger after RESET release");
    assert_eq!(host.intel8080_state().pc, 0x0000, "{engine:?}");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::ExecuteBreakpoint(0x0000)),
        "{engine:?}",
    );
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

fn exercise_stack_guarded_run_to(engine: EmulationEngine) {
    // CALL pushes return 0003h. The callee deliberately visits 0003h once
    // while SP is still two bytes deeper, then RET restores SP and reaches
    // 0003h a second time. A PC-only temporary breakpoint would stop too early.
    let mut host = prepared_host(
        engine,
        &[
            0xcd, 0x06, 0x00, // 0000 CALL 0006
            0xc3, 0x09, 0x00, // 0003 JMP 0009 (false early visit path)
            0xc3, 0x03, 0x00, // 0006 JMP 0003
            0xc9,             // 0009 RET -> real return to 0003
            0x76,             // 000A HLT
        ],
    );
    let initial_sp = host.intel8080_state().sp;

    host.debugger_run_to_with_sp(0x0003, initial_sp);
    host.run_cycles(512);

    let cpu = host.intel8080_state();
    assert_eq!(cpu.pc, 0x0003, "{engine:?}: guarded target PC");
    assert_eq!(cpu.sp, initial_sp, "{engine:?}: target must wait for caller stack depth");
    assert!(!host.running(), "{engine:?}: guarded run-to must stop execution");
    assert_eq!(host.debugger_stop_reason(), Some(DebugStopReason::RunTo(0x0003)), "{engine:?}");
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

fn exercise_halted_debugger_step_is_noop(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x76, 0x00]); // HLT / NOP
    host.set_instruction_trace_enabled(true);
    host.set_running(true);
    host.run_cycles(64);

    let halted = host.intel8080_state();
    assert!(halted.halted.unwrap_or(false), "{engine:?}: program must enter HLT");
    assert_eq!(halted.pc, 0x0001, "{engine:?}");
    host.set_running(false);

    let history_before = host.instruction_trace_snapshot();
    assert_eq!(history_before.len(), 1, "{engine:?}: only HLT should be captured");
    let t_states_before = host.intel8080_state().total_t_states;

    host.debugger_step_instruction();

    let after = host.intel8080_state();
    assert!(after.halted.unwrap_or(false), "{engine:?}");
    assert_eq!(after.pc, 0x0001, "{engine:?}: debugger step must not execute post-HLT byte");
    assert_eq!(after.total_t_states, t_states_before, "{engine:?}: debugger step while HLT must be a no-op");
    assert_eq!(
        host.instruction_trace_snapshot().len(),
        history_before.len(),
        "{engine:?}: HLT dwell must not invent a history entry",
    );
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

fn exercise_uninstalled_memory_read_watchpoint(engine: EmulationEngine) {
    // Only 256 bytes are installed. MOV A,M still performs a guest-visible
    // read at 0100h and the Altair bus returns 00h.
    let mut host = prepared_host_with_ram(
        engine,
        RamSize::Bytes256,
        &[0x21, 0x00, 0x01, 0x7e, 0x76], // LXI H,0100 / MOV A,M / HLT
    );
    assert_eq!(host.peek_memory(0x0100), None, "{engine:?}");
    host.debugger_set_watchpoint(0x0100, Some(MemoryWatchAccess::Read));
    host.set_running(true);
    host.run_cycles(256);

    let cpu = host.intel8080_state();
    assert!(!host.running(), "{engine:?}: unmapped read watchpoint must stop execution");
    assert_eq!(cpu.a, 0x00, "{engine:?}: uninstalled RAM must read as 00h");
    assert_eq!(cpu.pc, 0x0004, "{engine:?}");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::MemoryReadWatchpoint {
            instruction_pc: 0x0003,
            address: 0x0100,
            value: 0x00,
        }),
        "{engine:?}",
    );
}

fn exercise_uninstalled_memory_write_watchpoint(engine: EmulationEngine) {
    let mut host = prepared_host_with_ram(
        engine,
        RamSize::Bytes256,
        &[0x21, 0x00, 0x01, 0x36, 0x5a, 0x76], // LXI H,0100 / MVI M,5A / HLT
    );
    host.debugger_set_watchpoint(0x0100, Some(MemoryWatchAccess::Write));
    host.set_running(true);
    host.run_cycles(256);

    assert_eq!(host.peek_memory(0x0100), None, "{engine:?}: a write must not create uninstalled RAM");
    assert!(!host.running(), "{engine:?}: unmapped write transfer must trigger watchpoint");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::MemoryWriteWatchpoint {
            instruction_pc: 0x0003,
            address: 0x0100,
            value: 0x5a,
        }),
        "{engine:?}",
    );
}

fn exercise_protected_memory_write_watchpoint(engine: EmulationEngine) {
    let mut host = prepared_host(engine, &[0x21, 0x80, 0x00, 0x36, 0x5a, 0x76]);

    // Point the real front panel at board 0, protect that 1 KiB block, then
    // reset execution to 0000h. Programmatic test loading already happened and
    // deliberately bypasses front-panel protection.
    host.set_switch_register(0x0000);
    host.examine(false);
    host.protect_current_board(true);
    assert!(host.memory_is_protected(0x0080), "{engine:?}");
    host.front_panel_reset();

    host.debugger_set_watchpoint(0x0080, Some(MemoryWatchAccess::Write));
    host.set_running(true);
    host.run_cycles(256);

    assert_eq!(host.peek_memory(0x0080), Some(0x00), "{engine:?}: protected RAM must remain unchanged");
    assert!(!host.running(), "{engine:?}: blocked write transfer must still trigger watchpoint");
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::MemoryWriteWatchpoint {
            instruction_pc: 0x0003,
            address: 0x0080,
            value: 0x5a,
        }),
        "{engine:?}",
    );
}

fn exercise_debugger_suite(engine: EmulationEngine) {
    exercise_breakpoint(engine);
    exercise_fresh_breakpoint_at_current_pc(engine);
    exercise_breakpoint_waits_until_reset_is_released(engine);
    exercise_run_to(engine);
    exercise_run_to_from_breakpoint(engine);
    exercise_stack_guarded_run_to(engine);
    exercise_debugger_instruction_step(engine);
    exercise_halted_debugger_step_is_noop(engine);
    exercise_memory_read_watchpoint_without_history(engine);
    exercise_memory_write_watchpoint_without_history(engine);
    exercise_uninstalled_memory_read_watchpoint(engine);
    exercise_uninstalled_memory_write_watchpoint(engine);
    exercise_protected_memory_write_watchpoint(engine);
}

#[test]
fn fast_debugger_execution_control() {
    exercise_debugger_suite(EmulationEngine::RustFast8080);
}

#[test]
fn cycle_debugger_execution_control() {
    exercise_debugger_suite(EmulationEngine::RustCycleAccurate8080);
}
