const CYCLE: &str = include_str!("../src/backend/cycle.rs");
const CYCLE_HOST: &str = include_str!("../src/backend/cycle_host.rs");
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
fn remaining_cpu_run_and_inte_mirrors_are_explicit_debt_not_hidden() {
    let doc = include_str!("../docs/STATE_SOURCES.md");
    assert!(doc.contains("Cycle CPU mirror"));
    assert!(doc.contains("RUN latch mirror"));
    assert!(doc.contains("INTE mirror"));
}
