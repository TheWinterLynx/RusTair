use super::*;

mod asr33;
mod asr33_window;
pub(super) mod assets;
mod debugger_controls;
mod execution_position;
mod front_panel;
mod front_panel_assets;
mod front_panel_operator;
mod front_panel_switches;
mod instruction_history;
mod io_inspector;
mod loop_inspector;
mod memory_activity;
mod memory_viewer;
#[path = "../persistence.rs"]
pub(super) mod persistence;
pub(super) mod terminal;

pub(in crate::app) fn ensure_persistent_configuration_loaded(app: &mut RusTairApp) {
    app.ensure_persistent_configuration_loaded();
}

pub(in crate::app) fn persist_configuration_if_changed(app: &mut RusTairApp) {
    app.persist_configuration_if_changed();
}

pub(in crate::app) fn open_led_visual_controls(app: &mut RusTairApp) {
    app.open_led_visual_controls();
}

fn instruction_trace_requested(ctx: &egui::Context) -> bool {
    instruction_history::trace_requested(ctx)
        || debugger_controls::trace_requested(ctx)
        || memory_activity::trace_requested(ctx)
        || memory_viewer::trace_requested(ctx)
        || loop_inspector::trace_requested(ctx)
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
