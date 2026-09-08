use rustair::backend::BackendHost;
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, TwoSioInterruptWiring,
    TwoSioSignalInterface, TwoSioStraps,
};
use rustair::s100_chassis::S100ChassisConfig;

const ROUTER: &str = include_str!("../src/io/serial_router.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");

fn hardware_with_two_sio(straps: TwoSioStraps) -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            5,
            Some(S100InstalledCardConfig::Mits88TwoSio {
                straps,
                interrupt_wiring: TwoSioInterruptWiring::default(),
            }),
        )
        .unwrap();
    hardware.validate().unwrap()
}

#[test]
fn adaptive_machine_preserves_independent_port_signal_hardwiring_from_the_s100_card() {
    let straps = TwoSioStraps {
        port0_interface: TwoSioSignalInterface::Ttl,
        port1_interface: TwoSioSignalInterface::Tty20mA,
        ..TwoSioStraps::default()
    };
    let hardware = hardware_with_two_sio(straps);

    let mut host = BackendHost::default();
    host.configure_s100_hardware(hardware, RamInit::Zeroed);
    assert_eq!(host.s100_hardware(), hardware);

    let installed = host.s100_hardware().slot(5).expect("88-2SIO remains in slot 5");
    match installed {
        S100InstalledCardConfig::Mits88TwoSio { straps: actual, .. } => {
            assert_eq!(actual, straps);
            assert_eq!(actual.port_interface(0), Some(TwoSioSignalInterface::Ttl));
            assert_eq!(actual.port_interface(1), Some(TwoSioSignalInterface::Tty20mA));
        }
        other => panic!("expected slot-native 88-2SIO, got {other:?}"),
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
    assert!(APP.contains("device.supports_two_sio_interface(straps.port0_interface)"));
    assert!(APP.contains("device.supports_two_sio_interface(straps.port1_interface)"));
    assert!(APP.contains("no hidden level converter or phantom UART is inserted"));
}

#[test]
fn s100_remount_releases_the_old_asr_wire_before_reconciling_cables() {
    let release = APP
        .find("serial_set_receive_break_at(old_asr_connection, false)")
        .expect("old ASR receive wire must return to MARK before a remount");
    let configure = APP
        .find("self.machine\n            .configure_s100_hardware(hardware, self.config.machine.ram_init)")
        .or_else(|| APP.find("self.machine.configure_s100_hardware(hardware, self.config.machine.ram_init)"))
        .expect("app must remount complete S-100 hardware");
    assert!(release < configure);
    assert!(APP.contains("reconcile_serial_router_after_hardware_change(previous, hardware)"));
}

#[test]
fn persisted_wiring_is_validated_against_the_installed_two_sio_card() {
    assert!(PERSISTENCE.contains("Some(S100InstalledCardConfig::Mits88TwoSio { straps, .. })"));
    assert!(PERSISTENCE.contains("device.supports_two_sio_interface(straps.port0_interface)"));
    assert!(PERSISTENCE.contains("device.supports_two_sio_interface(straps.port1_interface)"));
    assert!(PERSISTENCE.contains("valid_connection(hardware, device, connection)"));
    assert!(!PERSISTENCE.contains("self.machine.configure_two_sio_straps("));
}
