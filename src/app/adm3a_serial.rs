use super::*;
use crate::app::adm3a_state::{Adm3aParity, Adm3aWordFormat};
use crate::config::SioParity;

const MAX_ADM3A_BYTES_PER_FRAME: usize = 4096;

type SerialBitRate = (u32, u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SerialParity {
    None,
    Even,
    Odd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SerialFrameFormat {
    data_bits: u8,
    parity: SerialParity,
    stop_bits: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SampledSerialFrame {
    byte: u8,
    framing_error: bool,
    parity_error: bool,
}

fn two_sio_effective_bit_rate(tap_baud: u32, control: u8) -> Option<SerialBitRate> {
    let divider = match control & 0x03 {
        0 => 1,
        1 => 16,
        2 => 64,
        _ => return None,
    };
    Some((tap_baud.saturating_mul(16), divider))
}

fn two_sio_word_format(control: u8) -> SerialFrameFormat {
    match (control >> 2) & 0x07 {
        0 => SerialFrameFormat {
            data_bits: 7,
            parity: SerialParity::Even,
            stop_bits: 2,
        },
        1 => SerialFrameFormat {
            data_bits: 7,
            parity: SerialParity::Odd,
            stop_bits: 2,
        },
        2 => SerialFrameFormat {
            data_bits: 7,
            parity: SerialParity::Even,
            stop_bits: 1,
        },
        3 => SerialFrameFormat {
            data_bits: 7,
            parity: SerialParity::Odd,
            stop_bits: 1,
        },
        4 => SerialFrameFormat {
            data_bits: 8,
            parity: SerialParity::None,
            stop_bits: 2,
        },
        5 => SerialFrameFormat {
            data_bits: 8,
            parity: SerialParity::None,
            stop_bits: 1,
        },
        6 => SerialFrameFormat {
            data_bits: 8,
            parity: SerialParity::Even,
            stop_bits: 1,
        },
        _ => SerialFrameFormat {
            data_bits: 8,
            parity: SerialParity::Odd,
            stop_bits: 1,
        },
    }
}

fn adm3a_word_format(format: Adm3aWordFormat) -> SerialFrameFormat {
    SerialFrameFormat {
        data_bits: format.data_bits.bits(),
        parity: match format.parity {
            Adm3aParity::None => SerialParity::None,
            Adm3aParity::Even => SerialParity::Even,
            Adm3aParity::Odd => SerialParity::Odd,
        },
        stop_bits: format.stop_bits.bits(),
    }
}

fn sio_word_format(format: crate::config::SioWordFormat) -> SerialFrameFormat {
    SerialFrameFormat {
        data_bits: format.data_bits.bits(),
        parity: match format.parity {
            SioParity::None => SerialParity::None,
            SioParity::Even => SerialParity::Even,
            SioParity::Odd => SerialParity::Odd,
        },
        stop_bits: format.stop_bits.bits(),
    }
}

fn parity_bit(byte: u8, data_bits: u8, parity: SerialParity) -> bool {
    let mask = if data_bits == 8 {
        0xff
    } else {
        ((1u16 << data_bits) - 1) as u8
    };
    let odd_ones = (byte & mask).count_ones() & 1 != 0;
    match parity {
        SerialParity::None => true,
        SerialParity::Even => odd_ones,
        SerialParity::Odd => !odd_ones,
    }
}

fn serial_frame_line_high(byte: u8, format: SerialFrameFormat, bit_index: u128) -> bool {
    if bit_index == 0 {
        return false;
    }
    if bit_index <= u128::from(format.data_bits) {
        return byte & (1u8 << (bit_index as u8 - 1)) != 0;
    }

    let parity_present = format.parity != SerialParity::None;
    let parity_index = 1 + u128::from(format.data_bits);
    if parity_present && bit_index == parity_index {
        return parity_bit(byte, format.data_bits, format.parity);
    }

    // Stop field and the idle line are both MARK/HIGH for a valid sender.
    true
}

/// Sample one asynchronous frame at the receiver's own bit centers. The sender
/// and receiver clocks are rational rates so fractional cases such as 27.5 baud
/// remain exact. A start pulse shorter than half a receiver bit is treated as not
/// qualified; otherwise data/parity/stop are sampled from the actual sender line
/// position rather than turning a baud mismatch into a magical perfect byte.
fn sample_async_frame(
    byte: u8,
    sender_format: SerialFrameFormat,
    sender_rate: SerialBitRate,
    receiver_format: SerialFrameFormat,
    receiver_rate: SerialBitRate,
) -> Option<SampledSerialFrame> {
    let (sender_numerator, sender_denominator) = sender_rate;
    let (receiver_numerator, receiver_denominator) = receiver_rate;
    if sender_numerator == 0
        || sender_denominator == 0
        || receiver_numerator == 0
        || receiver_denominator == 0
    {
        return None;
    }

    let start_qualified = 2u128
        .saturating_mul(u128::from(sender_denominator))
        .saturating_mul(u128::from(receiver_numerator))
        >= u128::from(receiver_denominator).saturating_mul(u128::from(sender_numerator));
    if !start_qualified {
        return None;
    }

    let sample_line = |slot: u8| {
        // Receiver slot 0 is its first data bit. Asynchronous data is sampled
        // 1.5 receiver bit-times after the detected start edge, then every bit.
        let half_bit_count = u128::from(3 + 2 * slot);
        let numerator = half_bit_count
            .saturating_mul(u128::from(receiver_denominator))
            .saturating_mul(u128::from(sender_numerator));
        let denominator = 2u128
            .saturating_mul(u128::from(receiver_numerator))
            .saturating_mul(u128::from(sender_denominator));
        let sender_bit_index = numerator / denominator;
        serial_frame_line_high(byte, sender_format, sender_bit_index)
    };

    let mut decoded = 0u8;
    for data_bit in 0..receiver_format.data_bits {
        if sample_line(data_bit) {
            decoded |= 1u8 << data_bit;
        }
    }

    let mut next_slot = receiver_format.data_bits;
    let parity_error = if receiver_format.parity == SerialParity::None {
        false
    } else {
        let received_parity = sample_line(next_slot);
        next_slot += 1;
        received_parity != parity_bit(decoded, receiver_format.data_bits, receiver_format.parity)
    };
    let framing_error = !sample_line(next_slot);

    Some(SampledSerialFrame {
        byte: decoded,
        framing_error,
        parity_error,
    })
}

impl RusTairApp {
    fn adm3a_card_link(
        &mut self,
        connection: SerialConnection,
    ) -> Option<(SerialBitRate, SerialFrameFormat)> {
        let hardware = self.config.machine.s100_hardware;
        let (_, card) = hardware.active_serial_card_slot()?;
        match (card, connection) {
            (S100InstalledCardConfig::Mits88Sio(config), SerialConnection::Port0) => {
                Some(((config.baud.baud(), 1), sio_word_format(config.format)))
            }
            (S100InstalledCardConfig::Mits88TwoSio { straps, .. }, connection) => {
                let (tap, status_port) = match connection {
                    SerialConnection::Port0 => (straps.port0_baud, straps.address.port0_status()),
                    SerialConnection::Port1 => (straps.port1_baud, straps.address.port1_status()),
                    SerialConnection::Disconnected => return None,
                };
                let control = self.machine.io_port_activity(status_port).1?;
                Some((
                    two_sio_effective_bit_rate(tap.baud(), control)?,
                    two_sio_word_format(control),
                ))
            }
            _ => None,
        }
    }

    /// Service both directions of the physical serial cable attached to the
    /// ADM-3A. Guest TX reaches the terminal only after the emulated UART has
    /// completed a frame; keyboard bytes begin at the ADM-3A transmitter's own
    /// schedule and are sampled by the card receiver at its independently chosen
    /// rate/word format. The terminal is therefore never flow-controlled by a
    /// hidden host queue merely because the receiving UART is busy.
    pub(in crate::app) fn process_adm3a_serial(&mut self, ctx: &egui::Context) {
        let connection = self.adm3a_connection();
        if !connection.is_connected() {
            return;
        }

        let powered = self.adm3a.powered();
        let now = Instant::now();
        let card_link = self.adm3a_card_link(connection);
        let terminal_rate = (self.adm3a.baud_rate().baud(), 1);
        let terminal_format = adm3a_word_format(self.adm3a.word_format());

        if powered && self.machine.powered() && self.adm3a.keyboard_pending_len() != 0 {
            let due_in = self.adm3a.keyboard_due_in(now);
            if !due_in.is_zero() {
                ctx.request_repaint_after(due_in);
            } else if let Some(byte) = self.adm3a.take_due_keyboard_byte(now) {
                if let Some((card_rate, card_format)) = card_link
                    && self.serial_rx_line_idle_at(connection)
                    && let Some(sampled) = sample_async_frame(
                        byte,
                        terminal_format,
                        terminal_rate,
                        card_format,
                        card_rate,
                    )
                {
                    // The public cable endpoint is still byte-oriented, so the
                    // sampled data byte enters the real timed UART receive path.
                    // Propagating sampled FE/PE into the card status bits is the
                    // next lower-boundary step; data corruption itself is already
                    // determined here from the physical clocks and framing.
                    self.serial_receive_at(connection, sampled.byte);
                }
                if self.adm3a.keyboard_pending_len() != 0 {
                    ctx.request_repaint_after(self.adm3a.keyboard_due_in(now));
                }
            }
        }

        let mut changed = false;
        for _ in 0..MAX_ADM3A_BYTES_PER_FRAME {
            let Some(byte) = self.serial_tx_complete_at(connection) else {
                break;
            };
            let Some((card_rate, card_format)) = card_link else {
                continue;
            };
            let Some(sampled) = sample_async_frame(
                byte,
                card_format,
                card_rate,
                terminal_format,
                terminal_rate,
            ) else {
                continue;
            };
            if powered && !sampled.framing_error && !sampled.parity_error {
                self.adm3a.receive_byte(sampled.byte);
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

    const FORMAT_8N1: SerialFrameFormat = SerialFrameFormat {
        data_bits: 8,
        parity: SerialParity::None,
        stop_bits: 1,
    };

    #[test]
    fn two_sio_effective_rate_tracks_mc6850_clock_divider() {
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x00), Some((153_600, 1)));
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x01), Some((153_600, 16)));
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x02), Some((153_600, 64)));
        assert_eq!(two_sio_effective_bit_rate(9_600, 0x03), None);
    }

    #[test]
    fn two_sio_control_decodes_the_same_word_formats_as_the_mc6850() {
        assert_eq!(
            two_sio_word_format(0x00),
            SerialFrameFormat {
                data_bits: 7,
                parity: SerialParity::Even,
                stop_bits: 2,
            }
        );
        assert_eq!(two_sio_word_format(0x14), FORMAT_8N1);
        assert_eq!(
            two_sio_word_format(0x1c),
            SerialFrameFormat {
                data_bits: 8,
                parity: SerialParity::Odd,
                stop_bits: 1,
            }
        );
    }

    #[test]
    fn matched_async_clocks_recover_the_exact_frame() {
        assert_eq!(
            sample_async_frame(b'A', FORMAT_8N1, (9_600, 1), FORMAT_8N1, (9_600, 1)),
            Some(SampledSerialFrame {
                byte: b'A',
                framing_error: false,
                parity_error: false,
            })
        );
    }

    #[test]
    fn grossly_fast_sender_is_not_magically_decoded_by_slow_receiver() {
        assert_eq!(
            sample_async_frame(b'A', FORMAT_8N1, (9_600, 1), FORMAT_8N1, (110, 1)),
            None
        );
    }

    #[test]
    fn slow_sender_sampled_by_fast_receiver_produces_wire_level_corruption() {
        let sampled = sample_async_frame(b'A', FORMAT_8N1, (110, 1), FORMAT_8N1, (9_600, 1))
            .expect("long start bit is detected by the faster receiver");
        assert_ne!(sampled.byte, b'A');
        assert!(sampled.framing_error);
    }

    #[test]
    fn parity_is_checked_from_the_sampled_wire_bit() {
        let even = SerialFrameFormat {
            data_bits: 7,
            parity: SerialParity::Even,
            stop_bits: 1,
        };
        let odd = SerialFrameFormat {
            parity: SerialParity::Odd,
            ..even
        };
        let sampled = sample_async_frame(b'A', even, (9_600, 1), odd, (9_600, 1)).unwrap();
        assert_eq!(sampled.byte, b'A');
        assert!(sampled.parity_error);
        assert!(!sampled.framing_error);
    }
}