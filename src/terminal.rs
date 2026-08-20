const TERMINAL_MAX_CHARS: usize = 200_000;

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
        self.machine.bus.serial_rx.extend(bytes);
        count
    }

    fn terminal_send_command(&mut self) {
        if self.terminal_command.is_empty() {
            return;
        }
        let command = std::mem::take(&mut self.terminal_command);
        let count = self.terminal_enqueue_text(&command, true);
        if count > 0 {
            self.status = format!("Terminal command queued: {count} bytes");
        }
    }

    fn terminal_send_program(&mut self) {
        if self.terminal_program.is_empty() {
            return;
        }
        let program = self.terminal_program.clone();
        let count = self.terminal_enqueue_text(&program, true);
        if count > 0 {
            self.status = format!("Terminal program queued: {count} bytes");
        }
    }

    fn terminal_send_control(&mut self, byte: u8, name: &str) {
        if !self.machine.powered {
            self.status = format!("{name} ignored: Altair power is off");
            return;
        }
        self.machine.bus.serial_rx.push_back(byte & 0x7f);
        self.status = format!("Terminal sent {name}");
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
                    self.status = format!("Terminal queued {count} bytes from {}", path.display());
                }
            }
            Err(e) => self.status = format!("Terminal file load failed: {e}"),
        }
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

        egui::CentralPanel::default().show(ctx, |ui| {
            let output_height = (ui.available_height() - 185.0).max(120.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
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

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(">").monospace().strong());
                let width = (ui.available_width() - 72.0).max(80.0);
                let response = ui.add_sized(
                    [width, 26.0],
                    egui::TextEdit::singleline(&mut self.terminal_command)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("command"),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Send").clicked() || enter {
                    self.terminal_send_command();
                    response.request_focus();
                }
            });

            egui::CollapsingHeader::new("Paste / program input")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("Paste one or many lines. Newlines are sent as carriage returns.");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.terminal_program)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Send program").clicked() {
                            self.terminal_send_program();
                        }
                        if ui.button("Clear editor").clicked() {
                            self.terminal_program.clear();
                        }
                    });
                });
        });

        egui::TopBottomPanel::bottom("terminal-status").show(ctx, |ui| {
            ui.small(format!(
                "TEXT TERMINAL  |  RX queued {}  |  TX {}  |  {} chars",
                self.machine.bus.serial_rx.len(),
                if self.machine.bus.tx_busy() { "BUSY" } else { "READY" },
                self.terminal_output.len(),
            ));
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
