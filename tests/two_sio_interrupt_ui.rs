const APP_SOURCE: &str = include_str!("../src/app/mod.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");

#[test]
fn engine_recreation_reapplies_physical_interrupt_wiring() {
    let start = APP_SOURCE
        .find("fn select_emulation_engine")
        .expect("app must own the engine-recreation boundary");
    let tail = &APP_SOURCE[start..];
    let end = tail
        .find("fn apply_memory_configuration")
        .expect("helper after engine-recreation boundary");
    let function = &tail[..end];

    assert!(function.contains("self.machine.replace_engine(engine)"));
    assert!(function.contains("self.machine.configure_two_sio_straps"));
    assert!(function.contains("self.machine.configure_two_sio_interrupt_wiring"));
    assert!(function.contains("self.config.machine.two_sio_interrupt_wiring"));
}

#[test]
fn interrupt_wiring_changes_require_power_off() {
    let start = APP_SOURCE
        .find("fn apply_two_sio_interrupt_wiring")
        .expect("app must own a physical interrupt-wiring apply boundary");
    let tail = &APP_SOURCE[start..];
    let end = tail
        .find("fn two_sio_vi_mask_label")
        .expect("helper after interrupt wiring apply boundary");
    let function = &tail[..end];
    assert!(function.contains("if self.machine.powered()"));
    assert!(function.contains("Power OFF the Altair before changing the physical 88-2SIO DI/EI interrupt wiring"));
}

#[test]
fn serial_configuration_exposes_independent_di_and_ei_targets() {
    assert!(RUNTIME_SOURCE.contains("Physical 88-2SIO interrupt wiring:"));
    assert!(RUNTIME_SOURCE.contains("DI / Port 0 IRQ:"));
    assert!(RUNTIME_SOURCE.contains("EI / Port 1 IRQ:"));
    assert!(RUNTIME_SOURCE.contains("crate::config::TwoSioInterruptTarget::ALL"));
    assert!(RUNTIME_SOURCE.contains("next.port0 = target"));
    assert!(RUNTIME_SOURCE.contains("next.port1 = target"));
    assert!(RUNTIME_SOURCE.contains("ui.add_enabled_ui(!powered"));
}

#[test]
fn ui_keeps_vector_interrupt_boundary_explicit_and_observable() {
    assert!(RUNTIME_SOURCE.contains(
        "selecting VIx never fabricates a CPU RST opcode inside the 88-2SIO"
    ));
    assert!(RUNTIME_SOURCE.contains("Active raw 88-2SIO vector outputs:"));
    assert!(RUNTIME_SOURCE.contains("self.machine.two_sio_vector_interrupt_requests()"));
}

#[test]
fn persistence_has_independent_port_keys_and_safe_old_config_default() {
    assert!(PERSISTENCE_SOURCE.contains("machine.two_sio_port0_irq"));
    assert!(PERSISTENCE_SOURCE.contains("machine.two_sio_port1_irq"));
    assert!(PERSISTENCE_SOURCE.contains("old_or_invalid_interrupt_wiring_keeps_safe_migration_default"));
}
