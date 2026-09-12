const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const MAIN_MENU_SOURCE: &str = include_str!("../src/app/ui/main_menu.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const MACHINE_CONFIG_SOURCE: &str = include_str!("../src/config/machine.rs");

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
    assert!(
        app.contains(
            "self.machine.configure_s100_hardware(hardware,self.config.machine.ram_init);"
        )
    );

    let persisted_apply = function_body(
        PERSISTENCE_SOURCE,
        "fn apply_persisted_settings",
        "fn capture_persisted_settings_with_leds",
    );
    assert!(persisted_apply.contains("configure_s100_hardware"));
    assert!(persisted_apply.contains("self.config.machine.s100_hardware"));
    assert!(persisted_apply.contains("self.config.machine.ram_init"));
    assert!(!persisted_apply.contains("configure_memory("));
    assert!(!persisted_apply.contains("configure_serial_board("));
    assert!(!persisted_apply.contains("configure_sio_hardware("));
    assert!(!persisted_apply.contains("configure_two_sio_straps("));

    assert!(S100_UI_SOURCE.contains("app.apply_s100_hardware_configuration(valid, action)"));
    assert!(
        RUNTIME_SOURCE
            .contains("self.machine.s100_hardware() != self.config.machine.s100_hardware")
    );
}

#[test]
fn machine_config_has_no_second_cpu_ram_or_serial_hardware_authority() {
    for forbidden in [
        "pub cpu_model:",
        "pub cpu_board:",
        "pub ram_size:",
        "pub ram_board_profile:",
        "pub serial_board:",
        "pub sio_hardware:",
        "pub two_sio_straps:",
        "pub two_sio_interrupt_wiring:",
    ] {
        assert!(
            !MACHINE_CONFIG_SOURCE.contains(forbidden),
            "obsolete hardware authority survived in MachineConfig: {forbidden}"
        );
    }
    assert!(MACHINE_CONFIG_SOURCE.contains("pub s100_hardware: S100HardwareConfig"));
}

#[test]
fn memory_menu_cannot_recreate_an_aggregate_runtime_topology() {
    for source in [RUNTIME_SOURCE, MAIN_MENU_SOURCE] {
        assert!(!source.contains("for ram_size in RamSize::ALL"));
        assert!(!source.contains("apply_memory_configuration("));
        assert!(!source.contains("apply_memory_board_profile("));
    }
    assert!(MAIN_MENU_SOURCE.contains(
        "Board type, address, population and timing are configured in Machine → S-100 Hardware."
    ));
    assert!(MAIN_MENU_SOURCE.contains("S-100 Hardware…"));
}

#[test]
fn serial_hardware_has_no_duplicate_configuration_menu() {
    assert!(!RUNTIME_SOURCE.contains("ui.menu_button(\"Serial board\""));
    assert!(!MAIN_MENU_SOURCE.contains("ui.menu_button(\"Serial board\""));
    assert!(!APP_SOURCE.contains("fn apply_serial_board_configuration"));
    assert!(!APP_SOURCE.contains("fn apply_two_sio_straps"));
    assert!(!APP_SOURCE.contains("fn apply_two_sio_interrupt_wiring"));
    assert!(APP_SOURCE.contains("fn serial_connection_supported"));
    assert!(APP_SOURCE.contains("active_serial_card_slot"));
}

#[test]
fn persisted_legacy_hardware_fields_are_migration_inputs_not_runtime_mount_calls() {
    assert!(PERSISTENCE_SOURCE.contains("S100HardwareConfig::from_legacy_globals("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_memory("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_memory_board_profile("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_serial_board("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_sio_hardware("));
    assert!(!PERSISTENCE_SOURCE.contains("self.machine.configure_two_sio_straps("));
}
