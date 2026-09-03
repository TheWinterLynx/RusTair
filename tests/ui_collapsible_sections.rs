#[test]
fn shared_tool_section_uses_collapsing_header_and_group_frame() {
    let ui_mod = include_str!("../src/app/ui/mod.rs");
    assert!(ui_mod.contains("pub(super) fn collapsible_section"));
    assert!(ui_mod.contains("egui::CollapsingHeader::new(title)"));
    assert!(ui_mod.contains("egui::Frame::group(ui.style())"));
}

fn assert_sections(source: &str, sections: &[&str]) {
    for section in sections {
        let direct = format!("collapsible_section(ui, \"{section}\"");
        assert!(
            source.contains(&direct),
            "expected conceptual UI section {section:?} to use the shared collapsible_section helper"
        );
    }
}

#[test]
fn ram_inspector_primary_sidebar_sections_are_collapsible() {
    let source = include_str!("../src/app/ui/memory_viewer.rs");
    assert_sections(
        source,
        &[
            "CURRENT INSTRUCTION",
            "Explain selected instruction",
            "Selected byte / editor",
            "S-100 RAM cards / physical map",
            "Memory activity overlay",
            "CPU REGISTERS",
            "How to read this inspector",
        ],
    );
}

#[test]
fn debugger_and_history_primary_sections_are_collapsible() {
    let debugger = include_str!("../src/app/ui/debugger_controls.rs");
    assert_sections(
        debugger,
        &[
            "Current instruction / status",
            "Execution controls",
            "Run to address",
            "Execute breakpoints",
            "Memory watchpoints",
            "Inference status",
            "Observed call frames",
            "Raw stack top",
        ],
    );

    let history = include_str!("../src/app/ui/instruction_history.rs");
    assert_sections(
        history,
        &[
            "Capture controls",
            "Completed instructions",
            "WHAT JUST HAPPENED?",
            "State changes",
            "Memory / I/O effects",
            "Before / after registers",
        ],
    );
}

#[test]
fn execution_history_dynamic_detail_sections_reserve_stable_heights() {
    let history = include_str!("../src/app/ui/instruction_history.rs");
    for constant in [
        "HISTORY_DETAIL_SUMMARY_HEIGHT",
        "HISTORY_STATE_CHANGES_HEIGHT",
        "HISTORY_EFFECTS_HEIGHT",
        "HISTORY_REGISTERS_HEIGHT",
    ] {
        assert!(history.contains(constant), "missing stable history-detail height {constant}");
    }
    assert!(history.contains("fn fixed_detail_body("));
    assert!(history.contains("instruction-history-effects-body"));
    assert!(history.contains("instruction-history-before-after-body"));
    assert!(
        !history.contains("fn draw_effect(&self, ui: &mut egui::Ui, effect: InstructionEffect8080) {\n        ui.horizontal_wrapped"),
        "effect rows must not wrap and change section geometry while following live history",
    );
}

#[test]
fn ram_activity_overlay_has_explicit_three_slot_markers() {
    let source = include_str!("../src/app/ui/memory_viewer.rs");
    assert!(source.contains("fn draw_activity_stripes("));
    assert!(source.contains("top = EXEC, middle = READ, bottom = WRITE"));
    assert!(source.contains("IN/OUT are I/O bus activity, not RAM activity"));
    assert!(source.contains("Open full Memory Activity"));
}

#[test]
fn ram_inspector_describes_physical_mapping_states() {
    let source = include_str!("../src/app/ui/memory_viewer.rs");
    assert!(source.contains("-- = UNMAPPED/open bus FFh"));
    assert!(source.contains("underlined byte = non-contended OVERLAP"));
    assert!(source.contains("!! = electrical CONTENTION"));
    assert!(source.contains("Patch physical RAM byte"));
    assert!(!source.contains("1 KiB protection map"));
    assert!(!source.contains("guest reads return 00h"));
}

#[test]
fn bus_teacher_sections_are_collapsible_and_explanation_height_is_stable() {
    let source = include_str!("../src/app/ui/bus_teacher.rs");
    assert_sections(
        source,
        &[
            "Teaching source / accuracy",
            "Execution stepping",
            "Instruction / machine cycle / T-state",
            "Intel 8080 pins",
            "S-100 status / front-panel LEDs",
            "Why are these signals active?",
        ],
    );
    assert!(source.contains("const WHY_HEIGHT: f32"));
    assert!(source.contains("bus-teacher-why-scroll"));
}

#[test]
fn auxiliary_tool_viewports_use_collapsible_conceptual_sections() {
    let loop_inspector = include_str!("../src/app/ui/loop_inspector.rs");
    assert_sections(loop_inspector, &["Loop state / exit condition", "Loop instructions"]);

    let memory_activity = include_str!("../src/app/ui/memory_activity.rs");
    assert_sections(
        memory_activity,
        &["Activity meaning / capture status", "Sort / controls", "Activity table"],
    );

    let io_inspector = include_str!("../src/app/ui/io_inspector.rs");
    assert_sections(
        io_inspector,
        &[
            "Selected I/O port",
            "Status interpretation",
            "Debugger I/O controls",
            "Serial DATA-port tools",
            "I/O port map 00h–FFh",
            "How to use the serial traces",
        ],
    );

    let terminal = include_str!("../src/app/ui/terminal.rs");
    assert_sections(terminal, &["Command / input", "Paste / program input", "Terminal output"]);
}

#[test]
fn transport_viewports_use_shared_collapsible_sections() {
    let tcp = include_str!("../src/app/external_serial.rs");
    assert_sections(
        tcp,
        &[
            "TCP endpoint configuration",
            "Connection guide",
            "Transport state",
            "Connected clients",
            "Transport actions",
            "How the serial bridge behaves",
        ],
    );

    let com = include_str!("../src/app/external_com.rs");
    assert_sections(
        com,
        &["Transport state", "Transport actions", "How the COM bridge behaves"],
    );
}

#[test]
fn modal_diagnostic_result_windows_remain_explicitly_non_collapsible() {
    // These are acknowledgement/result dialogs rather than tool sections. Keep
    // their OK action visible instead of allowing the whole modal to collapse.
    let cpu = include_str!("../src/app/cpu_diagnostics.rs");
    assert!(cpu.contains("egui::Window::new(\"CPU diagnostic complete\")"));
    assert!(cpu.contains(".collapsible(false).resizable(false)"));

    let embedded = include_str!("../src/app/embedded_cpu_diagnostics.rs");
    assert!(embedded.contains("egui::Window::new(\"CPU diagnostic complete\")"));
    assert!(embedded.contains("egui::Window::new(\"CPU diagnostic suite complete\")"));
    assert!(embedded.contains(".collapsible(false).resizable(true)"));
}
