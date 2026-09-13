use super::*;

const MAX_ADM3A_BYTES_PER_FRAME: usize = 4096;

impl RusTairApp {
    /// Deliver completed guest UART frames to the ADM-3A connected at the
    /// external serial boundary. The terminal never reads guest memory or card
    /// registers directly: only bytes that the installed UART has actually
    /// finished transmitting can reach its screen logic.
    pub(in crate::app) fn process_adm3a_serial(&mut self, ctx: &egui::Context) {
        let connection = self.adm3a_connection();
        if !connection.is_connected() {
            return;
        }

        let mut changed = false;
        for _ in 0..MAX_ADM3A_BYTES_PER_FRAME {
            let Some(byte) = self.serial_tx_complete_at(connection) else {
                break;
            };
            self.adm3a.receive_byte(byte);
            changed = true;
        }

        if self.adm3a.take_bell() {
            self.audio.play_once("assets/bellpadded.mp3");
        }
        if changed {
            ctx.request_repaint();
        }
    }
}
