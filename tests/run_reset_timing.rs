use rustair::backend::{BackendHost, BusTState};
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
fn run_sets_the_physical_latch_while_reset_is_held() {
    let mut host = prepared(&[0x00, 0x00]);
    assert!(!host.running());

    host.assert_front_panel_reset();
    assert!(!host.running());
    host.assert_run_stop(true);
    assert!(host.running(), "RUN must asynchronously set the D/C R-S latch during RESET");
    host.release_run_stop(true);

    let held = host.bus_teaching_snapshot().expect("RESET teaching state");
    assert_eq!(held.reset, Some(true));
    assert_eq!(
        held.ready,
        Some(true),
        "PRDY follows the RUN latch even while PRESET is asserted"
    );

    host.release_front_panel_reset();
    assert!(host.running(), "releasing RESET must preserve RUN");
    host.run_cycles(4);
    assert_eq!(
        host.intel8080_state().pc,
        1,
        "execution must begin at reset vector zero"
    );
}

#[test]
fn stop_held_during_reset_waits_for_the_first_post_reset_fetch() {
    // HLT reproduces the classic original-8800 lock-up: STOP cannot reset the
    // RUN latch because a halted 8080 produces no qualifying PSYNC.
    let mut host = prepared(&[0x76, 0x00]);
    host.assert_run_stop(true);
    host.release_run_stop(true);
    host.run_cycles(16);
    assert!(host.running(), "HLT leaves the physical RUN latch set");
    assert!(host.intel8080_state().halted.unwrap_or(false));

    host.assert_run_stop(false);
    assert!(host.running(), "STOP cannot clear RUN while the CPU is halted");
    host.assert_front_panel_reset();
    assert!(host.running(), "RESET itself must not clear the RUN/STOP latch");

    host.release_front_panel_reset();
    // Adaptive Cycle retains RUN until the real first T1/PSYNC is clocked.
    assert!(host.running());
    host.run_cycles(1);
    assert!(!host.running());
    let sample = host
        .bus_teaching_snapshot()
        .expect("post-reset STOP sample");
    assert_eq!(sample.t_state, BusTState::Tw);
    assert_eq!(sample.pins.wait, Some(true));
    assert_eq!(sample.ready, Some(false));

    host.release_run_stop(false);
}
