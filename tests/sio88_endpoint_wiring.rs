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
    assert!(APP.contains("!device.supports_sio_interface(self.config.machine.sio_hardware.interface)"));
    assert!(SERIAL_HARDWARE.contains("!device.supports_sio_interface(config.interface)"));
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
fn cable_labels_never_claim_the_old_fixed_00h_01h_address() {
    assert!(!APP.contains("88-SIO [00h/01h]"));
    assert!(APP.contains("88-SIO [configured I/O]"));
    assert!(APP.contains("sio.address.status()"));
    assert!(APP.contains("sio.address.data()"));
    assert!(APP.contains("sio.interface.label()"));
}
