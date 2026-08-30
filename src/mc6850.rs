/// Motorola MC6850 ACIA digital core.
///
/// Register state belongs to the emulated chip, never to a host terminal or
/// teletype. Serial bit timing is advanced explicitly by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Parity { None, Even, Odd }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WordFormat {
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct Mc6850 {
    control: u8,
    rdr: u8,
    rdr_full: bool,
    framing_error: bool,
    parity_error: bool,
    overrun_pending: bool,
    overrun_visible: bool,
    tdr: Option<u8>,
    tx_shift: Option<u8>,
    // Status-register sense of the active-low modem inputs.
    cts_high: bool,
    dcd_input_high: bool,
    dcd_status_latched: bool,
    dcd_irq_pending: bool,
    dcd_status_seen: bool,
}

impl Default for Mc6850 {
    fn default() -> Self {
        let mut acia = Self {
            control: 0,
            rdr: 0,
            rdr_full: false,
            framing_error: false,
            parity_error: false,
            overrun_pending: false,
            overrun_visible: false,
            tdr: None,
            tx_shift: None,
            // MITS instructs grounding unused CTS/DCD inputs.
            cts_high: false,
            dcd_input_high: false,
            dcd_status_latched: false,
            dcd_irq_pending: false,
            dcd_status_seen: false,
        };
        acia.master_reset();
        acia
    }
}

impl Mc6850 {
    pub(crate) fn word_format(&self) -> WordFormat {
        match (self.control >> 2) & 7 {
            0 => WordFormat { data_bits: 7, parity: Parity::Even, stop_bits: 2 },
            1 => WordFormat { data_bits: 7, parity: Parity::Odd, stop_bits: 2 },
            2 => WordFormat { data_bits: 7, parity: Parity::Even, stop_bits: 1 },
            3 => WordFormat { data_bits: 7, parity: Parity::Odd, stop_bits: 1 },
            4 => WordFormat { data_bits: 8, parity: Parity::None, stop_bits: 2 },
            5 => WordFormat { data_bits: 8, parity: Parity::None, stop_bits: 1 },
            6 => WordFormat { data_bits: 8, parity: Parity::Even, stop_bits: 1 },
            _ => WordFormat { data_bits: 8, parity: Parity::Odd, stop_bits: 1 },
        }
    }

    pub(crate) fn frame_bits(&self) -> u8 {
        let f = self.word_format();
        1 + f.data_bits + u8::from(f.parity != Parity::None) + f.stop_bits
    }

    fn receive_interrupt_enabled(&self) -> bool { self.control & 0x80 != 0 }
    fn transmit_interrupt_enabled(&self) -> bool { self.control & 0x60 == 0x20 }

    pub(crate) fn write_control(&mut self, value: u8) {
        self.control = value;
        if value & 3 == 3 { self.master_reset(); }
    }

    pub(crate) fn master_reset(&mut self) {
        self.rdr_full = false;
        self.framing_error = false;
        self.parity_error = false;
        self.overrun_pending = false;
        self.overrun_visible = false;
        self.tdr = None;
        self.tx_shift = None;
        self.dcd_status_latched = self.dcd_input_high;
        self.dcd_irq_pending = false;
        self.dcd_status_seen = false;
    }

    fn rdrf(&self) -> bool {
        !self.dcd_input_high && (self.rdr_full || self.overrun_visible)
    }

    fn tdre(&self) -> bool {
        // CTS high inhibits TDRE even if the holding register is empty.
        !self.cts_high && self.tdr.is_none()
    }

    pub(crate) fn interrupt_request(&self) -> bool {
        (self.receive_interrupt_enabled()
            && (self.rdrf() || self.overrun_visible || self.dcd_irq_pending))
            || (self.transmit_interrupt_enabled() && self.tdre())
    }

    fn status_value(&self) -> u8 {
        u8::from(self.rdrf())
            | (u8::from(self.tdre()) << 1)
            | (u8::from(self.dcd_input_high || self.dcd_status_latched) << 2)
            | (u8::from(self.cts_high) << 3)
            | (u8::from(self.framing_error && self.rdr_full) << 4)
            | (u8::from(self.overrun_visible) << 5)
            | (u8::from(self.parity_error && self.rdr_full) << 6)
            | (u8::from(self.interrupt_request()) << 7)
    }

