use rustair::backend::{
    BackendHost, BusMachineCycle, BusTeachingAccuracy, BusTState, EmulationEngine,
};
use rustair::config::{RamInit, RamSize};

fn prepared(engine: EmulationEngine, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

#[test]
fn fast_bus_teacher_is_explicitly_reconstructed() {
    let mut host = prepared(EmulationEngine::RustFast8080, &[0x00]);
    let snapshot = host.bus_teaching_snapshot().expect("reconstructed fast snapshot");
    assert_eq!(snapshot.accuracy, BusTeachingAccuracy::Reconstructed);
    assert_eq!(snapshot.machine_cycle, BusMachineCycle::Unknown);
    assert_eq!(snapshot.t_state, BusTState::Unknown);
    assert_eq!(snapshot.pins.sync, None);
    assert_eq!(snapshot.pins.dbin, None);
    assert_eq!(snapshot.pins.wr_n, None);
    assert!(!host.capabilities().exact_bus_activity);
    assert!(!host.capabilities().exact_t_state_timing);
}

#[test]
fn cycle_power_on_inte_raw_s100_matches_cycle_cpu_authority_before_reset() {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("built-in Cycle backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);

    let cpu = host.intel8080_state();
    let snapshot = host.bus_teaching_snapshot().expect("Cycle power-on control state");
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
fn cycle_teacher_exposes_reset_released_stop_before_first_t_state() {
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00]);
    let snapshot = host.bus_teaching_snapshot().expect("Cycle control snapshot");
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
fn cycle_t_state_step_exposes_exact_m1_t1_sample() {
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00]);
    assert_eq!(
        host.bus_teaching_snapshot().unwrap().accuracy,
        BusTeachingAccuracy::ControlState,
        "before the first CPU tick the teacher must expose control state, not fake T1",
    );

    let before = host.intel8080_state().total_t_states.unwrap();
    host.debugger_step_t_state();
    let after = host.intel8080_state().total_t_states.unwrap();
    assert_eq!(after, before + 1);

    let snapshot = host.bus_teaching_snapshot().expect("exact Cycle sample");
    assert_eq!(snapshot.accuracy, BusTeachingAccuracy::Exact);
    assert_eq!(snapshot.machine_cycle, BusMachineCycle::InstructionFetch);
    assert_eq!(snapshot.machine_cycle_index, Some(1));
    assert_eq!(snapshot.t_state, BusTState::T1);
    assert_eq!(snapshot.instruction_address, Some(0x0000));
    assert_eq!(snapshot.address, Some(0x0000));
    assert_eq!(snapshot.status_word, Some(0xA2));
    assert_eq!(snapshot.pins.sync, Some(true));
    assert_eq!(snapshot.status.memr, Some(true));
    assert_eq!(snapshot.status.m1, Some(true));
    assert_eq!(snapshot.status.wo, Some(true));
    assert_eq!(snapshot.total_t_states, Some(after));
}

#[test]
fn exact_sample_inputs_remain_historical_after_debugger_returns_to_pause() {
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00]);

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
fn cycle_t_state_step_advances_one_t_state_at_a_time() {
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00]);
    host.debugger_step_t_state();
    assert_eq!(host.bus_teaching_snapshot().unwrap().t_state, BusTState::T1);
    host.debugger_step_t_state();
    assert_eq!(host.bus_teaching_snapshot().unwrap().t_state, BusTState::T2);
    assert_eq!(host.intel8080_state().total_t_states, Some(2));
}

#[test]
fn cycle_machine_cycle_step_completes_one_fetch_cycle() {
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00, 0x00]);
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
fn cycle_teacher_retains_s100_status_latch_during_internal_cycle() {
    // DAD B performs its arithmetic through an internal machine cycle after the
    // opcode fetch. No new S-100 status byte is emitted for that internal work,
    // so the Display/Control status latch must retain the preceding M1 value.
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x09, 0x76]);
    let mut saw_internal = false;

    for _ in 0..16 {
        host.debugger_step_t_state();
        let snapshot = host.bus_teaching_snapshot().expect("Cycle teaching sample");
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
    let mut host = prepared(EmulationEngine::RustCycleAccurate8080, &[0x00, 0x76]);
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
