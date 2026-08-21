use super::*;

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();

        let dt = now.duration_since(self.last_tick).min(Duration::from_millis(20));
        self.last_tick = now;

        let two_sio = self.config.machine.serial_board == SerialBoard::TwoSio88;

        // The 88-SIO has one physical serial connection, so closing the text
        // terminal reconnects that single cable to the ASR-33. A fully populated
        // 88-2SIO has two independent ports and both endpoints remain connected.
        if !two_sio
            && !self.terminal.window_open
            && self.serial_router.endpoint() == SerialEndpoint::TextTerminal
        {
            self.terminal.tx_started = None;
            self.serial_router.select(SerialEndpoint::InternalAsr33);
        }

        self.update_paper_tape();
        if two_sio || self.serial_router.endpoint() == SerialEndpoint::TextTerminal {
            self.process_terminal_input(ctx);
        }

        if let Some(until) = self.reset_flash_until {
            if now >= until {
                self.machine.set_panel_lamps(0, 0);
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

        if two_sio {
            // Port 0 (10h/11h) is connected to the ASR-33 and Port 1
            // (12h/13h) to the Text Terminal. They run simultaneously.
            self.process_tty_serial(ctx);
            self.process_tty_answerback(ctx);
            self.process_terminal_serial(ctx);
        } else {
            match self.serial_router.endpoint() {
                SerialEndpoint::InternalAsr33 => {
                    self.process_tty_serial(ctx);
                    self.process_tty_answerback(ctx);
                }
                SerialEndpoint::TextTerminal => self.process_terminal_serial(ctx),
            }
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

                ui.menu_button("Configuration", |ui| {
                    ui.menu_button("Memory", |ui| {
                        ui.label(format!(
                            "Installed RAM: {}",
                            self.config.machine.ram_size.label()
                        ));
                        ui.separator();

                        for ram_size in RamSize::ALL {
                            let selected = self.config.machine.ram_size == ram_size;
                            if ui.selectable_label(selected, ram_size.label()).clicked() {
                                self.apply_memory_configuration(
                                    ram_size,
                                    self.config.machine.ram_init,
                                );
                                ui.close();
                            }
                        }

                        ui.separator();
                        ui.menu_button("Power-on contents", |ui| {
                            for ram_init in RamInit::ALL {
                                let selected = self.config.machine.ram_init == ram_init;
                                if ui.selectable_label(selected, ram_init.label()).clicked() {
                                    self.apply_memory_configuration(
                                        self.config.machine.ram_size,
                                        ram_init,
                                    );
                                    ui.close();
                                }
                            }
                        });
                    });

                    ui.menu_button("Serial board", |ui| {
                        let current = self.config.machine.serial_board;
                        ui.label(format!("Installed board: {}", current.label()));
                        match current {
                            SerialBoard::Sio88 => {
                                ui.small(format!(
                                    "Port: {:02X}h status / {:02X}h data",
                                    current.status_port(),
                                    current.data_port()
                                ));
                                ui.small("One physical connection: ASR-33 or Text Terminal");
                            }
                            SerialBoard::TwoSio88 => {
                                ui.small(format!(
                                    "Port 0: {:02X}h status/control / {:02X}h data → ASR-33",
                                    current.status_port(),
                                    current.data_port()
                                ));
                                ui.small(format!(
                                    "Port 1: {:02X}h status/control / {:02X}h data → Text Terminal",
                                    current.port1_status_port().unwrap_or(0),
                                    current.port1_data_port().unwrap_or(0)
                                ));
                            }
                        }
                        ui.separator();

                        for serial_board in SerialBoard::ALL {
                            let selected = current == serial_board;
                            if ui
                                .selectable_label(selected, serial_board.label())
                                .clicked()
                            {
                                self.apply_serial_board_configuration(serial_board);
                                ui.close();
                            }
                        }

                        ui.separator();
                        ui.small(
                            "Bundled BASIC 3.2: use sense 00h for 88-SIO or 08h (A11) for 88-2SIO. Changing the installed board does not alter the front-panel switches.",
                        );
                    });

                    ui.menu_button("Compatibility", |ui| {
                        ui.label("Software workarounds (off = historically faithful)");
                        ui.separator();

                        let mut basic32_workaround =
                            self.config.compatibility.basic32_64k_probe_workaround;
                        if ui
                            .checkbox(
                                &mut basic32_workaround,
                                "BASIC 3.2 64K memory-probe workaround",
                            )
                            .changed()
                        {
                            self.config.compatibility.basic32_64k_probe_workaround =
                                basic32_workaround;
                            if !basic32_workaround {
                                self.machine.bus.clear_transient_memory_guards();
                            }
                            self.status = if basic32_workaround {
                                "Compatibility enabled: BASIC 3.2 64K memory-probe workaround"
                                    .into()
                            } else {
                                "Compatibility disabled: authentic BASIC 3.2 64K bug is reproducible"
                                    .into()
                            };
                        }
                        ui.small(
                            "When enabled, bundled BASIC 3.2 avoids its 64K MEMORY SIZE wraparound bug. Disable it to reproduce the original hang.",
                        );
                    });
                });

                ui.separator();
                if ui.button("ASR-33 TELETYPE").clicked() {
                    if !two_sio {
                        self.asr33.tx_started = None;
                        self.terminal.tx_started = None;
                        self.serial_router.select(SerialEndpoint::InternalAsr33);
                    }
                    self.asr33.window_open = true;
                }
                if ui.button("TEXT TERMINAL").clicked() {
                    if !two_sio {
                        self.asr33.tx_started = None;
                        self.terminal.tx_started = None;
                        self.serial_router.select(SerialEndpoint::TextTerminal);
                    }
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
