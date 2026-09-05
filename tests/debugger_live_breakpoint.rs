use rustair::backend::{BackendHost, DebugStopReason};
use rustair::config::{RamInit, RamSize};

fn prepared(program: &[u8]) -> BackendHost {
    let mut host = BackendHost::default();
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

#[test]
fn adaptive_cycle_breakpoint_can_be_armed_while_loop_is_already_running() {
    let mut host = prepared(&[0x00, 0xc3, 0x00, 0x00]);

    host.set_running(true);
    host.run_cycles(128);
    assert!(host.running(), "loop should still be running before breakpoint is armed");

    // This is the exact UI use case: the debugger window is opened while RUN
    // is already active and the operator arms an execute breakpoint live.
    host.debugger_set_breakpoint(0x0000, true);
    assert_eq!(host.debugger_breakpoints(), vec![0x0000]);

    host.run_cycles(256);
    assert!(!host.running(), "live-armed breakpoint must stop the running loop");
    assert_eq!(
        host.intel8080_state().pc,
        0x0000,
        "stop must occur before fetching opcode at 0000h"
    );
    assert_eq!(
        host.debugger_stop_reason(),
        Some(DebugStopReason::ExecuteBreakpoint(0x0000)),
    );
}

#[test]
fn adaptive_cycle_execute_breakpoint_requires_opcode_boundary() {
    // 0000 JMP 0000. Addresses 0001/0002 are operands, never opcode fetches.
    let mut host = prepared(&[0xc3, 0x00, 0x00]);
    host.set_running(true);
    host.run_cycles(64);

    host.debugger_set_breakpoint(0x0001, true);
    host.run_cycles(256);

    assert!(
        host.running(),
        "an operand address must not trigger an execute breakpoint"
    );
    assert_eq!(host.debugger_stop_reason(), None);
}
