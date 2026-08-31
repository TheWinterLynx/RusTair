const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const ASR33_SOURCE: &str = include_str!("../src/app/ui/asr33_window.rs");
const TERMINAL_SOURCE: &str = include_str!("../src/app/ui/terminal.rs");
const TCP_SOURCE: &str = include_str!("../src/app/external_serial.rs");
const COM_SOURCE: &str = include_str!("../src/app/external_com.rs");

#[test]
fn engine_change_reapplies_physical_two_sio_straps() {
    assert!(
        APP_SOURCE.contains("configure_two_sio_straps(self.config.machine.two_sio_straps)"),
        "recreating Fast/Cycle must not silently restore the 88-2SIO default jumpers",
    );
}

#[test]
fn jumper_ui_is_explicitly_power_off_only() {
    assert!(
        APP_SOURCE.contains("Power OFF the Altair before moving the physical 88-2SIO address/baud straps"),
        "application guard must reject live jumper moves",
    );
    assert!(
        RUNTIME_SOURCE.contains("ui.add_enabled_ui(!powered"),
        "configuration UI must disable the physical jumper controls while powered",
    );
    assert!(
        RUNTIME_SOURCE.contains("these controls represent moving physical jumpers on the 88-2SIO board"),
        "UI must explain why POWER OFF is required",
    );
}

#[test]
fn ui_uses_physical_address_and_baud_straps_not_fixed_10h_labels() {
    assert!(RUNTIME_SOURCE.contains("straps.address.port0_status()"));
    assert!(RUNTIME_SOURCE.contains("straps.address.port1_data()"));
    assert!(RUNTIME_SOURCE.contains("straps.port0_baud.label()"));
    assert!(RUNTIME_SOURCE.contains("straps.port1_baud.label()"));
    assert!(RUNTIME_SOURCE.contains("(0u8..=0xf8).step_by(4)"));
    assert!(!RUNTIME_SOURCE.contains("88-2SIO Port 0 [10h/11h]"));
    assert!(!RUNTIME_SOURCE.contains("88-2SIO Port 1 [12h/13h]"));
}

#[test]
fn every_endpoint_label_receives_the_physical_straps() {
    for (name, source) in [
        ("ASR-33", ASR33_SOURCE),
        ("Text Terminal", TERMINAL_SOURCE),
        ("External TCP", TCP_SOURCE),
        ("External COM", COM_SOURCE),
    ] {
        assert!(
            source.contains("let straps = self.config.machine.two_sio_straps;")
                || source.contains("self.config.machine.two_sio_straps,\n                connection"),
            "{name} must source labels from the installed 88-2SIO straps",
        );
        assert!(
            source.contains("serial_connection_label(board, straps" )
                || source.contains("serial_connection_label(\n                self.config.machine.serial_board,\n                self.config.machine.two_sio_straps"),
            "{name} must not fall back to a two-argument/fixed-address serial label",
        );
    }
}

#[test]
fn ui_keeps_front_panel_ffh_out_of_the_address_selector() {
    assert!(RUNTIME_SOURCE.contains("FCh-FFh is intentionally unavailable"));
    assert!(RUNTIME_SOURCE.contains("FFh belongs to the Altair front-panel sense-switch input"));
}
