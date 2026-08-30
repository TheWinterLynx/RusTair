const CYCLE: &str = include_str!("../src/backend/cycle.rs");
const CYCLE_HOST: &str = include_str!("../src/backend/cycle_host.rs");
const MACHINE: &str = include_str!("../src/machine/mod.rs");
const CHASSIS: &str = include_str!("../src/machine/chassis.rs");
const MEMORY: &str = include_str!("../src/machine/memory.rs");
const PANEL_BUS: &str = include_str!("../src/machine/panel_bus.rs");

#[test]
fn cycle_teacher_has_no_parallel_s100_status_or_protection_latch() {
    assert!(!CYCLE.contains("teaching_status_latch"));
    assert!(!CYCLE.contains("teaching_prot_latch"));
    assert!(CYCLE.contains("raw_s100_status_word()"));
    assert!(CYCLE.contains("raw_s100_prot()"));
}

#[test]
fn lifecycle_teacher_does_not_reverse_engineer_raw_state_from_led_brightness() {
    assert!(CYCLE_HOST.contains("raw_s100_status_word()"));
    assert!(CYCLE_HOST.contains("raw_s100_inte()"));
    assert!(CYCLE_HOST.contains("raw_s100_wait()"));
    assert!(CYCLE_HOST.contains("visible_lamps: lamps"));
    assert!(!CYCLE_HOST.contains("let lamp = |value: f32| Some(value >= 0.5)"));
}

#[test]
fn canonical_raw_s100_accessors_read_signals_not_panel_lamp_snapshot() {
    for accessor in [
        "raw_s100_status_word",
        "raw_s100_inte",
        "raw_s100_prot",
        "raw_s100_wait",
        "raw_s100_hlda",
    ] {
        assert!(MEMORY.contains(accessor), "missing raw S-100 accessor {accessor}");
    }
    assert!(MEMORY.contains("self.s100.signals()"));
}

#[test]
fn panel_lamp_integrator_remains_presentation_only() {
    assert!(PANEL_BUS.contains("Presentation persistence only"));
    assert!(PANEL_BUS.contains("struct PanelLampIntegrator"));
    assert!(PANEL_BUS.contains("signals: S100Signals"));
    assert!(PANEL_BUS.contains("lamps: PanelLampIntegrator"));
}

#[test]
fn cycle_has_no_fast_cpu_mirror_or_sync_path() {
    assert!(
        !CYCLE.contains("sync_machine_cpu"),
        "Cycle must never restore the legacy Cpu8080 mirror synchronization path"
    );
    assert!(
        !CYCLE.contains("machine.cpu"),
        "Cycle backend must not read or write AltairMachine.cpu"
    );
    assert!(
        !CYCLE.contains("cycle_registers_from_fast"),
        "Cycle power-on state must not be seeded through the Fast CPU"
    );
    assert!(
        CYCLE.contains("random_power_on_cpu_state"),
        "Cycle should own its undefined power-on CPU sample"
    );
}

#[test]
fn cycle_physically_owns_cpu_free_chassis_while_fast_keeps_its_cpu() {
    assert!(CHASSIS.contains("pub struct AltairChassis"));
    assert!(CHASSIS.contains("pub bus: AltairBus"));
    assert!(!CHASSIS.contains("Cpu8080"), "physical chassis must not contain a Fast CPU");
    assert!(!CHASSIS.contains("Deref"), "chassis ownership must remain explicit");
    assert!(!CHASSIS.contains("DerefMut"), "chassis ownership must remain explicit");

    assert!(
        CYCLE.contains("use crate::machine::{AltairChassis,"),
        "Cycle must import the CPU-free chassis explicitly"
    );
    assert!(
        CYCLE.contains("machine: AltairChassis"),
        "Cycle's physical container must be AltairChassis"
    );
    assert!(
        !CYCLE.contains("AltairChassis as AltairMachine"),
        "Cycle must not hide its CPU-free chassis behind the Fast machine name"
    );
    assert!(
        !CYCLE.contains("machine: AltairMachine"),
        "Cycle must not own the Fast AltairMachine as its physical container"
    );

    assert!(MACHINE.contains("pub struct AltairMachine"));
    assert!(MACHINE.contains("pub cpu: Cpu8080"));
    assert!(MACHINE.contains("pub use chassis::AltairChassis"));
}

#[test]
fn cycle_memory_configuration_bypasses_fast_machine_cpu_helper() {
    assert!(CYCLE_HOST.contains("machine_mut().bus.configure_memory(size, init)"));
    assert!(!CYCLE_HOST.contains("machine_mut().configure_memory(size, init)"));
}

#[test]
fn state_source_documentation_matches_resolved_chassis_architecture() {
    let doc = include_str!("../docs/STATE_SOURCES.md");
    assert!(doc.contains("CPU/chassis type composition is resolved"));
    assert!(doc.contains("Cycle physically owns a CPU-free"));
    assert!(doc.contains("RUN latch duplication"));
    assert!(doc.contains("Backend encapsulation"));
    assert!(doc.contains("There is no `sync_machine_cpu()` path"));
    assert!(doc.contains("previous `AltairBus::cpu_inte` duplicate has already been removed"));
    assert!(
        !doc.contains("Cycle backend carries an unused `Cpu8080`"),
        "documentation must not resurrect the removed dormant Fast CPU"
    );
}
