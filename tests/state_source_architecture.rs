const BACKEND: &str = include_str!("../src/backend/mod.rs");
const CYCLE: &str = include_str!("../src/backend/cycle.rs");
const CYCLE_PARTIAL: &str = include_str!("../src/backend/cycle/partial_impl.rs");
const CYCLE_FULL: &str = include_str!("../src/backend/cycle/full.rs");
const CYCLE_HOST: &str = include_str!("../src/backend/cycle_host.rs");
const MACHINE: &str = include_str!("../src/machine/mod.rs");
const CHASSIS: &str = include_str!("../src/machine/chassis.rs");
const MEMORY: &str = include_str!("../src/machine/memory.rs");
const PANEL_BUS: &str = include_str!("../src/machine/panel_bus.rs");

fn cycle_module_contains(needle: &str) -> bool {
    CYCLE.contains(needle) || CYCLE_PARTIAL.contains(needle) || CYCLE_FULL.contains(needle)
}

#[test]
fn cycle_teacher_has_no_parallel_s100_status_or_protection_latch() {
    assert!(CYCLE.contains("include!(\"cycle/partial_impl.rs\")"));
    assert!(!cycle_module_contains("teaching_status_latch"));
    assert!(!cycle_module_contains("teaching_prot_latch"));
    assert!(CYCLE_PARTIAL.contains("raw_s100_status_word()"));
    assert!(CYCLE_PARTIAL.contains("raw_s100_prot()"));
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
        assert!(
            MEMORY.contains(accessor),
            "missing raw S-100 accessor {accessor}"
        );
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
fn cycle_has_no_cpu_mirror_or_architectural_state_sync_path() {
    assert!(
        !cycle_module_contains("sync_machine_cpu"),
        "Cycle must never restore a parallel CPU mirror synchronization path"
    );
    assert!(
        !cycle_module_contains("machine.cpu"),
        "Cycle backend must keep architectural CPU state in its one Cpu8080Cycle core"
    );
    assert!(
        CYCLE_PARTIAL.contains("random_power_on_cpu_state"),
        "Cycle should own its undefined power-on CPU sample"
    );
}

#[test]
fn cycle_physically_owns_cpu_free_chassis_with_one_cpu_authority() {
    assert!(CHASSIS.contains("pub struct AltairChassis"));
    assert!(CHASSIS.contains("pub bus: AltairBus"));
    assert!(
        !CHASSIS.contains("Cpu8080Cycle"),
        "physical chassis must not embed the CPU core"
    );
    assert!(
        !CHASSIS.contains("Deref"),
        "chassis ownership must remain explicit"
    );
    assert!(
        !CHASSIS.contains("DerefMut"),
        "chassis ownership must remain explicit"
    );

    assert!(
        CYCLE_PARTIAL.contains("use crate::machine::{AltairChassis,"),
        "Cycle must import the CPU-free chassis explicitly"
    );
    assert!(
        CYCLE_PARTIAL.contains("machine: AltairChassis"),
        "Cycle's physical container must be AltairChassis"
    );
    assert!(
        CYCLE_PARTIAL.contains("cpu: Cpu8080Cycle"),
        "CycleAccurateMachineBackend must own the single architectural CPU core"
    );

    assert!(MACHINE.contains("pub use chassis::AltairChassis"));
    assert!(
        !MACHINE.contains("pub cpu:"),
        "machine module must not expose a second CPU owner"
    );
}

#[test]
fn cycle_memory_configuration_uses_the_live_chassis_bus() {
    assert!(CYCLE_HOST.contains("machine_mut().bus.configure_memory(size, init)"));
    assert!(!CYCLE_HOST.contains("machine_mut().configure_memory(size, init)"));
}

#[test]
fn chassis_run_state_is_derived_from_canonical_s100_latch() {
    let chassis_struct = CHASSIS
        .split("pub struct AltairChassis")
        .nth(1)
        .expect("AltairChassis declaration")
        .split("impl Default for AltairChassis")
        .next()
        .expect("AltairChassis body");
    assert!(
        !chassis_struct.contains("running: bool"),
        "AltairChassis must not store a second RUN latch"
    );
    assert!(CHASSIS.contains("fn running(&self) -> bool"));
    assert!(CHASSIS.contains("self.bus.run_latched()"));
    assert!(MACHINE.contains("fn run_latched(&self) -> bool"));
    assert!(MACHINE.contains("self.s100.signals().run"));

    let reset_assert = PANEL_BUS
        .split("fn assert_front_panel_reset")
        .nth(1)
        .expect("RESET assert implementation")
        .split("fn release_front_panel_reset")
        .next()
        .expect("RESET assert body");
    let reset_release = PANEL_BUS
        .split("fn release_front_panel_reset")
        .nth(1)
        .expect("RESET release implementation")
        .split("fn drive_power_on_state")
        .next()
        .expect("RESET release body");
    assert!(reset_assert.contains("let run = self.signals.run;"));
    assert!(reset_release.contains("let run = self.signals.run;"));
    assert!(!reset_assert.contains("self.signals.run = run;"));
    assert!(!reset_release.contains("self.signals.run = run;"));
    assert!(!PANEL_BUS.contains("assert_front_panel_reset(&mut self, run: bool)"));
}

#[test]
fn state_source_documentation_matches_unified_cycle_architecture() {
    let doc = include_str!("../docs/STATE_SOURCES.md");
    assert!(doc.contains("single Adaptive Cycle execution engine"));
    assert!(doc.contains("CycleAccurateMachineBackend::cpu"));
    assert!(doc.contains("CPU-free `AltairChassis`"));
    assert!(doc.contains("RUN latch duplication is resolved"));
    assert!(doc.contains("Backend encapsulation"));
    assert!(doc.contains("There is no `sync_machine_cpu()` path"));
    assert!(doc.contains("previous `AltairBus::cpu_inte` duplicate has already been removed"));
    assert!(
        !doc.contains("two Rust engines"),
        "documentation must not resurrect the removed multi-engine architecture"
    );
}

#[test]
fn backend_contract_never_fabricates_a_reconstructed_bus_sample() {
    assert!(!BACKEND.contains("BusTeachingSnapshot::reconstructed(engine, panel, cpu)"));
    assert!(BACKEND.contains("fn bus_teaching_snapshot"));
    assert!(BACKEND.contains("Ok(None)"));
}
