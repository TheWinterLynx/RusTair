use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{
    SerialBoard, TwoSioSignalInterface, TwoSioStraps,
};

const ROUTER: &str = include_str!("../src/io/serial_router.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");

#[test]
fn both_backends_preserve_independent_port_signal_hardwiring() {
    let straps = TwoSioStraps {
        port0_interface: TwoSioSignalInterface::Ttl,
        port1_interface: TwoSioSignalInterface::Tty20mA,
        ..TwoSioStraps::default()
    };

    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).expect("built-in Rust 8080 engine");
        host.configure_serial_board(SerialBoard::TwoSio88);
        host.configure_two_sio_straps(straps);
        assert_eq!(host.two_sio_straps(), straps, "{engine:?}");
        assert_eq!(host.two_sio_straps().port_interface(0), Some(TwoSioSignalInterface::Ttl));
        assert_eq!(host.two_sio_straps().port_interface(1), Some(TwoSioSignalInterface::Tty20mA));
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
    assert!(APP.contains("no hidden level converter is installed"));
}

#[test]
fn rewiring_disconnects_wrong_family_physical_cables_and_releases_asr_break_first() {
    assert!(APP.contains("disconnected incompatible cable(s)"));
    assert!(APP.contains("serial_set_receive_break_at(old_asr_connection, false)"));
    let release = APP.find("serial_set_receive_break_at(old_asr_connection, false)").unwrap();
    let configure = APP.find("self.machine.configure_two_sio_straps(straps)").unwrap();
    assert!(release < configure, "old ASR wire must return to MARK before the board/cable rewire");
}

#[test]
fn persisted_wiring_cannot_bypass_the_same_endpoint_matrix() {
    assert!(PERSISTENCE.contains("machine.two_sio_port0_interface"));
    assert!(PERSISTENCE.contains("machine.two_sio_port1_interface"));
    assert!(PERSISTENCE.contains("supports_two_sio_interface(machine.two_sio_straps.port0_interface)"));
    assert!(PERSISTENCE.contains("supports_two_sio_interface(machine.two_sio_straps.port1_interface)"));
}