    pub(crate) fn peek_status(&self) -> u8 { self.status_value() }

    pub(crate) fn read_status(&mut self) -> u8 {
        let value = self.status_value();
        if value & 0x04 != 0 { self.dcd_status_seen = true; }
        value
    }

    pub(crate) fn peek_data(&self) -> u8 { self.rdr }

    pub(crate) fn read_data(&mut self) -> u8 {
        let value = self.rdr;
        if self.overrun_visible {
            self.overrun_visible = false;
            self.rdr_full = false;
        } else if self.rdr_full {
            self.rdr_full = false;
            if self.overrun_pending {
                self.overrun_pending = false;
                self.overrun_visible = true;
            }
        }
        if !self.rdr_full {
            self.framing_error = false;
            self.parity_error = false;
        }
        if self.dcd_status_seen {
            self.dcd_irq_pending = false;
            self.dcd_status_seen = false;
            if !self.dcd_input_high { self.dcd_status_latched = false; }
        }
        value
    }

    /// Receiver shift-register completion. If RDR is occupied, the new character
    /// is lost and OVRN remains latent until the valid old RDR byte is read.
    pub(crate) fn receive_character(&mut self, value: u8, framing_error: bool, parity_error: bool) {
        if self.rdr_full || self.overrun_visible {
            self.overrun_pending = true;
            return;
        }
        let format = self.word_format();
        self.rdr = if format.data_bits == 7 { value & 0x7f } else { value };
        self.rdr_full = true;
        self.framing_error = framing_error;
        self.parity_error = format.parity != Parity::None && parity_error;
    }

    pub(crate) fn receive_len(&self) -> usize { usize::from(self.rdrf()) }

    pub(crate) fn clear_receive_for_debugger(&mut self) {
        self.rdr_full = false;
        self.framing_error = false;
        self.parity_error = false;
        self.overrun_pending = false;
        self.overrun_visible = false;
    }

    pub(crate) fn write_data(&mut self, value: u8) { self.tdr = Some(value); }

    /// Transfer TDR -> transmit shift register. This transition, not completion
    /// at the terminal, is what raises TDRE.
    pub(crate) fn transfer_tdr_to_shift_if_idle(&mut self) -> bool {
        if self.cts_high || self.tx_shift.is_some() { return false; }
        let Some(value) = self.tdr.take() else { return false; };
        self.tx_shift = Some(value);
        true
    }

    pub(crate) fn tx_shift_front(&self) -> Option<u8> { self.tx_shift }
    pub(crate) fn transmit_busy(&self) -> bool { self.tdr.is_some() || self.tx_shift.is_some() }

    pub(crate) fn complete_tx_shift(&mut self) -> Option<u8> {
        let completed = self.tx_shift.take()?;
        let _ = self.transfer_tdr_to_shift_if_idle();
        Some(completed)
    }

    pub(crate) fn clear_transmit_for_debugger(&mut self) {
        self.tdr = None;
        self.tx_shift = None;
    }

