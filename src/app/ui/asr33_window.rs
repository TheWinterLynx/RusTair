use super::super::*;
use crate::config::TerminalDuplex;

impl RusTairApp {
    pub(in crate::app) fn update_paper_tape(&mut self) {
        self.update_paper_tape_reader();
        self.update_paper_tape_punch();
    }

    fn update_paper_tape_reader(&mut self) {
        if !self.asr33.reader_running {
            return;
        }

        if !self.tty.tape_input_pending() {
            self.asr33.reader_running = false;
            self.status = "ASR-33 paper tape reader: end of tape".into();
            return;
        }

        // Reading is a real LINE-mode input operation. Mounting/rewinding is
        // allowed with the computer stopped, but physical tape cannot advance
        // into the UART until the Altair is powered, RUNning and connected.
        if self.tty.mode != TtyMode::Line
            || !self.asr_connection().is_connected()
            || !self.machine.powered()
            || !self.machine.running()
            || !self.asr_serial_rx_empty()
        {
            return;
        }

        let now = Instant::now();
        let char_time = self.asr33.reader_speed.char_time();
        if !char_time.is_zero() && now.duration_since(self.asr33.last_reader_tick) < char_time {
            return;
        }

        if let Some(byte) = self.tty.next_tape_byte() {
            self.asr_serial_receive(byte);
            self.asr33.last_reader_tick = now;
            if self.asr33.media_sound_due(now) {
                // Dedicated transport feedback using the existing short
                // mechanical click; accelerated modes are rate-limited above.
                self.audio.play_once("assets/click.mp3");
            }
        }

        if !self.tty.tape_input_pending() {
            self.asr33.reader_running = false;
            self.status = "ASR-33 paper tape reader: end of tape".into();
        }
    }

    fn update_paper_tape_punch(&mut self) {
        if !self.asr33.punch_running || !self.tty.tape_capture_enabled() {
            return;
        }
        if self.tty.mode == TtyMode::Off {
            return;
        }
        if self.tty.tape_punch_pending_len() == 0 {
            return;
        }

        let now = Instant::now();
        let char_time = self.asr33.punch_speed.char_time();
        if !char_time.is_zero() && now.duration_since(self.asr33.last_punch_tick) < char_time {
            return;
        }

        let mut punched = 0usize;
        let budget = if char_time.is_zero() { 4096 } else { 1 };
        while punched < budget && self.tty.step_tape_punch().is_some() {
            punched += 1;
            if !char_time.is_zero() {
                break;
            }
        }
        if punched > 0 {
            self.asr33.last_punch_tick = now;
            if self.asr33.media_sound_due(now) {
                // The original imported sound set has no dedicated punch sample;
                // the printer impact is the closest existing electromechanical
                // transient and avoids inventing a falsely sourced recording.
                self.audio.play_once("assets/printcharpadded.mp3");
            }
        }
    }

