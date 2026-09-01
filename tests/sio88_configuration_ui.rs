const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const SERIAL_HARDWARE_SOURCE: &str = include_str!("../src/app/serial_hardware.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");

#[test]
fn sio_hardware_controls_are_power_off_only_and_physically_named() {
    assert!(RUNTIME_SOURCE.contains("Physical 88-SIO configuration:"));
    assert!(RUNTIME_SOURCE.contains("ui.add_enabled_ui(!powered"));
    assert!(RUNTIME_SOURCE.contains("Logic revision:"));
    assert!(RUNTIME_SOURCE.contains("Line interface:"));
    assert!(RUNTIME_SOURCE.contains("I/O address:"));
    assert!(RUNTIME_SOURCE.contains("Baud preset:"));
    assert!(RUNTIME_SOURCE.contains("Data bits:"));
    assert!(RUNTIME_SOURCE.contains("Parity:"));
    assert!(RUNTIME_SOURCE.contains("Stop bits:"));
    assert!(SERIAL_HARDWARE_SOURCE.contains("Power OFF the Altair before changing 88-SIO hardware wiring"));
}

#[test]
fn sio_ui_uses_documented_baud_table_not_terminal_speed_as_board_clock() {
    assert!(RUNTIME_SOURCE.contains("crate::config::SioBaudRate::STANDARD"));
    assert!(RUNTIME_SOURCE.contains("The MITS baud chart provides 110, 150, 300, 600, 1200, 2400, 4800, 9600 and 19200 baud presets."));
}

#[test]
fn sio_hardware_is_persisted_as_one_atomic_card_configuration() {
    assert!(PERSISTENCE_SOURCE.contains("const CONFIG_VERSION: u32 = 4;"));
    assert!(PERSISTENCE_SOURCE.contains("machine.sio_hardware"));
    assert!(PERSISTENCE_SOURCE.contains("SioHardwareConfig::from_persistence_key"));
    assert!(PERSISTENCE_SOURCE.contains("self.machine.configure_sio_hardware(self.config.machine.sio_hardware);"));
}
