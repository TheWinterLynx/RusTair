use std::collections::VecDeque;

use crate::mc6850::Mc6850;

/// Physical baud-generator tap selected by the 88-2SIO board strap. The MITS
/// manual exposes these eight taps independently for each ACIA. The complete set
/// is retained here before the Configuration UI wires all physical strap choices.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TwoSioBaudTap {
    Baud110,
    Baud150,
    Baud300,
    Baud1200,
    Baud1800,
    Baud2400,
    Baud4800,
    Baud9600,
}

impl TwoSioBaudTap {
    pub(super) const fn baud(self) -> u32 {
        match self {
            Self::Baud110 => 110,
            Self::Baud150 => 150,
            Self::Baud300 => 300,
            Self::Baud1200 => 1_200,
            Self::Baud1800 => 1_800,
            Self::Baud2400 => 2_400,
            Self::Baud4800 => 4_800,
            Self::Baud9600 => 9_600,
        }
    }
}

/// One of the two independent MC6850 channels on the MITS 88-2SIO.
///
/// `wire_tx` is downstream of the ACIA: a byte reaches it only after the
/// transmit shift register has consumed a complete configured serial frame.
/// Host presentation may drain that queue slowly without changing TDRE or the
/// ACIA's transmitter state.
pub(super) struct TwoSioPort {
    acia: Mc6850,
    /// Mirror of the physical ACIA control-register pins needed by the board's
    /// clock-generator wrapper. The ACIA remains authority for all status/data
    /// semantics; this board layer only needs CR1:CR0 to select /1,/16,/64/reset.
    control: u8,
    baud_tap: TwoSioBaudTap,
    bit_phase_numerator: u64,
    tx_bits_remaining: u8,
    wire_tx: VecDeque<u8>,
    /// Complete character currently traversing the external receive line / ACIA
    /// receiver shift register. This is deliberately not RDR/RDRF: status bit 0
    /// remains low until the configured serial frame has completed.
    rx_shift: Option<(u8, bool, bool)>,
    rx_bits_remaining: u8,
}

