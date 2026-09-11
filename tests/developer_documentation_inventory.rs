use std::fs;
use std::path::{Path, PathBuf};

fn collect_rust_sources(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("failed to enumerate {}: {error}", dir.display()));
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, root, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            let relative = path
                .strip_prefix(root)
                .expect("source path must be inside repository root");
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn manifest_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn unique_basename_is_documented(path: &str, all_paths: &[String], reference: &str) -> bool {
    let Some(file_name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    let occurrences = all_paths
        .iter()
        .filter(|candidate| {
            Path::new(candidate.as_str())
                .file_name()
                .and_then(|name| name.to_str())
                == Some(file_name)
        })
        .count();

    occurrences == 1 && reference.contains(&format!("`{file_name}`"))
}

fn assert_inventory_documented(
    dir: &Path,
    root: &Path,
    reference_path: &str,
    scope: &str,
    allow_unique_basename: bool,
) {
    let reference = fs::read_to_string(root.join(reference_path))
        .unwrap_or_else(|error| panic!("{reference_path} must exist: {error}"));

    let mut sources = Vec::new();
    collect_rust_sources(dir, root, &mut sources);
    assert!(!sources.is_empty(), "{scope} inventory unexpectedly empty");

    let undocumented = sources
        .iter()
        .filter(|path| {
            let exact = reference.contains(&format!("`{path}`"));
            let context_relative = allow_unique_basename
                && unique_basename_is_documented(path, &sources, &reference);
            !exact && !context_relative
        })
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        undocumented.is_empty(),
        "every {scope} Rust source must be documented in {reference_path}; missing:\n{}",
        undocumented.join("\n")
    );
}

#[test]
fn every_production_rust_source_file_is_listed_in_the_source_reference() {
    let root = manifest_root();
    assert_inventory_documented(
        &root.join("src"),
        &root,
        "docs/SOURCE_REFERENCE.md",
        "src/**/*.rs",
        true,
    );
}

#[test]
fn every_integration_test_source_is_listed_in_the_test_reference() {
    let root = manifest_root();
    assert_inventory_documented(
        &root.join("tests"),
        &root,
        "docs/TEST_REFERENCE.md",
        "tests/**/*.rs",
        false,
    );
}

#[test]
fn core_contributor_documents_exist_and_are_linked_from_the_index() {
    let root = manifest_root();
    let index = fs::read_to_string(root.join("docs/README.md"))
        .expect("docs/README.md must exist");

    let required = [
        "DEVELOPER_GUIDE.md",
        "GLOSSARY.md",
        "EMULATION_ARCHITECTURE.md",
        "ARCHITECTURAL_INVARIANTS.md",
        "DESIGN_RATIONALE.md",
        "RUNTIME_FLOWS.md",
        "SUPPORT_AND_LIMITATIONS.md",
        "SUBSYSTEM_REVIEW_MAP.md",
        "SOURCE_REFERENCE.md",
        "TEST_REFERENCE.md",
        "REPOSITORY_REFERENCE.md",
        "BUILD_AND_TOOLCHAIN.md",
        "CODING_CONVENTIONS.md",
        "TESTING_GUIDE.md",
        "EXTENDING_RUSTAIR.md",
        "DEBUGGING_AND_PERFORMANCE.md",
        "STATE_SOURCES.md",
        "CPU_BOARD_ARCHITECTURE.md",
        "HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md",
    ];

    for name in required {
        let path = root.join("docs").join(name);
        assert!(path.is_file(), "required contributor document missing: {}", path.display());
        assert!(
            index.contains(name),
            "docs/README.md must link or name required contributor document {name}"
        );
    }

    let contributing_path = root.join("CONTRIBUTING.md");
    let readme_path = root.join("README.md");
    assert!(contributing_path.is_file(), "CONTRIBUTING.md must exist");
    assert!(readme_path.is_file(), "README.md must exist");

    let readme = fs::read_to_string(readme_path).expect("README.md must be readable");
    assert!(
        readme.contains("CONTRIBUTING.md") && readme.contains("docs/README.md"),
        "README.md must direct new contributors to CONTRIBUTING.md and docs/README.md"
    );

    let contributing =
        fs::read_to_string(contributing_path).expect("CONTRIBUTING.md must be readable");
    assert!(
        contributing.contains("docs/SOURCE_REFERENCE.md")
            && contributing.contains("docs/TESTING_GUIDE.md"),
        "CONTRIBUTING.md must link the source map and testing guide"
    );
}
