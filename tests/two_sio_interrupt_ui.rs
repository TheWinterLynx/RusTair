const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const TWO_SIO_CONFIG_SOURCE: &str = include_str!("../src/config/two_sio.rs");
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
fn two_sio_interrupt_wiring_is_edited_as_part_of_the_physical_slot_card() {
    assert!(S100_UI_SOURCE.contains("ui.add_enabled_ui(!powered"));
    let ui = function_body(
        S100_UI_SOURCE,
        "fn draw_two_sio_card_configuration",
        "fn replace_slot",
    );
    let compact_ui = compact(ui);

    assert!(compact_ui.contains("fornext_targetinTwoSioInterruptTarget::ALL"));
    assert!(ui.contains("next.port0 = next_target"));
    assert!(ui.contains("next.port1 = next_target"));
    assert!(ui.contains("S100InstalledCardConfig::Mits88TwoSio"));
    assert!(ui.contains("interrupt_wiring: next"));

    assert!(!APP_SOURCE.contains("fn apply_two_sio_interrupt_wiring"));
    assert!(!APP_SOURCE.contains("fn apply_serial_board_configuration"));
}

#[test]
fn di_and_ei_remain_independent_physical_interrupt_destinations() {
    assert!(TWO_SIO_CONFIG_SOURCE.contains("pub struct TwoSioInterruptWiring"));
    assert!(TWO_SIO_CONFIG_SOURCE.contains("pub port0: TwoSioInterruptTarget"));
    assert!(TWO_SIO_CONFIG_SOURCE.contains("pub port1: TwoSioInterruptTarget"));
    assert!(TWO_SIO_CONFIG_SOURCE.contains("pub const fn drives_pint"));
    assert!(TWO_SIO_CONFIG_SOURCE.contains("pub const fn vector_level"));

    // Runtime IRQ state and the board's physical routing remain separate. A VI
    // target is exposed as a raw level; this layer never fabricates an RST byte.
    assert!(IO_DEVICES_SOURCE.contains("fn two_sio_irq(&self, index: usize) -> bool"));
    assert!(IO_DEVICES_SOURCE.contains("TwoSioInterruptTarget::drives_pint"));
    assert!(IO_DEVICES_SOURCE.contains("TwoSioInterruptTarget::vector_level"));
    assert!(compact(IO_DEVICES_SOURCE).contains("self.two_sio_interrupt_wiring.target(index)"));
}

#[test]
fn two_sio_interrupt_wiring_is_persisted_inside_s100_hardware_only() {
    assert!(PERSISTENCE_SOURCE.contains("const CONFIG_VERSION: u32 = 6;"));
    assert!(PERSISTENCE_SOURCE.contains("\"machine.s100_hardware={}\""));

    // Old independent keys survive only as migration inputs.
    assert!(PERSISTENCE_SOURCE.contains("\"machine.two_sio_port0_irq\""));
    assert!(PERSISTENCE_SOURCE.contains("\"machine.two_sio_port1_irq\""));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_port0_irq="));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_port1_irq="));
    assert!(PERSISTENCE_SOURCE.contains("S100HardwareConfig::from_legacy_globals("));
}

#[test]
fn editing_two_sio_irq_wiring_remounts_the_complete_s100_card() {
    let replace = function_body(S100_UI_SOURCE, "fn replace_slot", "fn commit_hardware");
    assert!(replace.contains("candidate.set_slot(slot, Some(card))"));

    let commit = function_body(S100_UI_SOURCE, "fn commit_hardware", "fn is_serial_kind");
    assert!(commit.contains("app.apply_s100_hardware_configuration(valid, action)"));
}
