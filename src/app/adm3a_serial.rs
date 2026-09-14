use super::*;

const MAX_ADM3A_BYTES_PER_FRAME: usize = 4096;

type SerialBitRate = (u32, u32);

fn bit_rate_matches(rate: SerialBitRate, baud: u32) -> bool {
    let (numerator, denominator) = rate;
    u64::from(numerator) == u64::from(baud).saturating_mul(u64::from(denominator))
}

impl RusTairApp {
    fn adm3a_card_bit_rate(&self, connection: SerialConnection) -> Option<SerialBitRate> {
        let hardware = self.config.machine.s100_hardware;
        let (_, card) = hardware.active_serial_card_slot()?;
        match (card, connection) {
            (S100InstalledCardConfig::Mits88Sio(config), SerialConnection::Port0) => {
                Some((config.baud.baud(), 1))
            }
            (S100InstalledCardConfig::Mits88TwoSio { straps, .. }, SerialConnection::Port0) => {
                Some((straps.port0_baud.baud(), 1))
            }
            (S100InstalledCardConfig::Mits88TwoSio { straps, .. }, SerialConnection::Port1) => {
                Some((straps.port1_baud.baud(), 1))
            }
            _ => None,
        }
    }

    fn adm3a_baud_matches_card(&self, connection: SerialConnection) -> bool {
        let terminal_baud = self.adm3a.baud_rate().baud();
        self.adm3a_card_bit_rate(connection)
            .is_some_and(|rate| bit_rate_matches(rate, terminal_baud))
    }

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
        let baud_matches = self.adm3a_baud_matches_card(connection);

        if powered && self.machine.powered() && self.adm3a.keyboard_pending_len() != 0 {
            let due_in = self.adm3a.keyboard_due_in(now);
            if !due_in.is_zero() {
                ctx.request_repaint_after(due_in);
            } else if !baud_matches {
                // The ADM-3A still transmits the character at its selected clock,
                // but a receiver running at another baud does not magically get
                // the original byte. The current byte-level cable boundary cannot
                // represent analogue sampling corruption, so discard the
                // undecodable frame rather than delivering a perfect character.
                let _ = self.adm3a.take_due_keyboard_byte(now);
                if self.adm3a.keyboard_pending_len() != 0 {
                    ctx.request_repaint_after(self.adm3a.keyboard_due_in(now));
                }
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
            if powered && baud_matches {
                self.adm3a.receive_byte(byte);
                changed = true;
            }
        }

        if powered {
            // Keep BEL terminal-local and silent. RusTair's audible bell belongs
            // to the electromechanical ASR-33 path, not to the ADM-3A endpoint.
            let _ = self.adm3a.take_bell();
        }
        if changed {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adm3a_baud_must_match_the_connected_card_rate() {
        assert!(bit_rate_matches((9_600, 1), 9_600));
        assert!(bit_rate_matches((2_400, 1), 2_400));
        assert!(!bit_rate_matches((9_600, 1), 110));
        assert!(!bit_rate_matches((110, 1), 9_600));
    }
}
