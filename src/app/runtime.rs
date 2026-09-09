use super::*;

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        super::ui::ensure_persistent_configuration_loaded(self);

        // `machine.s100_hardware` is the sole physical authority. Persistence
        // parses old aggregate keys only long enough to migrate them; guest
        // execution never runs against a second synthetic hardware topology.
        if !self.machine.powered()
            && self.machine.s100_hardware() != self.config.machine.s100_hardware
        {
            self.machine.configure_s100_hardware(
                self.config.machine.s100_hardware,
                self.config.machine.ram_init,
            );
            self.status = format!(
                "S-100 chassis mounted from configuration — {} KiB RAM across installed cards",
                self.config.machine.s100_hardware.installed_ram_bytes() / 1024
            );
        }

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

        super::ui::sync_instruction_trace_capture(self, ctx);

        let frame_dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;

        self.update_paper_tape();
        if self.terminal_connection().is_connected() {
            self.process_terminal_input(ctx);
        }

        let running = self.machine.running();
        let board = self
            .config
            .machine
            .s100_hardware
            .active_cpu_board()
            .expect("validated S-100 configuration has one CPU board");
        let speed = self.effective_emulation_speed();
        let budget = self
            .execution_clock
            .budget(now, running, board.clock_hz(), speed);

        if running && budget != 0 {
            let executed = super::execution_frame::run_cpu_frame(
                &mut self.machine, budget, super::execution_frame::CPU_FRAME_TIME,
            );

            if executed != 0 && executed < u64::from(budget) && self.machine.running() {
                // A host deadline is a yield, not stopped time. Continue paying
                // the exact retained clock debt on the next responsive frame.
                ctx.request_repaint();
            }

            if speed != EmulationSpeed::Unlimited {
                if executed == 0 {
                    self.execution_clock.discard_pending_debt();
                } else {
                    self.execution_clock.record_executed(executed);
                }
            }
        }

        if running {
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
                    if ui.button("Front Panel Operator…").clicked() {
                        self.open_standalone_front_panel_operator(ctx);
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
                        ui.small("Quick Load is the emulator convenience path. Authentic Load executes the historical bootstrap and receives BASIC through the installed S-100 serial card.");
                    });
                    ui.menu_button("CPU diagnostics", |ui| {
                        self.draw_cpu_diagnostics_menu(ui);
                    });
                });

                ui.menu_button("Configuration", |ui| {
                    ui.menu_button("S-100 Chassis / Cards", |ui| {
                        super::ui::draw_s100_hardware_menu(self, ui);
                    });
                    ui.separator();

                    ui.menu_button("CPU", |ui| {
                        ui.label(format!("Execution engine: {}", self.machine.engine().label()));
                        ui.small("RusTair has one Altair machine. Full semantic windows and exact Partial T-state execution are internal strategies over the same CPU, S-100 chassis, cards and front panel.");
                        ui.small("Full is used only while the chassis proves that no intermediate hardware event can distinguish it from T-state execution; Partial remains the exact electrical oracle at observable boundaries.");

                        let capabilities = self.machine.capabilities();
                        ui.separator();
                        debug_assert!(capabilities.exact_t_state_timing);
                        debug_assert!(capabilities.exact_bus_activity);
                        ui.small("Timing: exact Intel 8080 T-state accounting; front-panel SINGLE STEP uses the exact Partial path.");
                        ui.small("S-100 activity: exact physical samples in Partial and analytically equivalent timing/panel duty in proven Full windows.");

                        let board = self
                            .config
                            .machine
                            .s100_hardware
                            .active_cpu_board()
                            .expect("validated S-100 configuration has one CPU board");
                        let cpu = board.cpu_model();
                        ui.separator();
                        ui.label(format!("Installed CPU board: {}", board.label()));
                        ui.small(format!("Processor: {}", cpu.label()));
                        ui.small(format!(
                            "Authentic hardware clock: {:.1} MHz",
                            board.clock_hz() as f32 / 1_000_000.0
                        ));

                        ui.separator();
                        ui.label("Emulator speed");
                        let external_diagnostic_running = self.cpu_diagnostic_run_speed_label.is_some();
                        ui.add_enabled_ui(!external_diagnostic_running, |ui| {
                            for speed in SELECTABLE_EMULATION_SPEEDS {
                                let label = emulation_speed_label(speed, board);
                                if ui.selectable_label(self.config.preferences.emulation_speed == speed, label).clicked() {
                                    self.set_emulation_speed(speed);
                                    ui.close();
                                }
                            }
                        });
                        if self.config.preferences.emulation_speed == EmulationSpeed::X2 {
                            ui.small("Loaded legacy 2× preference. Select Authentic, 5×, 10× or Unlimited to replace it.");
                        }
                        if let Some(speed) = self.cpu_diagnostic_run_speed_label.as_deref() {
                            ui.small(format!("Speed locked while external CPU diagnostic runs: {speed}"));
                        }
                        ui.small("Acceleration changes host execution rate only; it does not alter the installed CPU board hardware clock or S-100 timing model.");
                    });

                    if ui.button("LED visuals…").clicked() {
                        super::ui::open_led_visual_controls(self);
                        ui.close();
                    }

                    ui.menu_button("Memory", |ui| {
                        let hardware = self.config.machine.s100_hardware;
                        ui.label(format!(
                            "Installed S-100 RAM: {} KiB across physical cards",
                            hardware.installed_ram_bytes() / 1024
                        ));
                        ui.small("Board type, base address, population and timing come only from Configuration → S-100 Chassis / Cards. Aggregate RAM-size/timing controls are migration-only and cannot fabricate a second runtime topology.");
                        ui.separator();
                        ui.menu_button("Power-on contents", |ui| {
                            for ram_init in RamInit::ALL {
                                let selected = self.config.machine.ram_init == ram_init;
                                if ui.selectable_label(selected, ram_init.label()).clicked() {
                                    self.apply_ram_initialization(ram_init);
                                    ui.close();
                                }
                            }
                        });
                        if self.machine.powered() {
                            ui.small("POWER OFF required before changing the RAM power-on initialization policy.");
                        }
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

                    ui.menu_button("External", |ui| {
                        ui.menu_button("TCP", |ui| { self.draw_external_serial_config_menu(ui); });
                        ui.menu_button("COM", |ui| { self.draw_external_com_config_menu(ui); });
                    });

                    ui.menu_button("Preferences", |ui| {
                        let mut auto_open_basic_console = self.config.preferences.auto_open_basic_console;
                        if ui.checkbox(&mut auto_open_basic_console, "Auto-open BASIC console").changed() {
                            self.config.preferences.auto_open_basic_console = auto_open_basic_console;
                            self.status = if auto_open_basic_console {
                                "Preference enabled: auto-open BASIC console".into()
                            } else {
                                "Preference disabled: BASIC loads without opening a terminal window".into()
                            };
                        }
                        ui.small("When bundled BASIC is loaded, reveal the endpoint already connected to Port 0. This never changes S-100 hardware or serial wiring.");
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
                ui.menu_button("EXTERNAL", |ui| {
                    if ui.button("TCP").clicked() {
                        self.external_serial.window_open = true;
                        ui.close();
                    }
                    if ui.button("COM").clicked() {
                        self.external_com.window_open = true;
                        if self.external_com.available_ports.is_empty() { self.refresh_external_com_ports(); }
                        ui.close();
                    }
                });
                if ui.button("RAM VIEWER").clicked() { self.open_memory_viewer(ctx); }
                if ui.button("DEBUGGER").clicked() { self.open_debugger_controls(ctx); }
                if ui.button("EXEC HISTORY").clicked() { self.open_instruction_history(ctx); }
                if ui.button("I/O INSPECTOR").clicked() { self.open_io_inspector(ctx); }
                if ui.button("T-STATE TEACHER").clicked() { self.open_bus_teacher(ctx); }
                if ui.button("PANEL OPERATOR").clicked() { self.open_standalone_front_panel_operator(ctx); }
                ui.separator();
                let mut muted = self.audio.muted();
                if ui.checkbox(&mut muted, "Mute").changed() { self.audio.set_muted(muted); }
                ui.separator();
                let cpu = self.machine.intel8080_state();
                let panel = self.machine.front_panel_state();
                ui.label(format!("PC {:04X}  SP {:04X}  A {:02X}  F {:02X}", cpu.pc, cpu.sp, cpu.a, cpu.flags));
                ui.separator();
                let board = self
                    .config
                    .machine
                    .s100_hardware
                    .active_cpu_board()
                    .expect("validated S-100 configuration has one CPU board");
                ui.label(emulation_speed_label(
                    self.effective_emulation_speed(),
                    board,
                ));
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
            ui.centered_and_justified(|ui| self.draw_altair(ui, frame_dt));
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.small(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(format!("Core: {}", self.machine.engine().label()));
                });
            });
        });

        let switches_before_helper_viewports = self.machine.switch_register();

        self.show_tty_viewport(ctx);
        self.show_terminal_viewport(ctx);
        self.show_external_serial_viewport(ctx);
        self.show_external_com_viewport(ctx);
        self.show_memory_viewer_viewport(ctx);
        self.show_debugger_controls_viewport(ctx);
        self.show_instruction_history_viewport(ctx);
        self.show_loop_inspector_viewport(ctx);
        self.show_io_inspector_viewport(ctx);
        self.show_standalone_front_panel_operator_viewport(ctx);
        self.draw_authentic_loader_window(ctx);

        let switches_after_helper_viewports = self.machine.switch_register();
        if switches_after_helper_viewports != switches_before_helper_viewports {
            self.audio.play_once("assets/click.mp3");
            ctx.request_repaint();
        }

        super::ui::persist_configuration_if_changed(self);
    }
}
