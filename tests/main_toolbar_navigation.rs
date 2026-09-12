const MENU_SOURCE: &str = include_str!("../src/app/ui/main_menu.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const UI_SOURCE: &str = include_str!("../src/app/ui/mod.rs");
const ASR33_WINDOW_SOURCE: &str = include_str!("../src/app/ui/asr33_window.rs");
const TERMINAL_WINDOW_SOURCE: &str = include_str!("../src/app/ui/terminal.rs");

#[test]
fn main_menu_uses_six_clear_top_level_sections() {
    for section in ["File", "Machine", "Peripherals", "View", "Tools", "Settings"] {
        let needle = format!("ui.menu_button(\"{section}\"");
        assert!(
            MENU_SOURCE.contains(needle.as_str()),
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

    let file_section = MENU_SOURCE
        .split("fn draw_file_menu")
        .nth(1)
        .expect("file menu function")
        .split("fn draw_machine_menu")
        .next()
        .expect("file menu body");
    assert!(!file_section.contains("Front Panel Operator"));
    assert!(!file_section.contains("CPU Diagnostics"));
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
fn peripheral_speed_controls_live_only_in_their_device_windows() {
    let peripherals_section = MENU_SOURCE
        .split("fn draw_peripherals_menu")
        .nth(1)
        .expect("peripherals menu function")
        .split("fn draw_view_menu")
        .next()
        .expect("peripherals menu body");

    for forbidden in [
        "Asr33Speed::ALL",
        "TerminalSpeed::ALL",
        "set_asr_speed",
        "set_terminal_speed",
    ] {
        assert!(
            !peripherals_section.contains(forbidden),
            "main Peripherals menu must not own device speed control: {forbidden}"
        );
    }

    assert!(ASR33_WINDOW_SOURCE.contains("fn draw_tty_speed_selector"));
    assert!(ASR33_WINDOW_SOURCE.contains("Asr33Speed::ALL"));
    assert!(ASR33_WINDOW_SOURCE.contains("self.set_asr_speed(selected)"));

    assert!(TERMINAL_WINDOW_SOURCE.contains("fn draw_terminal_speed_selector"));
    assert!(TERMINAL_WINDOW_SOURCE.contains("TerminalSpeed::ALL"));
    assert!(TERMINAL_WINDOW_SOURCE.contains("self.set_terminal_speed(selected)"));
}

#[test]
fn s100_hardware_opens_a_dedicated_editor_viewport() {
    assert!(MENU_SOURCE.contains("S-100 Hardware…"));
    assert!(MENU_SOURCE.contains("super::open_s100_hardware_editor(ctx)"));
    assert!(UI_SOURCE.contains("mod s100_hardware_editor;"));
    assert!(RUNTIME_SOURCE.contains("super::ui::show_s100_hardware_editor(self, ctx);"));
}

#[test]
fn application_uses_one_readable_dense_typography_policy() {
    assert!(RUNTIME_SOURCE.contains("super::ui::ensure_readable_ui_style(ctx);"));
    assert!(UI_SOURCE.contains("pub(in crate::app) fn ensure_readable_ui_style"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Small"));
    assert!(UI_SOURCE.contains("egui::FontId::new(12.5"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Body"));
    assert!(UI_SOURCE.contains("egui::FontId::new(15.0"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Button"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Monospace"));
    assert!(UI_SOURCE.contains("egui::FontId::new(14.0"));
    assert!(UI_SOURCE.contains("style.spacing.item_spacing.y = 3.0;"));
    assert!(UI_SOURCE.contains("style.spacing.button_padding = egui::vec2(5.0, 1.0);"));
}

#[test]
fn runtime_menu_is_navigation_only_and_status_bar_is_stable_and_responsive() {
    assert!(RUNTIME_SOURCE.contains("super::ui::draw_main_menu(self, ctx);"));
    assert!(RUNTIME_SOURCE.contains("const STATUS_BAR_FONT_SIZE: f32 = 16.0;"));
    assert!(RUNTIME_SOURCE.contains("const STATUS_BAR_STATE_WIDTH: f32 = 150.0;"));
    assert!(RUNTIME_SOURCE.contains("STATUS_BAR_REGISTERS_COMPACT_WIDTH: f32 = 285.0"));
    assert!(RUNTIME_SOURCE.contains("STATUS_BAR_REGISTERS_EXTENDED_WIDTH: f32 = 555.0"));
    assert!(RUNTIME_SOURCE.contains("STATUS_BAR_EXTENDED_BREAKPOINT: f32 = 1120.0"));
    assert!(RUNTIME_SOURCE.contains("const STATUS_BAR_SPEED_WIDTH: f32 = 165.0;"));
    assert!(RUNTIME_SOURCE.contains("ui.allocate_exact_size("));
    assert!(RUNTIME_SOURCE.contains("let message_rect = egui::Rect::from_min_max("));
    assert!(RUNTIME_SOURCE.contains("ui.painter().line_segment("));
    assert!(RUNTIME_SOURCE.contains("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}"));
    assert!(RUNTIME_SOURCE.contains("BC {:04X} DE {:04X} HL {:04X}"));
    assert!(RUNTIME_SOURCE.contains("cpu.bc()"));
    assert!(RUNTIME_SOURCE.contains("cpu.de()"));
    assert!(RUNTIME_SOURCE.contains("cpu.hl()"));
    assert!(RUNTIME_SOURCE.contains("S{}Z{}A{}P{}C{}"));
    assert!(RUNTIME_SOURCE.contains("RichText::new(execution_state)"));
    assert!(RUNTIME_SOURCE.contains("Speed: Unlimited"));
    assert!(RUNTIME_SOURCE.contains("Saved configuration loaded"));
    assert!(RUNTIME_SOURCE.contains("self.status.starts_with(\"Ready —\")"));
    assert!(RUNTIME_SOURCE.contains(".truncate()"));

    for forbidden in [
        "Core: {}",
        "RichText::new(\"│\")",
        "STATUS_BAR_PC_WIDTH",
        "STATUS_BAR_SEPARATOR_WIDTH",
        "Layout::right_to_left",
        "RusTair Adaptive Cycle 8080",
    ] {
        assert!(
            !RUNTIME_SOURCE.contains(forbidden),
            "status bar must not contain obsolete or unstable field: {forbidden}"
        );
    }

    assert!(!RUNTIME_SOURCE.contains("ASR-33 TELETYPE"));
    assert!(!RUNTIME_SOURCE.contains("EXEC HISTORY"));
    assert!(!RUNTIME_SOURCE.contains("PANEL OPERATOR"));
}
