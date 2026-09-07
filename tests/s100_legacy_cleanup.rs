const MACHINE: &str = include_str!("../src/config/machine.rs");
const CONFIG_MOD: &str = include_str!("../src/config/mod.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");

#[test]
fn migrated_aggregate_hardware_state_does_not_survive_in_machine_config() {
    for forbidden in [
        "pub cpu_model:",
        "pub ram_size:",
        "pub ram_board_profile:",
        "pub serial_board:",
        "pub sio_hardware:",
        "pub two_sio_straps:",
        "pub two_sio_interrupt_wiring:",
        "fn cpu_board(",
    ] {
        assert!(
            !MACHINE.contains(forbidden),
            "obsolete runtime authority survived: {forbidden}"
        );
    }
    assert!(MACHINE.contains("pub s100_hardware: S100HardwareConfig"));
    assert!(!CONFIG_MOD.contains("mod cpu_board_authority;"));
    assert!(!APP.contains(".machine.cpu_board()"));
    assert!(!RUNTIME.contains(".machine.cpu_board()"));
}

#[test]
fn old_hardware_keys_are_read_only_migration_inputs() {
    // They remain parseable so pre-slot-native config.ini files upgrade once.
    for key in [
        "\"machine.cpu_model\"",
        "\"machine.ram_size\"",
        "\"machine.ram_board_profile\"",
        "\"machine.serial_board\"",
        "\"machine.sio_hardware\"",
        "\"machine.two_sio_base\"",
        "\"machine.two_sio_port0_baud\"",
        "\"machine.two_sio_port1_baud\"",
        "\"machine.two_sio_port0_interface\"",
        "\"machine.two_sio_port1_interface\"",
        "\"machine.two_sio_port0_irq\"",
        "\"machine.two_sio_port1_irq\"",
    ] {
        assert!(PERSISTENCE.contains(key), "legacy migration parser lost {key}");
    }
    assert!(PERSISTENCE.contains("S100HardwareConfig::from_legacy_globals("));

    // New files serialize the physical assembly once. These exact assignment
    // prefixes must never reappear in `to_text()` output.
    for forbidden_write in [
        "writeln!(out, \"machine.cpu_model=",
        "writeln!(out, \"machine.ram_size=",
        "writeln!(out, \"machine.ram_board_profile=",
        "writeln!(out, \"machine.serial_board=",
        "writeln!(out, \"machine.sio_hardware=",
        "writeln!(out, \"machine.two_sio_",
    ] {
        assert!(
            !PERSISTENCE.contains(forbidden_write),
            "legacy hardware key became writable again: {forbidden_write}"
        );
    }
    assert!(PERSISTENCE.contains("machine.s100_hardware={}"));
    assert!(PERSISTENCE.contains("const CONFIG_VERSION: u32 = 6;"));
}

#[test]
fn app_has_no_duplicate_serial_hardware_apply_boundary() {
    for forbidden in [
        "fn apply_serial_board_configuration",
        "fn apply_sio_hardware",
        "fn apply_two_sio_straps",
        "fn apply_two_sio_interrupt_wiring",
    ] {
        assert!(
            !APP.contains(forbidden),
            "obsolete aggregate serial apply path survived: {forbidden}"
        );
    }
    assert!(APP.contains("fn apply_s100_hardware_configuration"));
    assert!(!RUNTIME.contains("ui.menu_button(\"Serial board\""));
}
