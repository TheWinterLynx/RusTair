use std::fs;
use std::path::{Path, PathBuf};

fn rust_files_under(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else { return; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files_under(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn removed_fast_backend_and_semantic_machine_cannot_reenter_product_or_tests() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files_under(&root.join("src"), &mut files);
    rust_files_under(&root.join("tests"), &mut files);

    let forbidden = [
        "RustFast8080",
        "FastMachineBackend",
        "NativeMachineBackend",
        "BackendHost::rust_fast",
        "mod fast_exec",
        "select_emulation_engine",
        "AltairMachine",
        "Fast8080S100Adapter",
        "fast_front_panel_",
        "service_fast_interrupt",
        "fast_wait_t_states",
    ];

    for path in files {
        if path.ends_with("unified_cycle_architecture.rs") { continue; }
        let source = fs::read_to_string(&path).expect("Rust source must be UTF-8");
        for token in forbidden {
            assert!(
                !source.contains(token),
                "removed Fast/semantic-machine token {token:?} reappeared in {}",
                path.display(),
            );
        }
    }
}

#[test]
fn cpu8080_semantic_core_remains_internal_full_executor_not_a_second_backend() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let full = fs::read_to_string(root.join("src/backend/cycle/full.rs")).unwrap();
    let backend = fs::read_to_string(root.join("src/backend/mod.rs")).unwrap();
    let machine = fs::read_to_string(root.join("src/machine/mod.rs")).unwrap();

    assert!(full.contains("use crate::cpu8080::Bus;"));
    assert!(full.contains("begin_full_execution_window"));
    assert!(full.contains("FullInstructionBus"));
    assert!(!backend.contains("mod native;"));
    assert!(backend.contains("RustCycleAccurate8080"));
    assert!(!machine.contains("pub struct AltairMachine"));
    assert!(!machine.contains("Cpu8080"));
}
