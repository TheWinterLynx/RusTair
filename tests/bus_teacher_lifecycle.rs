#[test]
fn lifecycle_states_are_explicitly_not_t_state_samples() {
    let contract = include_str!("../src/backend/bus_teaching.rs");
    assert!(contract.contains("ControlState"));
    for state in [
        "PowerOff",
        "PowerOnUndefined",
        "ResetAsserted",
        "ResetReleasedStopped",
        "ResetReleasedRunning",
    ] {
        assert!(contract.contains(state), "missing Bus Teacher lifecycle state {state}");
    }
    assert!(contract.contains("CONTROL STATE / NO T-STATE SAMPLE"));
}

#[test]
fn cycle_control_snapshot_does_not_advance_the_cpu() {
    let host = include_str!("../src/backend/cycle_host.rs");
    let start = host.find("fn control_teaching_snapshot").expect("control snapshot helper");
    let end = host[start..]
        .find("fn debugger_step_one_t_state")
        .map(|offset| start + offset)
        .expect("next helper boundary");
    let helper = &host[start..end];
    assert!(!helper.contains(".tick("), "POWER/RESET teaching must not fabricate a CPU T-state");
    assert!(!helper.contains("debugger_step"), "reading teaching state must be side-effect free");
    assert!(helper.contains("cpu_control_lines"));
    assert!(helper.contains("panel_lamps"));
}

#[test]
fn package_keeps_control_bus_and_cpu_bus_semantics_separate() {
    let diagram = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(diagram.contains("cpu_bus_pin_levels_available"));
    assert!(diagram.contains("snapshot.accuracy == BusTeachingAccuracy::Exact"));
    assert!(diagram.contains("snapshot.accuracy == BusTeachingAccuracy::ControlState"));
    assert!(diagram.contains("snapshot.machine_cycle == BusMachineCycle::ResetReleasedStopped"));
    assert!(diagram.contains("S-100/front-panel observations"));
    assert!(diagram.contains("CLOCK PRESENT - phase not modeled"));
}

#[test]
fn reset_lifecycle_is_explained_in_the_teacher_ui() {
    let ui = include_str!("../src/app/ui/bus_teacher.rs");
    assert!(ui.contains("RESET HIGH - CPU reset state forced"));
    assert!(ui.contains("RESET released - PC=$0000"));
    assert!(ui.contains("RESET is physically asserted: execution controls remain disabled"));
}
