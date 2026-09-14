use std::collections::VecDeque;

use crate::mc6850::{Mc6850, Parity};

/// Physical baud-generator tap selected by the 88-2SIO board strap. The MITS
/// manual exposes these eight taps independently for each ACIA. Configuration
/// maps every strap choice to its corresponding physical tap in `card_baud_tap`.
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
    /// Fractional phase of the external free-running MITS baud-generator tap.
    /// This phase is independent from the MC6850 divide selection and therefore
    /// survives ACIA master reset and CR1:CR0 changes.
    tap_phase_numerator: u64,
    /// Number of external tap pulses accumulated by the MC6850's currently
    /// selected /1, /16 or /64 divider since its previous effective clock edge.
    divider_phase: u8,
    tx_bits_remaining: u8,
    /// True once the currently shifting character has overlapped the physical
    /// BREAK/spacing level. Motorola makes BREAK an override of Tx Data, not a
    /// transmitter clock stop, so TDR/TSR keep advancing but such a frame can no
    /// longer be presented downstream as the original byte.
    tx_frame_corrupted_by_break: bool,
    wire_tx: VecDeque<u8>,
    /// Complete character currently traversing the external receive line / ACIA
    /// receiver shift register. This is deliberately not RDR/RDRF: status bit 0
    /// remains low until the configured serial frame has completed.
    rx_shift: Option<(u8, bool, bool)>,
    rx_bits_remaining: u8,
    /// Literal receive-wire BREAK/SPACE level driven by the attached peripheral.
    /// It is separate from the MC6850 transmitter BREAK control above.
    rx_break_active: bool,
    rx_shift_from_break: bool,
}

impl TwoSioPort {
    pub(super) fn new(baud_tap: TwoSioBaudTap) -> Self {
        Self {
            acia: Mc6850::default(),
            control: 0,
            baud_tap,
            tap_phase_numerator: 0,
            divider_phase: 0,
            tx_bits_remaining: 0,
            tx_frame_corrupted_by_break: false,
            wire_tx: VecDeque::new(),
            rx_shift: None,
            rx_bits_remaining: 0,
            rx_break_active: false,
            rx_shift_from_break: false,
        }
    }

    pub(super) fn reset(&mut self) {
        let baud_tap = self.baud_tap;
        *self = Self::new(baud_tap);
    }

    pub(super) fn read_status(&mut self) -> u8 {
        self.acia.read_status()
    }
    pub(super) fn peek_status(&self) -> u8 {
        self.acia.peek_status()
    }
    pub(super) fn read_data(&mut self) -> u8 {
        self.acia.read_data()
    }
    pub(super) fn peek_data(&self) -> u8 {
        self.acia.peek_data()
    }
    pub(super) fn interrupt_request(&self) -> bool {
        self.acia.interrupt_request()
    }

    /// Physical modem/control pins of this MC6850 channel. The 88-2SIO board
    /// does not reinterpret their polarity: attached cables/peripherals decide
    /// what a TTL HIGH/LOW means at the far end.
    pub(super) fn rts_high(&self) -> bool {
        self.acia.rts_high()
    }
    pub(super) fn break_active(&self) -> bool {
        self.acia.break_active()
    }
    pub(super) fn cts_high(&self) -> bool {
        self.acia.cts_high()
    }
    pub(super) fn dcd_high(&self) -> bool {
        self.acia.dcd_high()
    }
    pub(super) fn set_cts_high(&mut self, high: bool) {
        self.acia.set_cts_high(high);
    }
    pub(super) fn set_dcd_high(&mut self, high: bool) {
        self.acia.set_dcd_high(high);
    }

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

    fn break_parity_error(&self) -> bool {
        // Continuous SPACE supplies a LOW parity bit. All-zero data requires a
        // HIGH parity bit only in odd-parity modes.
        matches!(self.acia.word_format().parity, Parity::Odd)
    }

    fn start_break_frame_if_idle(&mut self) {
        if !self.rx_break_active || self.rx_shift.is_some() {
            return;
        }
        self.rx_shift = Some((0, true, self.break_parity_error()));
        self.rx_bits_remaining = self.acia.frame_bits();
        self.rx_shift_from_break = true;
    }