    #[cfg(test)]
    fn clock_divider(&self) -> Option<u8> {
        match self.control & 3 { 0 => Some(1), 1 => Some(16), 2 => Some(64), _ => None }
    }
    #[cfg(test)]
    fn rts_asserted(&self) -> bool { matches!(self.control & 0x60, 0x00 | 0x20 | 0x60) }
    #[cfg(test)]
    fn break_active(&self) -> bool { self.control & 0x60 == 0x60 }
    #[cfg(test)]
    fn set_cts_high(&mut self, high: bool) { self.cts_high = high; }
    #[cfg(test)]
    fn set_dcd_high(&mut self, high: bool) {
        if high && !self.dcd_input_high {
            self.dcd_status_latched = true;
            self.dcd_irq_pending = true;
        }
        self.dcd_input_high = high;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_word_decodes_clock_format_rts_and_break() {
        let mut a = Mc6850::default();
        a.write_control(0x95); // RX IRQ, 8N1, divide 16
        assert_eq!(a.clock_divider(), Some(16));
        assert_eq!(a.word_format(), WordFormat { data_bits: 8, parity: Parity::None, stop_bits: 1 });
        assert_eq!(a.frame_bits(), 10);
        assert!(a.rts_asserted());
        assert!(!a.break_active());
        a.write_control(0x60);
        assert!(a.break_active());
        assert!(a.rts_asserted());
    }

    #[test]
    fn tdr_tsr_are_distinct_and_tdre_tracks_tdr_only() {
        let mut a = Mc6850::default();
        assert_eq!(a.peek_status() & 2, 2);
        a.write_data(b'A');
        assert_eq!(a.peek_status() & 2, 0);
        assert!(a.transfer_tdr_to_shift_if_idle());
        assert_eq!(a.tx_shift_front(), Some(b'A'));
        assert_eq!(a.peek_status() & 2, 2);
        a.write_data(b'B');
        assert_eq!(a.peek_status() & 2, 0);
        assert_eq!(a.complete_tx_shift(), Some(b'A'));
        assert_eq!(a.tx_shift_front(), Some(b'B'));
        assert_eq!(a.peek_status() & 2, 2);
    }

    #[test]
    fn rdr_is_one_byte_and_overrun_is_delayed_exactly_as_documented() {
        let mut a = Mc6850::default();
        a.write_control(0x14);
        a.receive_character(b'A', false, false);
        a.receive_character(b'B', false, false);
        assert_eq!(a.peek_status() & 0x21, 0x01);
        assert_eq!(a.read_data(), b'A');
        assert_eq!(a.peek_status() & 0x21, 0x21);
        assert_eq!(a.read_data(), b'A');
        assert_eq!(a.peek_status() & 0x21, 0);
    }

    #[test]
    fn irq_follows_rdrf_tdre_instead_of_endpoint_busy() {
        let mut a = Mc6850::default();
        a.write_control(0xa0);
        assert!(a.interrupt_request());
        a.write_data(b'T');
        assert!(!a.interrupt_request());
        assert!(a.transfer_tdr_to_shift_if_idle());
        assert!(a.interrupt_request());
        assert!(a.transmit_busy());
        a.write_control(0x80);
        assert!(!a.interrupt_request());
        a.receive_character(b'R', false, false);
        assert!(a.interrupt_request());
        assert_eq!(a.read_data(), b'R');
        assert!(!a.interrupt_request());
    }

    #[test]
    fn cts_inhibits_tdre_and_tdr_transfer() {
        let mut a = Mc6850::default();
        a.set_cts_high(true);
        assert_eq!(a.peek_status() & 0x0a, 0x08);
        a.write_data(b'X');
        assert!(!a.transfer_tdr_to_shift_if_idle());
        a.set_cts_high(false);
        assert!(a.transfer_tdr_to_shift_if_idle());
        assert_eq!(a.peek_status() & 2, 2);
    }

    #[test]
    fn framing_and_parity_flags_belong_to_current_rdr_character() {
        let mut a = Mc6850::default();
        a.write_control(0x1c); // 8O1
        a.receive_character(0x33, true, true);
        assert_eq!(a.peek_status() & 0x51, 0x51);
        a.read_data();
        assert_eq!(a.peek_status() & 0x51, 0);
        a.write_control(0x14); // 8N1
        a.receive_character(0x44, false, true);
        assert_eq!(a.peek_status() & 0x40, 0);
    }

    #[test]
    fn dcd_irq_clears_only_after_status_then_data_read() {
        let mut a = Mc6850::default();
        a.write_control(0x80);
        a.set_dcd_high(true);
        assert_eq!(a.peek_status() & 0x84, 0x84);
        a.set_dcd_high(false);
        assert_eq!(a.peek_status() & 0x84, 0x84);
        let _ = a.read_status();
        assert!(a.interrupt_request());
        let _ = a.read_data();
        assert_eq!(a.peek_status() & 0x84, 0);
    }

    #[test]
    fn master_reset_preserves_external_cts_and_upper_control_bits() {
        let mut a = Mc6850::default();
        a.set_cts_high(true);
        a.write_control(0xbf); // CR1:CR0=11 master reset
        assert_eq!(a.control, 0xbf);
        assert_eq!(a.clock_divider(), None);
        assert_eq!(a.peek_status() & 0x08, 0x08);
        assert_eq!(a.peek_status() & 0x71, 0);
    }
}
