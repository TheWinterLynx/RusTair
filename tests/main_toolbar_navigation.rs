const MENU_SOURCE: &str = include_str!("../src/app/ui/main_menu.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const UI_SOURCE: &str = include_str!("../src/app/ui/mod.rs");

#[test]
fn main_menu_uses_six_clear_top_level_sections() {
    for section in ["File", "Machine", "Peripherals", "View", "Tools", "Settings"] {
        assert!(
            MENU_SOURCE.contains(&format!("ui.menu_button(\"{section}\"")),
            "missing top-level menu section {section}"
        );
    }
    assert!(!MENU_SOURCE.contains("ui.menu_button(\"Configuration\""));
}

#[test]
fn tools_group_debug_and_inspection_windows_instead_of_flat_toolbar_buttons() {
    for tool in [
        "Debugger",
        "RAM Viewer",
        "Execution History",
        "I/O Inspector",
        "T-State Teacher",
        "CPU Diagnostics",
    ] {
        assert!(MENU_SOURCE.contains(tool), "missing Tools entry {tool}");
    }
    assert!(!RUNTIME_SOURCE.contains("ui.button(\"DEBUGGER\")"));
    assert!(!RUNTIME_SOURCE.contains("ui.button(\"RAM VIEWER\")"));
    assert!(!RUNTIME_SOURCE.contains("ui.button(\"T-STATE TEACHER\")"));
}

#[test]
fn view_owns_operator_terminal_and_visual_windows() {
    assert!(MENU_SOURCE.contains("ui.menu_button(\"View\""));
    assert!(MENU_SOURCE.contains("Front Panel Operator"));
    assert!(MENU_SOURCE.contains("ASR-33 Teletype"));
    assert!(MENU_SOURCE.contains("Text Terminal"));
    assert!(MENU_SOURCE.contains("LED Appearance…"));
    assert!(!MENU_SOURCE.contains("File\", |ui| {\n        if ui.button(\"Front Panel Operator"));
}

#[test]
fn peripherals_group_external_tcp_and_com_configuration() {
    assert!(MENU_SOURCE.contains("ui.menu_button(\"Peripherals\""));
    assert!(MENU_SOURCE.contains("ui.menu_button(\"External Serial\""));
    assert!(MENU_SOURCE.contains("ui.menu_button(\"TCP\""));
    assert!(MENU_SOURCE.contains("app.draw_external_serial_config_menu(ui)"));
    assert!(MENU_SOURCE.contains("ui.menu_button(\"COM\""));
    assert!(MENU_SOURCE.contains("app.draw_external_com_config_menu(ui)"));
}

#[test]
fn s100_hardware_opens_a_dedicated_editor_viewport() {
    assert!(MENU_SOURCE.contains("S-100 Hardware…"));
    assert!(MENU_SOURCE.contains("super::open_s100_hardware_editor(ctx)"));
    assert!(UI_SOURCE.contains("mod s100_hardware_editor;"));
    assert!(RUNTIME_SOURCE.contains("super::ui::show_s100_hardware_editor(self, ctx);"));
}

#[test]
fn runtime_menu_is_navigation_only_and_machine_state_lives_in_status_bar() {
    assert!(RUNTIME_SOURCE.contains("super::ui::draw_main_menu(self, ctx);"));
    assert!(RUNTIME_SOURCE.contains("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}"));
    assert!(RUNTIME_SOURCE.contains("ui.strong(execution_state)"));
    assert!(!RUNTIME_SOURCE.contains("ASR-33 TELETYPE"));
    assert!(!RUNTIME_SOURCE.contains("EXEC HISTORY"));
    assert!(!RUNTIME_SOURCE.contains("PANEL OPERATOR"));
}
