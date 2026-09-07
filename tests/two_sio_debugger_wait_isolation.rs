use rustair::backend::BackendHost;
use rustair::config::{RamInit, S100HardwareConfig};

#[test]
fn debugger_two_sio_in_does_not_leak_prdy_wait_into_next_adaptive_instruction() {
    let mut backend = BackendHost::default();
    backend.configure_s100_hardware(
        S100HardwareConfig::historical_8800b_18_slot_starter()
            .validate()
            .unwrap(),
        RamInit::Zeroed,
    );
    backend.power(true);
    backend.assert_front_panel_reset();
    backend.release_front_panel_reset();
    backend.load_bytes(0x0000, &[0x00]); // NOP

    let before = backend.intel8080_state().total_t_states.unwrap_or(0);
    let _ = backend.debugger_input_port(0x10);
    assert_eq!(
        backend.intel8080_state().total_t_states.unwrap_or(0),
        before,
        "debugger I/O must not itself advance the Adaptive Cycle 8080 clock",
    );

    backend.debugger_step_instruction();
    assert_eq!(
        backend.intel8080_state().total_t_states.unwrap_or(0) - before,
        4,
        "the debugger's 88-2SIO +1Tw must not contaminate the following guest NOP",
    );
}
