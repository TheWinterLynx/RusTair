use super::*;

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();

        let dt = now.duration_since(self.last_tick).min(Duration::from_millis(20));
        self.last_tick = now;

        self.update_paper_tape();
        if self.terminal_connection().is_connected() {
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
            let authentic_cycles = (CLOCK_HZ as f64 * dt.as_secs_f64()) as u32;
            let authentic_cycles = authentic_cycles.clamp(1, 40_000);
            let speed = self.config.preferences.emulation_speed;
            self.machine.run_cycles(speed.cycle_budget(authentic_cycles));
            if speed == EmulationSpeed::Unlimited {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(PANEL_FRAME);
            }
        }

        // Peripheral clocks are intentionally wall-clock based and independent
        // of CPU emulation speed.
        if self.asr_connection().is_connected() {
            self.process_tty_serial(ctx);
            self.process_tty_answerback(ctx);
        }
        if self.terminal_connection().is_connected() {
            self.process_terminal_serial(ctx);
        }
        self.service_disconnected_serial_ports();
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
                    ui.menu_button("CPU", |ui| {
                        let cpu = self.config.machine.cpu_model;
                        ui.label(format!("Processor: {}", cpu.label()));
                        ui.small(format!("Authentic hardware clock: {:.1} MHz", cpu.clock_hz() as f32 / 1_000_000.0));
                        ui.separator();
                        ui.small("The emulated hardware remains an Intel 8080 at 2 MHz. Host-side acceleration is configured under Preferences → Emulation speed.");
                    });

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
                                    "Port 0: {:02X}h status / {:02X}h data",
                                    current.status_port(),
                                    current.data_port()
                                ));
                            }
                            SerialBoard::TwoSio88 => {
                                ui.small(format!(
                                    "Port 0: {:02X}h status/control / {:02X}h data",
                                    current.status_port(),
                                    current.data_port()
                                ));
                                ui.small(format!(
                                    "Port 1: {:02X}h status/control / {:02X}h data",
                                    current.port1_status_port().unwrap_or(0),
                                    current.port1_data_port().unwrap_or(0)
                                ));
                            }
                        }
                        ui.separator();
                        ui.label("External wiring:");
                        ui.small(format!(
                            "ASR-33 → {}",
                            Self::serial_connection_label(current, self.asr_connection())
                        ));
                        ui.small(format!(
                            "Text Terminal → {}",
                            Self::serial_connection_label(current, self.terminal_connection())
                        ));
                        ui.separator();

                        for serial_board in SerialBoard::ALL {
                            let selected = current == serial_board;
                            if ui.selectable_label(selected, serial_board.label()).clicked() {
                                self.apply_serial_board_configuration(serial_board);
                                ui.close();
                            }
                        }

                        ui.separator();
                        ui.small("Cable selection is available inside each terminal window. A port can have only one attached device.");
                        ui.small("Bundled BASIC 3.2: use sense 00h for 88-SIO or 08h (A11) for 88-2SIO. Changing the installed board does not alter the front-panel switches.");
                    });

                    ui.menu_button("Peripheral speed", |ui| {
                        ui.menu_button("ASR-33", |ui| {
                            for speed in Asr33Speed::ALL {
                                if ui
                                    .selectable_label(
                                        self.config.peripherals.asr33_speed == speed,
                                        speed.label(),
                                    )
                                    .clicked()
                                {
                                    self.set_asr_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Text Terminal", |ui| {
                            for speed in TerminalSpeed::ALL {
                                if ui
                                    .selectable_label(
                                        self.config.peripherals.terminal_speed == speed,
                                        speed.label(),
                                    )
                                    .clicked()
                                {
                                    self.set_terminal_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        ui.separator();
                        ui.small("Peripheral timing is independent of CPU emulation speed.");
                    });

                    ui.menu_button("Preferences", |ui| {
                        ui.menu_button("Emulation speed", |ui| {
                            for speed in EmulationSpeed::ALL {
                                if ui
                                    .selectable_label(
                                        self.config.preferences.emulation_speed == speed,
                                        speed.label(),
                                    )
                                    .clicked()
                                {
                                    self.set_emulation_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        ui.small("Acceleration changes host execution rate only; the emulated CPU remains an Intel 8080 at 2 MHz.");
                        ui.separator();

                        let mut auto_open_basic_console =
                            self.config.preferences.auto_open_basic_console;
                        if ui
                            .checkbox(&mut auto_open_basic_console, "Auto-open BASIC console")
                            .changed()
                        {
                            self.config.preferences.auto_open_basic_console = auto_open_basic_console;
                            self.status = if auto_open_basic_console {
                                "Preference enabled: auto-open BASIC console".into()
                            } else {
                                "Preference disabled: BASIC loads without opening a terminal window".into()
                            };
                        }
                        ui.small("When bundled BASIC is loaded, reveal the device connected to Port 0. This never changes the serial wiring.");
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
                                "Compatibility enabled: BASIC 3.2 64K memory-probe workaround".into()
                            } else {
                                "Compatibility disabled: authentic BASIC 3.2 64K bug is reproducible".into()
                            };
                        }
                        ui.small("When enabled, bundled BASIC 3.2 avoids its 64K MEMORY SIZE wraparound bug. Disable it to reproduce the original hang.");
                    });
                });

                ui.separator();
                if ui.button("ASR-33 TELETYPE").clicked() {
                    self.asr33.window_open = true;
                }
                if ui.button("TEXT TERMINAL").clicked() {
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
                ui.label(self.config.preferences.emulation_speed.label());
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
