const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");

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
fn app_mounts_slot_native_s100_hardware_at_every_runtime_configuration_boundary() {
    let app = compact(APP_SOURCE);
    assert!(app.contains(
        "self.machine.configure_s100_hardware(hardware,self.config.machine.ram_init);"
    ));

    let persisted_apply = function_body(
        PERSISTENCE_SOURCE,
        "fn apply_persisted_settings",
        "fn capture_persisted_settings_with_leds",
    );
    assert!(persisted_apply.contains("configure_s100_hardware"));
    assert!(persisted_apply.contains("self.config.machine.s100_hardware"));
    assert!(persisted_apply.contains("self.config.machine.ram_init"));
    assert!(!persisted_apply.contains("configure_memory("));

    assert!(S100_UI_SOURCE.contains("app.apply_s100_hardware_configuration(valid, action)"));
    assert!(RUNTIME_SOURCE.contains("self.machine.s100_hardware() != self.config.machine.s100_hardware"));
}

#[test]
fn memory_menu_cannot_recreate_an_aggregate_runtime_topology() {
    assert!(!RUNTIME_SOURCE.contains("for ram_size in RamSize::ALL"));
    assert!(!RUNTIME_SOURCE.contains("self.apply_memory_configuration("));
    assert!(!RUNTIME_SOURCE.contains("self.apply_memory_board_profile("));
    assert!(RUNTIME_SOURCE.contains("Board type, base address, population and timing now come only from Configuration → S-100 Chassis / Cards"));
}

#[test]
fn persisted_legacy_ram_fields_are_migration_inputs_not_runtime_mount_calls() {
    assert!(PERSISTENCE_SOURCE.contains("S100HardwareConfig::from_legacy_globals("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_memory("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_memory_board_profile("));
}
