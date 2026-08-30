const BACKEND: &str = include_str!("../src/backend/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const CYCLE: &str = include_str!("../src/backend/cycle.rs");

#[test]
fn runtime_uses_fallible_execution_and_surfaces_errors() {
    assert!(
        BACKEND.contains("pub fn try_run_cycles"),
        "BackendHost must expose a fallible execution path"
    );
    assert!(
        RUNTIME.contains("match self.machine.try_run_cycles"),
        "runtime must consume execution errors instead of routing them through BackendHost::call"
    );
    assert!(
        RUNTIME.contains("CPU ERROR — {error}"),
        "runtime must expose the backend diagnostic to the operator"
    );
    assert!(
        !RUNTIME.contains("self.machine.run_cycles(speed.cycle_budget(authentic_cycles));"),
        "normal runtime execution must not use the panic-on-error wrapper"
    );
}

#[test]
fn cycle_fault_boundary_stops_run_before_returning_error() {
    assert!(CYCLE.contains("fn fail_if_cpu_fault"));
    assert!(
        CYCLE.contains("self.machine.cycle_set_running(false);"),
        "a latched Cycle CPU fault must lower RUN before the error reaches the application"
    );
    assert!(
        CYCLE.contains("detail: format!(\"cycle-accurate 8080 fault: {fault:?}\")"),
        "Cycle faults must retain a concrete diagnostic detail"
    );
}
