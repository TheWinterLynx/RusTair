use std::sync::{Mutex, OnceLock};

use super::super::*;
use crate::backend::simh::{active_console_snapshot, submit_active_console};

const VIEWPORT_ID: &str = "rustair-simh-console-viewport";

#[derive(Default)]
struct ConsoleUiState {
    open: bool,
    initialized: bool,
    input: String,
    local_error: Option<String>,
}

static CONSOLE_UI: OnceLock<Mutex<ConsoleUiState>> = OnceLock::new();

fn console_ui() -> &'static Mutex<ConsoleUiState> {
    CONSOLE_UI.get_or_init(|| Mutex::new(ConsoleUiState::default()))
}

impl RusTairApp {
    /// Show Open-SIMH diagnostics/console in a real child viewport.
    ///
    /// This is deliberately a deferred viewport: it owns its repaint cadence
    /// and therefore does not make the animated Altair panel repaint merely
    /// because console text changed.
    pub(in crate::app) fn draw_simh_console(&mut self, ctx: &egui::Context) {
        let simh_active = matches!(
            self.machine.engine(),
            EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80
        );
        if !simh_active {
            return;
        }

        let open = {
            let mut state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
            if !state.initialized {
                state.initialized = true;
                state.open = true;
            }
            state.open
        };

        if !open {
            egui::Area::new(egui::Id::new("rustair-simh-console-reopen"))
                .anchor(egui::Align2::RIGHT_TOP, [-12.0, 38.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    if ui.button("SIMH CONSOLE").clicked() {
                        let mut state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
                        state.open = true;
                    }
                });
            return;
        }

        ctx.show_viewport_deferred(
            egui::ViewportId::from_hash_of(VIEWPORT_ID),
            egui::ViewportBuilder::default()
                .with_title("Open-SIMH Console")
                .with_inner_size([760.0, 500.0])
                .with_min_inner_size([560.0, 340.0]),
            move |viewport_ctx, _class| {
                if viewport_ctx.input(|i| i.viewport().close_requested()) {
                    let mut state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
                    state.open = false;
                    return;
                }

                egui::CentralPanel::default().show(viewport_ctx, |ui| {
                    let snapshot = active_console_snapshot();
                    let Some(snapshot) = snapshot.as_ref() else {
                        ui.label("No active Open-SIMH worker.");
                        return;
                    };

                    ui.horizontal_wrapped(|ui| {
                        ui.strong(&snapshot.engine);
                        ui.separator();
                        ui.label(if snapshot.powered { "POWER ON" } else { "POWER OFF" });
                        ui.separator();
                        ui.label(if snapshot.running { "RUN" } else { "STOP" });
                        if snapshot.busy {
                            ui.separator();
                            ui.spinner();
                            ui.label("worker busy");
                        }
                    });

                    if let Some(error) = snapshot.last_error.as_ref() {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    }
                    {
                        let state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
                        if let Some(error) = state.local_error.as_ref() {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    }

                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("rustair-simh-console-scroll")
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .max_height(340.0)
                        .show(ui, |ui| {
                            if snapshot.lines.is_empty() {
                                ui.monospace("(no SIMH activity yet)");
                            } else {
                                ui.monospace(snapshot.lines.join("\n"));
                            }
                        });

                    ui.separator();
                    let command_enabled = snapshot.powered
                        && !snapshot.running
                        && !snapshot.busy
                        && snapshot.console_available;

                    let mut submit = false;
                    let mut quick_command: Option<&'static str> = None;
                    {
                        let mut state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
                        ui.add_enabled_ui(command_enabled, |ui| {
                            ui.horizontal(|ui| {
                                ui.monospace("sim>");
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut state.input)
                                        .desired_width(f32::INFINITY)
                                        .hint_text("SHOW CPU, SHOW DEVICES, HELP, ..."),
                                );
                                if response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    submit = true;
                                }
                                if ui.button("Send").clicked() {
                                    submit = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                for command in ["SHOW CPU", "SHOW DEVICES", "SHOW CONFIG", "HELP"] {
                                    if ui.small_button(command).clicked() {
                                        quick_command = Some(command);
                                    }
                                }
                            });
                        });

                        if let Some(command) = quick_command {
                            state.input = command.to_owned();
                            submit = true;
                        }
                    }

                    if submit {
                        let command = {
                            let state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
                            state.input.trim().to_owned()
                        };
                        if !command.is_empty() {
                            let result = submit_active_console(command);
                            let mut state = console_ui().lock().unwrap_or_else(|p| p.into_inner());
                            match result {
                                Ok(()) => {
                                    state.input.clear();
                                    state.local_error = None;
                                }
                                Err(error) => state.local_error = Some(error),
                            }
                        }
                    }

                    if !snapshot.console_available {
                        ui.small("The current embedded FrontPanel DLL does not expose the optional RusTair sim> command extension. Worker timing/log output is still available here.");
                    } else if !snapshot.powered {
                        ui.small("POWER ON the selected SIMH backend to use the interactive console.");
                    } else if snapshot.running {
                        ui.small("Press STOP before issuing interactive SCP commands.");
                    } else if snapshot.busy {
                        ui.small("The SIMH worker is completing an operation; this viewport remains independent of the main panel.");
                    }
                });

                viewport_ctx.request_repaint_after(Duration::from_millis(100));
            },
        );
    }
}
