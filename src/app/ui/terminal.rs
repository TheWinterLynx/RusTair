use super::super::{RusTairApp, SerialBoard, SerialConnection, SerialDevice, TerminalSpeed, egui};
use crate::app::adm3a_state::{ADM3A_COLS, ADM3A_ROWS};
use crate::config::TerminalDuplex;

const ADM3A_SCREEN_LEFT: f32 = 0.289;
const ADM3A_SCREEN_TOP: f32 = 0.161;
const ADM3A_SCREEN_RIGHT: f32 = 0.713;
const ADM3A_SCREEN_BOTTOM: f32 = 0.614;
const ADM3A_CRT_ASPECT: f32 = 1.25;

impl RusTairApp {
    fn draw_terminal_input(&mut self, ui: &mut egui::Ui) {
        super::collapsible_section(ui, "Command / input", true, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(">").monospace().strong());
                let width = (ui.available_width() - 122.0).max(80.0);
                let response = ui.add_sized(
                    [width, 26.0],
                    egui::TextEdit::singleline(&mut self.terminal.command)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("command"),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Send").clicked() || enter {
                    self.terminal_send_command();
                    response.request_focus();
                }
                if ui
                    .button("CR")
                    .on_hover_text("Send carriage return only")
                    .clicked()
                {
                    self.terminal_send_control(b'\r', "CR");
                    response.request_focus();
                }
            });
        });

        ui.separator();
        super::collapsible_section(ui, "Paste / program input", true, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Send block").clicked() {
                    self.terminal_send_program();
                }
                if ui.button("Clear editor").clicked() {
                    self.terminal.program.clear();
                }
            });
            ui.small(format!(
                "One or many lines; input is paced at {} and newlines become carriage returns.",
                self.config.peripherals.terminal_speed.label()
            ));

            let editor_height = (ui.available_height() - 8.0).max(80.0);
            ui.add_sized(
                [ui.available_width(), editor_height],
                egui::TextEdit::multiline(&mut self.terminal.program)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
        });
    }

    fn draw_terminal_output(&self, ui: &mut egui::Ui) {
        super::collapsible_section(ui, "Terminal output", true, |ui| {
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
        let hardware = self.config.machine.s100_hardware;
        let board = hardware.active_serial_board();
        let current = self.terminal_connection();
        let mut selected = current;

        ui.label("Connection:");
        egui::ComboBox::from_id_salt("text-terminal-serial-connection")
            .selected_text(Self::serial_connection_label(hardware, current))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut selected,
                    SerialConnection::Disconnected,
                    "Disconnected",
                );
                if board.is_some() {
                    ui.selectable_value(
                        &mut selected,
                        SerialConnection::Port0,
                        Self::serial_connection_label(hardware, SerialConnection::Port0),
                    );
                }
                if board == Some(SerialBoard::TwoSio88) {
                    ui.selectable_value(
                        &mut selected,
                        SerialConnection::Port1,
                        Self::serial_connection_label(hardware, SerialConnection::Port1),
                    );
                }
            });
        if board.is_none() {
            ui.small("Install an 88-SIO or 88-2SIO in Configuration → S-100 Chassis / Cards to attach a cable.");
        }

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

    fn draw_terminal_duplex_selector(&mut self, ui: &mut egui::Ui) {
        ui.label("Duplex:");
        egui::ComboBox::from_id_salt("terminal-duplex")
            .selected_text(self.terminal.duplex.label())
            .show_ui(ui, |ui| {
                for duplex in TerminalDuplex::ALL {
                    ui.selectable_value(&mut self.terminal.duplex, duplex, duplex.label());
                }
            });
    }

    fn draw_terminal_window(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("terminal-menu")
            .resizable(false)
            .show(ctx, |ui| {
                // Match the ASR-33 toolbar model: controls wrap onto additional
                // rows instead of being clipped when the viewport narrows.
                ui.horizontal_wrapped(|ui| {
                    self.draw_terminal_connection_selector(ui);
                    ui.separator();
                    self.draw_terminal_speed_selector(ui);
                    ui.separator();
                    self.draw_terminal_duplex_selector(ui);
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
                Self::serial_connection_label(self.config.machine.s100_hardware, connection);
            let tx = if connection.is_connected() {
                if self.terminal_serial_tx_busy() {
                    "BUSY"
                } else {
                    "READY"
                }
            } else {
                "N/A"
            };
            ui.small(format!(
                "TEXT TERMINAL  |  {}  |  {}  |  {}  |  pending {}  |  RX {}  |  TX {}  |  {} chars",
                connection_label,
                self.config.peripherals.terminal_speed.label(),
                self.terminal.duplex.label(),
                self.terminal.input_pending_len(),
                self.terminal_serial_rx_len(),
                tx,
                self.terminal.output.len(),
            ));
        });

        egui::SidePanel::right("terminal-input-panel")
            .resizable(true)
            .default_width(360.0)
            .width_range(280.0..=620.0)
            .show(ctx, |ui| self.draw_terminal_input(ui));

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_terminal_output(ui);
        });
    }

    fn adm3a_crt_viewport_open_id() -> egui::Id {
        egui::Id::new("rustair-adm3a-crt-viewport-open")
    }

    pub(in crate::app) fn open_adm3a_viewport(&mut self, ctx: &egui::Context) {
        self.adm3a.window_open = true;
        ctx.request_repaint();
    }

    fn open_adm3a_crt_viewport(ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(Self::adm3a_crt_viewport_open_id(), true));
        ctx.request_repaint();
    }

    fn draw_adm3a_connection_selector(&mut self, ui: &mut egui::Ui) {
        let hardware = self.config.machine.s100_hardware;
        let current = self.adm3a_connection();
        let mut selected = current;

        ui.label("RS-232 cable:");
        egui::ComboBox::from_id_salt("adm3a-serial-connection")
            .selected_text(Self::serial_connection_label(hardware, current))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut selected,
                    SerialConnection::Disconnected,
                    "Disconnected",
                );
                for connection in [SerialConnection::Port0, SerialConnection::Port1] {
                    if Self::serial_connection_supported(hardware, SerialDevice::Adm3a, connection)
                    {
                        ui.selectable_value(
                            &mut selected,
                            connection,
                            Self::serial_connection_label(hardware, connection),
                        );
                    }
                }
            });

        if selected != current {
            self.set_serial_connection(SerialDevice::Adm3a, selected);
        }

        if ![SerialConnection::Port0, SerialConnection::Port1]
            .into_iter()
            .any(|connection| {
                Self::serial_connection_supported(hardware, SerialDevice::Adm3a, connection)
            })
        {
            ui.small("The ADM-3A needs a physical RS-232 interface; no hidden level converter is inserted.");
        }
    }

    fn adm3a_screen_rect(shell_rect: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::Pos2::new(
                shell_rect.left() + shell_rect.width() * ADM3A_SCREEN_LEFT,
                shell_rect.top() + shell_rect.height() * ADM3A_SCREEN_TOP,
            ),
            egui::Pos2::new(
                shell_rect.left() + shell_rect.width() * ADM3A_SCREEN_RIGHT,
                shell_rect.top() + shell_rect.height() * ADM3A_SCREEN_BOTTOM,
            ),
        )
    }

    fn fit_adm3a_crt_rect(available: egui::Rect) -> egui::Rect {
        let available_aspect = available.width() / available.height().max(1.0);
        let size = if available_aspect > ADM3A_CRT_ASPECT {
            egui::Vec2::new(available.height() * ADM3A_CRT_ASPECT, available.height())
        } else {
            egui::Vec2::new(available.width(), available.width() / ADM3A_CRT_ASPECT)
        };
        egui::Rect::from_center_size(available.center(), size)
    }

    fn draw_adm3a_crt(&self, painter: &egui::Painter, screen_rect: egui::Rect) {
        let corner_radius = ((screen_rect.height() * 0.045).clamp(4.0, 24.0)) as u8;
        painter.rect_filled(
            screen_rect,
            egui::CornerRadius::same(corner_radius),
            egui::Color32::from_rgb(10, 13, 11),
        );

        let active_rect = screen_rect.shrink2(egui::Vec2::new(
            screen_rect.width() * 0.055,
            screen_rect.height() * 0.070,
        ));
        let cell_width = active_rect.width() / ADM3A_COLS as f32;
        let cell_height = active_rect.height() / ADM3A_ROWS as f32;
        let font = egui::FontId::monospace((cell_height * 0.66).max(4.0));
        let glyph_color = egui::Color32::from_rgb(191, 225, 196);
        let clipped = painter.with_clip_rect(screen_rect);

        for row in 0..ADM3A_ROWS {
            for col in 0..ADM3A_COLS {
                let byte = self.adm3a.row(row)[col];
                if byte == b' ' {
                    continue;
                }
                let pos = egui::Pos2::new(
                    active_rect.left() + (col as f32 + 0.5) * cell_width,
                    active_rect.top() + (row as f32 + 0.5) * cell_height,
                );
                clipped.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    char::from(byte),
                    font.clone(),
                    glyph_color,
                );
            }
        }

        let (cursor_col, cursor_row) = self.adm3a.cursor();
        let cursor_cell = egui::Rect::from_min_size(
            egui::Pos2::new(
                active_rect.left() + cursor_col as f32 * cell_width,
                active_rect.top() + cursor_row as f32 * cell_height,
            ),
            egui::Vec2::new(cell_width, cell_height),
        );
        let cursor = egui::Rect::from_min_max(
            egui::Pos2::new(
                cursor_cell.left() + cell_width * 0.12,
                cursor_cell.bottom() - cell_height * 0.18,
            ),
            egui::Pos2::new(
                cursor_cell.right() - cell_width * 0.12,
                cursor_cell.bottom() - cell_height * 0.08,
            ),
        );
        clipped.rect_filled(
            cursor,
            egui::CornerRadius::same(1),
            egui::Color32::from_rgb(205, 238, 210),
        );
    }

    fn draw_adm3a_shell(&self, ui: &mut egui::Ui) {
        let Some(texture) = &self.tex.adm3a_shell else {
            ui.label("ADM-3A shell asset unavailable");
            return;
        };

        let source = texture.size_vec2();
        let available = ui.available_size();
        let scale = (available.x / source.x)
            .min(available.y / source.y)
            .max(0.0);
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }

        let size = source * scale;
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        self.draw_adm3a_crt(ui.painter(), Self::adm3a_screen_rect(rect));
    }

    fn show_adm3a_crt_viewport(&self, parent_ctx: &egui::Context) {
        let open = parent_ctx
            .data_mut(|data| *data.get_temp_mut_or(Self::adm3a_crt_viewport_open_id(), false));
        if !open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-adm3a-crt"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — ADM-3A Active CRT")
                .with_inner_size([1000.0, 800.0])
                .with_min_inner_size([500.0, 400.0])
                .with_resizable(true),
            |crt_ctx, _class| {
                egui::CentralPanel::default().show(crt_ctx, |ui| {
                    let available = ui.available_rect_before_wrap();
                    let rect = Self::fit_adm3a_crt_rect(available.shrink(12.0));
                    self.draw_adm3a_crt(ui.painter(), rect);
                });
                if crt_ctx.input(|i| i.viewport().close_requested()) {
                    crt_ctx.data_mut(|data| {
                        data.insert_temp(Self::adm3a_crt_viewport_open_id(), false)
                    });
                }
            },
        );
    }

    fn show_adm3a_viewport(&mut self, parent_ctx: &egui::Context) {
        if self.adm3a.window_open {
            parent_ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("rustair-adm3a-terminal"),
                egui::ViewportBuilder::default()
                    .with_title("RusTair — Lear Siegler ADM-3A")
                    .with_inner_size([1040.0, 780.0])
                    .with_min_inner_size([720.0, 540.0])
                    .with_resizable(true),
                |adm3a_ctx, _class| {
                    egui::TopBottomPanel::top("adm3a-controls")
                        .resizable(false)
                        .show(adm3a_ctx, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                self.draw_adm3a_connection_selector(ui);
                                ui.separator();
                                ui.label("80 × 24");
                                ui.separator();
                                if ui.button("Open active CRT…").clicked() {
                                    Self::open_adm3a_crt_viewport(adm3a_ctx);
                                }
                            });
                        });
                    egui::CentralPanel::default().show(adm3a_ctx, |ui| {
                        ui.centered_and_justified(|ui| self.draw_adm3a_shell(ui));
                    });
                    if adm3a_ctx.input(|i| i.viewport().close_requested()) {
                        self.adm3a.window_open = false;
                    }
                },
            );
        }

        self.show_adm3a_crt_viewport(parent_ctx);
    }

    pub(in crate::app) fn show_terminal_viewport(&mut self, parent_ctx: &egui::Context) {
        self.show_adm3a_viewport(parent_ctx);

        if !self.terminal.window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-text-terminal"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Text Terminal")
                .with_inner_size([1120.0, 680.0])
                .with_min_inner_size([760.0, 420.0])
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
