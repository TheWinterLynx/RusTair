use super::*;

impl RusTairApp {
    /// Drive whichever physical serial port the Text Terminal cable is attached
    /// to. Window visibility does not affect the cable: a hidden terminal keeps
    /// receiving guest output until the user explicitly disconnects it.
    pub(in crate::app) fn process_terminal_serial(&mut self, ctx: &egui::Context) {
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
