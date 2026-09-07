const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const SIO_CONFIG_SOURCE: &str = include_str!("../src/config/sio.rs");

#[test]
fn sio_hardware_controls_live_only_in_the_power_off_s100_slot_editor() {
    assert!(S100_UI_SOURCE.contains("POWER OFF required to move cards"));
    assert!(S100_UI_SOURCE.contains("88-SIO physical configuration"));
    assert!(S100_UI_SOURCE.contains("Revision:"));
    assert!(S100_UI_SOURCE.contains("Interface:"));
    assert!(S100_UI_SOURCE.contains("I/O address:"));
    assert!(S100_UI_SOURCE.contains("Baud:"));
    assert!(S100_UI_SOURCE.contains("Data bits:"));
    assert!(S100_UI_SOURCE.contains("Parity:"));
    assert!(S100_UI_SOURCE.contains("Stop bits:"));
    assert!(S100_UI_SOURCE.contains("Input IRQ:"));
    assert!(S100_UI_SOURCE.contains("Output IRQ:"));
    assert!(S100_UI_SOURCE.contains("S100InstalledCardConfig::Mits88Sio"));
}

#[test]
fn sio_slot_editor_uses_the_documented_card_configuration_types() {
    assert!(S100_UI_SOURCE.contains("SioBaudRate::STANDARD"));
    assert!(S100_UI_SOURCE.contains("SioInterruptTarget::ALL"));
    assert!(S100_UI_SOURCE.contains("replace_slot(app, hardware, slot"));
    assert!(SIO_CONFIG_SOURCE.contains("pub interrupt_wiring: SioInterruptWiring"));
}

#[test]
fn persistence_v6_writes_one_physical_s100_assembly_only() {
    assert!(PERSISTENCE_SOURCE.contains("const CONFIG_VERSION: u32 = 6;"));
    assert!(PERSISTENCE_SOURCE.contains("machine.s100_hardware"));
    assert!(PERSISTENCE_SOURCE.contains("SioHardwareConfig::from_persistence_key"));

    // Old keys remain read-only migration inputs. New files must never recreate
    // a second global serial-card authority beside the slot inventory.
    assert!(PERSISTENCE_SOURCE.contains("\"machine.sio_hardware\""));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.sio_hardware="));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.serial_board="));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_straps="));
}

#[test]
fn app_has_one_s100_remount_boundary_and_no_global_serial_apply_helpers() {
    assert!(APP_SOURCE.contains("configure_s100_hardware(hardware, self.config.machine.ram_init)"));
    assert!(!APP_SOURCE.contains("fn apply_serial_board_configuration"));
    assert!(!APP_SOURCE.contains("fn apply_sio_hardware"));
    assert!(!APP_SOURCE.contains("fn apply_two_sio_straps"));
    assert!(!APP_SOURCE.contains("fn apply_two_sio_interrupt_wiring"));
}
