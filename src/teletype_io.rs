impl RusTairApp {
    fn update_paper_tape(&mut self) {
        if self.last_tape_tick.elapsed() < Duration::from_millis(30) { return; }
        self.last_tape_tick = Instant::now();
        if self.machine.bus.serial_rx.is_empty() {
            if let Some(byte) = self.tty.next_tape_byte() {
                self.machine.bus.serial_rx.push_back(byte);
            }
        }
    }

    fn load_paper_tape(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Paper tape", &["txt", "tap", "bin"])
            .pick_file()
        else { return; };
        match std::fs::read(&path) {
            Ok(bytes) => {
                self.tty.load_tape(&bytes);
                self.status = format!("Paper tape loaded: {} bytes", bytes.len());
            }
            Err(e) => self.status = format!("Paper tape load failed: {e}"),
        }
    }

    fn save_punched_tape(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("myPaperTape.txt")
            .save_file()
        else { return; };
        match std::fs::write(&path, &self.tty.tape_out) {
            Ok(_) => self.status = format!("Punched tape saved: {} bytes", self.tty.tape_out.len()),
            Err(e) => self.status = format!("Paper tape save failed: {e}"),
        }
    }

    fn load_bundled_basic(&mut self) {
        match std::fs::read("assets/4kbas32.bin") {
            Ok(bytes) => {
                if !self.machine.powered {
                    self.set_altair_power(true);
                } else {
                    self.machine.set_running(false);
                    self.machine.reset();
                }
                self.tty_tx_started = None;
                self.machine.bus.clear_protection();
                self.machine.bus.load(0, &bytes);
                self.machine.cpu.pc = 0;
                self.tty_window_open = true;
                self.machine.set_running(true);
                self.status = "Microsoft 4K BASIC loaded and running".into();
            }
            Err(e) => self.status = format!("4K BASIC asset missing: {e}"),
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
                ui.selectable_value(&mut self.tty.paper_width, 52, "Large");
                ui.selectable_value(&mut self.tty.paper_width, 82, "Normal");
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

        if self.print_head_raise_until.is_some_and(|until| Instant::now() < until) {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
        if self.tty_power_flash_until.is_some_and(|until| Instant::now() < until) {
            ctx.request_repaint_after(PANEL_FRAME);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_teletype(ui));
        });
        egui::TopBottomPanel::bottom("tty-status").show(ctx, |ui| {
            ui.small(format!(
                "ASR-33 {}  |  RX {}  |  TX {}  |  column {}",
                match self.tty.mode {
                    TtyMode::Off => "OFF",
                    TtyMode::Line => "LINE",
                    TtyMode::Local => "LOCAL",
                },
                self.machine.bus.serial_rx.len(),
                if self.machine.bus.tx_busy() { "BUSY" } else { "READY" },
                self.tty.column,
            ));
        });
    }

    fn show_tty_viewport(&mut self, parent_ctx: &egui::Context) {
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
