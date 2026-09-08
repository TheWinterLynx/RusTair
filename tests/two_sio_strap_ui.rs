use rustair::config::TwoSioAddressBlock;

const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const ASR33_SOURCE: &str = include_str!("../src/app/ui/asr33_window.rs");
const TERMINAL_SOURCE: &str = include_str!("../src/app/ui/terminal.rs");
const TCP_SOURCE: &str = include_str!("../src/app/external_serial.rs");
const COM_SOURCE: &str = include_str!("../src/app/external_com.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const TWO_SIO_CONFIG_SOURCE: &str = include_str!("../src/config/two_sio.rs");

fn function_body<'a>(source: &'a str, start: &str, next: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing function boundary {start}"));
    let tail = &source[start..];
    let end = tail
        .find(next)
        .unwrap_or_else(|| panic!("missing following function boundary {next}"));
    &tail[..end]
}

#[test]
fn two_sio_straps_are_power_off_slot_configuration_not_an_engine_setting() {
    assert!(S100_UI_SOURCE.contains("ui.add_enabled_ui(!powered"));
    assert!(!APP_SOURCE.contains("configure_two_sio_straps(self.config.machine.two_sio_straps)"));
    assert!(!APP_SOURCE.contains("fn apply_two_sio_straps"));

    let ui = function_body(
        S100_UI_SOURCE,
        "fn draw_two_sio_card_configuration",
        "fn replace_slot",
    );
    assert!(ui.contains("88-2SIO physical straps"));
    assert!(ui.contains("TwoSioBaudTap::ALL"));
    assert!(ui.contains("TwoSioSignalInterface::ALL"));
    assert!(ui.contains("straps.port0_baud"));
    assert!(ui.contains("straps.port1_baud"));
    assert!(ui.contains("straps.port0_interface"));
    assert!(ui.contains("straps.port1_interface"));
    assert!(ui.contains("(0u8..=0xf8).step_by(4)"));
    assert!(ui.contains("S100InstalledCardConfig::Mits88TwoSio"));
}

#[test]
fn ffh_front_panel_port_cannot_be_hidden_inside_a_two_sio_decode_block() {
    assert!(TwoSioAddressBlock::try_new(0xf8).is_some());
    assert!(TwoSioAddressBlock::try_new(0xfc).is_none());
    assert!(TWO_SIO_CONFIG_SOURCE.contains("Address FFh belongs to the Altair front-panel sense switches"));
    assert!(TWO_SIO_CONFIG_SOURCE.contains("base > 0xf8"));
}

#[test]
fn every_endpoint_label_is_derived_from_the_installed_s100_hardware() {
    for (name, source) in [
        ("ASR-33", ASR33_SOURCE),
        ("Text Terminal", TERMINAL_SOURCE),
        ("External TCP", TCP_SOURCE),
        ("External COM", COM_SOURCE),
    ] {
        assert!(
            source.contains("let hardware = self.config.machine.s100_hardware"),
            "{name} must read the physical S-100 inventory"
        );
        assert!(
            source.contains("serial_connection_label(hardware"),
            "{name} must label the actual installed slot/card/port"
        );
        assert!(
            !source.contains("self.config.machine.two_sio_straps"),
            "{name} must not read retired aggregate 88-2SIO straps"
        );
    }
}

#[test]
fn app_rejects_wrong_family_direct_cables_without_level_conversion() {
    assert!(APP_SOURCE.contains("S100InstalledCardConfig::Mits88TwoSio { straps, .. }"));
    assert!(APP_SOURCE.contains("device.supports_two_sio_interface(straps.port0_interface)"));
    assert!(APP_SOURCE.contains("device.supports_two_sio_interface(straps.port1_interface)"));
    assert!(APP_SOURCE.contains("two_sio_requirement_label"));
    assert!(APP_SOURCE.contains("no hidden level converter or phantom UART is inserted"));
    assert!(APP_SOURCE.contains("serial_set_receive_break_at(old_asr_connection, false)"));
    assert!(APP_SOURCE.contains("self.machine.configure_s100_hardware(hardware, self.config.machine.ram_init)"));
}

#[test]
fn persisted_cables_are_revalidated_against_each_installed_port_interface() {
    assert!(PERSISTENCE_SOURCE.contains("Some(S100InstalledCardConfig::Mits88TwoSio { straps, .. })"));
    assert!(PERSISTENCE_SOURCE.contains("device.supports_two_sio_interface(straps.port0_interface)"));
    assert!(PERSISTENCE_SOURCE.contains("device.supports_two_sio_interface(straps.port1_interface)"));
    assert!(PERSISTENCE_SOURCE.contains("valid_connection(hardware, device, connection)"));

    // Legacy per-port keys remain readable but are no longer a live authority.
    assert!(PERSISTENCE_SOURCE.contains("\"machine.two_sio_port0_interface\""));
    assert!(PERSISTENCE_SOURCE.contains("\"machine.two_sio_port1_interface\""));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_port0_interface="));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_port1_interface="));
}
