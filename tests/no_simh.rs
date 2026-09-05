use std::path::Path;

use rustair::backend::EmulationEngine;

const BACKEND: &str = include_str!("../src/backend/mod.rs");
const BUS_TEACHING: &str = include_str!("../src/backend/bus_teaching.rs");
const APP: &str = include_str!("../src/app/mod.rs");
const PERSISTENCE: &str = include_str!("../src/app/persistence.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const BACKEND_README: &str = include_str!("../src/backend/README.md");
const TODO: &str = include_str!("../TODO.md");

#[test]
fn product_exposes_only_adaptive_cycle_8080() {
    assert_eq!(EmulationEngine::ALL, [EmulationEngine::RustCycleAccurate8080]);
}

#[test]
fn retired_backend_surface_is_absent() {
    assert!(!BACKEND.contains("Simh"), "backend API must not expose retired backend variants or modules");
    assert!(!BACKEND.contains("Z80State"), "retired Z80 state must not remain in the common backend API");
    assert!(!BUS_TEACHING.contains("CpuState::Z80"), "Bus Teacher must not retain the removed Z80 CPU-state branch");
    assert!(!APP.contains("SIMH"), "application control paths must not retain retired backend fallbacks");
    assert!(!PERSISTENCE.contains("SimhAltair"), "persistence must not retain removed engine variants");
    assert!(!PERSISTENCE.contains("simh-altair"), "persistence must not serialize removed engine keys");
    assert!(!RUNTIME.contains("SIMH"), "runtime UI must not advertise the retired integration");
}

#[test]
fn retired_backend_artifacts_and_build_scaffolding_are_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "SIMH-backend",
        "src/backend/simh",
        "tools/simh",
        "tests/simh_frontpanel_smoke.rs",
    ] {
        assert!(!root.join(path).exists(), "retired backend path was reintroduced: {path}");
    }
}

#[test]
fn retired_backend_is_not_part_of_current_docs_or_backlog() {
    for document in [BACKEND_README, TODO] {
        assert!(!document.contains("SimhAltair"));
        assert!(!document.contains("Open SIMH"));
        assert!(!document.contains("AltairZ80"));
        assert!(!document.contains("SIMH / Z80"));
    }
}
