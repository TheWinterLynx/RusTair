use rustair::backend::{MachineBackend, NativeMachineBackend};
use rustair::config::SerialBoard;

#[test]
fn debugger_two_sio_in_does_not_leak_prdy_wait_into_next_fast_instruction() {
    let mut backend = NativeMachineBackend::default();
    backend.configure_serial_board(SerialBoard::TwoSio88).unwrap();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0x0000, &[0x00]).unwrap(); // NOP

    let before = backend.machine().cpu.cycles;
    let _ = backend.debugger_input_port(0x10).unwrap();
    assert_eq!(
        backend.machine().cpu.cycles,
        before,
        "debugger I/O must not itself advance the Fast 8080 clock",
    );

    backend.debugger_step_instruction().unwrap();
    assert_eq!(
        backend.machine().cpu.cycles - before,
        4,
        "the debugger's 88-2SIO +1Tw must not contaminate the following guest NOP",
    );
}
