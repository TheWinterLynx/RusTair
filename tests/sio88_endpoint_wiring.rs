const ROUTER: &str = include_str!("../src/io/serial_router.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const SERIAL_HARDWARE: &str = include_str!("../src/app/serial_hardware.rs");
const ASR33: &str = include_str!("../src/app/asr33_controller.rs");
const TERMINAL: &str = include_str!("../src/app/terminal_serial.rs");
const TCP: &str = include_str!("../src/app/external_serial.rs");
const COM: &str = include_str!("../src/app/external_com.rs");

#[test]
fn physical_endpoints_do_not_gain_hidden_abc_level_converters() {
    assert!(ROUTER.contains("Self::InternalAsr33 => matches!(interface, SioInterface::TtyC)"));
    assert!(ROUTER.contains("Self::ExternalCom => matches!(interface, SioInterface::Rs232A)"));
    assert!(ROUTER.contains("Self::TextTerminal | Self::ExternalTcp => true"));
    assert!(APP.contains("device.supports_sio_interface(config.interface)"));
    assert!(SERIAL_HARDWARE.contains("serial_set_receive_break"));
    assert!(!APP.contains("machine.sio_hardware"));
}

#[test]
fn rev0_ready_pulses_are_not_fabricated_by_byte_oriented_endpoints() {
    for source in [ASR33, TERMINAL, TCP, COM] {
        assert!(!source.contains("sio_pulse_input_device_ready"));
        assert!(!source.contains("sio_pulse_output_device_ready"));
        assert!(!source.contains("pulse_sio_input_device_ready"));
        assert!(!source.contains("pulse_sio_output_device_ready"));
    }
}

#[test]
fn cable_labels_are_derived_from_the_installed_sio_card_and_real_address_pair() {
    assert!(APP.contains("active_serial_card_slot()"));
    assert!(APP.contains("S100InstalledCardConfig::Mits88Sio(config)"));
    assert!(APP.contains("config.address.status()"));
    assert!(APP.contains("config.address.data()"));
    assert!(APP.contains("config.interface.label()"));
    assert!(APP.contains("Slot {slot} · 88-SIO"));
    assert!(!APP.contains("88-SIO [00h/01h]"));
}
