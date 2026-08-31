use std::collections::VecDeque;

use crate::config::{SioHardwareConfig, SioRevision};

/// Digital COM2502/UART state used by one MITS 88-SIO card.
///
/// The UART is double buffered in both directions. `tx_holding` is the
/// transmitter data-bits holding register (TBMT reflects this register), while
/// `tx_shift` is the active serial shift register. `rx_shift` is the receiver
/// shift register and `rx_data` is the receiver data-bits holding register
/// (RDA reflects the latter).
pub(super) struct SioPort {
    config: SioHardwareConfig,
    bit_phase_numerator: u64,

    rx_data: u8,
    rx_full: bool,
    rx_shift: Option<(u8, bool, bool)>,
    rx_bits_remaining: u8,
    overrun: bool,
    framing_error: bool,
    parity_error: bool,

    tx_holding: Option<u8>,
    tx_shift: Option<u8>,
    tx_bits_remaining: u8,
    wire_tx: VecDeque<u8>,
}

impl Default for SioPort {
    fn default() -> Self { Self::new(SioHardwareConfig::default()) }
}

impl SioPort {
    pub(super) fn new(config: SioHardwareConfig) -> Self {
        Self {
            config,
            bit_phase_numerator: 0,
            rx_data: 0,
            rx_full: false,
            rx_shift: None,
            rx_bits_remaining: 0,
            overrun: false,
            framing_error: false,
            parity_error: false,
            tx_holding: None,
            tx_shift: None,
            tx_bits_remaining: 0,
            wire_tx: VecDeque::new(),
        }
    }

    pub(super) fn configure(&mut self, config: SioHardwareConfig) {
        if self.config == config { return; }
        *self = Self::new(config);
    }

    pub(super) fn config(&self) -> SioHardwareConfig { self.config }

    pub(super) fn clear(&mut self) {
        let config = self.config;
        *self = Self::new(config);
    }

    pub(super) fn receive_line_idle(&self) -> bool { self.rx_shift.is_none() }
    pub(super) fn receive_len(&self) -> usize {
        usize::from(self.rx_full) + usize::from(self.rx_shift.is_some())
    }
    pub(super) fn rx_full(&self) -> bool { self.rx_full }
    pub(super) fn tx_buffer_empty(&self) -> bool { self.tx_holding.is_none() }

    /// Begin one real serial receive frame. A full unread holding register does
    /// not stop the COM2502 receiver shift register; the resulting overwrite is
    /// handled only when this new frame completes.
    pub(super) fn queue_received_character(&mut self, value: u8) {
        if !self.receive_line_idle() { return; }
        self.rx_shift = Some((value, false, false));
        self.rx_bits_remaining = self.config.format.frame_bits();
    }

    /// Debugger injection bypasses serial line time but retains COM2502 holding
    /// register / overrun semantics.
    pub(super) fn debugger_inject_received_character(&mut self, value: u8) {
        self.complete_received_character(value, false, false);
    }

    fn complete_received_character(&mut self, value: u8, framing_error: bool, parity_error: bool) {
        // COM2502 differs importantly from MC6850: at completion it first notes
        // whether RDA was already high (overrun), then transfers the NEW shift
        // register contents into the receiver holding register. The old unread
        // character is therefore overwritten.
        self.overrun = self.rx_full;
        let bits = self.config.format.data_bits.bits();
        let mask = if bits == 8 { 0xff } else { ((1u16 << bits) - 1) as u8 };
        self.rx_data = value & mask;
        self.rx_full = true;
        self.framing_error = framing_error;
        self.parity_error = self.config.format.parity != crate::config::SioParity::None && parity_error;
    }

    pub(super) fn read_data(&mut self) -> u8 {
        let value = self.rx_data;
        // The board pulses RDAR when the CPU reads the data channel. COM2502
        // specifies RDAR as resetting RDA; error outputs are status-register
        // state and are refreshed by reception/master reset, not fabricated as
        // a side effect of this data read.
        self.rx_full = false;
        value
    }

    pub(super) fn peek_data(&self) -> u8 { self.rx_data }

