use super::*;

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        super::ui::ensure_persistent_configuration_loaded(self);

        // Embedded-suite completion must consume its meter result before the
        // generic external-.COM result dialog gets a chance to take it.
        self.poll_embedded_cpu_diagnostics(ctx);
        self.poll_cpu_diagnostic_dialog(ctx);

        let io_inspector_open = ctx.data_mut(|data| {
            *data.get_temp_mut_or(egui::Id::new("rustair-io-inspector-open"), false)
        });
        let io_capture_requested = ctx.data_mut(|data| {
            *data.get_temp_mut_or(
                egui::Id::new("rustair-io-inspector-capture-enabled"),
                true,
            )
        });
        let io_capture_active = io_inspector_open && io_capture_requested;
        if self.machine.io_trace_enabled() != io_capture_active {
            self.machine.set_io_trace_enabled(io_capture_active);
        }
        if self.external_serial.server.network_trace_enabled() != io_capture_active {
            self.external_serial.server.set_network_trace_enabled(io_capture_active);
        }
        if self.external_com.port.trace_enabled() != io_capture_active {
            self.external_com.port.set_trace_enabled(io_capture_active);
        }

        let dt = now.duration_since(self.last_tick).min(Duration::from_millis(20));
        self.last_tick = now;

        self.update_paper_tape();
        if self.terminal_connection().is_connected() {
            self.process_terminal_input(ctx);
        }

        if self.machine.running() {
            let authentic_cycles = (CLOCK_HZ as f64 * dt.as_secs_f64()) as u32;
            let authentic_cycles = authentic_cycles.clamp(1, 40_000);
            let speed = self.effective_emulation_speed();
            self.machine.run_cycles(speed.cycle_budget(authentic_cycles));
            if speed == EmulationSpeed::Unlimited {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(PANEL_FRAME);
            }
        }

        if self.asr_connection().is_connected() {
            self.process_tty_serial(ctx);
            self.process_tty_answerback(ctx);
        }
        if self.terminal_connection().is_connected() {
            self.process_terminal_serial(ctx);
        }
        self.process_external_serial(ctx);
        self.process_external_com(ctx);
        self.service_disconnected_serial_ports();
        self.update_teletype_mechanics(ctx);

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Load binary…").clicked() {
                        self.load_binary_dialog();
                        ui.close();
                    }
                    ui.menu_button("Microsoft 4K BASIC 3.2", |ui| {
                        if ui.button("Quick Load — direct RAM").clicked() {
                            self.load_bundled_basic();
                            ui.close();
                        }
                        if ui.button("Authentic Load — paper tape…").clicked() {
                            self.open_authentic_basic_loader();
                            ui.close();
                        }
                        ui.separator();
                        ui.small("Quick Load is the emulator convenience path. Authentic Load executes the historical bootstrap and receives BASIC through the emulated serial board.");
                    });
                    ui.menu_button("CPU diagnostics", |ui| {
                        self.draw_cpu_diagnostics_menu(ui);
                    });
                });

                ui.menu_button("Configuration", |ui| {
                    ui.menu_button("CPU", |ui| {
                        let active_engine = self.machine.engine();
                        ui.label(format!("Emulation engine: {}", active_engine.label()));
                        ui.small("Engine changes require POWER OFF. Runtime CPU/RAM/UART state is intentionally not migrated between engines.");
                        ui.separator();
                        for engine in [
                            EmulationEngine::RustFast8080,
                            EmulationEngine::RustCycleAccurate8080,
                        ] {
                            if ui.selectable_label(active_engine == engine, engine.label()).clicked() {
                                self.select_emulation_engine(engine);
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.add_enabled(false, egui::Button::new("Open SIMH — Altair (integration parked)"));
                        ui.add_enabled(false, egui::Button::new("Open SIMH — AltairZ80 (integration parked)"));

                        let capabilities = self.machine.capabilities();
                        ui.separator();
                        if capabilities.exact_t_state_timing {
                            ui.small("Timing: exact 8080 T-state core; front-panel SINGLE STEP advances one machine cycle.");
                        } else {
                            ui.small("Timing: fast instruction-level 8080; front-panel SINGLE STEP is an instruction-level approximation.");
                        }
                        ui.small(format!(
                            "S-100 activity: {}",
                            if capabilities.exact_bus_activity { "exact T-state samples" } else { "machine-cycle samples synthesized by the fast CPU-board adapter" }
                        ));

                        let cpu = self.config.machine.cpu_model;
                        ui.separator();
                        ui.label(format!("Processor: {}", cpu.label()));
                        ui.small(format!(
                            "Authentic hardware clock: {:.1} MHz",
                            cpu.clock_hz() as f32 / 1_000_000.0
                        ));
                        ui.small("Host-side acceleration is configured under Preferences → Emulation speed; it does not change the emulated 2 MHz hardware clock.");
                    });

                    if ui.button("LED visuals…").clicked() {
                        super::ui::open_led_visual_controls(self);
                        ui.close();
                    }

                    ui.menu_button("Memory", |ui| {
                        ui.label(format!("Installed RAM: {}", self.config.machine.ram_size.label()));
                        ui.separator();
                        for ram_size in RamSize::ALL {
                            let selected = self.config.machine.ram_size == ram_size;
                            if ui.selectable_label(selected, ram_size.label()).clicked() {
                                self.apply_memory_configuration(ram_size, self.config.machine.ram_init);
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.menu_button("Power-on contents", |ui| {
                            for ram_init in RamInit::ALL {
                                let selected = self.config.machine.ram_init == ram_init;
                                if ui.selectable_label(selected, ram_init.label()).clicked() {
                                    self.apply_memory_configuration(self.config.machine.ram_size, ram_init);
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
                                ui.small(format!("Port 0: {:02X}h status / {:02X}h data", current.status_port(), current.data_port()));
                            }
                            SerialBoard::TwoSio88 => {
                                ui.small(format!("Port 0: {:02X}h status/control / {:02X}h data", current.status_port(), current.data_port()));
                                ui.small(format!("Port 1: {:02X}h status/control / {:02X}h data", current.port1_status_port().unwrap_or(0), current.port1_data_port().unwrap_or(0)));
                            }
                        }
                        ui.separator();
                        ui.label("External wiring:");
                        ui.small(format!("ASR-33 → {}", Self::serial_connection_label(current, self.asr_connection())));
                        ui.small(format!("Text Terminal → {}", Self::serial_connection_label(current, self.terminal_connection())));
                        ui.small(format!("External TCP → {}", Self::serial_connection_label(current, self.external_tcp_connection())));
                        ui.small(format!("External COM → {}", Self::serial_connection_label(current, self.external_com_connection())));
                        ui.separator();
                        for serial_board in SerialBoard::ALL {
                            let selected = current == serial_board;
                            if ui.selectable_label(selected, serial_board.label()).clicked() {
                                self.apply_serial_board_configuration(serial_board);
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.small("Each emulated serial port has one attached endpoint/cable. TCP fan-out, when enabled, happens behind the single External TCP endpoint; External COM remains its own independent cable.");
                        ui.small("Bundled BASIC 3.2: use sense 00h for 88-SIO or 08h (A11) for 88-2SIO. Changing the installed board does not alter the front-panel switches.");
                    });

                    ui.menu_button("Peripheral speed", |ui| {
                        ui.menu_button("ASR-33", |ui| {
                            for speed in Asr33Speed::ALL {
                                if ui.selectable_label(self.config.peripherals.asr33_speed == speed, speed.label()).clicked() {
                                    self.set_asr_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("Text Terminal", |ui| {
                            for speed in TerminalSpeed::ALL {
                                if ui.selectable_label(self.config.peripherals.terminal_speed == speed, speed.label()).clicked() {
                                    self.set_terminal_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        ui.separator();
                        ui.small("Peripheral timing is independent of CPU emulation speed.");
                    });

                    ui.menu_button("External TCP", |ui| { self.draw_external_serial_config_menu(ui); });
                    ui.menu_button("External COM", |ui| { self.draw_external_com_config_menu(ui); });

                    ui.menu_button("Preferences", |ui| {
                        ui.menu_button("Emulation speed", |ui| {
                            for speed in EmulationSpeed::ALL {
                                if ui.selectable_label(self.config.preferences.emulation_speed == speed, speed.label()).clicked() {
                                    self.set_emulation_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        ui.small("Acceleration changes host execution rate only; the emulated CPU remains an Intel 8080 at 2 MHz.");
                        ui.separator();
                        let mut auto_open_basic_console = self.config.preferences.auto_open_basic_console;
                        if ui.checkbox(&mut auto_open_basic_console, "Auto-open BASIC console").changed() {
                            self.config.preferences.auto_open_basic_console = auto_open_basic_console;
                            self.status = if auto_open_basic_console {
                                "Preference enabled: auto-open BASIC console".into()
                            } else {
                                "Preference disabled: BASIC loads without opening a terminal window".into()
                            };
                        }
                        ui.small("When bundled BASIC is loaded, reveal the endpoint connected to Port 0. This never changes the serial wiring.");
                    });

                    ui.menu_button("Compatibility", |ui| {
                        ui.label("Software workarounds");
                        ui.separator();
                        let mut basic32_workaround = self.config.compatibility.basic32_64k_probe_workaround;
                        if ui.checkbox(&mut basic32_workaround, "BASIC 3.2 64K memory-probe workaround").changed() {
                            self.config.compatibility.basic32_64k_probe_workaround = basic32_workaround;
                            if !basic32_workaround {
                                self.machine.clear_transient_memory_guards();
                            }
                            self.status = if basic32_workaround {
                                "Compatibility enabled: BASIC 3.2 64K memory-probe workaround".into()
                            } else {
                                "Compatibility disabled: authentic BASIC 3.2 64K bug is reproducible".into()
                            };
                        }
                        ui.small("When enabled, bundled BASIC 3.2 avoids its 64K MEMORY SIZE wraparound bug. Disable it to reproduce the original hang.");

                        ui.separator();
                        ui.label("Historical hardware behaviour");
                        let mut historical_power_on = self.config.compatibility.historical_undefined_run_latch_power_on;
                        if ui.checkbox(&mut historical_power_on, "Undefined RUN/STOP latch at power-on").changed() {
                            self.config.compatibility.historical_undefined_run_latch_power_on = historical_power_on;
                            self.status = if historical_power_on {
                                "Historical power-on enabled: next POWER ON may start with RUN or STOP randomly".into()
                            } else {
                                "Historical power-on disabled: next POWER ON will safely start with STOP latch".into()
                            };
                        }
                        ui.small("Original 8800 RUN/STOP latch had no guaranteed power-on state. This option is OFF by default and only affects the next POWER ON.");
                    });
                });

                ui.separator();
                if ui.button("ASR-33 TELETYPE").clicked() { self.asr33.window_open = true; }
                if ui.button("TEXT TERMINAL").clicked() { self.terminal.window_open = true; }
                if ui.button("EXTERNAL TCP").clicked() { self.external_serial.window_open = true; }
                if ui.button("EXTERNAL COM").clicked() {
                    self.external_com.window_open = true;
                    if self.external_com.available_ports.is_empty() { self.refresh_external_com_ports(); }
                }
                if ui.button("RAM VIEWER").clicked() { self.open_memory_viewer(ctx); }
                if ui.button("I/O INSPECTOR").clicked() { self.open_io_inspector(ctx); }
                ui.separator();
                let mut muted = self.audio.muted();
                if ui.checkbox(&mut muted, "Mute").changed() { self.audio.set_muted(muted); }
                ui.separator();
                let cpu = self.machine.intel8080_state();
                let panel = self.machine.front_panel_state();
                ui.label(format!("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}", cpu.pc, cpu.sp, cpu.a, cpu.flags));
                ui.separator();
                ui.label(self.effective_emulation_speed().label());
                ui.separator();
                let execution_state = if !panel.powered {
                    "POWER OFF"
                } else if cpu.halted.unwrap_or(false) {
                    if panel.running { "HALTED · RUN latch ON" } else { "HALTED · RUN latch OFF" }
                } else if panel.running {
                    "RUNNING"
                } else {
                    "STOPPED"
                };
                ui.label(execution_state);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_altair(ui));
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.small(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(format!("Core: {}", self.machine.engine().label()));
                });
            });
        });

        self.show_tty_viewport(ctx);
        self.show_terminal_viewport(ctx);
        self.show_external_serial_viewport(ctx);
        self.show_external_com_viewport(ctx);
        self.show_memory_viewer_viewport(ctx);
        self.show_io_inspector_viewport(ctx);
        self.draw_authentic_loader_window(ctx);

        super::ui::persist_configuration_if_changed(self);
    }
}
