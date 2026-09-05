use rustair::backend::CycleAccurateMachineBackend;
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
fn adaptive_cycle_exposes_rev0_wiring_and_idle_six_line_state() {
    let config = configured(SioInterface::TtyC);
    let mut cycle = CycleAccurateMachineBackend::default();
    cycle.machine_mut().configure_sio_hardware(config);

    let expected_wiring = Some((
        SioRevision::Rev0,
        SioInterruptTarget::Vi2,
        SioInterruptTarget::Pint,
    ));
    let expected_lines = Some((true, false, false, true, false, false));

    assert_eq!(cycle.machine().bus.sio_physical_wiring(), expected_wiring);
    assert_eq!(cycle.machine().bus.sio_logical_lines(), expected_lines);
}

#[test]
fn adaptive_cycle_projects_the_selected_abc_electrical_levels() {
    for interface in [SioInterface::Rs232A, SioInterface::TtlB, SioInterface::TtyC] {
        let config = configured(interface);
        let mut cycle = CycleAccurateMachineBackend::default();
        cycle.machine_mut().configure_sio_hardware(config);
        assert!(cycle.machine_mut().bus.pulse_sio_input_device_ready());
        assert!(cycle.machine_mut().bus.pulse_sio_output_device_ready());

        let lines = cycle.machine().bus.sio_logical_lines();
        assert_eq!(
            lines,
            Some((true, true, true, true, true, true)),
            "A/B/C must never change board-side logical line semantics"
        );
        assert!(
            cycle.machine().bus.sio_connector_outputs().is_some(),
            "the selected physical interface must project connector outputs"
        );
    }
}

#[test]
fn connector_input_family_mismatch_is_rejected_by_adaptive_cycle() {
    let config = configured(SioInterface::Rs232A);
    let mut cycle = CycleAccurateMachineBackend::default();
    cycle.machine_mut().configure_sio_hardware(config);

    assert_eq!(
        cycle
            .machine()
            .bus
            .sio_decode_connector_input(SioElectricalLevel::Rs232Negative),
        Some(true)
    );
    assert_eq!(
        cycle
            .machine()
            .bus
            .sio_decode_connector_input(SioElectricalLevel::TtlHigh),
        None
    );
}
