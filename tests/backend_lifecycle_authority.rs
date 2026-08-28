use rustair::backend::{
    BackendHost, BusMachineCycle, BusTeachingAccuracy, EmulationEngine,
};

fn cycle_host() -> BackendHost {
    BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("cycle backend must be built in")
}

#[test]
fn cycle_power_on_inte_has_one_cpu_and_s100_truth() {
    let mut host = cycle_host();
    host.power(true);

    let cpu = host.intel8080_state();
    let teaching = host
        .bus_teaching_snapshot()
        .expect("power-on control snapshot");

    assert_eq!(teaching.accuracy, BusTeachingAccuracy::ControlState);
    assert_eq!(teaching.machine_cycle, BusMachineCycle::PowerOnUndefined);
    assert_eq!(
        teaching.status.inte,
        Some(cpu.inte),
        "the undefined power-on INTE sample must be identical in the cycle core and canonical S-100 state"
    );
    assert_eq!(
        teaching.pins.inte, None,
        "POWER ON remains an undefined lifecycle state; matching internal/S-100 state must not fabricate a sampled CPU pin"
    );

    host.assert_front_panel_reset();
    let reset_cpu = host.intel8080_state();
    let held = host
        .bus_teaching_snapshot()
        .expect("RESET control snapshot");
    assert!(!reset_cpu.inte, "8080 RESET must disable interrupts");
    assert_eq!(held.machine_cycle, BusMachineCycle::ResetAsserted);
    assert_eq!(held.status.inte, Some(false));
    assert_eq!(held.pins.inte, Some(false));

    host.release_front_panel_reset();
    let released = host
        .bus_teaching_snapshot()
        .expect("released RESET control snapshot");
    assert_eq!(released.machine_cycle, BusMachineCycle::ResetReleasedStopped);
    assert_eq!(released.status.inte, Some(false));
    assert_eq!(released.pins.inte, Some(false));
}

#[test]
fn cycle_run_latch_drives_ready_wait_without_changing_reset_semantics() {
    let mut host = cycle_host();
    host.power(true);
    host.assert_front_panel_reset();
    host.release_front_panel_reset();

    let stopped_panel = host.front_panel_state();
    let stopped = host
        .bus_teaching_snapshot()
        .expect("STOP-WAIT control snapshot");
    assert!(!stopped_panel.running);
    assert_eq!(stopped.machine_cycle, BusMachineCycle::ResetReleasedStopped);
    assert_eq!(stopped.reset, Some(false));
    assert_eq!(stopped.ready, Some(false));
    assert_eq!(stopped.status.wait, Some(true));
    assert_eq!(stopped.pins.wait, Some(true));

    // RUN is the physical R-S latch. With RESET released it releases READY and
    // the stable STOP-WAIT condition disappears immediately.
    host.assert_run_stop(true);
    let running_panel = host.front_panel_state();
    let running = host
        .bus_teaching_snapshot()
        .expect("RUN control snapshot before first exact T-state");
    assert!(running_panel.running);
    assert_eq!(running.machine_cycle, BusMachineCycle::ResetReleasedRunning);
    assert_eq!(running.reset, Some(false));
    assert_eq!(running.ready, Some(true));
    assert_eq!(running.status.wait, Some(false));
    host.release_run_stop(true);

    // Physical RESET does not clear the independent RUN/STOP latch. While RESET
    // is held the CPU cannot run, so READY and WAIT are both low. Releasing RESET
    // restores READY from the still-set RUN latch and execution resumes at 0000h.
    host.assert_front_panel_reset();
    let held_panel = host.front_panel_state();
    let held = host
        .bus_teaching_snapshot()
        .expect("RESET-held control snapshot while RUN remains latched");
    assert!(held_panel.running, "RESET must preserve the RUN/STOP latch");
    assert_eq!(held.machine_cycle, BusMachineCycle::ResetAsserted);
    assert_eq!(held.reset, Some(true));
    assert_eq!(held.ready, Some(false));
    assert_eq!(held.status.wait, Some(false));

    host.release_front_panel_reset();
    let resumed_panel = host.front_panel_state();
    let resumed_cpu = host.intel8080_state();
    let resumed = host
        .bus_teaching_snapshot()
        .expect("RUN-after-RESET control snapshot");
    assert!(resumed_panel.running);
    assert_eq!(resumed_cpu.pc, 0x0000);
    assert_eq!(resumed.machine_cycle, BusMachineCycle::ResetReleasedRunning);
    assert_eq!(resumed.reset, Some(false));
    assert_eq!(resumed.ready, Some(true));
    assert_eq!(resumed.status.wait, Some(false));
}

#[test]
fn cycle_power_off_removes_all_lifecycle_bus_truth() {
    let mut host = cycle_host();
    host.power(true);
    host.assert_front_panel_reset();
    host.release_front_panel_reset();
    host.power(false);

    let panel = host.front_panel_state();
    let teaching = host
        .bus_teaching_snapshot()
        .expect("POWER OFF control snapshot");

    assert!(!panel.powered);
    assert!(!panel.running);
    assert_eq!(teaching.accuracy, BusTeachingAccuracy::ControlState);
    assert_eq!(teaching.machine_cycle, BusMachineCycle::PowerOff);
    assert_eq!(teaching.address, None);
    assert_eq!(teaching.data, None);
    assert_eq!(teaching.ready, None);
    assert_eq!(teaching.hold, None);
    assert_eq!(teaching.reset, None);
    assert_eq!(teaching.status.inte, None);
    assert_eq!(teaching.status.wait, None);
}
