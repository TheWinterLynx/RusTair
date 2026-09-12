const MENU_SOURCE: &str = include_str!("../src/app/ui/main_menu.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");
const UI_SOURCE: &str = include_str!("../src/app/ui/mod.rs");
const ASR33_WINDOW_SOURCE: &str = include_str!("../src/app/ui/asr33_window.rs");
const ASR33_CONTROLLER_SOURCE: &str = include_str!("../src/app/asr33_controller.rs");
const TERMINAL_WINDOW_SOURCE: &str = include_str!("../src/app/ui/terminal.rs");
const EXTERNAL_TCP_SOURCE: &str = include_str!("../src/app/external_serial.rs");
const EXTERNAL_COM_SOURCE: &str = include_str!("../src/app/external_com.rs");
const DEBUGGER_SOURCE: &str = include_str!("../src/app/ui/debugger_controls.rs");
const LOOP_INSPECTOR_SOURCE: &str = include_str!("../src/app/ui/loop_inspector.rs");
const MEMORY_ACTIVITY_SOURCE: &str = include_str!("../src/app/ui/memory_activity.rs");
const BUS_TEACHER_SOURCE: &str = include_str!("../src/app/ui/bus_teacher.rs");
const S100_EDITOR_SOURCE: &str = include_str!("../src/app/ui/s100_hardware_editor.rs");
const AUDIO_SOURCE: &str = include_str!("../src/audio.rs");
const PERSISTENCE_SOURCE: &str = include_str!("../src/app/persistence.rs");

fn function_section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split(start)
        .nth(1)
        .unwrap_or_else(|| panic!("missing section start {start}"))
        .split(end)
        .next()
        .unwrap_or_else(|| panic!("missing section end {end}"))
}

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
fn peripherals_owns_device_window_launchers_not_transport_configuration() {
    let peripherals = function_section(
        MENU_SOURCE,
        "fn draw_peripherals_menu",
        "fn draw_view_menu",
    );

    assert!(peripherals.contains("ASR-33 Teletype"));
    assert!(peripherals.contains("app.asr33.window_open = true"));
    assert!(peripherals.contains("Text Terminal"));
    assert!(peripherals.contains("app.terminal.window_open = true"));
    assert!(peripherals.contains("ui.menu_button(\"External Serial\""));
    assert!(peripherals.contains("app.external_serial.window_open = true"));
    assert!(peripherals.contains("app.external_com.window_open = true"));
    assert!(!peripherals.contains("draw_external_serial_config_menu"));
    assert!(!peripherals.contains("draw_external_com_config_menu"));
}

#[test]
fn view_owns_operator_and_visuals_without_duplicate_peripheral_launchers() {
    let view = function_section(MENU_SOURCE, "fn draw_view_menu", "fn draw_tools_menu");
    assert!(view.contains("Front Panel Operator"));
    assert!(view.contains("LED Appearance…"));
    for duplicate in ["ASR-33 Teletype", "Text Terminal", "External Serial"] {
        assert!(
            !view.contains(duplicate),
            "View must not duplicate peripheral launcher {duplicate}"
        );
    }

    let file = function_section(MENU_SOURCE, "fn draw_file_menu", "fn draw_machine_menu");
    assert!(!file.contains("Front Panel Operator"));
    assert!(!file.contains("CPU Diagnostics"));
}

#[test]
fn settings_owns_external_tcp_and_com_configuration() {
    let settings = MENU_SOURCE
        .split("fn draw_settings_menu")
        .nth(1)
        .expect("settings menu function");
    assert!(settings.contains("ui.menu_button(\"External Serial\""));
    assert!(settings.contains("ui.menu_button(\"TCP\""));
    assert!(settings.contains("app.draw_external_serial_config_menu(ui)"));
    assert!(settings.contains("ui.menu_button(\"COM\""));
    assert!(settings.contains("app.draw_external_com_config_menu(ui)"));
}

