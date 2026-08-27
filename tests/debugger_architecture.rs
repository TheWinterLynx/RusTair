const CYCLE_HOST_SOURCE: &str = include_str!("../src/backend/cycle_host.rs");
const CYCLE_SOURCE: &str = include_str!("../src/backend/cycle.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const MEMORY_VIEWER_SOURCE: &str = include_str!("../src/app/ui/memory_viewer.rs");
const INSTRUCTION_HISTORY_SOURCE: &str = include_str!("../src/app/ui/instruction_history.rs");
const DEBUGGER_CONTROLS_SOURCE: &str = include_str!("../src/app/ui/debugger_controls.rs");
const MEMORY_ACTIVITY_SOURCE: &str = include_str!("../src/app/ui/memory_activity.rs");
const LOOP_INSPECTOR_SOURCE: &str = include_str!("../src/app/ui/loop_inspector.rs");

#[test]
fn cycle_debugger_keeps_the_t_state_loop_inside_the_cycle_backend() {
    assert!(
        CYCLE_SOURCE.contains("service_execution_with_observer"),
        "cycle backend must own debugger-aware T-state iteration",
    );
    assert!(
        CYCLE_SOURCE.contains("CycleExecutionEvent::BeforeInstruction")
            && CYCLE_SOURCE.contains("CycleExecutionEvent::InstructionComplete"),
        "cycle observer must expose semantic instruction boundaries",
    );
    assert!(
        !CYCLE_HOST_SOURCE.contains("self.inner.service_execution(1)"),
        "cycle host must not redispatch through MachineBackend once per T-state",
    );
    assert!(
        CYCLE_HOST_SOURCE.contains("service_execution_with_observer(t_state_budget"),
        "cycle host should delegate one whole host budget to the cycle backend",
    );
}

#[test]
fn debugger_ui_has_one_instruction_trace_enable_owner() {
    assert!(
        RUNTIME_SOURCE.contains("sync_instruction_trace_capture(self, ctx)"),
        "runtime must centrally aggregate debugger trace demand before execution",
    );

    for (name, source) in [
        ("memory viewer", MEMORY_VIEWER_SOURCE),
        ("execution history", INSTRUCTION_HISTORY_SOURCE),
        ("debugger controls", DEBUGGER_CONTROLS_SOURCE),
        ("memory activity", MEMORY_ACTIVITY_SOURCE),
        ("loop inspector", LOOP_INSPECTOR_SOURCE),
    ] {
        assert!(
            !source.contains("set_instruction_trace_enabled("),
            "{name} must publish demand rather than directly owning the backend trace enable flag",
        );
    }
}

#[test]
fn loop_inspector_has_one_shared_native_viewport_implementation() {
    assert!(
        LOOP_INSPECTOR_SOURCE.contains("rustair-shared-8080-loop-inspector-viewport"),
        "shared Loop Inspector viewport is missing",
    );
    assert!(
        MEMORY_VIEWER_SOURCE.contains("open_loop_inspector(ui.ctx(), loop_info.clone())"),
        "RAM Viewer must delegate to the shared Loop Inspector",
    );
    assert!(
        INSTRUCTION_HISTORY_SOURCE.contains("open_loop_inspector(ui.ctx(), loop_info)"),
        "Execution History must delegate to the shared Loop Inspector",
    );
    assert!(
        !MEMORY_VIEWER_SOURCE.contains("rustair-ram-loop-inspector-viewport")
            && !MEMORY_VIEWER_SOURCE.contains("loop_inspector_open"),
        "RAM Viewer must not grow a second Loop Inspector implementation",
    );
}