    pub(super) fn clear_receive_for_debugger(&mut self) {
        self.rx_full = false;
        self.rx_shift = None;
        self.rx_bits_remaining = 0;
        self.overrun = false;
        self.framing_error = false;
        self.parity_error = false;
    }

    pub(super) fn write_data(&mut self, value: u8) {
        self.tx_holding = Some(value);
        // COM2502 loads an idle transmitter shift register immediately from the
        // holding register. TBMT therefore returns HIGH even though the serial
        // character is still physically transmitting.
        if self.tx_shift.is_none() { self.promote_tx_holding(); }
    }

    fn promote_tx_holding(&mut self) {
        if self.tx_shift.is_some() { return; }
        let Some(value) = self.tx_holding.take() else { return; };
        self.tx_shift = Some(value);
        self.tx_bits_remaining = self.config.format.frame_bits();
    }

    pub(super) fn endpoint_tx_front(&self) -> Option<u8> { self.wire_tx.front().copied() }
    pub(super) fn endpoint_tx_complete(&mut self) -> Option<u8> { self.wire_tx.pop_front() }
    pub(super) fn endpoint_tx_pending_or_hardware_busy(&self) -> bool {
        !self.wire_tx.is_empty() || self.tx_holding.is_some() || self.tx_shift.is_some()
    }

    pub(super) fn clear_transmit_for_debugger(&mut self) {
        self.tx_holding = None;
        self.tx_shift = None;
        self.tx_bits_remaining = 0;
        self.wire_tx.clear();
    }

    pub(super) fn debugger_complete_one_tx(&mut self) -> Option<u8> {
        if let Some(byte) = self.wire_tx.pop_front() { return Some(byte); }
        if self.tx_shift.is_none() { self.promote_tx_holding(); }
        let byte = self.tx_shift.take()?;
        self.tx_bits_remaining = 0;
        self.promote_tx_holding();
        Some(byte)
    }

    pub(super) fn status(&self) -> u8 {
        let errors = (u8::from(self.overrun) << 4)
            | (u8::from(self.framing_error) << 3)
            | (u8::from(self.parity_error) << 2);
        match self.config.revision {
            // Original status word: RDA on D5 and TBMT on D1, both active HIGH.
            // External device-ready inputs D7/D0 are modeled in their normal
            // ready (LOW) state until the physical handshake lines are exposed.
            SioRevision::Rev0 => {
                errors | (u8::from(self.rx_full) << 5) | (u8::from(self.tx_buffer_empty()) << 1)
            }
            // Rev 1 modification: receive ready is D0 active LOW and transmit
            // buffer empty is D7 active LOW. Error positions remain D4:D2.
            SioRevision::Rev1 => {
                errors
                    | u8::from(!self.rx_full)
                    | (u8::from(!self.tx_buffer_empty()) << 7)
            }
        }
    }

    fn transmitter_bit_boundary(&mut self) {
        let Some(byte) = self.tx_shift else {
            self.promote_tx_holding();
            return;
        };
        if self.tx_bits_remaining > 1 {
            self.tx_bits_remaining -= 1;
            return;
        }
        if self.tx_bits_remaining == 1 {
            self.tx_bits_remaining = 0;
            self.tx_shift = None;
            self.wire_tx.push_back(byte);
            self.promote_tx_holding();
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
            self.complete_received_character(value, framing_error, parity_error);
        }
    }