    /// Drive/release the external receive wire's BREAK condition. A held BREAK
    /// is continuous SPACE, so the receiver sees zero data followed by a missing
    /// stop bit. Holding it across multiple frame times can therefore produce
    /// normal MC6850 FE/RDRF and delayed OVRN behavior.
    pub(super) fn set_receive_break(&mut self, active: bool) {
        if self.rx_break_active == active {
            return;
        }
        self.rx_break_active = active;
        if active {
            self.start_break_frame_if_idle();
        } else if self.rx_shift_from_break {
            self.rx_shift = None;
            self.rx_bits_remaining = 0;
            self.rx_shift_from_break = false;
        }
    }

    pub(super) fn write_control(&mut self, value: u8) {
        let was_break = self.acia.break_active();
        let previous_divider = self.clock_divider();
        self.control = value;
        self.acia.write_control(value);
        let next_divider = self.clock_divider();
        let now_break = self.acia.break_active();

        // CR1:CR0 select the MC6850's internal divider, not the external MITS
        // oscillator. A new divide ratio starts a new internal divide epoch while
        // preserving the free-running external tap phase.
        if previous_divider != next_divider {
            self.divider_phase = 0;
        }

        // BREAK controls the physical Tx Data pin immediately. If a character is
        // already shifting when spacing begins, that frame is irreversibly
        // corrupted even if BREAK is released before its nominal stop bit.
        if !was_break && now_break && self.acia.tx_shift_front().is_some() {
            self.tx_frame_corrupted_by_break = true;
        }

        if value & 0x03 == 0x03 {
            // Master reset initializes the ACIA and its internal divider, but it
            // cannot reset the separate MITS baud generator feeding the RxC/TxC
            // pins. Preserve `tap_phase_numerator` so the next external pulse
            // remains on the same physical oscillator timeline.
            self.divider_phase = 0;
            self.tx_bits_remaining = 0;
            self.tx_frame_corrupted_by_break = false;
            self.rx_shift = None;
            self.rx_bits_remaining = 0;
            self.rx_shift_from_break = false;
        } else {
            self.start_break_frame_if_idle();
        }
    }

    pub(super) fn write_data(&mut self, value: u8) {
        // Do not move TDR to TSR here. Motorola specifies that the transfer is
        // synchronized by the transmitter clock and occurs within one bit time
        // when the transmitter is idle. BREAK is not a transfer inhibit; only
        // the physical Tx Data level is overridden while CR6:CR5=11.
        self.acia.write_data(value);
    }

    /// A normal endpoint starts one physical serial character. There is no hidden
    /// byte FIFO in front of the MC6850 receiver. An overlapping host presentation
    /// is rejected rather than accumulating a non-historical queue behind it.
    pub(super) fn queue_received_character(&mut self, value: u8) {
        if !self.receive_line_idle() || self.rx_break_active {
            return;
        }
        self.rx_shift = Some((value, false, false));
        self.rx_bits_remaining = self.acia.frame_bits();
        self.rx_shift_from_break = false;
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
        self.rx_shift_from_break = false;
        self.start_break_frame_if_idle();
    }

    pub(super) fn clear_transmit_for_debugger(&mut self) {
        self.acia.clear_transmit_for_debugger();
        self.tx_bits_remaining = 0;
        self.tx_frame_corrupted_by_break = false;
        self.wire_tx.clear();
    }

    /// Force one debugger-visible UART TX completion. Prefer an already
    /// completed wire byte; otherwise finish the active hardware character (or
    /// first promote a waiting TDR) without leaving a duplicate for an endpoint.
    /// The returned value is the internal TSR byte for debugger inspection; a
    /// BREAK-corrupted frame is still not queued as valid wire data.
    pub(super) fn debugger_complete_one_tx(&mut self) -> Option<u8> {
        if let Some(byte) = self.wire_tx.pop_front() {
            return Some(byte);
        }
        if self.acia.tx_shift_front().is_none() {
            if self.acia.transfer_tdr_to_shift_if_idle() {
                self.tx_frame_corrupted_by_break = self.break_active();
            }
        }
        let byte = self.acia.complete_tx_shift()?;
        self.tx_bits_remaining = if self.acia.tx_shift_front().is_some() {
            self.tx_frame_corrupted_by_break = self.break_active();
            self.acia.frame_bits()
        } else {
            self.tx_frame_corrupted_by_break = false;
            0
        };
        Some(byte)
    }

