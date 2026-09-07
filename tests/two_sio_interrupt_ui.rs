const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");
const S100_UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");
const CONFIG_SOURCE: &str = include_str!("../src/config/two_sio.rs");

#[test]
fn two_sio_interrupt_wiring_is_edited_inside_the_installed_slot() {
    assert!(S100_UI_SOURCE.contains("88-2SIO physical straps"));
    assert!(S100_UI_SOURCE.contains("Port {port} IRQ:"));
    assert!(S100_UI_SOURCE.contains("TwoSioInterruptTarget::ALL"));
    assert!(S100_UI_SOURCE.contains("next.port0 = next_target"));
    assert!(S100_UI_SOURCE.contains("next.port1 = next_target"));
    assert!(S100_UI_SOURCE.contains("S100InstalledCardConfig::Mits88TwoSio"));
    assert!(S100_UI_SOURCE.contains("POWER OFF required to move cards"));
}

#[test]
fn independent_di_and_ei_targets_remain_part_of_the_card_configuration() {
    assert!(CONFIG_SOURCE.contains("pub port0: TwoSioInterruptTarget"));
    assert!(CONFIG_SOURCE.contains("pub port1: TwoSioInterruptTarget"));
    assert!(CONFIG_SOURCE.contains("pub const fn target(self, port_index: usize)"));
}

#[test]
fn legacy_interrupt_keys_are_migration_inputs_not_a_second_persisted_authority() {
    assert!(PERSISTENCE_SOURCE.contains("machine.two_sio_port0_irq"));
    assert!(PERSISTENCE_SOURCE.contains("machine.two_sio_port1_irq"));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_port0_irq="));
    assert!(!PERSISTENCE_SOURCE.contains("writeln!(out, \"machine.two_sio_port1_irq="));
    assert!(PERSISTENCE_SOURCE.contains("machine.s100_hardware"));
}