    /// Advance the independent 88-SIO baud clock by elapsed chassis T-states.
    /// `baud` is the serial bit rate; the physical COM2502 receives a 16x clock,
    /// but its internal divider yields exactly one serial bit boundary per
    /// `cpu_clock_hz / baud` elapsed chassis quanta.
    pub(super) fn advance_t_states(&mut self, t_states: u64, cpu_clock_hz: u32) {
        if t_states == 0 || cpu_clock_hz == 0 { return; }
        let baud = self.config.baud.baud();
        if baud == 0 { return; }
        let total = self.bit_phase_numerator
            .saturating_add(t_states.saturating_mul(u64::from(baud)));
        let threshold = u64::from(cpu_clock_hz);
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
    use crate::config::{SioBaudRate, SioRevision, SioStopBits, SioWordFormat};

    const TWO_MHZ: u32 = 2_000_000;

    fn config(revision: SioRevision) -> SioHardwareConfig {
        SioHardwareConfig { revision, ..SioHardwareConfig::default() }
    }

    #[test]
    fn rev0_and_rev1_ready_flags_have_historical_positions_and_polarity() {
        let mut rev0 = SioPort::new(config(SioRevision::Rev0));
        let mut rev1 = SioPort::new(config(SioRevision::Rev1));
        assert_eq!(rev0.status(), 0x02, "Rev0 TBMT is D1 active high");
        assert_eq!(rev1.status(), 0x01, "Rev1 not-RDA is D0 high while empty; D7 low means TX ready");

        rev0.debugger_inject_received_character(b'R');
        rev1.debugger_inject_received_character(b'R');
        assert_eq!(rev0.status() & 0x20, 0x20, "Rev0 RDA is D5 active high");
        assert_eq!(rev1.status() & 0x01, 0x00, "Rev1 receive-ready is D0 active low");

        rev0.write_data(b'A');
        rev1.write_data(b'A');
        // Idle shift register consumes holding data immediately, so TBMT is
        // already ready again while the character itself remains in flight.
        assert_eq!(rev0.status() & 0x02, 0x02);
        assert_eq!(rev1.status() & 0x80, 0x00);
        rev0.write_data(b'B');
        rev1.write_data(b'B');
        assert_eq!(rev0.status() & 0x02, 0x00, "second byte occupies Rev0 holding register");
        assert_eq!(rev1.status() & 0x80, 0x80, "second byte makes Rev1 active-low TX ready false");
    }

    #[test]
    fn com2502_overrun_overwrites_old_unread_character_with_new_character() {
        let mut p = SioPort::new(config(SioRevision::Rev1));
        p.debugger_inject_received_character(b'A');
        p.debugger_inject_received_character(b'B');
        assert_eq!(p.status() & 0x10, 0x10);
        assert_eq!(p.read_data(), b'B', "COM2502 transfers the new shift-register character into the holding register");
        assert_eq!(p.status() & 0x01, 0x01, "data read resets RDA");
        assert_eq!(p.status() & 0x10, 0x10, "RDAR does not fabricate an error clear");
        p.debugger_inject_received_character(b'C');
        assert_eq!(p.status() & 0x10, 0x00, "next reception with RDA previously low refreshes overrun false");
    }

    #[test]
    fn double_buffered_transmitter_returns_tbmt_before_character_finishes() {
        let mut c = config(SioRevision::Rev1);
        c.baud = SioBaudRate::try_new(9_600).unwrap();
        c.format = SioWordFormat { stop_bits: SioStopBits::One, ..SioWordFormat::default() };
        let mut p = SioPort::new(c);
        p.write_data(b'A');
        assert_eq!(p.status() & 0x80, 0, "holding register is already empty after idle promotion");
        assert_eq!(p.endpoint_tx_front(), None);
        p.write_data(b'B');
        assert_eq!(p.status() & 0x80, 0x80, "holding register now contains the next byte");

        p.advance_t_states(2_083, TWO_MHZ);
        assert_eq!(p.endpoint_tx_front(), None);
        p.advance_t_states(1, TWO_MHZ);
        assert_eq!(p.endpoint_tx_front(), Some(b'A'));
        assert_eq!(p.status() & 0x80, 0, "B promoted at the exact A frame boundary");
    }

    #[test]
    fn unread_rda_does_not_stop_receiver_shift_and_real_overrun_occurs_after_next_frame() {
        let mut p = SioPort::new(config(SioRevision::Rev1));
        p.queue_received_character(b'A');
        p.advance_t_states(200_000, TWO_MHZ); // 110 baud, 8N2 => 100 ms
        assert!(!p.receive_line_idle() || p.rx_full());
        if !p.receive_line_idle() { p.advance_t_states(1, TWO_MHZ); }
        assert!(p.rx_full());
        assert!(p.receive_line_idle());
        p.queue_received_character(b'B');
        p.advance_t_states(200_001, TWO_MHZ);
        assert_eq!(p.status() & 0x10, 0x10);
        assert_eq!(p.peek_data(), b'B');
    }
}
