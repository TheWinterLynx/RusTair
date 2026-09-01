const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const SERIAL_HARDWARE_SOURCE: &str = include_str!("../src/app/serial_hardware.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const SIO_CONFIG_SOURCE: &str = include_str!("../src/config/sio.rs");

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
    assert!(RUNTIME_SOURCE.contains("Input IRQ source:"));
    assert!(RUNTIME_SOURCE.contains("Output IRQ source:"));
    assert!(SERIAL_HARDWARE_SOURCE.contains("Power OFF the Altair before changing 88-SIO hardware wiring"));
}

#[test]
fn sio_ui_uses_documented_baud_table_not_terminal_speed_as_board_clock() {
    assert!(RUNTIME_SOURCE.contains("crate::config::SioBaudRate::STANDARD"));
    assert!(RUNTIME_SOURCE.contains("The MITS baud chart provides 110, 150, 300, 600, 1200, 2400, 4800, 9600 and 19200 baud presets."));
}

#[test]
fn sio_interrupt_ui_keeps_runtime_enables_separate_from_physical_routing() {
    assert!(RUNTIME_SOURCE.contains("crate::config::SioInterruptTarget::ALL"));
    assert!(RUNTIME_SOURCE.contains("D0 enables the input interrupt source and D1 enables the output source at runtime"));
    assert!(RUNTIME_SOURCE.contains("Selecting the same destination for both sources represents the equivalent combined BH wiring result."));
    assert!(RUNTIME_SOURCE.contains("VI0..VI7 are raw requests for a separate 88-Vector Interrupt system"));
    assert!(RUNTIME_SOURCE.contains("Rev 0 interrupt assertion depends on the original external input/output device-ready flip-flops"));
}

#[test]
fn sio_hardware_is_persisted_as_one_atomic_card_configuration() {
    assert!(PERSISTENCE_SOURCE.contains("const CONFIG_VERSION: u32 = 4;"));
    assert!(PERSISTENCE_SOURCE.contains("machine.sio_hardware"));
    assert!(PERSISTENCE_SOURCE.contains("SioHardwareConfig::from_persistence_key"));
    assert!(PERSISTENCE_SOURCE.contains("self.machine.configure_sio_hardware(self.config.machine.sio_hardware);"));
    assert!(SIO_CONFIG_SOURCE.contains("pub interrupt_wiring: SioInterruptWiring"));
    assert!(SIO_CONFIG_SOURCE.contains("fields.len() != 7 && fields.len() != 9"));
    assert!(SIO_CONFIG_SOURCE.contains("SioInterruptWiring::default()"));
}

#[test]
fn engine_recreation_reapplies_physical_sio_hardware() {
    let start = APP_SOURCE
        .find("fn select_emulation_engine")
        .expect("app must own the engine-recreation boundary");
    let tail = &APP_SOURCE[start..];
    let end = tail
        .find("fn apply_memory_configuration")
        .expect("helper after engine-recreation boundary");
    let function = &tail[..end];

    assert!(function.contains("self.machine.replace_engine(engine)"));
    assert!(function.contains("self.machine.configure_sio_hardware"));
    assert!(function.contains("self.config.machine.sio_hardware"));
    assert!(function.contains("self.machine.configure_serial_board"));
}

#[test]
fn selecting_sio_reapplies_its_dormant_physical_configuration() {
    let start = APP_SOURCE
        .find("fn apply_serial_board_configuration")
        .expect("app must own serial-board selection");
    let tail = &APP_SOURCE[start..];
    let end = tail
        .find("fn apply_two_sio_straps")
        .expect("helper after serial-board selection");
    let function = &tail[..end];

    assert!(function.contains("SerialBoard::Sio88"));
    assert!(function.contains("self.machine.configure_sio_hardware"));
    assert!(function.contains("self.config.machine.sio_hardware"));
    assert!(function.contains("address.status()"));
    assert!(function.contains("address.data()"));
}
