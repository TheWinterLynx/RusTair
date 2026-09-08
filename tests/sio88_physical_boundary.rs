use rustair::backend::CycleAccurateMachineBackend;
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, SioElectricalLevel,
    SioHardwareConfig, SioInterface, SioInterruptTarget, SioInterruptWiring, SioRevision,
};
use rustair::s100_chassis::S100ChassisConfig;

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

fn cycle_with_sio(config: SioHardwareConfig) -> CycleAccurateMachineBackend {
    let mut hardware =
        S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(2, Some(S100InstalledCardConfig::Mits88Sio(config)))
        .unwrap();

    let mut cycle = CycleAccurateMachineBackend::default();
    cycle
        .configure_s100_hardware(hardware.validate().unwrap(), RamInit::Zeroed)
        .unwrap();
    cycle
}

#[test]
fn adaptive_cycle_exposes_rev0_wiring_and_idle_six_line_state() {
    let config = configured(SioInterface::TtyC);
    let cycle = cycle_with_sio(config);

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
fn abc_electrical_variants_preserve_board_side_logical_semantics() {
    let mut reference_lines = None;

    for interface in [SioInterface::Rs232A, SioInterface::TtlB, SioInterface::TtyC] {
        let config = configured(interface);
        let mut cycle = cycle_with_sio(config);
        assert!(cycle.machine_mut().bus.pulse_sio_input_device_ready());
        assert!(cycle.machine_mut().bus.pulse_sio_output_device_ready());

        let lines = cycle.machine().bus.sio_logical_lines();
        if let Some(reference) = reference_lines {
            assert_eq!(
                lines, reference,
                "A/B/C electrical families must not change board-side logical line semantics"
            );
        } else {
            reference_lines = Some(lines);
        }
        assert!(
            cycle.machine().bus.sio_connector_outputs().is_some(),
            "the selected physical interface must project connector outputs"
        );
    }
}

#[test]
fn connector_input_family_mismatch_is_rejected_by_adaptive_cycle() {
    let config = configured(SioInterface::Rs232A);
    let cycle = cycle_with_sio(config);

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
