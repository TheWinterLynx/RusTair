use super::*;

const MAX_ADM3A_BYTES_PER_FRAME: usize = 4096;

impl RusTairApp {
    /// Service both directions of the physical serial cable attached to the
    /// ADM-3A. Guest TX reaches the terminal only after the emulated UART has
    /// completed a frame; keyboard bytes enter the guest only through the
    /// physical receive shift path and are paced by the terminal transmitter.
    pub(in crate::app) fn process_adm3a_serial(&mut self, ctx: &egui::Context) {
        let connection = self.adm3a_connection();
        if !connection.is_connected() {
            return;
        }

        let powered = self.adm3a.powered();
        let now = Instant::now();

        if powered && self.machine.powered() && self.adm3a.keyboard_pending_len() != 0 {
            let due_in = self.adm3a.keyboard_due_in(now);
            if !due_in.is_zero() {
                ctx.request_repaint_after(due_in);
            } else if self.serial_rx_line_idle_at(connection) {
                if let Some(byte) = self.adm3a.take_due_keyboard_byte(now) {
                    self.serial_receive_at(connection, byte);
                    if self.adm3a.keyboard_pending_len() != 0 {
                        ctx.request_repaint_after(self.adm3a.keyboard_due_in(now));
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(1));
            }
        }

        let mut changed = false;
        for _ in 0..MAX_ADM3A_BYTES_PER_FRAME {
            let Some(byte) = self.serial_tx_complete_at(connection) else {
                break;
            };
            if powered {
                self.adm3a.receive_byte(byte);
                changed = true;
            }
        }

        if powered && self.adm3a.take_bell() {
            self.audio.play_once("assets/bellpadded.mp3");
        }
        if changed {
            ctx.request_repaint();
        }
    }
}
