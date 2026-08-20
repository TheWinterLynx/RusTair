use eframe::egui;
use std::time::Instant;

use crate::RusTairApp;

impl RusTairApp {
    /// Drive the Altair TX holding register only when the text terminal owns the
    /// serial line. This is deliberately separate from `process_tty_serial` so
    /// changing the text-terminal baud rate cannot alter the ASR-33 timing.
    pub(crate) fn process_terminal_serial(&mut self, ctx: &egui::Context) {
        if !self.terminal_window_open {
            self.terminal_tx_started = None;
            return;
        }

        // A reset/power cycle clears the hardware TX register. Drop any stale
        // text-terminal timer at the same time so it cannot delay the next byte.
        if self.machine.bus.serial_tx.is_empty() {
            self.terminal_tx_started = None;
            return;
        }

        let now = Instant::now();
        let char_time = self.terminal_speed.char_time();

        if let Some(started) = self.terminal_tx_started {
            let elapsed = now.duration_since(started);
            if char_time.is_zero() || elapsed >= char_time {
                self.machine.bus.serial_tx.pop_front();
                self.terminal_tx_started = None;
            } else {
                ctx.request_repaint_after(char_time - elapsed);
                return;
            }
        }

        if self.terminal_tx_started.is_none() {
            if let Some(&byte) = self.machine.bus.serial_tx.front() {
                self.terminal_receive_byte(byte);
                self.terminal_tx_started = Some(now);

                if char_time.is_zero() {
                    self.machine.bus.serial_tx.pop_front();
                    self.terminal_tx_started = None;
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(char_time);
                }
            }
        }
    }
}
