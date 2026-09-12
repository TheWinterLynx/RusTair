use super::*;

const STATUS_BAR_FONT_SIZE: f32 = 16.0;
const STATUS_BAR_STATE_WIDTH: f32 = 150.0;
const STATUS_BAR_REGISTERS_WIDTH: f32 = 285.0;
const STATUS_BAR_SPEED_WIDTH: f32 = 165.0;
const STATUS_BAR_FIELD_GAP: f32 = 16.0;
const STATUS_BAR_ROW_HEIGHT: f32 = 24.0;

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
            self.status = "S-100 hardware configuration mounted".into();
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
                &mut self.machine,
                budget,
                super::execution_frame::CPU_FRAME_TIME,
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

        super::ui::draw_main_menu(self, ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_altair(ui, frame_dt));
        });

        let cpu = self.machine.intel8080_state();
        let panel = self.machine.front_panel_state();
        let board = self
            .config
            .machine
            .s100_hardware
            .active_cpu_board()
            .expect("validated S-100 configuration has one CPU board");
        let execution_state = if !panel.powered {
            "POWER OFF"
        } else if cpu.halted.unwrap_or(false) {
            if panel.running {
                "HALTED / RUN ON"
            } else {
                "HALTED / RUN OFF"
            }
        } else if panel.running {
            "RUNNING"
        } else {
            "STOPPED"
        };
        let speed_label = match self.effective_emulation_speed() {
            EmulationSpeed::Authentic => format!(
                "Speed: {:.1} MHz",
                board.clock_hz() as f32 / 1_000_000.0
            ),
            EmulationSpeed::X2 => "Speed: 2x".into(),
            EmulationSpeed::X5 => "Speed: 5x".into(),
            EmulationSpeed::X10 => "Speed: 10x".into(),
            EmulationSpeed::Unlimited => "Speed: Unlimited".into(),
        };
        let status_text = if self.status.ends_with("saved configuration loaded") {
            "Saved configuration loaded"
        } else if self.status.starts_with("Ready —") {
            "Ready"
        } else {
            self.status.as_str()
        };

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let (bar_rect, _) = ui.allocate_exact_size(
                egui::Vec2::new(ui.available_width(), STATUS_BAR_ROW_HEIGHT),
                egui::Sense::hover(),
            );

            // Operational fields are anchored to the right edge with fixed
            // widths. Only the transient message expands with the window, so
            // changing registers or execution state never moves a boundary.
            let mut right = bar_rect.right();
            let speed_rect = egui::Rect::from_min_max(
                egui::Pos2::new(right - STATUS_BAR_SPEED_WIDTH, bar_rect.top()),
                egui::Pos2::new(right, bar_rect.bottom()),
            );
            right = speed_rect.left() - STATUS_BAR_FIELD_GAP;
            let registers_rect = egui::Rect::from_min_max(
                egui::Pos2::new(right - STATUS_BAR_REGISTERS_WIDTH, bar_rect.top()),
                egui::Pos2::new(right, bar_rect.bottom()),
            );
            right = registers_rect.left() - STATUS_BAR_FIELD_GAP;
            let state_rect = egui::Rect::from_min_max(
                egui::Pos2::new(right - STATUS_BAR_STATE_WIDTH, bar_rect.top()),
                egui::Pos2::new(right, bar_rect.bottom()),
            );
            right = state_rect.left() - STATUS_BAR_FIELD_GAP;
            let message_rect = egui::Rect::from_min_max(
                bar_rect.min,
                egui::Pos2::new(right.max(bar_rect.left()), bar_rect.bottom()),
            );

            let separator_stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            for x in [
                state_rect.left() - STATUS_BAR_FIELD_GAP * 0.5,
                registers_rect.left() - STATUS_BAR_FIELD_GAP * 0.5,
                speed_rect.left() - STATUS_BAR_FIELD_GAP * 0.5,
            ] {
                ui.painter().line_segment(
                    [
                        egui::Pos2::new(x, bar_rect.top() + 3.0),
                        egui::Pos2::new(x, bar_rect.bottom() - 3.0),
                    ],
                    separator_stroke,
                );
            }

            ui.put(
                message_rect,
                egui::Label::new(
                    egui::RichText::new(status_text).size(STATUS_BAR_FONT_SIZE),
                )
                .truncate()
                .halign(egui::Align::LEFT),
            );
            ui.put(
                state_rect,
                egui::Label::new(
                    egui::RichText::new(execution_state)
                        .size(STATUS_BAR_FONT_SIZE)
                        .strong(),
                )
                .truncate()
                .halign(egui::Align::Center),
            );
            ui.put(
                registers_rect,
                egui::Label::new(
                    egui::RichText::new(format!(
                        "PC {:04X}  SP {:04X}  A {:02X}  F {:02X}",
                        cpu.pc, cpu.sp, cpu.a, cpu.flags
                    ))
                    .size(STATUS_BAR_FONT_SIZE)
                    .monospace(),
                )
                .halign(egui::Align::Center),
            );
            ui.put(
                speed_rect,
                egui::Label::new(
                    egui::RichText::new(speed_label).size(STATUS_BAR_FONT_SIZE),
                )
                .truncate()
                .halign(egui::Align::Center),
            );
        });

        let switches_before_helper_viewports = self.machine.switch_register();

        self.show_tty_viewport(ctx);
        self.show_terminal_viewport(ctx);
        self.show_external_serial_viewport(ctx);
        self.show_external_com_viewport(ctx);
        super::ui::show_s100_hardware_editor(self, ctx);
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
