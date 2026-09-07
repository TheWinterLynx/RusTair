const MACHINE: &str = include_str!("../src/config/machine.rs");
const CONFIG_MOD: &str = include_str!("../src/config/mod.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");
const MEMORY: &str = include_str!("../src/machine/memory.rs");
const MACHINE_MOD: &str = include_str!("../src/machine/mod.rs");
const SERIAL_DEVICES: &str = include_str!("../src/machine/serial_devices.rs");
const SERIAL_CARD: &str = include_str!("../src/machine/serial_card.rs");
const SERIAL_BUS: &str = include_str!("../src/machine/serial_bus.rs");
const BACKEND: &str = include_str!("../src/backend/mod.rs");
const CYCLE_HOST: &str = include_str!("../src/backend/cycle_host.rs");

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

#[test]
fn backend_has_no_aggregate_serial_configuration_contract() {
    for forbidden in [
        "fn configure_serial_board(",
        "fn serial_board(&mut self)",
        "fn configure_sio_hardware(",
        "fn sio_hardware(&mut self)",
        "fn configure_two_sio_straps(",
        "fn two_sio_straps(&mut self)",
        "fn configure_two_sio_interrupt_wiring(",
        "fn two_sio_interrupt_wiring(&mut self)",
    ] {
        assert!(
            !BACKEND.contains(forbidden),
            "aggregate backend serial authority returned: {forbidden}"
        );
        assert!(
            !CYCLE_HOST.contains(forbidden),
            "Cycle host reintroduced aggregate serial authority: {forbidden}"
        );
    }
    assert!(BACKEND.contains("fn configure_s100_hardware("));
    assert!(BACKEND.contains("fn s100_hardware("));
}

#[test]
fn memory_has_one_physical_s100_runtime_representation() {
    assert!(!MEMORY.contains("legacy_aggregate"));
    assert!(!MEMORY.contains("legacy_fabric"));
    assert!(!MEMORY.contains("uses_explicit_hardware"));
    assert!(MEMORY.contains("S100RuntimeFabric"));
    assert!(MEMORY.contains("S100HardwareConfig::from_legacy_globals("));
    assert!(MEMORY.contains("compatibility S-100 assembly"));
    assert!(MEMORY.contains("Physical RAM and I/O cards drive PRDY through the live backplane"));
}

#[test]
fn altair_bus_owns_no_parallel_uart_state() {
    for forbidden in [
        "mod io_devices;",
        "io: IoDevices",
        "sio_interrupt_control:",
        "exact_t_state_clock_owner:",
    ] {
        assert!(
            !MACHINE_MOD.contains(forbidden),
            "machine-wide serial singleton returned: {forbidden}"
        );
    }
    assert!(MACHINE_MOD.contains("mod serial_devices;"));
    assert!(MACHINE_MOD.contains("mod serial_card;"));
    assert!(MACHINE_MOD.contains("mod serial_bus;"));
    assert!(!SERIAL_DEVICES.contains("impl AltairBus"));
    assert!(SERIAL_CARD.contains("RuntimeSerialCardHandle"));
    assert!(SERIAL_CARD.contains("impl S100IoRegisterDevice for RuntimeSerialCardDevice"));
    assert!(SERIAL_BUS.contains("self.memory.serial_receive"));
    assert!(!SERIAL_BUS.contains("self.io."));
}