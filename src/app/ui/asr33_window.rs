use super::super::*;

impl RusTairApp {
    pub(in crate::app) fn update_paper_tape(&mut self) {
        if self.last_tape_tick.elapsed() < Duration::from_millis(30) { return; }
        self.last_tape_tick = Instant::now();
        if self.machine.bus.serial_rx_empty() {
            if let Some(byte) = self.tty.next_tape_byte() {
                self.machine.bus.serial_receive(byte);
            }
        }
    }

    fn draw_tty_menu(&mut self, ctx: &egui::Context) {
        self.process_tty_keyboard(ctx);
        egui::TopBottomPanel::top("tty-menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label("POWER:");
                if ui.selectable_label(self.tty.mode == TtyMode::Off, "OFF").clicked() {
                    self.set_tty_mode(TtyMode::Off);
                }
                if ui.selectable_label(self.tty.mode == TtyMode::Line, "LINE").clicked() {
                    self.set_tty_mode(TtyMode::Line);
                }
                if ui.selectable_label(self.tty.mode == TtyMode::Local, "LOCAL").clicked() {
                    self.set_tty_mode(TtyMode::Local);
                }
                ui.separator();
                ui.label(format!("{} columns", self.tty.paper_width));
                ui.separator();
                if ui.button("Clear paper").clicked() { self.tty.clear_paper(); }
                if ui.button("Read tape…").clicked() { self.load_paper_tape(); }
                let punch_label = if self.tty.capture_to_tape { "Finish punch" } else { "Punch tape" };
                if ui.button(punch_label).clicked() {
                    if self.tty.capture_to_tape {
                        self.tty.capture_to_tape = false;
                        self.save_punched_tape();
                    } else {
                        self.tty.tape_out.clear();
                        self.tty.capture_to_tape = true;
                    }
                }
            });
        });
    }

    fn draw_tty_window(&mut self, ctx: &egui::Context) {
        self.update_key_animation(ctx);
        self.draw_tty_menu(ctx);

        if self.tty_power_flash_until.is_some_and(|until| Instant::now() < until) {
            ctx.request_repaint_after(PANEL_FRAME);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_teletype(ui));
        });
        egui::TopBottomPanel::bottom("tty-status").show(ctx, |ui| {
            ui.small(format!(
                "ASR-33 {}  |  RX {}  |  TX {}  |  column {}/{}",
                match self.tty.mode {
                    TtyMode::Off => "OFF",
                    TtyMode::Line => "LINE",
                    TtyMode::Local => "LOCAL",
                },
                self.machine.bus.serial_rx_len(),
                if self.machine.bus.tx_busy() { "BUSY" } else { "READY" },
                self.tty.column,
                self.tty.paper_width,
            ));
        });
    }

    pub(in crate::app) fn show_tty_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.tty_window_open { return; }
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-asr33"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — ASR-33 Teletype")
                .with_inner_size([820.0, 820.0])
                .with_min_inner_size([520.0, 520.0])
                .with_resizable(true),
            |tty_ctx, _class| {
                self.draw_tty_window(tty_ctx);
                if tty_ctx.input(|i| i.viewport().close_requested()) {
                    self.tty_window_open = false;
                    self.set_tty_mode(TtyMode::Off);
                }
            },
        );
    }
}
