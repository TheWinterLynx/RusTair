use super::super::*;
use crate::backend::simh::{active_console_snapshot, submit_active_console};

const OPEN_ID: &str = "rustair-simh-console-open";
const INPUT_ID: &str = "rustair-simh-console-input";

impl RusTairApp {
    /// Draw a nonblocking console/log window for the currently selected embedded
    /// Open-SIMH backend. The log works with every RusTair bundle. Interactive
    /// SCP commands become available when the embedded FrontPanel DLL exports
    /// the RusTair console extension.
    pub(in crate::app) fn draw_simh_console(&mut self, ctx: &egui::Context) {
        let simh_active = matches!(
            self.machine.engine(),
            EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80
        );
        if !simh_active {
            return;
        }

        let open_id = egui::Id::new(OPEN_ID);
        let mut open = ctx.data_mut(|data| *data.get_temp_mut_or(open_id, true));

        // Keep a small reopen affordance independent of the main menu. This is
        // intentionally always responsive because it only touches egui state.
        if !open {
            egui::Area::new(egui::Id::new("rustair-simh-console-reopen"))
                .anchor(egui::Align2::RIGHT_TOP, [-12.0, 38.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    if ui.button("SIMH CONSOLE").clicked() {
                        open = true;
                    }
                });
        }

        if !open {
            ctx.data_mut(|data| data.insert_temp(open_id, open));
            return;
        }

        let snapshot = active_console_snapshot();
        let input_id = egui::Id::new(INPUT_ID);
        let mut input = ctx.data(|data| data.get_temp::<String>(input_id).unwrap_or_default());
        let mut submit = false;

        let mut window_open = open;
        egui::Window::new("Open-SIMH Console")
            .id(egui::Id::new("rustair-simh-console-window"))
            .open(&mut window_open)
            .default_width(720.0)
            .default_height(430.0)
            .resizable(true)
            .show(ctx, |ui| {
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

                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("rustair-simh-console-scroll")
                    .stick_to_bottom(true)
                    .max_height(300.0)
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

                ui.add_enabled_ui(command_enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.monospace("sim>");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut input)
                                .desired_width(f32::INFINITY)
                                .hint_text("SHOW CPU, SHOW DEVICES, HELP, ..."),
                        );
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            submit = true;
                        }
                        if ui.button("Send").clicked() {
                            submit = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        for command in ["SHOW CPU", "SHOW DEVICES", "SHOW CONFIG", "HELP"] {
                            if ui.small_button(command).clicked() {
                                input = command.to_owned();
                                submit = true;
                            }
                        }
                    });
                });

                if !snapshot.console_available {
                    ui.small("Interactive SCP commands require the current RusTair FrontPanel DLL extension. The worker log above is already available; rebuild/update the embedded SIMH bundle once to enable the sim> prompt.");
                } else if !snapshot.powered {
                    ui.small("POWER ON the selected SIMH backend to use the interactive console.");
                } else if snapshot.running {
                    ui.small("Press STOP before issuing SCP commands, matching the real SIMH sim> prompt.");
                } else if snapshot.busy {
                    ui.small("The SIMH worker is completing an operation; the UI remains responsive.");
                }
            });

        open = window_open;
        if submit {
            let command = input.trim().to_owned();
            if !command.is_empty() {
                match submit_active_console(command.clone()) {
                    Ok(()) => input.clear(),
                    Err(error) => self.status = format!("SIMH console: {error}"),
                }
            }
        }

        ctx.data_mut(|data| {
            data.insert_temp(open_id, open);
            data.insert_temp(input_id, input);
        });
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}
