use super::*;

pub(in crate::app) fn draw_main_menu(app: &mut RusTairApp, ctx: &egui::Context) {
    egui::TopBottomPanel::top("menu").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            draw_file_menu(app, ui);
            draw_machine_menu(app, ctx, ui);
            draw_peripherals_menu(app, ui);
            draw_view_menu(app, ctx, ui);
            draw_tools_menu(app, ctx, ui);
            draw_settings_menu(app, ui);
        });
    });
}

fn draw_file_menu(app: &mut RusTairApp, ui: &mut egui::Ui) {
    ui.menu_button("File", |ui| {
        if ui.button("Load Binary…").clicked() {
            app.load_binary_dialog();
            ui.close();
        }

        ui.separator();
        ui.menu_button("Microsoft 4K BASIC 3.2", |ui| {
            if ui.button("Quick Load — Direct RAM").clicked() {
                app.load_bundled_basic();
                ui.close();
            }
            if ui.button("Authentic Load — Paper Tape…").clicked() {
                app.open_authentic_basic_loader();
                ui.close();
            }
            ui.separator();
            ui.small("Quick Load is an emulator convenience. Authentic Load executes the historical bootstrap and receives BASIC through the installed S-100 serial card.");
        });
    });
}

fn draw_machine_menu(app: &mut RusTairApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.menu_button("Machine", |ui| {
        if ui.button("S-100 Hardware…").clicked() {
            super::open_s100_hardware_editor(ctx);
            ui.close();
        }

        ui.separator();
        ui.menu_button("CPU", |ui| {
            let board = app
                .config
                .machine
                .s100_hardware
                .active_cpu_board()
                .expect("validated S-100 configuration has one CPU board");
            let cpu = board.cpu_model();
            ui.label(format!("Installed CPU board: {}", board.label()));
            ui.small(format!("Processor: {}", cpu.label()));
            ui.small(format!(
                "Hardware clock: {:.1} MHz",
                board.clock_hz() as f32 / 1_000_000.0
            ));
            ui.separator();
            ui.small("Full and Partial are internal execution strategies over this same physical CPU board, not separate machines.");
        });

        ui.menu_button("Memory", |ui| {
            let hardware = app.config.machine.s100_hardware;
            ui.label(format!(
                "Installed S-100 RAM: {} KiB across physical cards",
                hardware.installed_ram_bytes() / 1024
            ));
            ui.small("Board type, address, population and timing are configured in Machine → S-100 Hardware.");
            ui.separator();
            ui.menu_button("Power-on Contents", |ui| {
                for ram_init in RamInit::ALL {
                    let selected = app.config.machine.ram_init == ram_init;
                    if ui.selectable_label(selected, ram_init.label()).clicked() {
                        app.apply_ram_initialization(ram_init);
                        ui.close();
                    }
                }
            });
            if app.machine.powered() {
                ui.small("POWER OFF required before changing RAM power-on contents.");
            }
        });

        ui.menu_button("Historical Hardware", |ui| {
            let mut historical_power_on = app
                .config
                .compatibility
                .historical_undefined_run_latch_power_on;
            if ui
                .checkbox(
                    &mut historical_power_on,
                    "Undefined RUN/STOP latch at power-on",
                )
                .changed()
            {
                app.config
                    .compatibility
                    .historical_undefined_run_latch_power_on = historical_power_on;
                app.status = if historical_power_on {
                    "Historical power-on enabled: next POWER ON may start with RUN or STOP randomly".into()
                } else {
                    "Historical power-on disabled: next POWER ON will safely start with STOP latch".into()
                };
            }
            ui.small("The original Altair 8800 RUN/STOP latch had no guaranteed power-on state. This affects the next POWER ON only.");
        });
    });
}

fn draw_peripherals_menu(app: &mut RusTairApp, ui: &mut egui::Ui) {
    ui.menu_button("Peripherals", |ui| {
        ui.menu_button("External Serial", |ui| {
            ui.menu_button("TCP", |ui| {
                app.draw_external_serial_config_menu(ui);
            });
            ui.menu_button("COM", |ui| {
                app.draw_external_com_config_menu(ui);
            });
        });

        ui.separator();
        ui.small("ASR-33 and Text Terminal speed are configured in their respective windows.");
    });
}

