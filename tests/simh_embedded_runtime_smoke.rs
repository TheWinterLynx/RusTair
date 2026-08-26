#![cfg(all(feature = "simh-ffi", windows))]

use std::error::Error;

use rustair::backend::simh::{RUSTAIR_SIMH_BUNDLE_REVISION, prepare_embedded_runtime};
use rustair::backend::{BackendHost, EmulationEngine};

#[test]
#[ignore = "spawns the Open-SIMH executables embedded in RusTair"]
fn embedded_simh_bundle_runs_without_external_installation() -> Result<(), Box<dyn Error>> {
    // No RUSTAIR_SIMH_* variable is required here. These paths must be created
    // from the bytes compiled into the Rust test executable itself.
    let runtime = prepare_embedded_runtime()?;
    assert!(runtime.root.to_string_lossy().contains(RUSTAIR_SIMH_BUNDLE_REVISION));
    assert!(runtime.altair_exe.is_file());
    assert!(runtime.altairz80_exe.is_file());
    assert!(runtime.frontpanel_dll.is_file());

    let mut classic = BackendHost::from_engine(EmulationEngine::SimhAltair)?;
    assert!(!classic.powered(), "selecting classic SIMH must leave POWER OFF");
    classic.power(true);
    assert!(classic.powered());
    classic.power(false);
    assert!(!classic.powered());

    let mut altairz80 = BackendHost::from_engine(EmulationEngine::SimhAltairZ80)?;
    assert!(!altairz80.powered(), "selecting AltairZ80 SIMH must leave POWER OFF");
    altairz80.power(true);
    assert!(altairz80.powered());
    altairz80.power(false);
    assert!(!altairz80.powered());

    println!(
        "Embedded Open-SIMH bundle {} passed from {}",
        RUSTAIR_SIMH_BUNDLE_REVISION,
        runtime.root.display()
    );
    Ok(())
}
