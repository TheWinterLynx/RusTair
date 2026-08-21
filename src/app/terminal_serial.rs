use super::*;

impl RusTairApp {
    /// Drive the Altair TX holding register only when the text terminal owns the
    /// serial line. This is deliberately separate from `process_tty_serial` so
    /// changing the text-terminal baud rate cannot alter the ASR-33 timing.
    pub(in crate::app) fn process_terminal_serial(&mut self, ctx: &egui::Context) {
        if !self.terminal.window_open {
            self.terminal.tx_started = None;
            return;
        }

        // A reset/power cycle clears the hardware TX register. Drop any stale
        // text-terminal timer at the same time so it cannot delay the next byte.
        if !self.machine.bus.tx_busy() {
            self.terminal.tx_started = None;
            return;
        }

        let now = Instant::now();
        let char_time = self.terminal.speed.char_time();

        if let Some(started) = self.terminal.tx_started {
            let elapsed = now.duration_since(started);
            if char_time.is_zero() || elapsed >= char_time {
                self.machine.bus.serial_tx_complete();
                self.terminal.tx_started = None;
            } else {
                ctx.request_repaint_after(char_time - elapsed);
                return;
            }
        }

        if self.terminal.tx_started.is_none() {
            if let Some(byte) = self.machine.bus.serial_tx_front() {
                self.terminal.receive_byte(byte);
                self.terminal.tx_started = Some(now);

                if char_time.is_zero() {
                    self.machine.bus.serial_tx_complete();
                    self.terminal.tx_started = None;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(char_time);
                }
            }
        }
    }
}
