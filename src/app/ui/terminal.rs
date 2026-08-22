use super::super::{
    egui, Pos2, Rect, RusTairApp, Sense, SerialBoard, SerialConnection, SerialDevice, TerminalSpeed,
};

const TERMINAL_INPUT_DEFAULT_HEIGHT: f32 = 235.0;
const TERMINAL_INPUT_MIN_HEIGHT: f32 = 115.0;
const TERMINAL_OUTPUT_MIN_HEIGHT: f32 = 100.0;
const TERMINAL_SPLITTER_THICKNESS: f32 = 6.0;

impl RusTairApp {
    fn draw_terminal_input(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(">").monospace().strong());
            let width = (ui.available_width() - 122.0).max(80.0);
            let response = ui.add_sized(
                [width, 26.0],
                egui::TextEdit::singleline(&mut self.terminal.command)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("command (blank + Enter sends CR)"),
            );
            let enter = response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Send").clicked() || enter {
                self.terminal_send_command();
                response.request_focus();
            }
            if ui.button("CR").on_hover_text("Send carriage return only").clicked() {
                self.terminal_send_control(b'\r', "CR");
                response.request_focus();
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Paste / program input");
            if ui.button("Send block").clicked() {
                self.terminal_send_program();
            }
            if ui.button("Clear editor").clicked() {
                self.terminal.program.clear();
            }
        });
        ui.small(format!(
            "Paste one or many lines. Input is paced at {}; newlines become carriage returns.",
            self.config.peripherals.terminal_speed.label()
        ));

        let editor_height = (ui.available_height() - 8.0).max(30.0);
        ui.add_sized(
            [ui.available_width(), editor_height],
            egui::TextEdit::multiline(&mut self.terminal.program)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
    }

    fn draw_terminal_output(&self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let output_height = ui.available_height();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .max_height(output_height)
                .show(ui, |ui| {
                    ui.set_min_height(output_height);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.terminal.output)
                                .monospace()
                                .size(15.0),
                        )
                        .selectable(true),
                    );
                });
        });
    }

    fn draw_terminal_connection_selector(&mut self, ui: &mut egui::Ui) {
        let board = self.config.machine.serial_board;
        let current = self.terminal_connection();
        let mut selected = current;

        ui.label("Connection:");
        egui::ComboBox::from_id_salt("text-terminal-serial-connection")
            .selected_text(Self::serial_connection_label(board, current))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut selected,
                    SerialConnection::Disconnected,
                    "Disconnected",
                );
                ui.selectable_value(
                    &mut selected,
                    SerialConnection::Port0,
                    Self::serial_connection_label(board, SerialConnection::Port0),
                );
                if board == SerialBoard::TwoSio88 {
                    ui.selectable_value(
                        &mut selected,
                        SerialConnection::Port1,
                        Self::serial_connection_label(board, SerialConnection::Port1),
                    );
                }
            });

        if selected != current {
            self.set_serial_connection(SerialDevice::TextTerminal, selected);
        }
    }

    fn draw_terminal_speed_selector(&mut self, ui: &mut egui::Ui) {
        let current = self.config.peripherals.terminal_speed;
        let mut selected = current;
        ui.label("Speed:");
        egui::ComboBox::from_id_salt("terminal-speed")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for speed in TerminalSpeed::ALL {
                    ui.selectable_value(&mut selected, speed, speed.label());
                }
            });
        if selected != current {
            self.set_terminal_speed(selected);
        }
    }

    fn draw_terminal_window(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("terminal-menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.draw_terminal_connection_selector(ui);
                ui.separator();
                self.draw_terminal_speed_selector(ui);
                ui.separator();
                if ui.button("Clear").clicked() {
                    self.terminal.clear_output();
                }
                if ui.button("Send text/BASIC file…").clicked() {
                    self.load_terminal_text_file();
                }
                ui.separator();
                ui.checkbox(&mut self.terminal.uppercase, "Uppercase input");
                ui.separator();
                if ui.button("CTRL-C").clicked() {
                    self.terminal_send_control(0x03, "CTRL-C");
                }
                if ui.button("ESC").clicked() {
                    self.terminal_send_control(0x1b, "ESC");
                }
            });
        });

        egui::TopBottomPanel::bottom("terminal-status").show(ctx, |ui| {
            let connection = self.terminal_connection();
            let connection_label =
                Self::serial_connection_label(self.config.machine.serial_board, connection);
            let tx = if connection.is_connected() {
                if self.terminal_serial_tx_busy() { "BUSY" } else { "READY" }
            } else {
                "N/A"
            };
            ui.small(format!(
                "TEXT TERMINAL  |  {}  |  {}  |  input pending {}  |  RX register {}  |  TX {}  |  {} chars",
                connection_label,
                self.config.peripherals.terminal_speed.label(),
                self.terminal.input_pending_len(),
                self.terminal_serial_rx_len(),
                tx,
                self.terminal.output.len(),
            ));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_rect_before_wrap();
            let splitter_state_id = egui::Id::new("rustair-terminal-input-height");

            let max_input_height = (available.height()
                - TERMINAL_OUTPUT_MIN_HEIGHT
                - TERMINAL_SPLITTER_THICKNESS)
                .max(TERMINAL_INPUT_MIN_HEIGHT);

            let desired_input_height = ctx
                .data(|data| data.get_temp::<f32>(splitter_state_id))
                .unwrap_or(TERMINAL_INPUT_DEFAULT_HEIGHT);
            let mut input_height = desired_input_height
                .clamp(TERMINAL_INPUT_MIN_HEIGHT, max_input_height);
            let mut splitter_y = available.max.y - input_height;

            let splitter_hit_rect = Rect::from_min_max(
                Pos2::new(
                    available.min.x,
                    splitter_y - TERMINAL_SPLITTER_THICKNESS * 0.5,
                ),
                Pos2::new(
                    available.max.x,
                    splitter_y + TERMINAL_SPLITTER_THICKNESS * 0.5,
                ),
            );
            let splitter_id = ui.make_persistent_id("terminal-input-splitter");
            let response = ui.interact(splitter_hit_rect, splitter_id, Sense::drag());

            if response.hovered() || response.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }

            if response.dragged() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    input_height = (available.max.y - pointer.y)
                        .clamp(TERMINAL_INPUT_MIN_HEIGHT, max_input_height);
                    splitter_y = available.max.y - input_height;
                    ctx.data_mut(|data| {
                        data.insert_temp(splitter_state_id, input_height);
                    });
                    ctx.request_repaint();
                }
            }

            let half_splitter = TERMINAL_SPLITTER_THICKNESS * 0.5;
            let output_rect = Rect::from_min_max(
                available.min,
                Pos2::new(available.max.x, splitter_y - half_splitter),
            );
            let input_rect = Rect::from_min_max(
                Pos2::new(available.min.x, splitter_y + half_splitter),
                available.max,
            );

            let separator_stroke = if response.hovered() || response.dragged() {
                ui.visuals().widgets.hovered.fg_stroke
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke
            };
            ui.painter().line_segment(
                [
                    Pos2::new(available.min.x, splitter_y),
                    Pos2::new(available.max.x, splitter_y),
                ],
                separator_stroke,
            );

            let mut output_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("terminal-output-area")
                    .max_rect(output_rect),
            );
            output_ui.set_clip_rect(output_rect);
            self.draw_terminal_output(&mut output_ui);

            let mut input_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("terminal-input-area")
                    .max_rect(input_rect),
            );
            input_ui.set_clip_rect(input_rect);
            self.draw_terminal_input(&mut input_ui);

            ui.advance_cursor_after_rect(available);
        });
    }

    pub(in crate::app) fn show_terminal_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.terminal.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-text-terminal"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Text Terminal")
                .with_inner_size([820.0, 640.0])
                .with_min_inner_size([520.0, 360.0])
                .with_resizable(true),
            |terminal_ctx, _class| {
                self.draw_terminal_window(terminal_ctx);
                if terminal_ctx.input(|i| i.viewport().close_requested()) {
                    self.terminal.window_open = false;
                }
            },
        );
    }
}
