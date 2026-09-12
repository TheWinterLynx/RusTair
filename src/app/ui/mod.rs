use super::*;

mod asr33;
mod asr33_window;
pub(super) mod assets;
mod bus_teacher;
mod cpu_pin_diagram;
mod debugger_controls;
mod execution_position;
mod front_panel;
mod front_panel_assets;
mod front_panel_operator;
mod front_panel_switches;
mod instruction_history;
mod io_inspector;
mod loop_inspector;
mod main_menu;
mod memory_activity;
mod memory_viewer;
mod s100_hardware;
mod s100_hardware_editor;
mod s100_memory_inspection;
#[path = "../persistence.rs"]
pub(super) mod persistence;
pub(super) mod terminal;

/// Install one application-wide typography policy instead of fixing individual
/// windows piecemeal. The goal is noticeably better legibility while keeping
/// the desktop dense: text grows, but inter-widget vertical whitespace remains
/// compact. `Small` is deliberately still readable because it is also the text
/// style used by the ASR-33, text-terminal and external-serial status bars.
/// Photographic front-panel labels and the ASR-33's own teletype font use
/// explicit renderers and are therefore unaffected by these egui text styles.
pub(in crate::app) fn ensure_readable_ui_style(ctx: &egui::Context) {
    let installed = ctx.data_mut(|data| {
        let id = egui::Id::new("rustair-readable-ui-style-installed");
        let installed = *data.get_temp_mut_or(id, false);
        if !installed {
            data.insert_temp(id, true);
        }
        installed
    });
    if installed {
        return;
    }

    ctx.style_mut(|style| {
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(15.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(15.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(14.0, egui::FontFamily::Monospace),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(19.0, egui::FontFamily::Proportional),
        );

        // egui's default item spacing is already compact. Hold the vertical
        // spacing at 3 px and trim button padding so larger glyphs do not turn
        // menus/tool windows into substantially taller layouts.
        style.spacing.item_spacing.y = 3.0;
        style.spacing.button_padding = egui::vec2(5.0, 1.0);
    });
}

/// Standard collapsible section used by debugger/tool viewports.
///
/// `CollapsingHeader` owns the fold/unfold interaction; `Frame::group` only
/// supplies the visual border/background around the section body. Keeping this
/// helper in one place prevents individual tool windows from inventing slightly
/// different section behavior.
pub(super) fn collapsible_section(
    ui: &mut egui::Ui,
    title: &'static str,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    egui::CollapsingHeader::new(title)
        .default_open(default_open)
        .show(ui, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| add_contents(ui));
        });
}

pub(in crate::app) fn ensure_persistent_configuration_loaded(app: &mut RusTairApp) {
    app.ensure_persistent_configuration_loaded();
}

pub(in crate::app) fn persist_configuration_if_changed(app: &mut RusTairApp) {
    app.persist_configuration_if_changed();
}

pub(in crate::app) fn draw_main_menu(app: &mut RusTairApp, ctx: &egui::Context) {
    main_menu::draw_main_menu(app, ctx);
}

pub(in crate::app) fn open_led_visual_controls(app: &mut RusTairApp) {
    app.open_led_visual_controls();
}

pub(in crate::app) fn draw_s100_hardware_menu(app: &mut RusTairApp, ui: &mut egui::Ui) {
    s100_hardware::draw_s100_hardware_menu(app, ui);
}

pub(in crate::app) fn open_s100_hardware_editor(ctx: &egui::Context) {
    s100_hardware_editor::open_s100_hardware_editor(ctx);
}

pub(in crate::app) fn show_s100_hardware_editor(
    app: &mut RusTairApp,
    ctx: &egui::Context,
) {
    s100_hardware_editor::show_s100_hardware_editor(app, ctx);
}

fn instruction_trace_requested(ctx: &egui::Context) -> bool {
    instruction_history::trace_requested(ctx)
        || debugger_controls::trace_requested(ctx)
        || memory_activity::trace_requested(ctx)
        || loop_inspector::trace_requested(ctx)
        || memory_viewer::trace_requested(ctx)
}

/// One authoritative owner for the global instruction-trace switch. Individual
/// windows only publish demand through their UI state; none may enable/disable
/// the backend directly. Calling this before execution prevents a one-frame gap
/// when one consumer closes while another remains open.
pub(in crate::app) fn sync_instruction_trace_capture(
    app: &mut RusTairApp,
    ctx: &egui::Context,
) {
    let requested = instruction_trace_requested(ctx);
    if app.machine.instruction_trace_enabled() != requested {
        app.machine.set_instruction_trace_enabled(requested);
    }
}
