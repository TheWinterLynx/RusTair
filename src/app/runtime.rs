use super::*;

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();

        // Keep emulation work bounded to roughly one visual frame. The previous
        // 50 ms / 200k-cycle catch-up window could occasionally execute a large
        // CPU burst on the UI thread after an OS scheduling hiccup, producing a
        // visible whole-panel stutter/flash. At normal cadence this still runs
        // the 2 MHz CPU at the requested rate (about 32k cycles per 16 ms frame),
        // but deliberately drops excessive backlog instead of blocking drawing.
        let dt = now.duration_since(self.last_tick).min(Duration::from_millis(20));
        self.last_tick = now;

        // Preserve the existing user-visible rule that closing the text terminal
        // hands the serial line back to the ASR-33, but make that ownership
        // transition explicit instead of inferring routing everywhere from a
        // window boolean.
        if !self.terminal.window_open
            && self.serial_router.endpoint() == SerialEndpoint::TextTerminal
        {
            self.terminal.tx_started = None;
            self.serial_router.select(SerialEndpoint::InternalAsr33);
        }

        self.update_paper_tape();
        if self.serial_router.endpoint() == SerialEndpoint::TextTerminal {
            self.process_terminal_input(ctx);
        }

        if let Some(until) = self.reset_flash_until {
            if now >= until {
                self.machine.address_leds = 0;
                self.machine.bus.data_leds = 0;
                self.reset_flash_until = None;
            } else {
                ctx.request_repaint_after(PANEL_FRAME);
            }
        }

        if self.machine.running {
            let cycles = (CLOCK_HZ as f64 * dt.as_secs_f64()) as u32;
            self.machine.run_cycles(cycles.clamp(1, 40_000));
            ctx.request_repaint_after(PANEL_FRAME);
        }

        match self.serial_router.endpoint() {
            SerialEndpoint::InternalAsr33 => {
                self.process_tty_serial(ctx);
                self.process_tty_answerback(ctx);
            }
            SerialEndpoint::TextTerminal => self.process_terminal_serial(ctx),
        }
        self.update_teletype_mechanics(ctx);

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Load binary…").clicked() {
                        self.load_binary_dialog();
                        ui.close();
                    }
                    if ui.button("Load bundled Microsoft 4K BASIC").clicked() {
                        self.load_bundled_basic();
                        ui.close();
                    }
                });

                ui.separator();
                if ui.button("ASR-33 TELETYPE").clicked() {
                    self.asr33.window_open = true;
                }
                if ui.button("TEXT TERMINAL").clicked() {
                    // Cancel any in-progress ASR-33 holding-register timer when
                    // explicitly handing the serial line to the text terminal.
                    self.asr33.tx_started = None;
                    self.terminal.tx_started = None;
                    self.serial_router.select(SerialEndpoint::TextTerminal);
                    self.terminal.window_open = true;
                }
                ui.separator();
                let mut muted = self.audio.muted();
                if ui.checkbox(&mut muted, "Mute").changed() {
                    self.audio.set_muted(muted);
                }
                ui.separator();
                ui.label(format!(
                    "PC {:04X}  SP {:04X}  A {:02X}  F {:02X}",
                    self.machine.cpu.pc,
                    self.machine.cpu.sp,
                    self.machine.cpu.a,
                    self.machine.cpu.f
                ));
                ui.separator();
                ui.label(if self.machine.running {
                    "RUNNING"
                } else if self.machine.powered {
                    "STOPPED"
                } else {
                    "POWER OFF"
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_altair(ui));
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.small(&self.status);
        });

        self.show_tty_viewport(ctx);
        self.show_terminal_viewport(ctx);
    }
}
