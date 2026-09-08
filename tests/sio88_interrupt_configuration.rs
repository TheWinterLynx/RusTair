use rustair::backend::BackendHost;
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, SioHardwareConfig,
    SioInterruptTarget, SioInterruptWiring,
};
use rustair::s100_chassis::S100ChassisConfig;

#[test]
fn adaptive_machine_preserves_irq_wiring_inside_the_installed_physical_sio_card() {
    let mut sio = SioHardwareConfig::default();
    sio.interrupt_wiring = SioInterruptWiring {
        input: SioInterruptTarget::Vi3,
        output: SioInterruptTarget::Disconnected,
    };

    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(5, Some(S100InstalledCardConfig::Mits88Sio(sio)))
        .unwrap();
    let hardware = hardware.validate().unwrap();

    let mut host = BackendHost::default();
    host.configure_s100_hardware(hardware, RamInit::Zeroed);
    assert_eq!(host.s100_hardware(), hardware);

    match host.s100_hardware().slot(5).expect("88-SIO remains in slot 5") {
        S100InstalledCardConfig::Mits88Sio(actual) => {
            assert_eq!(actual, sio);
            assert_eq!(actual.interrupt_wiring.input, SioInterruptTarget::Vi3);
            assert_eq!(
                actual.interrupt_wiring.output,
                SioInterruptTarget::Disconnected
            );
        }
        other => panic!("expected slot-native 88-SIO, got {other:?}"),
    }
}
