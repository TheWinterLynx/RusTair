/// Motorola MC6850 ACIA digital core.
///
/// This models the chip-owned register/flag state independently from any host
/// terminal or teletype. Serial bit timing is driven explicitly by callers via
/// transmitter/receiver completion events; presentation endpoints never own
/// TDRE/RDRF semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Parity {
    None,
    Even,
    Odd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WordFormat {
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct Mc6850 {
    control: u8,

    // Receive Data Register plus its character-associated error flags. The 6850
    // RDR is one byte deep; the receiver shift register is represented by the
    // completed-character event entering `receive_character`.
    rdr: u8,
    rdr_full: bool,
    framing_error: bool,
    parity_error: bool,
    overrun_pending: bool,
    overrun_visible: bool,

    // The transmitter really contains both a TDR and a shift register. TDRE
    // describes only the TDR, not whether a character is still shifting out.
    tdr: Option<u8>,
    tx_shift: Option<u8>,

    // MC6850 modem inputs are active low. These booleans store the status-register
    // sense: true means the external input is high/inactive and therefore the
    // corresponding status bit reads as one.
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
            // MITS documents grounding unused CTS/DCD inputs. Low therefore
            // represents the normal directly-connected terminal condition.
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
    pub(crate) fn control(&self) -> u8 {
        self.control
    }

    pub(crate) fn clock_divider(&self) -> Option<u8> {
        match self.control & 0x03 {
            0x00 => Some(1),
            0x01 => Some(16),
            0x02 => Some(64),
            _ => None,
        }
    }

    pub(crate) fn word_format(&self) -> WordFormat {
        match (self.control >> 2) & 0x07 {
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

    pub(crate) fn receive_interrupt_enabled(&self) -> bool {
        self.control & 0x80 != 0
    }

    pub(crate) fn transmit_interrupt_enabled(&self) -> bool {
        self.control & 0x60 == 0x20
    }

    /// Active-low RTS output expressed as an asserted boolean.
    pub(crate) fn rts_asserted(&self) -> bool {
        matches!(self.control & 0x60, 0x00 | 0x20 | 0x60)
    }

    pub(crate) fn break_active(&self) -> bool {
        self.control & 0x60 == 0x60
    }

    pub(crate) fn write_control(&mut self, value: u8) {
        self.control = value;
        if value & 0x03 == 0x03 {
            self.master_reset();
        }
    }

    /// MC6850 master reset clears internal status/error state and initializes
    /// transmitter/receiver logic, but does not alter CR2..CR7 or external CTS/
    /// DCD levels.
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
        // Motorola specifies that DCD high forces RDRF empty. During an overrun
        // indication RDRF intentionally remains set until the overrun is reset.
        !self.dcd_input_high && (self.rdr_full || self.overrun_visible)
    }

    fn tdre(&self) -> bool {
        // CTS high inhibits the TDRE indication even if the TDR is physically
        // empty. The shift register may remain busy while TDRE is already high.
        !self.cts_high && self.tdr.is_none()
    }

    pub(crate) fn interrupt_request(&self) -> bool {
        let rx_irq = self.receive_interrupt_enabled()
            && (self.rdrf() || self.overrun_visible || self.dcd_irq_pending);
        let tx_irq = self.transmit_interrupt_enabled() && self.tdre();
        rx_irq || tx_irq
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

    pub(crate) fn peek_status(&self) -> u8 {
        self.status_value()
    }

    pub(crate) fn read_status(&mut self) -> u8 {
        let status = self.status_value();
        if status & 0x04 != 0 {
            self.dcd_status_seen = true;
        }
        status
    }

    pub(crate) fn peek_data(&self) -> u8 {
        self.rdr
    }

    pub(crate) fn read_data(&mut self) -> u8 {
        let value = self.rdr;

        if self.overrun_visible {
            // Motorola keeps RDRF asserted together with OVRN after the valid
            // pre-overrun character has been read. A subsequent data-register
            // read clears the overrun indication.
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

        // Loss-of-carrier IRQ is cleared only by status-read followed by a data
        // read (or reset). If DCD is still high, its status bit remains high but
        // the edge-triggered IRQ condition is cleared until another transition.
        if self.dcd_status_seen {
            self.dcd_irq_pending = false;
            self.dcd_status_seen = false;
            if !self.dcd_input_high {
                self.dcd_status_latched = false;
            }
        }

        value
    }

    /// Complete one received serial character and transfer it from the receiver
    /// shift register into the one-byte RDR when possible. Additional complete
    /// characters while RDR is occupied are lost and arm the documented delayed
    /// overrun indication.
    pub(crate) fn receive_character(
        &mut self,
        value: u8,
        framing_error: bool,
        parity_error: bool,
    ) {
        if self.rdr_full || self.overrun_visible {
            self.overrun_pending = true;
            return;
        }
        self.rdr = if self.word_format().data_bits == 7 { value & 0x7f } else { value };
        self.rdr_full = true;
        self.framing_error = framing_error;
        self.parity_error = self.word_format().parity != Parity::None && parity_error;
    }

    pub(crate) fn clear_receive_for_debugger(&mut self) {
        self.rdr_full = false;
        self.framing_error = false;
        self.parity_error = false;
        self.overrun_pending = false;
        self.overrun_visible = false;
    }

    pub(crate) fn receive_len(&self) -> usize {
        usize::from(self.rdrf())
    }

    pub(crate) fn write_data(&mut self, value: u8) {
        self.tdr = Some(value);
    }

    /// One transmitter clock opportunity: if the shift register is idle and CTS
    /// permits transmission, move TDR into TSR. This is the transition that sets
    /// TDRE; completion of the character is a separate event.
    pub(crate) fn transfer_tdr_to_shift_if_idle(&mut self) -> bool {
        if self.cts_high || self.tx_shift.is_some() {
            return false;
        }
        let Some(value) = self.tdr.take() else {
            return false;
        };
        self.tx_shift = Some(value);
        true
    }

    pub(crate) fn tx_shift_front(&self) -> Option<u8> {
        self.tx_shift
    }

    pub(crate) fn transmit_busy(&self) -> bool {
        self.tdr.is_some() || self.tx_shift.is_some()
    }

    /// Complete the current shifted character. If another byte is waiting in
    /// TDR, the real transmitter starts it as soon as the previous character is
    /// complete, so promote it to TSR at this same boundary.
    pub(crate) fn complete_tx_shift(&mut self) -> Option<u8> {
        let completed = self.tx_shift.take()?;
        let _ = self.transfer_tdr_to_shift_if_idle();
        Some(completed)
    }

    pub(crate) fn clear_transmit_for_debugger(&mut self) {
        self.tdr = None;
        self.tx_shift = None;
    }

    pub(crate) fn set_cts_high(&mut self, high: bool) {
        self.cts_high = high;
    }

    pub(crate) fn set_dcd_high(&mut self, high: bool) {
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
    fn control_register_decodes_motorola_clock_word_and_modem_modes() {
        let mut acia = Mc6850::default();
        acia.write_control(0b1001_0101); // RX IRQ, 8N1, divide 16
        assert_eq!(acia.clock_divider(), Some(16));
        assert_eq!(
            acia.word_format(),
            WordFormat { data_bits: 8, parity: Parity::None, stop_bits: 1 }
        );
        assert!(acia.receive_interrupt_enabled());
        assert!(!acia.transmit_interrupt_enabled());
        assert!(acia.rts_asserted());
        assert!(!acia.break_active());

        acia.write_control(0x60);
        assert!(acia.break_active());
        assert!(acia.rts_asserted());
        assert!(!acia.transmit_interrupt_enabled());
    }

    #[test]
    fn tdr_and_shift_register_are_distinct_and_tdre_tracks_only_tdr() {
        let mut acia = Mc6850::default();
        assert_eq!(acia.peek_status() & 0x02, 0x02);

        acia.write_data(b'A');
        assert_eq!(acia.peek_status() & 0x02, 0x00);
        assert_eq!(acia.tx_shift_front(), None);

        assert!(acia.transfer_tdr_to_shift_if_idle());
        assert_eq!(acia.tx_shift_front(), Some(b'A'));
        assert_eq!(acia.peek_status() & 0x02, 0x02);

        acia.write_data(b'B');
        assert_eq!(acia.peek_status() & 0x02, 0x00);
        assert!(!acia.transfer_tdr_to_shift_if_idle());
        assert_eq!(acia.complete_tx_shift(), Some(b'A'));
        assert_eq!(acia.tx_shift_front(), Some(b'B'));
        assert_eq!(acia.peek_status() & 0x02, 0x02);
    }

    #[test]
    fn receive_register_is_one_byte_and_rdrf_clears_on_read() {
        let mut acia = Mc6850::default();
        acia.write_control(0x14); // 8N1, divide 1
        acia.receive_character(0x5a, false, false);
        assert_eq!(acia.peek_status() & 0x01, 0x01);
        assert_eq!(acia.peek_data(), 0x5a);
        assert_eq!(acia.read_data(), 0x5a);
        assert_eq!(acia.peek_status() & 0x01, 0x00);
    }

    #[test]
    fn overrun_is_delayed_until_valid_character_is_read_then_requires_data_read_to_clear() {
        let mut acia = Mc6850::default();
        acia.write_control(0x14);
        acia.receive_character(b'A', false, false);
        acia.receive_character(b'B', false, false); // lost in receiver overrun

        assert_eq!(acia.peek_status() & 0x21, 0x01, "OVRN is latent while A is unread");
        assert_eq!(acia.read_data(), b'A');
        assert_eq!(acia.peek_status() & 0x21, 0x21, "RDRF stays set when OVRN becomes visible");
        assert_eq!(acia.read_data(), b'A', "second read sees retained RDR contents");
        assert_eq!(acia.peek_status() & 0x21, 0x00);
    }

    #[test]
    fn receive_and_transmit_interrupts_follow_rdrf_tdre_not_endpoint_busy() {
        let mut acia = Mc6850::default();
        acia.write_control(0xa0); // RX IRQ + TX-empty IRQ, divide 1
        assert!(acia.interrupt_request(), "empty TDR requests TX service");

        acia.write_data(b'T');
        assert!(!acia.interrupt_request(), "writing TDR clears TX-empty condition");
        assert!(acia.transfer_tdr_to_shift_if_idle());
        assert!(acia.interrupt_request(), "TDRE returns while TSR is still transmitting");
        assert_eq!(acia.tx_shift_front(), Some(b'T'));

        acia.write_control(0x80); // RX IRQ only
        assert!(!acia.interrupt_request());
        acia.receive_character(b'R', false, false);
        assert!(acia.interrupt_request());
        assert_eq!(acia.read_data(), b'R');
        assert!(!acia.interrupt_request());
    }

    #[test]
    fn cts_high_inhibits_tdre_and_prevents_tdr_transfer() {
        let mut acia = Mc6850::default();
        acia.set_cts_high(true);
        assert_eq!(acia.peek_status() & 0x0a, 0x08);
        acia.write_data(b'X');
        assert!(!acia.transfer_tdr_to_shift_if_idle());
        assert_eq!(acia.tx_shift_front(), None);

        acia.set_cts_high(false);
        assert!(acia.transfer_tdr_to_shift_if_idle());
        assert_eq!(acia.tx_shift_front(), Some(b'X'));
        assert_eq!(acia.peek_status() & 0x02, 0x02);
    }

    #[test]
    fn framing_and_parity_errors_belong_to_the_current_receive_character() {
        let mut acia = Mc6850::default();
        acia.write_control(0x1c); // 8 odd parity, 1 stop, divide 1
        acia.receive_character(0x33, true, true);
        assert_eq!(acia.peek_status() & 0x51, 0x51);
        acia.read_data();
        assert_eq!(acia.peek_status() & 0x51, 0x00);

        acia.write_control(0x14); // 8N1: parity checking inhibited
        acia.receive_character(0x44, false, true);
        assert_eq!(acia.peek_status() & 0x40, 0x00);
    }

    #[test]
    fn dcd_interrupt_requires_status_then_data_read_to_clear_the_edge_latch() {
        let mut acia = Mc6850::default();
        acia.write_control(0x80);
        acia.set_dcd_high(true);
        assert_eq!(acia.peek_status() & 0x84, 0x84);
        assert!(acia.interrupt_request());

        acia.set_dcd_high(false);
        assert_eq!(acia.peek_status() & 0x84, 0x84, "DCD status remains latched after carrier returns");
        let _ = acia.read_status();
        assert!(acia.interrupt_request());
        let _ = acia.read_data();
        assert_eq!(acia.peek_status() & 0x84, 0x00);
        assert!(!acia.interrupt_request());
    }

    #[test]
    fn master_reset_clears_internal_status_but_preserves_upper_control_and_external_cts() {
        let mut acia = Mc6850::default();
        acia.set_cts_high(true);
        acia.write_control(0xbc); // upper control bits plus master reset
        assert_eq!(acia.control(), 0xbc);
        assert_eq!(acia.clock_divider(), None);
        assert_eq!(acia.peek_status() & 0x08, 0x08);
        assert_eq!(acia.peek_status() & 0x71, 0x00);
    }
}