#[test]
fn peripheral_speed_controls_live_only_in_their_device_windows() {
    let peripherals = function_section(
        MENU_SOURCE,
        "fn draw_peripherals_menu",
        "fn draw_view_menu",
    );

    for forbidden in [
        "Asr33Speed::ALL",
        "TerminalSpeed::ALL",
        "set_asr_speed",
        "set_terminal_speed",
    ] {
        assert!(
            !peripherals.contains(forbidden),
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
fn altair_and_asr33_audio_mutes_are_independent_and_available_where_expected() {
    assert!(MENU_SOURCE.contains("ui.checkbox(&mut altair_muted, \"Mute Altair\")"));
    assert!(MENU_SOURCE.contains("app.audio.set_altair_muted(altair_muted)"));

    let settings = MENU_SOURCE
        .split("fn draw_settings_menu")
        .nth(1)
        .expect("settings menu function");
    assert!(settings.contains("ui.menu_button(\"Audio\""));
    assert!(settings.contains("\"Mute Altair\""));
    assert!(settings.contains("\"Mute ASR-33\""));
    assert!(settings.contains("app.audio.set_altair_muted(altair_muted)"));
    assert!(settings.contains("app.audio.set_asr33_muted(asr33_muted)"));

    assert!(ASR33_WINDOW_SOURCE.contains("ui.checkbox(&mut muted, \"Mute audio\")"));
    assert!(ASR33_WINDOW_SOURCE.contains("self.audio.set_asr33_muted(muted)"));

    assert!(AUDIO_SOURCE.contains("altair_muted: bool"));
    assert!(AUDIO_SOURCE.contains("asr33_muted: bool"));
    assert!(AUDIO_SOURCE.contains("pub fn play_asr_once"));
    assert!(AUDIO_SOURCE.contains("pub fn start_asr_loop"));
    assert!(AUDIO_SOURCE.contains("AudioDomain::Altair"));
    assert!(AUDIO_SOURCE.contains("AudioDomain::Asr33"));

    assert!(ASR33_CONTROLLER_SOURCE.contains("self.audio.play_asr_once"));
    assert!(ASR33_CONTROLLER_SOURCE.contains("self.audio.start_asr_loop"));
    assert!(!ASR33_CONTROLLER_SOURCE.contains("self.audio.play_once("));
    assert!(!ASR33_CONTROLLER_SOURCE.contains("self.audio.start_loop("));

    assert!(PERSISTENCE_SOURCE.contains("audio.altair_muted"));
    assert!(PERSISTENCE_SOURCE.contains("audio.asr33_muted"));
    assert!(PERSISTENCE_SOURCE.contains("\"audio.muted\""));
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
    assert!(UI_SOURCE.contains("egui::FontId::new(14.0"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Body"));
    assert!(UI_SOURCE.contains("egui::FontId::new(15.0"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Button"));
    assert!(UI_SOURCE.contains("egui::TextStyle::Monospace"));
    assert!(UI_SOURCE.contains("style.spacing.item_spacing.y = 3.0;"));
    assert!(UI_SOURCE.contains("style.spacing.button_padding = egui::vec2(5.0, 1.0);"));
}

#[test]
fn auxiliary_status_bars_share_the_readable_small_text_policy() {
    assert!(ASR33_WINDOW_SOURCE.contains("TopBottomPanel::bottom(\"tty-status\")"));
    assert!(ASR33_WINDOW_SOURCE.contains("ui.small(format!("));
    assert!(TERMINAL_WINDOW_SOURCE.contains("TopBottomPanel::bottom(\"terminal-status\")"));
    assert!(TERMINAL_WINDOW_SOURCE.contains("ui.small(format!("));
    assert!(EXTERNAL_TCP_SOURCE.contains("TopBottomPanel::bottom(\"external-tcp-status\")"));
    assert!(EXTERNAL_TCP_SOURCE.contains("ui.small(self.external_tcp_status_text())"));
    assert!(EXTERNAL_COM_SOURCE.contains("TopBottomPanel::bottom(\"external-com-status\")"));
    assert!(EXTERNAL_COM_SOURCE.contains("ui.small(self.external_com_status_text())"));
    assert!(UI_SOURCE.contains("egui::FontId::new(14.0, egui::FontFamily::Proportional)"));
}

#[test]
fn resizable_tool_windows_wrap_controls_and_scroll_real_tables() {
    assert!(TERMINAL_WINDOW_SOURCE.contains("TopBottomPanel::top(\"terminal-menu\")"));
    assert!(TERMINAL_WINDOW_SOURCE.contains("ui.horizontal_wrapped(|ui|"));
    assert!(
        !TERMINAL_WINDOW_SOURCE.contains("egui::MenuBar::new()"),
        "Text Terminal must wrap like ASR-33 instead of clipping a rigid menu bar"
    );
    assert!(ASR33_WINDOW_SOURCE.contains("ui.horizontal_wrapped(|ui|"));

    assert!(DEBUGGER_SOURCE.contains("ui.horizontal_wrapped(|ui|"));
    assert!(DEBUGGER_SOURCE.contains(
        "egui::ScrollArea::both().id_salt(\"debugger-watchpoint-list\")"
    ));
    assert!(DEBUGGER_SOURCE.contains(".with_min_inner_size([680.0, 620.0])"));

    assert!(LOOP_INSPECTOR_SOURCE.contains("egui::ScrollArea::both()"));
    assert!(LOOP_INSPECTOR_SOURCE.contains(".with_min_inner_size([560.0, 360.0])"));

    assert!(MEMORY_ACTIVITY_SOURCE.contains("memory-activity-table-scroll"));
    assert!(MEMORY_ACTIVITY_SOURCE.contains("egui::ScrollArea::both()"));
    assert!(MEMORY_ACTIVITY_SOURCE.contains(".with_min_inner_size([760.0, 480.0])"));

    assert!(BUS_TEACHER_SOURCE.contains(
        "const BUS_TEACHER_TWO_COLUMN_MIN_WIDTH: f32 = 1160.0;"
    ));
    assert!(BUS_TEACHER_SOURCE.contains(
        "if ui.available_width() >= BUS_TEACHER_TWO_COLUMN_MIN_WIDTH"
    ));
    assert!(BUS_TEACHER_SOURCE.contains("ui.horizontal_wrapped(|ui|"));
    assert!(
        !BUS_TEACHER_SOURCE.contains(".exact_height(38.0)"),
        "T-state Teacher header must grow when its wrapped controls need another row"
    );

    assert!(S100_EDITOR_SOURCE.contains(".width_range(190.0..=360.0)"));
    assert!(S100_EDITOR_SOURCE.contains("ui.horizontal_wrapped(|ui|"));
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
