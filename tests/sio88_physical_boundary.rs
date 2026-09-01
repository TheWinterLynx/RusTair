use rustair::backend::{CycleAccurateMachineBackend, NativeMachineBackend};
use rustair::config::{
    SioElectricalLevel, SioHardwareConfig, SioInterface, SioInterruptTarget,
    SioInterruptWiring, SioRevision,
};

fn configured(interface: SioInterface) -> SioHardwareConfig {
    SioHardwareConfig {
        revision: SioRevision::Rev0,
        interface,
        interrupt_wiring: SioInterruptWiring {
            input: SioInterruptTarget::Vi2,
            output: SioInterruptTarget::Pint,
        },
        ..SioHardwareConfig::default()
    }
}

#[test]
fn fast_and_cycle_expose_identical_rev0_wiring_and_idle_six_line_state() {
    let config = configured(SioInterface::TtyC);

    let mut fast = NativeMachineBackend::default();
    fast.machine_mut().configure_sio_hardware(config);

    let mut cycle = CycleAccurateMachineBackend::default();
    cycle.machine_mut().configure_sio_hardware(config);

    let expected_wiring = Some((
        SioRevision::Rev0,
        SioInterruptTarget::Vi2,
        SioInterruptTarget::Pint,
    ));
    let expected_lines = Some((true, false, false, true, false, false));

    assert_eq!(fast.machine().bus.sio_physical_wiring(), expected_wiring);
    assert_eq!(cycle.machine().bus.sio_physical_wiring(), expected_wiring);
    assert_eq!(fast.machine().bus.sio_logical_lines(), expected_lines);
    assert_eq!(cycle.machine().bus.sio_logical_lines(), expected_lines);
}

#[test]
fn fast_and_cycle_project_the_same_abc_electrical_levels() {
    for interface in [SioInterface::Rs232A, SioInterface::TtlB, SioInterface::TtyC] {
        let config = configured(interface);

        let mut fast = NativeMachineBackend::default();
        fast.machine_mut().configure_sio_hardware(config);
        assert!(fast.machine_mut().bus.pulse_sio_input_device_ready());
        assert!(fast.machine_mut().bus.pulse_sio_output_device_ready());

        let mut cycle = CycleAccurateMachineBackend::default();
        cycle.machine_mut().configure_sio_hardware(config);
        assert!(cycle.machine_mut().bus.pulse_sio_input_device_ready());
        assert!(cycle.machine_mut().bus.pulse_sio_output_device_ready());

        assert_eq!(
            fast.machine().bus.sio_logical_lines(),
            cycle.machine().bus.sio_logical_lines(),
            "A/B/C must never change board-side logical line semantics"
        );
        assert_eq!(
            fast.machine().bus.sio_connector_outputs(),
            cycle.machine().bus.sio_connector_outputs(),
            "Fast and Cycle must project the same selected physical interface"
        );
    }
}

#[test]
fn connector_input_family_mismatch_is_rejected_by_both_engines() {
    let config = configured(SioInterface::Rs232A);

    let mut fast = NativeMachineBackend::default();
    fast.machine_mut().configure_sio_hardware(config);
    let mut cycle = CycleAccurateMachineBackend::default();
    cycle.machine_mut().configure_sio_hardware(config);

    assert_eq!(
        fast.machine().bus.sio_decode_connector_input(SioElectricalLevel::Rs232Negative),
        Some(true)
    );
    assert_eq!(
        cycle.machine().bus.sio_decode_connector_input(SioElectricalLevel::Rs232Negative),
        Some(true)
    );
    assert_eq!(
        fast.machine().bus.sio_decode_connector_input(SioElectricalLevel::TtlHigh),
        None
    );
    assert_eq!(
        cycle.machine().bus.sio_decode_connector_input(SioElectricalLevel::TtlHigh),
        None
    );
}
