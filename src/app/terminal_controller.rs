use super::*;

impl RusTairApp {
    /// Queue host text for delivery to the Altair through the selected serial
    /// endpoint. Text normalization and queue state belong to TerminalState;
    /// this controller only applies machine/application policy.
    pub(in crate::app) fn terminal_enqueue_text(
        &mut self,
        text: &str,
        append_final_cr: bool,
    ) -> usize {
        if !self.machine.powered {
            self.status = "Terminal input ignored: Altair power is off".into();
            return 0;
        }

        self.terminal
            .enqueue_text(text, append_final_cr, Instant::now())
    }

    pub(in crate::app) fn process_terminal_input(&mut self, ctx: &egui::Context) {
        if self.terminal.input_is_empty() {
            self.terminal.clear_input_timing_if_empty();
            return;
        }
        if !self.machine.powered {
            return;
        }

        // Model a one-character receive register. Host-side queued input only
        // advances after guest software consumes the previous byte.
        if !self.machine.bus.serial_rx_empty() {
            ctx.request_repaint_after(Duration::from_millis(1));
            return;
        }

        let now = Instant::now();
        let due_in = self.terminal.input_due_in(now);
        if !due_in.is_zero() {
            ctx.request_repaint_after(due_in);
            return;
        }

        if let Some((byte, next_delay)) = self.terminal.take_due_input(now) {
            self.machine.bus.serial_receive(byte & 0x7f);
            if !self.terminal.input_is_empty() {
                if next_delay.is_zero() {
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(next_delay);
                }
            }
        }
    }

    pub(in crate::app) fn terminal_send_command(&mut self) {
        let command = std::mem::take(&mut self.terminal.command);
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

    pub(in crate::app) fn terminal_send_program(&mut self) {
        if self.terminal.program.is_empty() {
            return;
        }
        let program = self.terminal.program.clone();
        let count = self.terminal_enqueue_text(&program, true);
        if count > 0 {
            self.status = format!(
                "Terminal program queued: {count} bytes at {}",
                self.terminal.speed.label()
            );
        }
    }

    pub(in crate::app) fn terminal_send_control(&mut self, byte: u8, name: &str) {
        if !self.machine.powered {
            self.status = format!("{name} ignored: Altair power is off");
            return;
        }

        self.terminal.queue_control(byte, Instant::now());
        self.status = format!("Terminal queued {name}");
    }
}
