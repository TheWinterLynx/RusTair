use super::super::*;
use crate::config::TerminalDuplex;

impl RusTairApp {
    pub(in crate::app) fn update_paper_tape(&mut self) {
        if self.asr33.last_tape_tick.elapsed() < Duration::from_millis(30) {
            return;
        }
        self.asr33.last_tape_tick = Instant::now();
        if self.asr_serial_rx_empty() {
            if let Some(byte) = self.tty.next_tape_byte() {
                self.asr_serial_receive(byte);
            }
        }
    }

    fn draw_tty_connection_selector(&mut self, ui: &mut egui::Ui) {
        let board = self.config.machine.serial_board;
        let current = self.asr_connection();
        let mut selected = current;

        ui.label("Connection:");
        egui::ComboBox::from_id_salt("asr33-serial-connection")
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
            self.set_serial_connection(SerialDevice::InternalAsr33, selected);
        }
    }

    fn draw_tty_speed_selector(&mut self, ui: &mut egui::Ui) {
        let current = self.config.peripherals.asr33_speed;
        let mut selected = current;
        ui.label("Speed:");
        egui::ComboBox::from_id_salt("asr33-speed")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for speed in Asr33Speed::ALL {
                    ui.selectable_value(&mut selected, speed, speed.label());
                }
            });
        if selected != current {
            self.set_asr_speed(selected);
        }
    }

    fn draw_tty_duplex_selector(&mut self, ui: &mut egui::Ui) {
        ui.label("DUPLEX:");
        egui::ComboBox::from_id_salt("asr33-duplex")
            .selected_text(self.asr33.duplex.label())
            .show_ui(ui, |ui| {
                for duplex in TerminalDuplex::ALL {
                    ui.selectable_value(&mut self.asr33.duplex, duplex, duplex.label());
                }
            });
    }

    fn draw_tty_menu(&mut self, ctx: &egui::Context) {
        self.process_tty_keyboard(ctx);
        egui::TopBottomPanel::top("tty-menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label("POWER:");
                if ui
                    .selectable_label(self.tty.mode == TtyMode::Off, "OFF")
                    .clicked()
                {
                    self.set_tty_mode(TtyMode::Off);
                }
                if ui
                    .selectable_label(self.tty.mode == TtyMode::Line, "LINE")
                    .clicked()
                {
                    self.set_tty_mode(TtyMode::Line);
                }
                if ui
                    .selectable_label(self.tty.mode == TtyMode::Local, "LOCAL")
                    .clicked()
                {
                    self.set_tty_mode(TtyMode::Local);
                }
                ui.separator();
                self.draw_tty_connection_selector(ui);
                ui.separator();
                self.draw_tty_speed_selector(ui);
                ui.separator();
                self.draw_tty_duplex_selector(ui);
                ui.separator();
                ui.label(format!("{} columns", self.tty.paper_width));
                ui.separator();
                if ui.button("Clear paper").clicked() {
                    self.tty.clear_paper();
                }
                if ui.button("Read tape…").clicked() {
                    self.load_paper_tape();
                }
                let punching = self.tty.tape_capture_enabled();
                let punch_label = if punching { "Finish punch" } else { "Punch tape" };
                if ui.button(punch_label).clicked() {
                    if punching {
                        self.tty.finish_tape_punch();
                        self.save_punched_tape();
                    } else {
                        self.tty.start_tape_punch();
                    }
                }
            });
        });
    }

    fn draw_tty_window(&mut self, ctx: &egui::Context) {
        self.update_key_animation(ctx);
        self.draw_tty_menu(ctx);

        if self
            .asr33
            .power_flash_until
            .is_some_and(|until| Instant::now() < until)
        {
            ctx.request_repaint_after(PANEL_FRAME);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_teletype(ui));
        });
        egui::TopBottomPanel::bottom("tty-status").show(ctx, |ui| {
            let connection = self.asr_connection();
            let connection_label =
                Self::serial_connection_label(self.config.machine.serial_board, connection);
            let tx = if connection.is_connected() {
                if self.asr_serial_tx_busy() {
                    "BUSY"
                } else {
                    "READY"
                }
            } else {
                "N/A"
            };
            let duplex = if self.tty.mode == TtyMode::Local {
                "LOCAL ONLY"
            } else {
                self.asr33.duplex.label()
            };
            ui.small(format!(
                "ASR-33 {}  |  {}  |  {}  |  {}  |  RX {}  |  TX {}  |  column {}/{}",
                match self.tty.mode {
                    TtyMode::Off => "OFF",
                    TtyMode::Line => "LINE",
                    TtyMode::Local => "LOCAL",
                },
                self.config.peripherals.asr33_speed.label(),
                duplex,
                connection_label,
                self.asr_serial_rx_len(),
                tx,
                self.tty.column,
                self.tty.paper_width,
            ));
        });
    }

    pub(in crate::app) fn show_tty_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.asr33.window_open {
            return;
        }
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-asr33"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — ASR-33 Teletype")
                .with_inner_size([1040.0, 760.0])
                .with_min_inner_size([700.0, 520.0])
                .with_resizable(true),
            |tty_ctx, _class| {
                self.draw_tty_window(tty_ctx);
                if tty_ctx.input(|i| i.viewport().close_requested()) {
                    self.asr33.window_open = false;
                    self.set_tty_mode(TtyMode::Off);
                }
            },
        );
    }
}
