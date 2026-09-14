use super::*;

const MAX_ADM3A_BYTES_PER_FRAME: usize = 4096;

type SerialBitRate = (u32, u32);

fn two_sio_effective_bit_rate(tap_baud: u32, control: u8) -> Option<SerialBitRate> {
    let divider = match control & 0x03 {
        0 => 1,
        1 => 16,
        2 => 64,
        _ => return None,
    };
    Some((tap_baud.saturating_mul(16), divider))
}

fn bit_rate_matches(rate: SerialBitRate, baud: u32) -> bool {
    let (numerator, denominator) = rate;
    u64::from(numerator) == u64::from(baud).saturating_mul(u64::from(denominator))
}

impl RusTairApp {
    fn adm3a_card_bit_rate(&mut self, connection: SerialConnection) -> Option<SerialBitRate> {
        let hardware = self.config.machine.s100_hardware;
        let (_, card) = hardware.active_serial_card_slot()?;
        match (card, connection) {
            (S100InstalledCardConfig::Mits88Sio(config), SerialConnection::Port0) => {
                Some((config.baud.baud(), 1))
            }
            (S100InstalledCardConfig::Mits88TwoSio { straps, .. }, connection) => {
                let (tap, status_port) = match connection {
                    SerialConnection::Port0 => (straps.port0_baud, straps.address.port0_status()),
                    SerialConnection::Port1 => (straps.port1_baud, straps.address.port1_status()),
                    SerialConnection::Disconnected => return None,
                };
                let control = self.machine.io_port_activity(status_port).1.unwrap_or(0);
                two_sio_effective_bit_rate(tap.baud(), control)
            }
            _ => None,
        }
    }

    fn adm3a_baud_matches_card(&mut self, connection: SerialConnection) -> bool {
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
                // represent the analogue sampling corruption, so discard the
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
    fn two_sio_effective_rate_tracks_mc6850_clock_divider() {
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x00), Some((153_600, 1)));
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x01), Some((153_600, 16)));
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x02), Some((153_600, 64)));
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x03), None);
    }

    #[test]
    fn adm3a_baud_must_match_the_effective_card_clock() {
        assert!(bit_rate_matches((153_600, 16), 9_600));
        assert!(bit_rate_matches((38_400, 16), 2_400));
        assert!(!bit_rate_matches((153_600, 16), 110));
        assert!(!bit_rate_matches((1_760, 64), 27));
    }
}