    fn draw_tty_connection_selector(&mut self, ui: &mut egui::Ui) {
        let board = self.config.machine.serial_board;
        let current = self.asr_connection();
        let mut selected = current;

        ui.label("Connection:");
        egui::ComboBox::from_id_salt("asr33-serial-connection")
            .selected_text(Self::serial_connection_label(board, current))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, SerialConnection::Disconnected, "Disconnected");
                ui.selectable_value(
                    &mut selected,
                    SerialConnection::Port0,
                    Self::serial_connection_label(board, SerialConnection::Port0),
                );
                if board == SerialBoard::TwoSio88 {
                    ui.selectable_value(
                        &mut selected,
                        SerialConnection::Port1,
                        Self::serial_connection_label(board, SerialConnection::Port1),
                    );
                }
            });

        if selected != current {
            self.set_serial_connection(SerialDevice::InternalAsr33, selected);
        }
    }

    fn draw_tty_speed_selector(&mut self, ui: &mut egui::Ui) {
        let current = self.config.peripherals.asr33_speed;
        let mut selected = current;
        ui.label("Speed:");
        egui::ComboBox::from_id_salt("asr33-speed")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for speed in Asr33Speed::ALL {
                    ui.selectable_value(&mut selected, speed, speed.label());
                }
            });
        if selected != current {
            self.set_asr_speed(selected);
        }
    }

    fn draw_tty_duplex_selector(&mut self, ui: &mut egui::Ui) {
        ui.label("DUPLEX:");
        egui::ComboBox::from_id_salt("asr33-duplex")
            .selected_text(self.asr33.duplex.label())
            .show_ui(ui, |ui| {
                for duplex in TerminalDuplex::ALL {
                    ui.selectable_value(&mut self.asr33.duplex, duplex, duplex.label());
                }
            });
    }

    fn draw_tty_power_controls(&mut self, ui: &mut egui::Ui) {
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
    }

    fn draw_tape_speed_selector(
        ui: &mut egui::Ui,
        id: &'static str,
        speed: &mut TapeTransportSpeed,
    ) {
        egui::ComboBox::from_id_salt(id)
            .selected_text(speed.label())
            .show_ui(ui, |ui| {
                for candidate in TapeTransportSpeed::ALL {
                    ui.selectable_value(speed, candidate, candidate.label());
                }
            });
    }

    fn reader_can_run(&self) -> bool {
        self.tty.tape_input_total_len() > 0
            && self.tty.tape_input_pending()
            && self.tty.mode == TtyMode::Line
            && self.asr_connection().is_connected()
            && self.machine.powered()
            && self.machine.running()
    }

    fn draw_tty_reader_controls(&mut self, ui: &mut egui::Ui) {
        ui.strong("READER");
        if ui.button("Put tape…").clicked() {
            self.load_paper_tape();
        }

        let can_run = self.reader_can_run();
        let read = ui.add_enabled(
            can_run && !self.asr33.reader_running,
            egui::Button::new(if self.tty.tape_input_position() == 0 { "Read" } else { "Resume" }),
        );
        if read.clicked() {
            self.asr33.reader_running = true;
            self.asr33.last_reader_tick = Instant::now()
                .checked_sub(self.asr33.reader_speed.char_time())
                .unwrap_or_else(Instant::now);
            self.audio.play_once("assets/click.mp3");
            self.status = format!(
                "ASR-33 reader started — {}",
                self.asr33.reader_speed.label()
            );
        }
        if !can_run {
            read.on_disabled_hover_text(
                "Reading requires a mounted tape, ASR-33 LINE mode, a connected serial port, and an Altair that is powered and RUNning.",
            );
        }

        if ui.add_enabled(self.asr33.reader_running, egui::Button::new("Pause")).clicked() {
            self.asr33.reader_running = false;
            self.audio.play_once("assets/click.mp3");
            self.status = "ASR-33 paper tape reader paused".into();
        }

        let mounted = self.tty.tape_input_total_len() > 0;
        if ui.add_enabled(mounted, egui::Button::new("Rewind")).clicked() {
            self.asr33.reader_running = false;
            self.tty.rewind_tape_reader();
            self.asr33.last_reader_tick = Instant::now();
            self.audio.play_once("assets/click.mp3");
            self.status = "ASR-33 paper tape rewound to leader".into();
        }
        if ui.add_enabled(mounted, egui::Button::new("Eject")).clicked() {
            self.asr33.reader_running = false;
            self.tty.eject_tape_reader();
            self.audio.play_once("assets/click.mp3");
            self.status = "ASR-33 paper tape ejected".into();
        }

        ui.label("Rate:");
        Self::draw_tape_speed_selector(ui, "asr33-reader-speed", &mut self.asr33.reader_speed);
        ui.monospace(format!(
            "{}/{} bytes",
            self.tty.tape_input_position(),
            self.tty.tape_input_total_len()
        ));
    }

    fn draw_tty_punch_controls(&mut self, ui: &mut egui::Ui) {
        ui.strong("PUNCH");
        let mounted = self.tty.tape_capture_enabled();
        if ui.add_enabled(!mounted, egui::Button::new("Put blank tape")).clicked() {
            self.tty.prepare_tape_punch();
            self.asr33.punch_running = false;
            self.asr33.last_punch_tick = Instant::now();
            self.audio.play_once("assets/click.mp3");
            self.status = "Blank paper tape mounted in ASR-33 punch".into();
        }

        let can_punch = self.tty.tape_capture_enabled() && self.tty.mode != TtyMode::Off;
        let punch = ui.add_enabled(
            can_punch && !self.asr33.punch_running,
            egui::Button::new(if self.tty.punched_tape_len() == 0 { "Punch" } else { "Resume" }),
        );
        if punch.clicked() {
            self.tty.resume_tape_punch();
            self.asr33.punch_running = true;
            self.asr33.last_punch_tick = Instant::now()
                .checked_sub(self.asr33.punch_speed.char_time())
                .unwrap_or_else(Instant::now);
            self.audio.play_once("assets/click.mp3");
            self.status = format!("ASR-33 punch started — {}", self.asr33.punch_speed.label());
        }
        if self.tty.mode == TtyMode::Off {
            punch.on_disabled_hover_text("Switch the ASR-33 to LINE or LOCAL before running the punch.");
        }

        if ui.add_enabled(self.asr33.punch_running, egui::Button::new("Pause")).clicked() {
            self.asr33.punch_running = false;
            self.tty.pause_tape_punch();
            self.audio.play_once("assets/click.mp3");
            self.status = "ASR-33 paper tape punch paused".into();
        }

        if ui.add_enabled(mounted, egui::Button::new("Finish & save…")).clicked() {
            self.asr33.punch_running = false;
            self.tty.finish_tape_punch();
            self.audio.play_once("assets/click.mp3");
            self.save_punched_tape();
        }

        ui.label("Rate:");
        Self::draw_tape_speed_selector(ui, "asr33-punch-speed", &mut self.asr33.punch_speed);
        let pending = self.tty.tape_punch_pending_len();
        ui.monospace(if pending == 0 {
            format!("{} bytes", self.tty.punched_tape_len())
        } else {
            format!("{} bytes + {pending} queued", self.tty.punched_tape_len())
        });
    }

    fn draw_tty_menu(&mut self, ctx: &egui::Context) {
        self.process_tty_keyboard(ctx);
        egui::TopBottomPanel::top("tty-menu").show(ctx, |ui| {
            // Do not switch layouts at an arbitrary pixel breakpoint. Every row
            // wraps from its first widget, so resizing can never hide the tail
            // of a toolbar while waiting to cross a magic width threshold.
            ui.horizontal_wrapped(|ui| {
                self.draw_tty_power_controls(ui);
                ui.separator();
                self.draw_tty_connection_selector(ui);
                ui.separator();
                self.draw_tty_speed_selector(ui);
                ui.separator();
                self.draw_tty_duplex_selector(ui);
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| self.draw_tty_reader_controls(ui));
            ui.horizontal_wrapped(|ui| self.draw_tty_punch_controls(ui));
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} columns", self.tty.paper_width));
                if ui.button("Clear printed paper").clicked() {
                    self.tty.clear_paper();
                }
            });
        });
    }

    fn draw_tty_window(&mut self, ctx: &egui::Context) {
        self.update_key_animation(ctx);
        self.draw_tty_menu(ctx);

        if self.asr33.power_flash_until.is_some_and(|until| Instant::now() < until) {
            ctx.request_repaint_after(PANEL_FRAME);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| self.draw_teletype(ui));
        });
        egui::TopBottomPanel::bottom("tty-status").show(ctx, |ui| {
            let connection = self.asr_connection();
            let connection_label = Self::serial_connection_label(self.config.machine.serial_board, connection);
            let tx = if connection.is_connected() {
                if self.asr_serial_tx_busy() { "BUSY" } else { "READY" }
            } else {
                "N/A"
            };
            let duplex = if self.tty.mode == TtyMode::Local {
                "LOCAL ONLY"
            } else {
                self.asr33.duplex.label()
            };
            let reader = if self.asr33.reader_running { "READ" } else { "STOP" };
            let punch = if self.asr33.punch_running { "PUNCH" } else { "STOP" };
            ui.small(format!(
                "ASR-33 {}  |  {}  |  {}  |  {}  |  RX {}  |  TX {}  |  READER {} {}  |  PUNCH {} {}  |  column {}/{}",
                match self.tty.mode {
                    TtyMode::Off => "OFF",
                    TtyMode::Line => "LINE",
                    TtyMode::Local => "LOCAL",
                },
                self.config.peripherals.asr33_speed.label(),
                duplex,
                connection_label,
                self.asr_serial_rx_len(),
                tx,
                reader,
                self.asr33.reader_speed.short_label(),
                punch,
                self.asr33.punch_speed.short_label(),
                self.tty.column,
                self.tty.paper_width,
            ));
        });
    }

    pub(in crate::app) fn show_tty_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.asr33.window_open {
            return;
        }
        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-asr33"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — ASR-33 Teletype")
                .with_inner_size([1040.0, 760.0])
                .with_min_inner_size([700.0, 520.0])
                .with_resizable(true),
            |tty_ctx, _class| {
                self.draw_tty_window(tty_ctx);
                if tty_ctx.input(|i| i.viewport().close_requested()) {
                    self.asr33.window_open = false;
                    self.asr33.reader_running = false;
                    self.asr33.punch_running = false;
                    self.tty.pause_tape_punch();
                    self.set_tty_mode(TtyMode::Off);
                }
            },
        );
    }
}