impl TwoSioPort {
    pub(super) fn new(baud_tap: TwoSioBaudTap) -> Self {
        Self {
            acia: Mc6850::default(),
            control: 0,
            baud_tap,
            bit_phase_numerator: 0,
            tx_bits_remaining: 0,
            wire_tx: VecDeque::new(),
            rx_shift: None,
            rx_bits_remaining: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        let baud_tap = self.baud_tap;
        *self = Self::new(baud_tap);
    }

    pub(super) fn read_status(&mut self) -> u8 { self.acia.read_status() }
    pub(super) fn peek_status(&self) -> u8 { self.acia.peek_status() }
    pub(super) fn read_data(&mut self) -> u8 { self.acia.read_data() }
    pub(super) fn peek_data(&self) -> u8 { self.acia.peek_data() }
    pub(super) fn interrupt_request(&self) -> bool { self.acia.interrupt_request() }

    /// Host-facing pending receive depth. This intentionally counts a character
    /// still in the timed receiver shift path as pending even while MC6850 RDRF
    /// is zero. Guest status remains governed only by `peek_status/read_status`.
    pub(super) fn receive_len(&self) -> usize {
        self.acia.receive_len() + usize::from(self.rx_shift.is_some())
    }

    /// Raw physical receive-line occupancy, independent from RDR/RDRF.
    pub(super) fn receive_line_idle(&self) -> bool {
        self.rx_shift.is_none()
    }

    pub(super) fn write_control(&mut self, value: u8) {
        self.control = value;
        self.acia.write_control(value);
        if value & 0x03 == 0x03 {
            // Reset aborts the character currently inside the ACIA. Bytes that
            // already left the transmit shift register remain on the external
            // wire queue.
            self.bit_phase_numerator = 0;
            self.tx_bits_remaining = 0;
            self.rx_shift = None;
            self.rx_bits_remaining = 0;
        }
    }

    pub(super) fn write_data(&mut self, value: u8) {
        // Do not move TDR to TSR here. Motorola specifies that the transfer is
        // synchronized by the transmitter clock and occurs within one bit time
        // when the transmitter is idle. The next bit boundary below owns it.
        self.acia.write_data(value);
    }

    /// A normal endpoint starts one physical serial character. There is no hidden
    /// byte FIFO in front of the MC6850 receiver. An overlapping host presentation
    /// is rejected rather than accumulating a non-historical queue behind it.
    pub(super) fn queue_received_character(&mut self, value: u8) {
        if !self.receive_line_idle() {
            return;
        }
        self.rx_shift = Some((value, false, false));
        self.rx_bits_remaining = self.acia.frame_bits();
    }

    /// The I/O Inspector explicitly says “directly into UART RX”. This debugger
    /// operation intentionally bypasses cable/baud timing while preserving the
    /// real one-byte RDR and overrun semantics of the ACIA itself.
    pub(super) fn debugger_inject_received_character(&mut self, value: u8) {
        self.acia.receive_character(value, false, false);
    }

    pub(super) fn clear_receive_for_debugger(&mut self) {
        self.acia.clear_receive_for_debugger();
        self.rx_shift = None;
        self.rx_bits_remaining = 0;
    }

    pub(super) fn clear_transmit_for_debugger(&mut self) {
        self.acia.clear_transmit_for_debugger();
        self.tx_bits_remaining = 0;
        self.wire_tx.clear();
    }

    /// Force one debugger-visible UART TX completion. Prefer an already
    /// completed wire byte; otherwise finish the active hardware character (or
    /// first promote a waiting TDR) without leaving a duplicate for an endpoint.
    pub(super) fn debugger_complete_one_tx(&mut self) -> Option<u8> {
        if let Some(byte) = self.wire_tx.pop_front() {
            return Some(byte);
        }
        if self.acia.tx_shift_front().is_none() {
            let _ = self.acia.transfer_tdr_to_shift_if_idle();
        }
        let byte = self.acia.complete_tx_shift()?;
        self.tx_bits_remaining = if self.acia.tx_shift_front().is_some() {
            self.acia.frame_bits()
        } else {
            0
        };
        Some(byte)
    }

    pub(super) fn endpoint_tx_front(&self) -> Option<u8> {
        self.wire_tx.front().copied()
    }

    /// Endpoint acknowledgement removes only a byte that has already completed
    /// on the emulated wire. It never changes TDR/TSR or TDRE.
    pub(super) fn endpoint_tx_complete(&mut self) -> Option<u8> {
        self.wire_tx.pop_front()
    }

    pub(super) fn endpoint_tx_pending_or_hardware_busy(&self) -> bool {
        !self.wire_tx.is_empty() || self.acia.transmit_busy()
    }

    fn clock_divider(&self) -> Option<u8> {
        match self.control & 0x03 {
            0 => Some(1),
            1 => Some(16),
            2 => Some(64),
            _ => None,
        }
    }

    fn transmitter_bit_boundary(&mut self) {
        if self.acia.tx_shift_front().is_none() {
            if self.acia.transfer_tdr_to_shift_if_idle() {
                self.tx_bits_remaining = self.acia.frame_bits();
            }
            return;
        }

        if self.tx_bits_remaining > 1 {
            self.tx_bits_remaining -= 1;
            return;
        }

        if self.tx_bits_remaining == 1 {
            self.tx_bits_remaining = 0;
            if let Some(byte) = self.acia.complete_tx_shift() {
                self.wire_tx.push_back(byte);
            }
            if self.acia.tx_shift_front().is_some() {
                // `complete_tx_shift` promotes a waiting TDR at exactly the
                // previous character boundary, so back-to-back characters have
                // no fictitious idle gap.
                self.tx_bits_remaining = self.acia.frame_bits();
            }
        }
    }

    fn receiver_bit_boundary(&mut self) {
        let Some((value, framing_error, parity_error)) = self.rx_shift else { return; };

        if self.rx_bits_remaining > 1 {
            self.rx_bits_remaining -= 1;
            return;
        }

        if self.rx_bits_remaining == 1 {
            self.rx_bits_remaining = 0;
            self.rx_shift = None;
            self.acia.receive_character(value, framing_error, parity_error);
        }
    }

    /// Advance this card channel by elapsed Altair CPU-clock T-states. The baud
    /// generator is independent hardware, but expressing elapsed chassis time in
    /// CPU-clock quanta gives both emulator engines one deterministic time unit.
    ///
    /// MITS' tap produces 16x the labelled baud. The ACIA then divides that clock
    /// by CR1:CR0 (/1, /16 or /64). Accumulating `tap*16` against
    /// `cpu_clock*divider` preserves fractional rates such as 27.5 baud exactly.
    pub(super) fn advance_t_states(&mut self, t_states: u64, cpu_clock_hz: u32) {
        if t_states == 0 || cpu_clock_hz == 0 { return; }
        let Some(divider) = self.clock_divider() else {
            self.bit_phase_numerator = 0;
            return;
        };

        let numerator_per_t_state = u64::from(self.baud_tap.baud()) * 16;
        let threshold = u64::from(cpu_clock_hz) * u64::from(divider);
        let added = t_states.saturating_mul(numerator_per_t_state);
        let total = self.bit_phase_numerator.saturating_add(added);
        let boundaries = total / threshold;
        self.bit_phase_numerator = total % threshold;

        for _ in 0..boundaries {
            self.transmitter_bit_boundary();
            self.receiver_bit_boundary();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_MHZ: u32 = 2_000_000;

    #[test]
    fn tx_tdr_waits_for_next_bit_clock_then_tdre_returns_before_character_finishes() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x15); // 8N1, /16 => 9600 baud
        port.write_data(b'A');
        assert_eq!(port.peek_status() & 0x02, 0);
        assert_eq!(port.endpoint_tx_front(), None);

        port.advance_t_states(208, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x02, 0);
        port.advance_t_states(1, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x02, 0x02);
        assert_eq!(port.endpoint_tx_front(), None);

        port.advance_t_states(2_083, TWO_MHZ);
        assert_eq!(port.endpoint_tx_front(), Some(b'A'));
    }

    #[test]
    fn endpoint_drain_does_not_control_acia_tdre_or_shift_completion() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x35); // 8N1, TX-empty IRQ, /16
        port.write_data(b'A');
        port.advance_t_states(2_292, TWO_MHZ);
        assert_eq!(port.endpoint_tx_front(), Some(b'A'));
        assert!(port.interrupt_request(), "TDR is empty regardless of endpoint presentation delay");

