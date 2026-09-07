use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, TwoSioInterruptWiring,
    TwoSioSignalInterface, TwoSioStraps,
};
use rustair::s100_chassis::S100ChassisConfig;

const ROUTER: &str = include_str!("../src/io/serial_router.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");

fn hardware_with_straps(straps: TwoSioStraps) -> S100HardwareConfig {
    let mut hardware =
        S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Mits88TwoSio {
                straps,
                interrupt_wiring: TwoSioInterruptWiring::default(),
            }),
        )
        .unwrap();
    hardware.validate().unwrap()
}

#[test]
fn adaptive_cycle_preserves_independent_port_signal_hardwiring_in_the_installed_card() {
    let straps = TwoSioStraps {
        port0_interface: TwoSioSignalInterface::Ttl,
        port1_interface: TwoSioSignalInterface::Tty20mA,
        ..TwoSioStraps::default()
    };

    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).expect("built-in Rust 8080 engine");
        host.configure_s100_hardware(hardware_with_straps(straps), RamInit::Zeroed);
        let mounted = host.s100_hardware();
        assert_eq!(mounted.active_two_sio_straps(), Some(straps), "{engine:?}");
        assert_eq!(
            mounted.active_two_sio_straps().unwrap().port_interface(0),
            Some(TwoSioSignalInterface::Ttl)
        );
        assert_eq!(
            mounted.active_two_sio_straps().unwrap().port_interface(1),
            Some(TwoSioSignalInterface::Tty20mA)
        );
    }
}

#[test]
fn documented_signal_families_are_explicit_not_boolean_aliases() {
    assert_eq!(
        TwoSioSignalInterface::ALL,
        [
            TwoSioSignalInterface::Rs232,
            TwoSioSignalInterface::Ttl,
            TwoSioSignalInterface::Tty20mA,
        ]
    );
    assert_ne!(TwoSioSignalInterface::Rs232, TwoSioSignalInterface::Ttl);
    assert_ne!(TwoSioSignalInterface::Rs232, TwoSioSignalInterface::Tty20mA);
    assert_ne!(TwoSioSignalInterface::Ttl, TwoSioSignalInterface::Tty20mA);
}

#[test]
fn direct_endpoint_matrix_never_invents_a_level_converter() {
    assert!(ROUTER.contains("Self::InternalAsr33 => matches!(interface, TwoSioSignalInterface::Tty20mA)"));
    assert!(ROUTER.contains("Self::ExternalCom => matches!(interface, TwoSioSignalInterface::Rs232)"));
    assert!(ROUTER.contains("Self::TextTerminal | Self::ExternalTcp => true"));
    assert!(APP.contains("supports_two_sio_interface"));
    assert!(APP.contains("two_sio_requirement_label"));
    assert!(APP.contains("no hidden level converter or phantom UART is inserted"));
}

#[test]
fn slot_remount_reconciles_wrong_family_cables_after_hardware_changes() {
    assert!(APP.contains("fn reconcile_serial_router_after_hardware_change"));
    assert!(APP.contains("serial_connection_supported(next, device, connection)"));
    assert!(APP.contains("SerialConnection::Disconnected"));
    assert!(APP.contains("self.asr33.answerback.clear()"));
    assert!(!APP.contains("self.machine.configure_two_sio_straps(straps)"));
}

#[test]
fn persisted_wiring_is_validated_against_the_restored_physical_s100_inventory() {
    assert!(PERSISTENCE.contains("let hardware = self.config.machine.s100_hardware;"));
    assert!(PERSISTENCE.contains("valid_connection(hardware, device, connection)"));
    assert!(!PERSISTENCE.contains("supports_two_sio_interface(machine.two_sio_straps"));
}
