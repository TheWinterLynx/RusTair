use rustair::backend::{BackendHost, BusMachineCycle, BusTeachingAccuracy, BusTState};
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
fn adaptive_cycle_power_on_inte_raw_s100_matches_cpu_authority_before_reset() {
    let mut host = BackendHost::default();
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);

    let cpu = host.intel8080_state();
    let snapshot = host.bus_teaching_snapshot().expect("Adaptive Cycle power-on control state");
    assert_eq!(snapshot.accuracy, BusTeachingAccuracy::ControlState);
    assert_eq!(snapshot.machine_cycle, BusMachineCycle::PowerOnUndefined);
    assert_eq!(snapshot.status.inte, Some(cpu.inte));
    assert_eq!(snapshot.pins.inte, None, "undefined pre-RESET CPU pin timing must not be invented");

    host.assert_front_panel_reset();
    let cpu_after_reset = host.intel8080_state();
    let reset = host.bus_teaching_snapshot().expect("RESET control state");
    assert!(!cpu_after_reset.inte, "8080 RESET disables the interrupt flip-flop");
    assert_eq!(reset.status.inte, Some(false));
}

#[test]
fn adaptive_cycle_teacher_exposes_reset_released_stop_before_first_t_state() {
    let mut host = prepared(&[0x00]);
    let snapshot = host.bus_teaching_snapshot().expect("Adaptive Cycle control snapshot");
    assert_eq!(snapshot.accuracy, BusTeachingAccuracy::ControlState);
    assert_eq!(snapshot.machine_cycle, BusMachineCycle::ResetReleasedStopped);
    assert_eq!(snapshot.t_state, BusTState::Unknown);
    assert_eq!(snapshot.instruction_address, Some(0x0000));
    assert_eq!(snapshot.status_word, Some(0xA2));
    assert_eq!(snapshot.reset, Some(false));
    assert_eq!(snapshot.ready, Some(false));
    assert_eq!(snapshot.total_t_states, Some(0));
}

