use std::path::Path;

use rustair::backend::EmulationEngine;

const BACKEND: &str = include_str!("../src/backend/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");

#[test]
fn product_exposes_only_the_two_rust_8080_engines() {
    assert_eq!(EmulationEngine::ALL.len(), 2);
    assert_eq!(EmulationEngine::ALL[0], EmulationEngine::RustFast8080);
    assert_eq!(EmulationEngine::ALL[1], EmulationEngine::RustCycleAccurate8080);
}

#[test]
fn simh_backend_surface_is_absent() {
    assert!(!BACKEND.contains("Simh"), "backend API must not expose SIMH variants or modules");
    assert!(!BACKEND.contains("Z80State"), "SIMH-only Z80 state must not remain in the common backend API");
    assert!(!RUNTIME.contains("SIMH"), "runtime UI must not advertise the removed SIMH integration");
}

#[test]
fn simh_artifacts_and_build_scaffolding_are_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in [
        "SIMH-backend",
        "src/backend/simh",
        "tools/simh",
        "tests/simh_frontpanel_smoke.rs",
    ] {
        assert!(!root.join(path).exists(), "removed SIMH path was reintroduced: {path}");
    }
}
