#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalSpeed {
    Instant,
    Baud300,
    Baud1200,
    Baud2400,
    Baud9600,
}

impl TerminalSpeed {
    const ALL: [Self; 5] = [
        Self::Instant,
        Self::Baud300,
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud9600,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Instant => "Instant",
            Self::Baud300 => "300 baud",
            Self::Baud1200 => "1200 baud",
            Self::Baud2400 => "2400 baud",
            Self::Baud9600 => "9600 baud",
        }
    }

    // One asynchronous serial character is modelled as 10 bits: start,
    // 8 data bits and stop. The ASR-33 keeps its separate mechanical timing;
    // these rates belong only to the electronic text terminal.
    fn char_time(self) -> Duration {
        match self {
            Self::Instant => Duration::ZERO,
            Self::Baud300 => Duration::from_micros(33_333),
            Self::Baud1200 => Duration::from_micros(8_333),
            Self::Baud2400 => Duration::from_micros(4_167),
            Self::Baud9600 => Duration::from_micros(1_042),
        }
    }
}

const TERMINAL_MAX_CHARS: usize = 200_000;
const TERMINAL_INPUT_DEFAULT_HEIGHT: f32 = 235.0;
const TERMINAL_INPUT_MIN_HEIGHT: f32 = 115.0;
const TERMINAL_OUTPUT_MIN_HEIGHT: f32 = 100.0;
const TERMINAL_SPLITTER_THICKNESS: f32 = 6.0;

impl RusTairApp {
    fn terminal_receive_byte(&mut self, byte: u8) {
        match byte & 0x7f {
            b'\r' => {
                if !self.terminal_output.ends_with('\n') {
                    self.terminal_output.push('\n');
                }
                self.terminal_last_was_cr = true;
            }
            b'\n' => {
                if !self.terminal_last_was_cr && !self.terminal_output.ends_with('\n') {
                    self.terminal_output.push('\n');
                }
                self.terminal_last_was_cr = false;
            }
            0x08 | 0x7f => {
                if !self.terminal_output.ends_with('\n') {
                    self.terminal_output.pop();
                }
                self.terminal_last_was_cr = false;
            }
            b'\t' => {
                self.terminal_output.push('\t');
                self.terminal_last_was_cr = false;
            }
            0x20..=0x7e => {
                self.terminal_output.push((byte & 0x7f) as char);
                self.terminal_last_was_cr = false;
            }
            _ => {
                self.terminal_last_was_cr = false;
            }
        }

        if self.terminal_output.len() > TERMINAL_MAX_CHARS {
            let excess = self.terminal_output.len() - TERMINAL_MAX_CHARS;
            let cut = self.terminal_output[excess..]
                .find('\n')
                .map(|offset| excess + offset + 1)
                .unwrap_or(excess);
            self.terminal_output.drain(..cut);
        }
    }

    fn terminal_enqueue_text(&mut self, text: &str, append_final_cr: bool) -> usize {
        if !self.machine.powered {
            self.status = "Terminal input ignored: Altair power is off".into();
            return 0;
        }

        let mut bytes = Vec::with_capacity(text.len() + 1);
        let mut previous_was_cr = false;

        for byte in text.bytes() {
            match byte {
                b'\r' => {
                    bytes.push(b'\r');
                    previous_was_cr = true;
                }
                b'\n' => {
                    if !previous_was_cr {
                        bytes.push(b'\r');
                    }
                    previous_was_cr = false;
                }
                0x08 | 0x09 | 0x1b | 0x20..=0x7e => {
                    bytes.push(if self.terminal_uppercase {
                        byte.to_ascii_uppercase()
                    } else {
                        byte
                    });
                    previous_was_cr = false;
                }
                _ => {
                    previous_was_cr = false;
                }
            }
        }

        if append_final_cr && bytes.last().copied() != Some(b'\r') {
            bytes.push(b'\r');
        }

        let count = bytes.len();
        if count > 0 {
            let was_empty = self.terminal_input_queue.is_empty();
            self.terminal_input_queue.extend(bytes);
            if was_empty {
                // The first character may start immediately. Subsequent bytes
                // are released at the selected serial character interval.
                self.terminal_rx_next_at = Some(Instant::now());
            }
        }
        count
    }

