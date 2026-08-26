use super::super::*;
use crate::app::asr33_state::TapeTransportSpeed;
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
            self.asr33.last_reader_byte = Some(byte);
            self.asr33.last_reader_tick = now;
            if self.asr33.media_sound_due(now) {
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
        if self.tty.mode == TtyMode::Off || self.tty.tape_punch_pending_len() == 0 {
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
                // No dedicated punch recording is bundled in RusTair today;
                // use the existing electromechanical impact sample rather than
                // claiming a synthetic sound is an historical recording.
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

    fn reader_can_run(&mut self) -> bool {
        self.tty.tape_input_total_len() > 0
            && self.tty.tape_input_pending()
            && self.tty.mode == TtyMode::Line
            && self.asr_connection().is_connected()
            && self.machine.powered()
            && self.machine.running()
    }

    fn reader_state_label(&mut self) -> (&'static str, Color32) {
        if self.tty.tape_input_total_len() == 0 {
            return ("NO TAPE", Color32::GRAY);
        }

        if !self.tty.tape_input_pending() {
            if !self.asr_serial_rx_empty() {
                return ("END · RX PENDING", Color32::from_rgb(220, 170, 70));
            }
            return ("END", Color32::from_rgb(110, 190, 120));
        }

        if !self.asr33.reader_running {
            return if self.tty.tape_input_position() == 0 {
                ("READY", Color32::from_rgb(110, 190, 120))
            } else {
                ("PAUSED", Color32::from_rgb(220, 170, 70))
            };
        }
        if self.tty.mode != TtyMode::Line {
            return ("WAIT LINE", Color32::from_rgb(220, 170, 70));
        }
        if !self.asr_connection().is_connected() {
            return ("WAIT PORT", Color32::from_rgb(220, 170, 70));
        }
        if !self.machine.powered() {
            return ("WAIT POWER", Color32::from_rgb(220, 170, 70));
        }
        if !self.machine.running() {
            return ("WAIT RUN", Color32::from_rgb(220, 170, 70));
        }
        if !self.asr_serial_rx_empty() {
            return ("WAIT GUEST RX", Color32::from_rgb(235, 145, 65));
        }
        ("READING", Color32::from_rgb(110, 190, 120))
    }

    fn reader_byte_label(byte: Option<u8>) -> String {
        match byte {
            Some(byte) => {
                let ascii = if (0x20..=0x7e).contains(&byte) {
                    (byte as char).to_string()
                } else {
                    "·".to_owned()
                };
                format!("{ascii}  0x{byte:02X}  {byte:04o}")
            }
            None => "—  0x--  ----".into(),
        }
    }

    /// Compact 8-level paper-tape view matching the visual language of the
    /// original ASR emulator: travel arrow followed by eight data-hole tracks.
    /// Filled circles are punched bits; outlined circles are unpunched tracks.
    fn draw_reader_byte_visual(ui: &mut egui::Ui, byte: Option<u8>) {
        let (rect, response) = ui.allocate_exact_size(Vec2::new(150.0, 24.0), Sense::hover());
        let painter = ui.painter();
        let paper = Color32::from_rgb(205, 194, 165);
        let ink = Color32::from_rgb(78, 76, 69);
        let empty = Color32::from_rgb(165, 156, 134);
        painter.rect_filled(rect, 1.5, paper);
        painter.text(
            Pos2::new(rect.left() + 10.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            "▶",
            FontId::monospace(10.0),
            ink,
        );

        let byte = byte.unwrap_or(0);
        let first_x = rect.left() + 31.0;
        for (slot, bit) in (0..8).rev().enumerate() {
            let center = Pos2::new(first_x + slot as f32 * 14.0, rect.center().y);
            if byte & (1 << bit) != 0 {
                painter.circle_filled(center, 4.3, ink);
            } else {
                painter.circle_stroke(center, 4.3, egui::Stroke::new(1.0, empty));
            }
        }

        response.on_hover_text("8-level paper tape, MSB → LSB. Filled circles are punched holes in the last byte offered to the UART.");
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
            self.status = format!("ASR-33 reader started — {}", self.asr33.reader_speed.label());
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
            self.asr33.last_reader_byte = None;
            self.asr33.last_reader_tick = Instant::now();
            self.audio.play_once("assets/click.mp3");
            self.status = "ASR-33 paper tape rewound to leader".into();
        }
        if ui.add_enabled(mounted, egui::Button::new("Eject")).clicked() {
            self.asr33.reader_running = false;
            self.tty.eject_tape_reader();
            self.asr33.last_reader_byte = None;
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
        ui.separator();
        ui.label("BYTE:");
        ui.monospace(Self::reader_byte_label(self.asr33.last_reader_byte));
        Self::draw_reader_byte_visual(ui, self.asr33.last_reader_byte);
        let (reader_state, state_color) = self.reader_state_label();
        let state = ui.colored_label(state_color, reader_state);
        if reader_state == "WAIT GUEST RX" {
            state.on_hover_text("The reader has already placed the displayed byte in the emulated UART. It cannot advance until the program running on the Altair reads that RX byte from the selected serial port.");
        }
    }

    fn draw_tty_punch_controls(&mut self, ui: &mut egui::Ui) {
        ui.strong("PUNCH");
        let mounted = self.tty.tape_capture_enabled();
        let finished_unsaved = !mounted && self.tty.punched_tape_len() > 0;

        let put_blank = ui.add_enabled(
            !mounted && !finished_unsaved,
            egui::Button::new("Put blank tape"),
        );
        if put_blank.clicked() {
            self.tty.prepare_tape_punch();
            self.asr33.punch_running = false;
            self.asr33.last_punch_tick = Instant::now();
            self.audio.play_once("assets/click.mp3");
            self.status = "Blank paper tape mounted in ASR-33 punch".into();
        }
        if finished_unsaved {
            put_blank.on_disabled_hover_text("Save the finished tape first; it is being retained so a cancelled Save dialog cannot destroy it.");
        }

        let can_punch = mounted && self.tty.mode != TtyMode::Off;
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

        let save_label = if mounted { "Finish & save…" } else { "Save tape…" };
        let can_save = mounted || finished_unsaved;
        if ui.add_enabled(can_save, egui::Button::new(save_label)).clicked() {
            if mounted {
                self.asr33.punch_running = false;
                self.tty.finish_tape_punch();
                self.audio.play_once("assets/click.mp3");
            }
            let _ = self.save_punched_tape();
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
            // No arbitrary breakpoint: each logical row can wrap at any width,
            // so resizing never hides its tail while waiting for a threshold.
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

    fn request_tape_transport_repaint(&self, ctx: &egui::Context) {
        let reader = self.asr33.reader_running.then(|| self.asr33.reader_speed.char_time());
        let punch = (self.asr33.punch_running && self.tty.tape_punch_pending_len() > 0)
            .then(|| self.asr33.punch_speed.char_time());
        let wait = match (reader, punch) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => return,
        };
        if wait.is_zero() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(wait);
        }
    }

    fn draw_tty_window(&mut self, ctx: &egui::Context) {
        self.update_key_animation(ctx);
        self.draw_tty_menu(ctx);
        self.request_tape_transport_repaint(ctx);

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
            let (reader, _) = self.reader_state_label();
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