        port.advance_t_states(20_000, TWO_MHZ);
        assert_eq!(port.endpoint_tx_front(), Some(b'A'));
        assert!(port.interrupt_request());
        assert_eq!(port.endpoint_tx_complete(), Some(b'A'));
    }

    #[test]
    fn receive_character_reaches_rdr_only_after_full_card_timed_frame() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x95); // RX IRQ, 8N1, /16 => 110 baud
        port.queue_received_character(b'R');
        assert!(!port.receive_line_idle());
        assert_eq!(port.receive_len(), 1);
        assert_eq!(port.peek_status() & 0x81, 0);

        port.advance_t_states(181_818, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x81, 0);
        assert!(!port.receive_line_idle());
        port.advance_t_states(1, TWO_MHZ);
        assert!(port.receive_line_idle());
        assert_eq!(port.peek_status() & 0x81, 0x81);
        assert_eq!(port.receive_len(), 1);
        assert_eq!(port.read_data(), b'R');
        assert_eq!(port.receive_len(), 0);
    }

    #[test]
    fn receive_path_does_not_hide_an_unbounded_pre_acia_queue() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x15);
        port.queue_received_character(b'A');
        port.queue_received_character(b'B');

        port.advance_t_states(181_819, TWO_MHZ);
        assert!(port.receive_line_idle());
        assert_eq!(port.read_data(), b'A');
        assert_eq!(port.peek_status() & 0x01, 0);
    }

    #[test]
    fn unread_rdr_does_not_block_next_raw_physical_frame_and_can_overrun() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x15);
        port.queue_received_character(b'A');
        port.advance_t_states(181_819, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x01, 0x01);
        assert!(port.receive_line_idle(), "RDRF must not masquerade as raw line busy");

        // Exercise the card primitive directly: a real source can begin its next
        // frame even while software has left the previous RDR unread.
        port.queue_received_character(b'B');
        port.advance_t_states(181_819, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x21, 0x01, "overrun remains latent until valid RDR is read");
        assert_eq!(port.read_data(), b'A');
        assert_eq!(port.peek_status() & 0x21, 0x21);
    }

    #[test]
    fn debugger_injection_bypasses_wire_time_but_keeps_finite_rdr() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x95);
        port.debugger_inject_received_character(b'A');
        assert_eq!(port.peek_status() & 0x81, 0x81);
        port.debugger_inject_received_character(b'B');
        assert_eq!(port.read_data(), b'A');
        assert_eq!(port.peek_status() & 0x21, 0x21);
    }

    #[test]
    fn debugger_tx_completion_does_not_leave_duplicate_endpoint_byte() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x15);
        port.write_data(b'D');
        assert_eq!(port.debugger_complete_one_tx(), Some(b'D'));
        assert_eq!(port.endpoint_tx_front(), None);
        assert!(!port.endpoint_tx_pending_or_hardware_busy());
    }

    #[test]
    fn divide_64_preserves_fractional_27_point_5_baud_exactly() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x16); // 8N1, /64 => 27.5 baud
        port.write_data(b'Z');

        port.advance_t_states(72_727, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x02, 0);
        port.advance_t_states(1, TWO_MHZ);
        assert_eq!(port.peek_status() & 0x02, 0x02);
    }
}