    pub(super) fn endpoint_tx_front(&self) -> Option<u8> {
        self.wire_tx.front().copied()
    }

    pub(super) fn timing_is_quiet(&self) -> bool {
        self.rx_shift.is_none() && !self.rx_break_active && !self.acia.transmit_busy()
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
                self.tx_frame_corrupted_by_break = self.break_active();
            }
            return;
        }

        if self.break_active() {
            self.tx_frame_corrupted_by_break = true;
        }

        if self.tx_bits_remaining > 1 {
            self.tx_bits_remaining -= 1;
            return;
        }

        if self.tx_bits_remaining == 1 {
            self.tx_bits_remaining = 0;
            let corrupted = self.tx_frame_corrupted_by_break;
            if let Some(byte) = self.acia.complete_tx_shift() {
                if !corrupted {
                    self.wire_tx.push_back(byte);
                }
            }
            if self.acia.tx_shift_front().is_some() {
                // `complete_tx_shift` promotes a waiting TDR at exactly the
                // previous character boundary, so back-to-back characters have
                // no fictitious idle gap. A newly promoted frame starts corrupt
                // when the physical line is still being forced to BREAK.
                self.tx_bits_remaining = self.acia.frame_bits();
                self.tx_frame_corrupted_by_break = self.break_active();
            } else {
                self.tx_frame_corrupted_by_break = false;
            }
        }
    }

    fn receiver_bit_boundary(&mut self) {
        let Some((value, framing_error, parity_error)) = self.rx_shift else {
            self.start_break_frame_if_idle();
            return;
        };

        if self.rx_bits_remaining > 1 {
            self.rx_bits_remaining -= 1;
            return;
        }

        if self.rx_bits_remaining == 1 {
            let from_break = self.rx_shift_from_break;
            self.rx_bits_remaining = 0;
            self.rx_shift = None;
            self.rx_shift_from_break = false;
            self.acia.receive_character(
                value,
                framing_error || self.rx_break_active,
                parity_error || (self.rx_break_active && self.break_parity_error()),
            );
            if from_break || self.rx_break_active {
                self.start_break_frame_if_idle();
            }
        }
    }

    /// Return the canonical chassis T-state count until the next effective
    /// MC6850 clock boundary. The external MITS tap still free-runs continuously,
    /// but idle ports and intermediate /16 or /64 tap pulses cannot affect UART
    /// state and therefore do not need to interrupt a CPU execution window.
    pub(super) fn t_states_until_next_clock_boundary(&self, cpu_clock_hz: u32) -> Option<u64> {
        if cpu_clock_hz == 0 || self.timing_is_quiet() {
            return None;
        }
        let divider = u64::from(self.clock_divider()?);
        let numerator_per_t_state = u64::from(self.baud_tap.baud()) * 16;
        if numerator_per_t_state == 0 {
            return None;
        }

        let threshold = u128::from(cpu_clock_hz);
        let pulses_needed = divider.saturating_sub(u64::from(self.divider_phase)).max(1);
        let required_numerator = u128::from(pulses_needed)
            .saturating_mul(threshold)
            .saturating_sub(u128::from(self.tap_phase_numerator));
        let numerator_per_t_state = u128::from(numerator_per_t_state);
        Some(
            ((required_numerator.saturating_add(numerator_per_t_state - 1) / numerator_per_t_state)
                .max(1)
                .min(u128::from(u64::MAX))) as u64,
        )
    }

    /// Advance this card channel by elapsed Altair CPU-clock T-states. The MITS
    /// baud-generator tap is a free-running external oscillator at 16x the
    /// labelled rate. Its fractional phase is retained independently, then the
    /// MC6850's /1, /16 or /64 counter decides which tap pulses reach the UART.
    pub(super) fn advance_t_states(&mut self, t_states: u64, cpu_clock_hz: u32) {
        if t_states == 0 || cpu_clock_hz == 0 {
            return;
        }

        let numerator_per_t_state = u64::from(self.baud_tap.baud()) * 16;
        if numerator_per_t_state == 0 {
            return;
        }
        let threshold = u64::from(cpu_clock_hz);
        let total = self
            .tap_phase_numerator
            .saturating_add(t_states.saturating_mul(numerator_per_t_state));
        let tap_pulses = total / threshold;
        self.tap_phase_numerator = total % threshold;

        let Some(divider) = self.clock_divider() else {
            self.divider_phase = 0;
            return;
        };
        let divider = u64::from(divider);
        let divided_total = u64::from(self.divider_phase).saturating_add(tap_pulses);
        let boundaries = divided_total / divider;
        self.divider_phase = (divided_total % divider) as u8;

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
    fn modem_control_pins_follow_mc6850_transmitter_control_bits() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x11);
        assert!(!port.rts_high());
        assert!(!port.break_active());
        assert!(!port.cts_high());
        assert!(!port.dcd_high());

        port.write_control(0x51);
        assert!(
            port.rts_high(),
            "MITS 121-octal/51h reader-control value must drive RTS physically HIGH"
        );
        assert!(!port.break_active());

        port.write_control(0x71);
        assert!(!port.rts_high(), "BREAK encoding drives RTS LOW");
        assert!(port.break_active());
    }

    #[test]
    fn external_cts_and_dcd_levels_reach_the_acia_status_logic() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x95); // RX IRQ enabled
        port.set_cts_high(true);
        assert!(port.cts_high());
        assert_eq!(port.peek_status() & 0x0a, 0x08);
        port.set_cts_high(false);
        assert_eq!(port.peek_status() & 0x08, 0);

        port.set_dcd_high(true);
        assert!(port.dcd_high());
        assert_eq!(port.peek_status() & 0x84, 0x84);
        port.set_dcd_high(false);
        assert!(!port.dcd_high());
        assert_eq!(
            port.peek_status() & 0x84,
            0x84,
            "DCD transition remains latched until status/data clear sequence"
        );
        let _ = port.read_status();
        let _ = port.read_data();
        assert_eq!(port.peek_status() & 0x84, 0);
    }

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
    fn scheduler_deadline_skips_intermediate_divide_16_tap_pulses() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), None);
        port.write_control(0x15); // /16
        port.write_data(b'A');
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), Some(209));
        port.advance_t_states(208, TWO_MHZ);
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), Some(1));
        port.advance_t_states(1, TWO_MHZ);
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), Some(208));
    }

    #[test]
    fn scheduler_deadline_tracks_divide_64_effective_boundary() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x16); // /64
        port.write_data(b'A');
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), Some(834));
        port.advance_t_states(833, TWO_MHZ);
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), Some(1));
    }

    #[test]
    fn idle_port_has_no_scheduler_deadline_but_external_phase_keeps_running() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        assert!(port.timing_is_quiet());
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), None);
        port.advance_t_states(7, TWO_MHZ);
        let phase = port.tap_phase_numerator;
        assert_ne!(phase, 0);
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), None);
        port.write_data(b'A');
        assert!(port.t_states_until_next_clock_boundary(TWO_MHZ).is_some());
    }

    #[test]
    fn acia_master_reset_does_not_reset_the_external_baud_generator_phase() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.advance_t_states(7, TWO_MHZ);
        let phase = port.tap_phase_numerator;
        port.write_control(0x03);
        assert_eq!(port.tap_phase_numerator, phase);
        assert_eq!(port.divider_phase, 0);
        assert_eq!(port.t_states_until_next_clock_boundary(TWO_MHZ), None);
    }

    #[test]
    fn changing_divider_restarts_only_the_internal_divide_epoch() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x15); // /16
        port.write_data(b'A');
        port.advance_t_states(100, TWO_MHZ);
        assert_ne!(port.divider_phase, 0);
        let external_phase = port.tap_phase_numerator;
        port.write_control(0x16); // /64
        assert_eq!(port.divider_phase, 0);
        assert_eq!(port.tap_phase_numerator, external_phase);
    }

    #[test]
    fn endpoint_drain_does_not_control_acia_tdre_or_shift_completion() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x35); // 8N1, TX-empty IRQ, /16
        port.write_data(b'A');
        port.advance_t_states(2_292, TWO_MHZ);
        assert_eq!(port.endpoint_tx_front(), Some(b'A'));
        assert!(
            port.interrupt_request(),
            "TDR is empty regardless of endpoint presentation delay"
        );

        port.advance_t_states(20_000, TWO_MHZ);
        assert_eq!(port.endpoint_tx_front(), Some(b'A'));
        assert!(port.interrupt_request());
        assert_eq!(port.endpoint_tx_complete(), Some(b'A'));
    }

    #[test]
    fn break_overrides_wire_without_freezing_tdr_tsr_or_tdre() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x75); // /16, 8N1, CR6:CR5=11 BREAK
        port.write_data(b'B');
        assert_eq!(
            port.peek_status() & 0x02,
            0,
            "TDR is full before the first transmitter boundary"
        );

        port.advance_t_states(209, TWO_MHZ);
        assert_eq!(
            port.peek_status() & 0x02,
            0x02,
            "BREAK must not inhibit the normal TDR->TSR transfer"
        );
        assert!(port.endpoint_tx_pending_or_hardware_busy());

        port.advance_t_states(2_100, TWO_MHZ);
        assert!(
            !port.endpoint_tx_pending_or_hardware_busy(),
            "TSR continues clocking internally while BREAK holds TxD spacing"
        );
        assert_eq!(
            port.endpoint_tx_front(),
            None,
            "a frame transmitted under BREAK is not a valid downstream byte"
        );
    }

    #[test]
    fn break_asserted_mid_frame_irreversibly_corrupts_only_that_frame() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud9600);
        port.write_control(0x15); // /16, 8N1, normal TxD
        port.write_data(b'A');
        port.advance_t_states(209, TWO_MHZ); // TDR -> TSR, frame begins
        assert_eq!(port.peek_status() & 0x02, 0x02);

        port.advance_t_states(417, TWO_MHZ); // part-way through the frame
        port.write_control(0x75); // BREAK immediately overrides TxD
        assert!(port.break_active());
        port.write_control(0x15); // release before the nominal frame completes
        assert!(!port.break_active());

        port.advance_t_states(2_000, TWO_MHZ);
        assert_eq!(
            port.endpoint_tx_front(),
            None,
            "releasing BREAK cannot repair an already corrupted frame"
        );

        port.write_data(b'Z');
        port.advance_t_states(2_300, TWO_MHZ);
        assert_eq!(
            port.endpoint_tx_front(),
            Some(b'Z'),
            "the next complete post-BREAK frame is valid again"
        );
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
    fn held_receive_break_produces_zero_with_framing_error_and_can_overrun() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x15); // /16, 8N1 => 110 baud
        port.set_receive_break(true);
        assert!(!port.receive_line_idle());

        port.advance_t_states(181_819, TWO_MHZ);
        assert_eq!(port.peek_data(), 0x00);
        assert_eq!(
            port.peek_status() & 0x11,
            0x11,
            "BREAK frame reaches RDR as zero with FE"
        );
        assert!(
            !port.receive_line_idle(),
            "held BREAK immediately starts the next frame"
        );

        port.advance_t_states(181_819, TWO_MHZ);
        assert_eq!(
            port.peek_status() & 0x21,
            0x01,
            "MC6850 overrun remains delayed while old RDR is unread"
        );
        assert_eq!(port.read_data(), 0x00);
        assert_eq!(
            port.peek_status() & 0x21,
            0x21,
            "reading the valid BREAK character exposes delayed OVRN"
        );

        port.set_receive_break(false);
        assert!(port.receive_line_idle());
    }

    #[test]
    fn short_receive_break_release_aborts_incomplete_break_frame() {
        let mut port = TwoSioPort::new(TwoSioBaudTap::Baud110);
        port.write_control(0x15);
        port.set_receive_break(true);
        port.advance_t_states(40_000, TWO_MHZ);
        port.set_receive_break(false);
        assert!(port.receive_line_idle());
        assert_eq!(port.peek_status() & 0x11, 0);
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
        assert!(
            port.receive_line_idle(),
            "RDRF must not masquerade as raw line busy"
        );

        // Exercise the card primitive directly: a real source can begin its next
        // frame even while software has left the previous RDR unread.
        port.queue_received_character(b'B');
        port.advance_t_states(181_819, TWO_MHZ);
        assert_eq!(
            port.peek_status() & 0x21,
            0x01,
            "overrun remains latent until valid RDR is read"
        );
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
