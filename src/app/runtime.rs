use super::*;

impl eframe::App for RusTairApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        super::ui::ensure_persistent_configuration_loaded(self);

        // `machine.s100_hardware` is the physical authority. Persistence parses
        // old aggregate RAM keys only long enough to migrate pre-v5 files; no
        // guest execution is allowed before the selected backend has the exact
        // persisted slot topology mounted.
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
            let before_t_states = self.machine.intel8080_state().total_t_states;
            self.machine.run_cycles(budget);
            let after_t_states = self.machine.intel8080_state().total_t_states;

            if speed != EmulationSpeed::Unlimited {
                let executed = match (before_t_states, after_t_states) {
                    (Some(before), Some(after)) => after.saturating_sub(before),
                    _ => u64::from(budget),
                };
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
                        ui.small("Quick Load is the emulator convenience path. Authentic Load executes the historical bootstrap and receives BASIC through the emulated serial board.");
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
                        ui.small("Acceleration changes host execution rate only; it does not alter the installed CPU board hardware clock.");
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
                        ui.small("Board type, base address, population and timing now come only from Configuration → S-100 Chassis / Cards. Aggregate RAM-size/timing controls have been retired so they cannot fabricate a different runtime topology.");
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

                    ui.menu_button("Serial board", |ui| {
                        let current = self.config.machine.serial_board;
                        let sio = self.config.machine.sio_hardware;
                        let straps = self.config.machine.two_sio_straps;
                        let irq_wiring = self.config.machine.two_sio_interrupt_wiring;
                        ui.label(format!("Installed board: {}", current.label()));
                        ui.small("Transitional serial runtime selector: this disappears when 88-SIO/88-2SIO are live per-slot S-100 cards, allowing multiple serial boards exactly as in real hardware.");
                        match current {
                            SerialBoard::Sio88 => {
                                ui.small(format!(
                                    "Port 0: {:02X}h status/control / {:02X}h data · {} · {} · {}",
                                    sio.address.status(), sio.address.data(), sio.revision.label(), sio.baud.label(), sio.format.label()
                                ));
                                ui.small(format!("Line interface: {}", sio.interface.label()));
                                ui.small(format!("IN IRQ → {}", sio.interrupt_wiring.input.label()));
                                ui.small(format!("OUT IRQ → {}", sio.interrupt_wiring.output.label()));
                            }
                            SerialBoard::TwoSio88 => {
                                ui.small(format!(
                                    "Port 0: {:02X}h status/control / {:02X}h data · {} baud-generator tap · {}",
                                    straps.address.port0_status(), straps.address.port0_data(), straps.port0_baud.label(), straps.port0_interface.label()
                                ));
                                ui.small(format!(
                                    "Port 1: {:02X}h status/control / {:02X}h data · {} baud-generator tap · {}",
                                    straps.address.port1_status(), straps.address.port1_data(), straps.port1_baud.label(), straps.port1_interface.label()
                                ));
                                ui.small(format!("DI / Port 0 IRQ → {}", irq_wiring.port0.label()));
                                ui.small(format!("EI / Port 1 IRQ → {}", irq_wiring.port1.label()));
                            }
                        }

                        if current == SerialBoard::Sio88 {
                            ui.separator();
                            ui.label("Physical 88-SIO configuration:");
                            let powered = self.machine.powered();
                            ui.add_enabled_ui(!powered, |ui| {
                                ui.menu_button(format!("Logic revision: {}", sio.revision.label()), |ui| {
                                    for revision in crate::config::SioRevision::ALL {
                                        if ui.selectable_label(sio.revision == revision, revision.label()).clicked() {
                                            let mut next = sio;
                                            next.revision = revision;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(format!("Line interface: {}", sio.interface.label()), |ui| {
                                    for interface in crate::config::SioInterface::ALL {
                                        if ui.selectable_label(sio.interface == interface, interface.label()).clicked() {
                                            let mut next = sio;
                                            next.interface = interface;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(
                                    format!("I/O address: {:02X}h/{:02X}h", sio.address.status(), sio.address.data()),
                                    |ui| {
                                        for base in (0u8..=0xfe).step_by(2) {
                                            let pair = crate::config::SioAddressPair::try_new(base).expect("even 88-SIO address pair");
                                            let label = format!("{:02X}h/{:02X}h", pair.status(), pair.data());
                                            if ui.selectable_label(sio.address == pair, label).clicked() {
                                                let mut next = sio;
                                                next.address = pair;
                                                self.apply_sio_hardware(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                                ui.menu_button(format!("Baud preset: {}", sio.baud.label()), |ui| {
                                    for baud in crate::config::SioBaudRate::STANDARD {
                                        if ui.selectable_label(sio.baud == baud, baud.label()).clicked() {
                                            let mut next = sio;
                                            next.baud = baud;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(format!("Data bits: {}", sio.format.data_bits.bits()), |ui| {
                                    for bits in crate::config::SioDataBits::ALL {
                                        if ui.selectable_label(sio.format.data_bits == bits, bits.label()).clicked() {
                                            let mut next = sio;
                                            next.format.data_bits = bits;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(format!("Parity: {}", sio.format.parity.label()), |ui| {
                                    for parity in crate::config::SioParity::ALL {
                                        if ui.selectable_label(sio.format.parity == parity, parity.label()).clicked() {
                                            let mut next = sio;
                                            next.format.parity = parity;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(format!("Stop bits: {}", sio.format.stop_bits.bits()), |ui| {
                                    for stop_bits in crate::config::SioStopBits::ALL {
                                        if ui.selectable_label(sio.format.stop_bits == stop_bits, stop_bits.label()).clicked() {
                                            let mut next = sio;
                                            next.format.stop_bits = stop_bits;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(format!("Input IRQ source: {}", sio.interrupt_wiring.input.label()), |ui| {
                                    for target in crate::config::SioInterruptTarget::ALL {
                                        if ui.selectable_label(sio.interrupt_wiring.input == target, target.label()).clicked() {
                                            let mut next = sio;
                                            next.interrupt_wiring.input = target;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.menu_button(format!("Output IRQ source: {}", sio.interrupt_wiring.output.label()), |ui| {
                                    for target in crate::config::SioInterruptTarget::ALL {
                                        if ui.selectable_label(sio.interrupt_wiring.output == target, target.label()).clicked() {
                                            let mut next = sio;
                                            next.interrupt_wiring.output = target;
                                            self.apply_sio_hardware(next);
                                            ui.close();
                                        }
                                    }
                                });
                            });
                            if powered {
                                ui.small("POWER OFF required: revision wiring, address jumpers, baud counter preset, UART format pins, A/B/C interface and interrupt jumpers are physical card configuration.");
                            }
                            ui.small("The MITS baud chart provides 110, 150, 300, 600, 1200, 2400, 4800, 9600 and 19200 baud presets. The hardware counter can also be wired for non-table rates up to 25 kbaud; persisted non-standard presets remain representable but are not fabricated as standard menu choices.");
                            ui.small("88-SIO A is RS-232 level, B is TTL level, and C is the TTY/current-loop interface used with Teletypes.");
                            ui.small("D0 enables the input interrupt source and D1 enables the output source at runtime; these menus model the separate physical IN/OUT routing pads after those enables. Selecting the same destination for both sources represents the equivalent combined BH wiring result.");
                            ui.small("PINT is the direct processor interrupt path. VI0..VI7 are raw requests for a separate 88-Vector Interrupt system and never fabricate an 8080 RST opcode inside the 88-SIO.");
                            if sio.revision == crate::config::SioRevision::Rev0 {
                                ui.small("Rev 0 uses the original external RIN/ROT device-ready flip-flops for interrupt source state; COM2502 RDA/TBMT remain separate signals.");
                            }
                        }

                        if current == SerialBoard::TwoSio88 {
                            ui.separator();
                            ui.label("Physical 88-2SIO straps and signal interconnects:");
                            let powered = self.machine.powered();
                            ui.add_enabled_ui(!powered, |ui| {
                                ui.menu_button(
                                    format!("A2-A7 address block: {:02X}h-{:02X}h", straps.address.base(), straps.address.base() + 3),
                                    |ui| {
                                        for base in (0u8..=0xf8).step_by(4) {
                                            let block = crate::config::TwoSioAddressBlock::try_new(base)
                                                .expect("aligned non-FF 88-2SIO block");
                                            let selected = straps.address == block;
                                            let label = format!("{:02X}h-{:02X}h", base, base + 3);
                                            if ui.selectable_label(selected, label).clicked() {
                                                let mut next = straps;
                                                next.address = block;
                                                self.apply_two_sio_straps(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                                ui.menu_button(
                                    format!("Port 0 baud tap: {}", straps.port0_baud.label()),
                                    |ui| {
                                        for tap in crate::config::TwoSioBaudTap::ALL {
                                            if ui.selectable_label(straps.port0_baud == tap, tap.label()).clicked() {
                                                let mut next = straps;
                                                next.port0_baud = tap;
                                                self.apply_two_sio_straps(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                                ui.menu_button(
                                    format!("Port 1 baud tap: {}", straps.port1_baud.label()),
                                    |ui| {
                                        for tap in crate::config::TwoSioBaudTap::ALL {
                                            if ui.selectable_label(straps.port1_baud == tap, tap.label()).clicked() {
                                                let mut next = straps;
                                                next.port1_baud = tap;
                                                self.apply_two_sio_straps(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                                ui.menu_button(
                                    format!("Port 0 signal interface: {}", straps.port0_interface.label()),
                                    |ui| {
                                        for interface in crate::config::TwoSioSignalInterface::ALL {
                                            if ui.selectable_label(straps.port0_interface == interface, interface.label()).clicked() {
                                                let mut next = straps;
                                                next.port0_interface = interface;
                                                self.apply_two_sio_straps(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                                ui.menu_button(
                                    format!("Port 1 signal interface: {}", straps.port1_interface.label()),
                                    |ui| {
                                        for interface in crate::config::TwoSioSignalInterface::ALL {
                                            if ui.selectable_label(straps.port1_interface == interface, interface.label()).clicked() {
                                                let mut next = straps;
                                                next.port1_interface = interface;
                                                self.apply_two_sio_straps(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                            });
                            if powered {
                                ui.small("POWER OFF required: these controls represent moving physical jumpers/hardwiring on the 88-2SIO board.");
                            }
                            ui.small("A2-A7 select one four-port block; A0/A1 select the two ACIAs/registers inside it. FCh-FFh is intentionally unavailable because FFh belongs to the Altair front-panel sense-switch input.");
                            ui.small("The selected baud tap is the board clock source. MC6850 CR1:CR0 still selects /1, /16 or /64; the tap is not a terminal-speed override.");
                            ui.small("MITS allows each port to be hardwired independently for RS-232 voltage levels, TTL voltage levels, or a TTY 20 mA current loop. Direct ASR-33 requires the current-loop option; direct External COM requires RS-232. No hidden level converter is inserted.");

                            ui.separator();
                            ui.label("Physical 88-2SIO interrupt wiring:");
                            ui.add_enabled_ui(!powered, |ui| {
                                ui.menu_button(
                                    format!("DI / Port 0 IRQ: {}", irq_wiring.port0.label()),
                                    |ui| {
                                        for target in crate::config::TwoSioInterruptTarget::ALL {
                                            if ui.selectable_label(irq_wiring.port0 == target, target.label()).clicked() {
                                                let mut next = irq_wiring;
                                                next.port0 = target;
                                                self.apply_two_sio_interrupt_wiring(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                                ui.menu_button(
                                    format!("EI / Port 1 IRQ: {}", irq_wiring.port1.label()),
                                    |ui| {
                                        for target in crate::config::TwoSioInterruptTarget::ALL {
                                            if ui.selectable_label(irq_wiring.port1 == target, target.label()).clicked() {
                                                let mut next = irq_wiring;
                                                next.port1 = target;
                                                self.apply_two_sio_interrupt_wiring(next);
                                                ui.close();
                                            }
                                        }
                                    },
                                );
                            });
                            if powered {
                                ui.small("POWER OFF required: DI/EI are physical interrupt-request wires, not runtime software settings.");
                            }
                            ui.small("MITS DI is Port 0 and EI is Port 1. Each may be disconnected, wired to the single PINT processor request, or wired to VI0..VI7 for a separate 88-Vector Interrupt system.");
                            ui.small("VI0..VI7 stop at the raw chassis boundary until an 88-VI board is installed; selecting VIx never fabricates a CPU RST opcode inside the 88-2SIO.");
                            let vi_mask = self.machine.two_sio_vector_interrupt_requests();
                            ui.small(format!(
                                "Active raw 88-2SIO vector outputs: {}",
                                Self::two_sio_vi_mask_label(vi_mask)
                            ));
                        }

                        ui.separator();
                        ui.label("External wiring:");
                        ui.small(format!("ASR-33 → {}", Self::serial_connection_label(current, straps, self.asr_connection())));
                        ui.small(format!("Text Terminal → {}", Self::serial_connection_label(current, straps, self.terminal_connection())));
                        ui.small(format!("External TCP → {}", Self::serial_connection_label(current, straps, self.external_tcp_connection())));
                        ui.small(format!("External COM → {}", Self::serial_connection_label(current, straps, self.external_com_connection())));
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
