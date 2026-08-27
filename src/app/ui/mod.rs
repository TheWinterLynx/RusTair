use super::*;

mod asr33;
mod asr33_window;
pub(super) mod assets;
mod debugger_controls;
mod front_panel;
mod front_panel_assets;
mod front_panel_operator;
mod front_panel_switches;
mod instruction_history;
mod io_inspector;
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