#[test]
fn adaptive_cycle_t1_exposes_raw_status_before_8212_latches_on_t2_phi1() {
    let mut host = prepared(&[0x00]);
    assert_eq!(
        host.bus_teaching_snapshot().unwrap().accuracy,
        BusTeachingAccuracy::ControlState,
        "before the first CPU tick the teacher must expose control state, not fake T1",
    );

    let before = host.intel8080_state().total_t_states.unwrap();
    host.debugger_step_t_state();
    let after_t1 = host.intel8080_state().total_t_states.unwrap();
    assert_eq!(after_t1, before + 1);

    let t1 = host.bus_teaching_snapshot().expect("exact Adaptive Cycle T1 sample");
    assert_eq!(t1.accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(t1.machine_cycle, BusMachineCycle::InstructionFetch);
    assert_eq!(t1.machine_cycle_index, Some(1));
    assert_eq!(t1.t_state, BusTState::T1);
    assert_eq!(t1.instruction_address, Some(0x0000));
    assert_eq!(t1.address, Some(0x0000));
    assert_eq!(t1.pins.sync, Some(true));
    assert_eq!(t1.cpu_data, Some(0xA2));
    assert_eq!(t1.s100_do, Some(0xA2));
    assert_eq!(t1.status_word, Some(0x00));
    assert_eq!(t1.status.memr, Some(false));
    assert_eq!(t1.status.m1, Some(false));
    assert_eq!(t1.status.wo, Some(false));
    assert_eq!(t1.total_t_states, Some(after_t1));

    host.debugger_step_t_state();
    let after_t2 = host.intel8080_state().total_t_states.unwrap();
    assert_eq!(after_t2, after_t1 + 1);
    let t2 = host.bus_teaching_snapshot().expect("exact Adaptive Cycle T2 sample");
    assert_eq!(t2.t_state, BusTState::T2);
    assert_eq!(t2.status_word, Some(0xA2));
    assert_eq!(t2.status.memr, Some(true));
    assert_eq!(t2.status.m1, Some(true));
    assert_eq!(t2.status.wo, Some(true));
}

#[test]
fn exact_sample_inputs_remain_historical_after_debugger_returns_to_pause() {
    let mut host = prepared(&[0x00]);
    host.debugger_step_t_state();

    let sample = host.bus_teaching_snapshot().expect("exact T1 sample");
    let panel = host.front_panel_state();
    assert_eq!(sample.accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(sample.t_state, BusTState::T1);
    assert_eq!(sample.ready, Some(true), "READY belongs to the captured T1 input sample");
    assert_eq!(sample.hold, Some(false));
    assert_eq!(sample.reset, Some(false));
    assert_eq!(sample.pins.wait, Some(false));
    assert!(!panel.running, "debugger stepping returns the live chassis to pause after capturing T1");

    let same_sample = host.bus_teaching_snapshot().expect("retained exact T1 sample");
    assert_eq!(same_sample.ready, Some(true));
    assert_eq!(same_sample.t_state, BusTState::T1);
}

#[test]
fn exact_sample_and_current_chassis_are_distinct_after_debugger_pause() {
    let mut host = prepared(&[0x00]);
    host.debugger_step_t_state();

    let view = host.bus_teaching_snapshot().expect("dual-state teaching view");
    let current = view.current_chassis.expect("current chassis plane");
    assert_eq!(view.accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(view.t_state, BusTState::T1);
    assert_eq!(view.ready, Some(true), "exact T1 retains sampled READY HIGH");
    assert!(!current.running, "debugger has already returned the chassis to STOP");
    assert_eq!(current.ready, Some(false), "present chassis READY follows STOP");
    assert_eq!(current.reset, Some(false));
}

#[test]
fn hold_request_after_exact_sample_does_not_rewrite_captured_input() {
    let mut host = prepared(&[0x00]);
    host.debugger_step_t_state();
    let before = host.bus_teaching_snapshot().expect("exact T1 sample");
    assert_eq!(before.hold, Some(false));
    let before_t_states = host.intel8080_state().total_t_states;

    host.request_hold(true);
    host.debugger_step_t_state();

    let retained = host.bus_teaching_snapshot().expect("retained exact T1 sample");
    assert_eq!(retained.hold, Some(false), "HOLD is the value sampled at displayed T1, not a later request");
    assert_eq!(retained.t_state, BusTState::T1);
    assert_eq!(retained.current_chassis.expect("current chassis").hold, Some(true), "live chassis must expose the later HOLD request separately");
    assert_eq!(host.intel8080_state().total_t_states, before_t_states, "live HOLD request must block debugger stepping before a new CPU sample is captured");

    host.request_hold(false);
}

#[test]
fn reset_replaces_exact_sample_with_control_state_immediately() {
    let mut host = prepared(&[0x00]);
    host.debugger_step_t_state();
    assert_eq!(host.bus_teaching_snapshot().unwrap().accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(host.bus_teaching_snapshot().unwrap().t_state, BusTState::T1);

    host.assert_front_panel_reset();

    let reset = host.bus_teaching_snapshot().expect("RESET control state");
    assert_eq!(reset.accuracy, BusTeachingAccuracy::ControlState);
    assert_eq!(reset.machine_cycle, BusMachineCycle::ResetAsserted);
    assert_eq!(reset.t_state, BusTState::Unknown);
    assert_eq!(reset.reset, Some(true));
    assert_eq!(reset.pins.sync, None, "RESET must not reuse a stale exact SYNC output");

    host.release_front_panel_reset();
    let released = host.bus_teaching_snapshot().expect("reset-released control state");
    assert_eq!(released.accuracy, BusTeachingAccuracy::ControlState);
    assert_eq!(released.machine_cycle, BusMachineCycle::ResetReleasedStopped);
    assert_eq!(released.reset, Some(false));
}

#[test]
fn adaptive_cycle_t_state_step_advances_one_t_state_at_a_time() {
    let mut host = prepared(&[0x00]);
    host.debugger_step_t_state();
    assert_eq!(host.bus_teaching_snapshot().unwrap().t_state, BusTState::T1);
    host.debugger_step_t_state();
    assert_eq!(host.bus_teaching_snapshot().unwrap().t_state, BusTState::T2);
    assert_eq!(host.intel8080_state().total_t_states, Some(2));
}

#[test]
fn adaptive_cycle_machine_cycle_step_completes_one_fetch_cycle() {
    let mut host = prepared(&[0x00, 0x00]);
    host.debugger_step_machine_cycle();
    assert_eq!(host.intel8080_state().total_t_states, Some(4));
    let snapshot = host.bus_teaching_snapshot().expect("last exact T-state of M1");
    assert_eq!(snapshot.accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(snapshot.machine_cycle, BusMachineCycle::InstructionFetch);
    assert_eq!(snapshot.machine_cycle_index, Some(1));
    assert_eq!(snapshot.t_state, BusTState::T4);
    assert_eq!(snapshot.instruction_complete, Some(true));
}

#[test]
fn adaptive_cycle_teacher_retains_s100_status_latch_during_internal_cycle() {
    let mut host = prepared(&[0x09, 0x76]);
    let mut saw_internal = false;

    for _ in 0..16 {
        host.debugger_step_t_state();
        let snapshot = host.bus_teaching_snapshot().expect("Adaptive Cycle teaching sample");
        if snapshot.machine_cycle == BusMachineCycle::Internal {
            assert_eq!(snapshot.status_word, Some(0xA2));
            assert_eq!(snapshot.status.memr, Some(true));
            assert_eq!(snapshot.status.m1, Some(true));
            assert_eq!(snapshot.status.wo, Some(true));
            saw_internal = true;
            break;
        }
    }

    assert!(saw_internal, "DAD B should enter an internal machine cycle");
}

#[test]
fn t_state_stepping_still_closes_shared_instruction_history() {
    let mut host = prepared(&[0x00, 0x76]);
    host.set_instruction_trace_enabled(true);
    for _ in 0..4 {
        host.debugger_step_t_state();
    }
    let history = host.instruction_trace_snapshot();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].address, 0x0000);
    assert_eq!(history[0].bytes[0], 0x00);
    assert_eq!(history[0].t_states, 4);
}
