const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const ASR33_SOURCE: &str = include_str!("../src/app/ui/asr33_window.rs");
const TERMINAL_SOURCE: &str = include_str!("../src/app/ui/terminal.rs");
const TCP_SOURCE: &str = include_str!("../src/app/external_serial.rs");
const COM_SOURCE: &str = include_str!("../src/app/external_com.rs");

#[test]
fn jumper_and_signal_wiring_are_power_off_slot_properties() {
    assert!(S100_UI_SOURCE.contains("POWER OFF required to move cards"));
    assert!(S100_UI_SOURCE.contains("88-2SIO physical straps"));
    assert!(S100_UI_SOURCE.contains("I/O block:"));
    assert!(S100_UI_SOURCE.contains("Port {port} baud tap:"));
    assert!(S100_UI_SOURCE.contains("Port {port} interface:"));
    assert!(S100_UI_SOURCE.contains("TwoSioSignalInterface::ALL"));
    assert!(S100_UI_SOURCE.contains("(0u8..=0xf8).step_by(4)"));
}

#[test]
fn endpoints_derive_labels_from_the_slot_native_hardware_inventory() {
    for (name, source) in [
        ("ASR-33", ASR33_SOURCE),
        ("Text Terminal", TERMINAL_SOURCE),
        ("External TCP", TCP_SOURCE),
        ("External COM", COM_SOURCE),
    ] {
        assert!(
            source.contains("s100_hardware"),
            "{name} must inspect the installed S-100 card inventory"
        );
        assert!(
            !source.contains("machine.serial_board") && !source.contains("machine.two_sio_straps"),
            "{name} must not use removed global serial-card state"
        );
    }
}

#[test]
fn app_has_no_global_two_sio_reconfiguration_helpers() {
    assert!(!APP_SOURCE.contains("fn apply_serial_board_configuration"));
    assert!(!APP_SOURCE.contains("fn apply_two_sio_straps"));
    assert!(!APP_SOURCE.contains("configure_two_sio_straps(self.config.machine"));
}
