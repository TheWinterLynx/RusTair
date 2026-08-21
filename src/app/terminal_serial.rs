use super::*;

impl RusTairApp {
    /// Drive the Text Terminal's physical serial connection. With an 88-SIO it
    /// uses the board's single port when the terminal owns that cable. With a
    /// fully populated 88-2SIO it is permanently attached to Port 1 and can run
    /// simultaneously with the ASR-33 on Port 0.
    pub(in crate::app) fn process_terminal_serial(&mut self, ctx: &egui::Context) {
        // A hidden terminal window is still a connected physical terminal. Keep
        // receiving guest output into TerminalState even when the UI is closed.
        if !self.terminal_serial_tx_busy() {
            self.terminal.tx_started = None;
            return;
        }

        let now = Instant::now();
        let char_time = self.terminal.speed.char_time();

        if let Some(started) = self.terminal.tx_started {
            let elapsed = now.duration_since(started);
            if char_time.is_zero() || elapsed >= char_time {
                self.terminal_serial_tx_complete();
                self.terminal.tx_started = None;
            } else {
                ctx.request_repaint_after(char_time - elapsed);
                return;
            }
        }

        if self.terminal.tx_started.is_none() {
            if let Some(byte) = self.terminal_serial_tx_front() {
                self.terminal.receive_byte(byte);
                self.terminal.tx_started = Some(now);

                if char_time.is_zero() {
                    self.terminal_serial_tx_complete();
                    self.terminal.tx_started = None;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(char_time);
                }
            }
        }
    }
}