    fn process_terminal_input(&mut self, ctx: &egui::Context) {
        if self.terminal_input_queue.is_empty() {
            self.terminal_rx_next_at = None;
            return;
        }
        if !self.machine.powered {
            return;
        }

        // Model a one-character receive register. Pasted programs are kept in
        // our host-side queue and are only presented to the Altair after the
        // previous character has been consumed by software. This prevents a
        // large paste from being dumped into serial_rx all at once.
        if !self.machine.bus.serial_rx.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(1));
            return;
        }

        let now = Instant::now();
        let due = self.terminal_rx_next_at.unwrap_or(now);
        if now < due {
            ctx.request_repaint_after(due.duration_since(now));
            return;
        }

        if let Some(byte) = self.terminal_input_queue.pop_front() {
            self.machine.bus.serial_rx.push_back(byte & 0x7f);
        }

        if self.terminal_input_queue.is_empty() {
            self.terminal_rx_next_at = None;
        } else {
            let char_time = self.terminal_speed.char_time();
            self.terminal_rx_next_at = Some(now + char_time);
            ctx.request_repaint_after(char_time);
        }
    }

    fn terminal_send_command(&mut self) {
        let command = std::mem::take(&mut self.terminal_command);
        let blank = command.is_empty();
        let count = self.terminal_enqueue_text(&command, true);
        if count > 0 {
            if blank {
                self.status = "Terminal queued CR".into();
            } else {
                self.status = format!("Terminal command queued: {count} bytes");
            }
        }
    }

    fn terminal_send_program(&mut self) {
        if self.terminal_program.is_empty() {
            return;
        }
        let program = self.terminal_program.clone();
        let count = self.terminal_enqueue_text(&program, true);
        if count > 0 {
            self.status = format!(
                "Terminal program queued: {count} bytes at {}",
                self.terminal_speed.label()
            );
        }
    }

    fn terminal_send_control(&mut self, byte: u8, name: &str) {
        if !self.machine.powered {
            self.status = format!("{name} ignored: Altair power is off");
            return;
        }

        // Manual control keys should not sit behind a pasted BASIC listing.
        // They still wait for the Altair's one-byte receive register to become
        // free, but otherwise take priority over queued paste data.
        self.terminal_input_queue.push_front(byte & 0x7f);
        self.terminal_rx_next_at = Some(Instant::now());
        self.status = format!("Terminal queued {name}");
    }

    fn load_terminal_text_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text / BASIC", &["txt", "bas", "basic"])
            .pick_file()
        else { return; };

        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let count = self.terminal_enqueue_text(&text, true);
                if count > 0 {
                    self.status = format!(
                        "Terminal queued {count} bytes from {} at {}",
                        path.display(),
                        self.terminal_speed.label()
                    );
                }
            }
            Err(e) => self.status = format!("Terminal file load failed: {e}"),
        }
    }

    fn draw_terminal_input(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(">").monospace().strong());
            let width = (ui.available_width() - 122.0).max(80.0);
            let response = ui.add_sized(
                [width, 26.0],
                egui::TextEdit::singleline(&mut self.terminal_command)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("command (blank + Enter sends CR)"),
            );
            let enter = response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Send").clicked() || enter {
                self.terminal_send_command();
                response.request_focus();
            }
            if ui.button("CR").on_hover_text("Send carriage return only").clicked() {
                self.terminal_send_control(b'\r', "CR");
                response.request_focus();
            }
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Paste / program input");
            if ui.button("Send block").clicked() {
                self.terminal_send_program();
            }
            if ui.button("Clear editor").clicked() {
                self.terminal_program.clear();
            }
        });
        ui.small(format!(
            "Paste one or many lines. Input is paced at {}; newlines become carriage returns.",
            self.terminal_speed.label()
        ));

        let editor_height = (ui.available_height() - 8.0).max(30.0);
        ui.add_sized(
            [ui.available_width(), editor_height],
            egui::TextEdit::multiline(&mut self.terminal_program)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        );
    }

    fn draw_terminal_output(&self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let output_height = ui.available_height();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .max_height(output_height)
                .show(ui, |ui| {
                    ui.set_min_height(output_height);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&self.terminal_output)
                                .monospace()
                                .size(15.0),
                        )
                        .selectable(true),
                    );
                });
        });
    }

    fn draw_terminal_window(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("terminal-menu").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                if ui.button("Clear").clicked() {
                    self.terminal_output.clear();
                    self.terminal_last_was_cr = false;
                }
                if ui.button("Send text/BASIC file…").clicked() {
                    self.load_terminal_text_file();
                }
                ui.separator();
                ui.label("Speed:");
                let previous_speed = self.terminal_speed;
                egui::ComboBox::from_id_salt("terminal-speed")
                    .selected_text(self.terminal_speed.label())
                    .show_ui(ui, |ui| {
                        for speed in TerminalSpeed::ALL {
                            ui.selectable_value(&mut self.terminal_speed, speed, speed.label());
                        }
                    });
                if self.terminal_speed != previous_speed && !self.terminal_input_queue.is_empty() {
                    self.terminal_rx_next_at = Some(Instant::now());
                }
                ui.separator();
                ui.checkbox(&mut self.terminal_uppercase, "Uppercase input");
                ui.separator();
                if ui.button("CTRL-C").clicked() {
                    self.terminal_send_control(0x03, "CTRL-C");
                }
                if ui.button("ESC").clicked() {
                    self.terminal_send_control(0x1b, "ESC");
                }
            });
        });

        egui::TopBottomPanel::bottom("terminal-status").show(ctx, |ui| {
            ui.small(format!(
                "TEXT TERMINAL  |  {}  |  input pending {}  |  RX register {}  |  TX {}  |  {} chars",
                self.terminal_speed.label(),
                self.terminal_input_queue.len(),
                self.machine.bus.serial_rx.len(),
                if self.machine.bus.tx_busy() { "BUSY" } else { "READY" },
                self.terminal_output.len(),
            ));
        });

        // Do not use a resizable TopBottomPanel for the input area here. egui
        // panels negotiate their size with their contents, so a multiline
        // TextEdit or an almost-empty output area can change the requested
        // minimum size and make the separator appear to "bounce" after a drag.
        //
        // Instead we own one explicit splitter coordinate. Its desired input
        // height is stored independently of either pane's contents, and the two
        // child UIs are clipped to rectangles derived solely from that value.
        egui::CentralPanel::default().show(ctx, |ui| {
            let available = ui.available_rect_before_wrap();
            let splitter_state_id = egui::Id::new("rustair-terminal-input-height");

            let max_input_height = (available.height()
                - TERMINAL_OUTPUT_MIN_HEIGHT
                - TERMINAL_SPLITTER_THICKNESS)
                .max(TERMINAL_INPUT_MIN_HEIGHT);

            // Keep the user's desired height even if the window is temporarily
            // too small. We only clamp the height used for this frame. When the
            // window grows again the splitter returns to the user's position.
            let desired_input_height = ctx
                .data(|data| data.get_temp::<f32>(splitter_state_id))
                .unwrap_or(TERMINAL_INPUT_DEFAULT_HEIGHT);
            let mut input_height = desired_input_height
                .clamp(TERMINAL_INPUT_MIN_HEIGHT, max_input_height);
            let mut splitter_y = available.max.y - input_height;

            let splitter_hit_rect = Rect::from_min_max(
                Pos2::new(
                    available.min.x,
                    splitter_y - TERMINAL_SPLITTER_THICKNESS * 0.5,
                ),
                Pos2::new(
                    available.max.x,
                    splitter_y + TERMINAL_SPLITTER_THICKNESS * 0.5,
                ),
            );
            let splitter_id = ui.make_persistent_id("terminal-input-splitter");
            let response = ui.interact(splitter_hit_rect, splitter_id, Sense::drag());

            if response.hovered() || response.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }

            if response.dragged() {
                if let Some(pointer) = response.interact_pointer_pos() {
                    input_height = (available.max.y - pointer.y)
                        .clamp(TERMINAL_INPUT_MIN_HEIGHT, max_input_height);
                    splitter_y = available.max.y - input_height;
                    ctx.data_mut(|data| {
                        data.insert_temp(splitter_state_id, input_height);
                    });
                    ctx.request_repaint();
                }
            }

            let half_splitter = TERMINAL_SPLITTER_THICKNESS * 0.5;
            let output_rect = Rect::from_min_max(
                available.min,
                Pos2::new(available.max.x, splitter_y - half_splitter),
            );
            let input_rect = Rect::from_min_max(
                Pos2::new(available.min.x, splitter_y + half_splitter),
                available.max,
            );

            let separator_stroke = if response.hovered() || response.dragged() {
                ui.visuals().widgets.hovered.fg_stroke
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke
            };
            ui.painter().line_segment(
                [
                    Pos2::new(available.min.x, splitter_y),
                    Pos2::new(available.max.x, splitter_y),
                ],
                separator_stroke,
            );

            let mut output_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("terminal-output-area")
                    .max_rect(output_rect),
            );
            output_ui.set_clip_rect(output_rect);
            self.draw_terminal_output(&mut output_ui);

            let mut input_ui = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("terminal-input-area")
                    .max_rect(input_rect),
            );
            input_ui.set_clip_rect(input_rect);
            self.draw_terminal_input(&mut input_ui);

            // new_child deliberately does not allocate parent layout space.
            // Mark the full central region as consumed after both fixed panes.
            ui.advance_cursor_after_rect(available);
        });
    }

    fn show_terminal_viewport(&mut self, parent_ctx: &egui::Context) {
        if !self.terminal_window_open {
            return;
        }

        parent_ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("rustair-text-terminal"),
            egui::ViewportBuilder::default()
                .with_title("RusTair — Text Terminal")
                .with_inner_size([820.0, 640.0])
                .with_min_inner_size([520.0, 360.0])
                .with_resizable(true),
            |terminal_ctx, _class| {
                self.draw_terminal_window(terminal_ctx);
                if terminal_ctx.input(|i| i.viewport().close_requested()) {
                    self.terminal_window_open = false;
                }
            },
        );
    }
}
