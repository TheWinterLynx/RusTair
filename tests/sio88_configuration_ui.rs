const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const SIO_CONFIG_SOURCE: &str = include_str!("../src/config/sio.rs");
const S100_CONFIG_SOURCE: &str = include_str!("../src/config/s100_hardware.rs");
const IO_DEVICES_SOURCE: &str = include_str!("../src/machine/io_devices.rs");

fn compact(source: &str) -> String {
    source.split_whitespace().collect()
}

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
fn sio_hardware_controls_are_power_off_only_and_physically_named() {
    assert!(S100_UI_SOURCE.contains("ui.add_enabled_ui(!powered"));
    let sio_ui = function_body(
        S100_UI_SOURCE,
        "fn draw_sio_card_configuration",
        "fn draw_two_sio_card_configuration",
    );

    for label in [
        "88-SIO physical configuration",
        "Revision:",
        "Interface:",
        "I/O address:",
        "Baud:",
        "Data bits:",
        "Parity:",
        "Stop bits:",
        "Input IRQ:",
        "Output IRQ:",
    ] {
        assert!(sio_ui.contains(label), "missing physical 88-SIO control: {label}");
    }

    assert!(sio_ui.contains("S100InstalledCardConfig::Mits88Sio(next)"));
    assert!(!APP_SOURCE.contains("fn apply_sio_hardware"));
}

#[test]
fn sio_ui_uses_documented_baud_table_not_terminal_speed_as_board_clock() {
    let sio_ui = function_body(
        S100_UI_SOURCE,
        "fn draw_sio_card_configuration",
        "fn draw_two_sio_card_configuration",
    );
    assert!(compact(sio_ui).contains("forbaudinSioBaudRate::STANDARD"));
    assert!(!sio_ui.contains("TerminalSpeed"));

    assert!(SIO_CONFIG_SOURCE.contains("pub const STANDARD: [Self; 9]"));
    assert!(SIO_CONFIG_SOURCE.contains("Self(110), Self(150), Self(300), Self(600), Self(1_200), Self(2_400)"));
    assert!(SIO_CONFIG_SOURCE.contains("Self(4_800), Self(9_600), Self(19_200)"));
    assert!(SIO_CONFIG_SOURCE.contains("pub const MAX: u32 = 25_000;"));
}

#[test]
fn sio_interrupt_ui_keeps_runtime_enables_separate_from_physical_routing() {
    let sio_ui = function_body(
        S100_UI_SOURCE,
        "fn draw_sio_card_configuration",
        "fn draw_two_sio_card_configuration",
    );
    assert!(compact(sio_ui).contains("fortargetinSioInterruptTarget::ALL"));
    assert!(sio_ui.contains("next.interrupt_wiring.input = target"));
    assert!(sio_ui.contains("next.interrupt_wiring.output = target"));

    assert!(SIO_CONFIG_SOURCE.contains("pub struct SioInterruptWiring"));
    assert!(SIO_CONFIG_SOURCE.contains("pub input: SioInterruptTarget"));
    assert!(SIO_CONFIG_SOURCE.contains("pub output: SioInterruptTarget"));
    assert!(SIO_CONFIG_SOURCE.contains("pub const fn vector_level(self) -> Option<u8>"));

    // Runtime D0/D1 enables remain separate from the physical destination
    // selected by SioInterruptWiring. This is stronger than checking UI prose.
    assert!(IO_DEVICES_SOURCE.contains("self.sio_control & 0x01 != 0"));
    assert!(IO_DEVICES_SOURCE.contains("self.sio_control & 0x02 != 0"));
    assert!(IO_DEVICES_SOURCE.contains("let wiring = self.sio.config().interrupt_wiring"));
    assert!(IO_DEVICES_SOURCE.contains("wiring.input.vector_level()"));
    assert!(IO_DEVICES_SOURCE.contains("wiring.output.vector_level()"));

    // Rev 0 external device-ready state is still a separate physical path.
    assert!(IO_DEVICES_SOURCE.contains("pulse_sio_input_device_ready"));
    assert!(IO_DEVICES_SOURCE.contains("pulse_sio_output_device_ready"));
    assert!(IO_DEVICES_SOURCE.contains("SioRevision::Rev0"));
}

#[test]
fn sio_hardware_is_persisted_atomically_inside_the_s100_card_inventory() {
    assert!(PERSISTENCE_SOURCE.contains("const CONFIG_VERSION: u32 = 6;"));
    assert!(PERSISTENCE_SOURCE.contains("\"machine.s100_hardware={}\""));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.sio_hardware="));

    // Pre-v6 aggregate cards remain readable strictly as migration input.
    assert!(PERSISTENCE_SOURCE.contains("\"machine.sio_hardware\""));
    assert!(PERSISTENCE_SOURCE.contains("SioHardwareConfig::from_persistence_key"));
    assert!(PERSISTENCE_SOURCE.contains("S100HardwareConfig::from_legacy_globals("));

    // The live/persisted card variant owns the whole SioHardwareConfig atomically.
    assert!(S100_CONFIG_SOURCE.contains("Mits88Sio(SioHardwareConfig)"));
    assert!(SIO_CONFIG_SOURCE.contains("pub interrupt_wiring: SioInterruptWiring"));
    assert!(SIO_CONFIG_SOURCE.contains("fields.len() != 7 && fields.len() != 9"));
    assert!(SIO_CONFIG_SOURCE.contains("SioInterruptWiring::default()"));

    assert!(PERSISTENCE_SOURCE.contains("configure_s100_hardware("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_sio_hardware("));
}

#[test]
fn s100_remount_uses_the_single_live_cycle_backend() {
    let function = function_body(
        APP_SOURCE,
        "fn apply_s100_hardware_configuration",
        "fn apply_ram_initialization",
    );
    let compact = compact(function);

    assert!(compact.contains(
        "self.machine.configure_s100_hardware(hardware,self.config.machine.ram_init);"
    ));
    assert!(compact.contains("self.config.machine.s100_hardware=hardware;"));
    assert!(!function.contains("replace_engine"));
    assert!(!compact.contains("self.machine.configure_memory("));
}

#[test]
fn editing_an_88_sio_slot_remounts_the_complete_physical_card_configuration() {
    let sio_ui = function_body(
        S100_UI_SOURCE,
        "fn draw_sio_card_configuration",
        "fn draw_two_sio_card_configuration",
    );
    for mutation in [
        "next.revision = revision",
        "next.interface = interface",
        "next.address = address",
        "next.baud = baud",
        "next.format.data_bits = data_bits",
        "next.format.parity = parity",
        "next.format.stop_bits = stop_bits",
        "next.interrupt_wiring.input = target",
        "next.interrupt_wiring.output = target",
    ] {
        assert!(sio_ui.contains(mutation), "88-SIO field no longer edited through the slot card: {mutation}");
    }
    assert!(sio_ui.matches("S100InstalledCardConfig::Mits88Sio(next)").count() >= 9);

    let replace = function_body(S100_UI_SOURCE, "fn replace_slot", "fn commit_hardware");
    assert!(replace.contains("candidate.set_slot(slot, Some(card))"));
    assert!(replace.contains("commit_hardware(app, candidate"));

    let commit = function_body(S100_UI_SOURCE, "fn commit_hardware", "fn is_serial_kind");
    assert!(commit.contains("app.apply_s100_hardware_configuration(valid, action)"));
}
