use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=RUSTAIR_SIMH_FRONTPANEL_DIR");

    if env::var_os("CARGO_FEATURE_SIMH_FFI").is_none() {
        return;
    }

    let Some(dir) = env::var_os("RUSTAIR_SIMH_FRONTPANEL_DIR") else {
        panic!(
            "simh-ffi requires RUSTAIR_SIMH_FRONTPANEL_DIR to point at the directory containing the Open-SIMH-built simh_frontpanel library"
        );
    };

    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        panic!(
            "RUSTAIR_SIMH_FRONTPANEL_DIR does not exist or is not a directory: {}",
            dir.display()
        );
    }

    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=dylib=simh_frontpanel");

    // simh_frontpanel itself is linked by Open-SIMH's CMake against its thread,
    // OS and Windows networking dependencies. RusTair intentionally does not
    // reproduce those dependency choices here.
}
