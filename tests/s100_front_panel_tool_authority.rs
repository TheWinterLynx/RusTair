const FRONT_PANEL: &str = include_str!("../src/app/ui/front_panel.rs");
const TEACHER_BACKEND: &str = include_str!("../src/backend/cycle.rs");
const TEACHER_PARTIAL_BACKEND: &str = include_str!("../src/backend/cycle/partial_impl.rs");
const TEACHER_UI: &str = include_str!("../src/app/ui/bus_teacher.rs");

#[test]
fn main_front_panel_never_bypasses_backend_panel_operations_with_flat_memory_access() {
    for forbidden in [
        "peek_memory(",
        "write_memory(",
        "installed_ram_bytes(",
        "config.machine.ram_size",
    ] {
        assert!(
            !FRONT_PANEL.contains(forbidden),
            "main front panel must not use flat/host memory shortcut {forbidden:?}"
        );
    }
    assert!(FRONT_PANEL.contains("self.machine.examine("));
    assert!(FRONT_PANEL.contains("self.machine.deposit("));
    assert!(FRONT_PANEL.contains("self.machine.protect_current_board("));
}

#[test]
fn exact_cycle_teacher_uses_live_s100_sample_for_cpu_memory_cycles() {
    // `cycle.rs` is the dispatcher root and includes the exact Partial backend.
    // The Teacher invariant is ownership by the Cycle backend as a module, not
    // that every exact-sample implementation detail remains in the root file.
    assert!(TEACHER_BACKEND.contains("include!(\"cycle/partial_impl.rs\")"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("cycle_live_s100_sample()"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("cycle_live_s100_status_word()"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("InstructionFetch"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("MemoryRead"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("MemoryWrite"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("StackRead"));
    assert!(TEACHER_PARTIAL_BACKEND.contains("StackWrite"));
}

#[test]
fn teacher_keeps_exact_and_reconstructed_sources_explicitly_distinct() {
    assert!(TEACHER_UI.contains("EXACT"));
    assert!(TEACHER_UI.contains("RECONSTRUCTED"));
    assert!(TEACHER_UI.contains("Cycle Accurate"));
}