fn draw_view_menu(app: &mut RusTairApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.menu_button("View", |ui| {
        if ui.button("Front Panel Operator").clicked() {
            app.open_standalone_front_panel_operator(ctx);
            ui.close();
        }
        ui.separator();
        if ui.button("ASR-33 Teletype").clicked() {
            app.asr33.window_open = true;
            ui.close();
        }
        if ui.button("Text Terminal").clicked() {
            app.terminal.window_open = true;
            ui.close();
        }
        ui.menu_button("External Serial", |ui| {
            if ui.button("TCP").clicked() {
                app.external_serial.window_open = true;
                ui.close();
            }
            if ui.button("COM").clicked() {
                app.external_com.window_open = true;
                if app.external_com.available_ports.is_empty() {
                    app.refresh_external_com_ports();
                }
                ui.close();
            }
        });
        ui.separator();
        if ui.button("LED Appearance…").clicked() {
            super::open_led_visual_controls(app);
            ui.close();
        }
    });
}

fn draw_tools_menu(app: &mut RusTairApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    ui.menu_button("Tools", |ui| {
        if ui.button("Debugger").clicked() {
            app.open_debugger_controls(ctx);
            ui.close();
        }
        if ui.button("RAM Viewer").clicked() {
            app.open_memory_viewer(ctx);
            ui.close();
        }
        if ui.button("Execution History").clicked() {
            app.open_instruction_history(ctx);
            ui.close();
        }
        if ui.button("I/O Inspector").clicked() {
            app.open_io_inspector(ctx);
            ui.close();
        }
        if ui.button("T-State Teacher").clicked() {
            app.open_bus_teacher(ctx);
            ui.close();
        }

        ui.separator();
        ui.menu_button("CPU Diagnostics", |ui| {
            app.draw_cpu_diagnostics_menu(ui);
        });
    });
}

fn draw_settings_menu(app: &mut RusTairApp, ui: &mut egui::Ui) {
    ui.menu_button("Settings", |ui| {
        ui.menu_button("Host Execution Speed", |ui| {
            let board = app
                .config
                .machine
                .s100_hardware
                .active_cpu_board()
                .expect("validated S-100 configuration has one CPU board");
            let diagnostic_running = app.cpu_diagnostic_run_speed_label.is_some();
            ui.add_enabled_ui(!diagnostic_running, |ui| {
                for speed in SELECTABLE_EMULATION_SPEEDS {
                    let label = emulation_speed_label(speed, board);
                    if ui
                        .selectable_label(app.config.preferences.emulation_speed == speed, label)
                        .clicked()
                    {
                        app.set_emulation_speed(speed);
                        ui.close();
                    }
                }
            });
            if app.config.preferences.emulation_speed == EmulationSpeed::X2 {
                ui.small("Loaded legacy 2× preference. Select Authentic, 5×, 10× or Unlimited to replace it.");
            }
            if let Some(speed) = app.cpu_diagnostic_run_speed_label.as_deref() {
                ui.small(format!("Locked while external CPU diagnostic runs: {speed}"));
            }
            ui.separator();
            ui.small("This changes how quickly the host advances emulated time. It never changes the installed CPU board's hardware clock or S-100 timing model.");
        });

        ui.menu_button("Behaviour", |ui| {
            let mut auto_open_basic_console = app.config.preferences.auto_open_basic_console;
            if ui
                .checkbox(&mut auto_open_basic_console, "Auto-open BASIC console")
                .changed()
            {
                app.config.preferences.auto_open_basic_console = auto_open_basic_console;
                app.status = if auto_open_basic_console {
                    "Preference enabled: auto-open BASIC console".into()
                } else {
                    "Preference disabled: BASIC loads without opening a terminal window".into()
                };
            }
            ui.small("When bundled BASIC is loaded, reveal the endpoint already connected to Port 0. This never rewires the machine.");
        });

        ui.menu_button("Compatibility", |ui| {
            let mut basic32_workaround = app
                .config
                .compatibility
                .basic32_64k_probe_workaround;
            if ui
                .checkbox(
                    &mut basic32_workaround,
                    "BASIC 3.2 64K memory-probe workaround",
                )
                .changed()
            {
                app.config.compatibility.basic32_64k_probe_workaround = basic32_workaround;
                if !basic32_workaround {
                    app.machine.clear_transient_memory_guards();
                }
                app.status = if basic32_workaround {
                    "Compatibility enabled: BASIC 3.2 64K memory-probe workaround".into()
                } else {
                    "Compatibility disabled: authentic BASIC 3.2 64K bug is reproducible".into()
                };
            }
            ui.small("This is a software workaround, not historical Altair hardware behaviour.");
        });

        ui.menu_button("Audio", |ui| {
            let mut muted = app.audio.muted();
            if ui.checkbox(&mut muted, "Mute").changed() {
                app.audio.set_muted(muted);
            }
        });
    });
}
