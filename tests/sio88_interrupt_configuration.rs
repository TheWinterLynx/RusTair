use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, SioHardwareConfig, SioInterruptTarget,
    SioInterruptWiring,
};
use rustair::s100_chassis::S100ChassisConfig;

fn hardware_with_sio(config: SioHardwareConfig) -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(2, Some(S100InstalledCardConfig::Mits88Sio(config)))
        .unwrap();
    hardware.validate().unwrap()
}

#[test]
fn adaptive_cycle_preserves_irq_wiring_inside_the_installed_s100_sio_card() {
    let mut config = SioHardwareConfig::default();
    config.interrupt_wiring = SioInterruptWiring {
        input: SioInterruptTarget::Vi3,
        output: SioInterruptTarget::Disconnected,
    };

    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).unwrap();
        host.configure_s100_hardware(hardware_with_sio(config), RamInit::Zeroed);

        let mounted = host.s100_hardware();
        assert_eq!(
            mounted.active_serial_card_slot().map(|(slot, _)| slot),
            Some(2)
        );
        assert_eq!(mounted.active_sio_hardware(), Some(config));
        assert_eq!(
            mounted
                .active_sio_hardware()
                .unwrap()
                .interrupt_wiring
                .input,
            SioInterruptTarget::Vi3
        );
        assert_eq!(
            mounted
                .active_sio_hardware()
                .unwrap()
                .interrupt_wiring
                .output,
            SioInterruptTarget::Disconnected
        );
    }
}
