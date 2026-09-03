const MACHINE: &str = include_str!("../src/config/machine.rs");
const CONFIG_MOD: &str = include_str!("../src/config/mod.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");

#[test]
fn migrated_cpu_and_aggregate_ram_state_do_not_survive_in_machine_config() {
    for forbidden in [
        "pub cpu_model:",
        "pub ram_size:",
        "pub ram_board_profile:",
        "fn cpu_board(",
    ] {
        assert!(!MACHINE.contains(forbidden), "obsolete runtime authority survived: {forbidden}");
    }
    assert!(!CONFIG_MOD.contains("mod cpu_board_authority;"));
    assert!(!APP.contains(".machine.cpu_board()"));
    assert!(!RUNTIME.contains(".machine.cpu_board()"));
}

#[test]
fn old_cpu_and_ram_keys_are_read_only_migration_inputs() {
    // They must remain parseable so old config.ini files upgrade automatically.
    assert!(PERSISTENCE.contains("\"machine.cpu_model\""));
    assert!(PERSISTENCE.contains("\"machine.ram_size\""));
    assert!(PERSISTENCE.contains("\"machine.ram_board_profile\""));
    assert!(PERSISTENCE.contains("S100HardwareConfig::from_legacy_globals("));

    // New files serialize the physical assembly only. A writeln! containing one
    // of the old keys would make the migration state persistent again.
    assert!(!PERSISTENCE.contains("writeln!(out, \"machine.cpu_model="));
    assert!(!PERSISTENCE.contains("writeln!(out, \"machine.ram_size="));
    assert!(!PERSISTENCE.contains("writeln!(out, \"machine.ram_board_profile="));
    assert!(PERSISTENCE.contains("writeln!(out, \"machine.s100_hardware={}\""));
}
