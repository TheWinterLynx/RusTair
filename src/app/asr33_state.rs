use std::time::{Duration, Instant};

use crate::config::TerminalDuplex;
use crate::peripherals::asr33::{Answerback, MechanicsState};

/// Physical reader motor control wiring.
///
/// `Manual` models the operator starting/stopping the reader locally. `Mits88TyaRts`
/// models the MITS 88-TYA Reader Control option where the 88-2SIO MC6850 RTS
/// output drives ReaderRun+: physical RTS HIGH runs the reader, LOW stops it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ReaderControlMode {
    #[default]
    Manual,
    Mits88TyaRts,
}

impl ReaderControlMode {
    pub(super) const ALL: [Self; 2] = [Self::Manual, Self::Mits88TyaRts];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual reader switch",
            Self::Mits88TyaRts => "MITS 88-TYA — 88-2SIO RTS",
        }
    }

    /// Whether the reader motor is electrically commanded to run. In RTS mode
    /// absence of an MC6850 RTS pin means the optional Reader Control wiring has
    /// no valid source and therefore cannot energize ReaderRun+.
    pub(super) const fn effective_running(self, manual_running: bool, rts_high: Option<bool>) -> bool {
        match self {
            Self::Manual => manual_running,
            Self::Mits88TyaRts => matches!(rts_high, Some(true)),
        }
    }
}

/// Independent paper-tape transport speed. Historical Model 33 media advances
/// at the same 10 character/s cadence as its 110-baud distributor; accelerated
/// modes intentionally remove only the mechanical delay, not UART/CPU timing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum TapeTransportSpeed {
    #[default]
    Historical1x,
    X5,
    X10,
    Unlimited,
}

impl TapeTransportSpeed {
    pub(super) const ALL: [Self; 4] = [Self::Historical1x, Self::X5, Self::X10, Self::Unlimited];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Historical1x => "1x — 10 cps (historical)",
            Self::X5 => "5x — 50 cps",
            Self::X10 => "10x — 100 cps",
            Self::Unlimited => "Unlimited",
        }
    }

    pub(super) const fn short_label(self) -> &'static str {
        match self {
            Self::Historical1x => "1x",
            Self::X5 => "5x",
            Self::X10 => "10x",
            Self::Unlimited => "MAX",
        }
    }

    pub(super) const fn char_time(self) -> Duration {
        match self {
            Self::Historical1x => Duration::from_millis(100),
            Self::X5 => Duration::from_millis(20),
            Self::X10 => Duration::from_millis(10),
            Self::Unlimited => Duration::ZERO,
        }
    }
}

/// Left-to-right channel order used only by the mini paper-tape visualization.
/// DEC's contemporary ASR-33 documentation numbers tape bits right-to-left
/// 1..8, so a normal illustrated view reads channel 8 down to channel 1 from
/// left to right. Reversing this never changes the actual byte delivered by the
/// reader; it is purely an operator display preference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum TapeBitOrder {
    #[default]
    Historical8To1,
    Reversed1To8,
}

impl TapeBitOrder {
    pub(super) const fn toggle(self) -> Self {
        match self {
            Self::Historical8To1 => Self::Reversed1To8,
            Self::Reversed1To8 => Self::Historical8To1,
        }
    }
}

pub(super) struct Asr33State {
    pub(super) window_open: bool,
    pub(super) tx_started: Option<Instant>,
    pub(super) duplex: TerminalDuplex,
    pub(super) answerback: Answerback,
    /// Local operator reader switch. In 88-TYA RTS mode this state is retained
    /// but ignored electrically; the guest's MC6850 RTS pin becomes authority.
    pub(super) reader_running: bool,
    pub(super) reader_control: ReaderControlMode,
    pub(super) reader_speed: TapeTransportSpeed,
    pub(super) last_reader_tick: Instant,
    /// Last physical tape character advanced past the reader and offered to the
    /// selected UART. Keeping this in the presentation/controller state lets
    /// the UI show what is currently waiting in RX without mutating tape media.
    pub(super) last_reader_byte: Option<u8>,
    pub(super) tape_bit_order: TapeBitOrder,
    pub(super) punch_running: bool,
    pub(super) punch_speed: TapeTransportSpeed,
    pub(super) last_punch_tick: Instant,
    pub(super) last_media_sound: Instant,
    pub(super) power_flash_until: Option<Instant>,
    pub(super) mechanics: MechanicsState,
    pub(super) keyboard: Asr33KeyboardState,
}

impl Asr33State {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            window_open: false,
            tx_started: None,
            duplex: TerminalDuplex::default(),
            answerback: Answerback::default(),
            reader_running: false,
            reader_control: ReaderControlMode::default(),
            reader_speed: TapeTransportSpeed::default(),
            last_reader_tick: now,
            last_reader_byte: None,
            tape_bit_order: TapeBitOrder::default(),
            punch_running: false,
            punch_speed: TapeTransportSpeed::default(),
            last_punch_tick: now,
            last_media_sound: now.checked_sub(Duration::from_secs(1)).unwrap_or(now),
            power_flash_until: None,
            mechanics: MechanicsState::new(),
            keyboard: Asr33KeyboardState::new(now),
        }
    }

    /// Keep accelerated media from spawning an unbounded number of detached
    /// sound sinks. Mechanical audio is sampled at at most 20 events/s.
    pub(super) fn media_sound_due(&mut self, now: Instant) -> bool {
        if now.duration_since(self.last_media_sound) < Duration::from_millis(50) {
            return false;
        }
        self.last_media_sound = now;
        true
    }
}

pub(super) struct Asr33KeyboardState {
    pub(super) animated_key: Option<usize>,
    pub(super) pressed_key: Option<usize>,
    pub(super) auto_release_at: Option<Instant>,
    pub(super) displacement: f32,
    pub(super) anim_tick: Instant,
    distributor_ready_at: Instant,
}

impl Asr33KeyboardState {
    fn new(now: Instant) -> Self {
        Self {
            animated_key: None,
            pressed_key: None,
            auto_release_at: None,
            displacement: 0.0,
            anim_tick: now,
            distributor_ready_at: now,
        }
    }

    /// The Model 33 keyboard trips a mechanical distributor that serializes one
    /// character at the configured line rate. Until that distributor/reset
    /// cycle finishes, another primary key cannot start a second character.
    pub(super) fn try_begin_transmission(
        &mut self,
        now: Instant,
        char_time: Duration,
    ) -> bool {
        if char_time.is_zero() {
            self.distributor_ready_at = now;
            return true;
        }
        if now < self.distributor_ready_at {
            return false;
        }
        self.distributor_ready_at = now + char_time;
        true
    }

    pub(super) fn reset_distributor(&mut self, now: Instant) {
        self.distributor_ready_at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_control_keeps_manual_and_88_tya_rts_wiring_distinct() {
        assert!(ReaderControlMode::Manual.effective_running(true, None));
        assert!(!ReaderControlMode::Manual.effective_running(false, Some(true)));
        assert!(!ReaderControlMode::Mits88TyaRts.effective_running(true, None));
        assert!(!ReaderControlMode::Mits88TyaRts.effective_running(true, Some(false)));
        assert!(ReaderControlMode::Mits88TyaRts.effective_running(false, Some(true)));
    }

    #[test]
    fn tape_transport_speeds_match_requested_character_rates() {
        assert_eq!(TapeTransportSpeed::Historical1x.char_time(), Duration::from_millis(100));
        assert_eq!(TapeTransportSpeed::X5.char_time(), Duration::from_millis(20));
        assert_eq!(TapeTransportSpeed::X10.char_time(), Duration::from_millis(10));
        assert_eq!(TapeTransportSpeed::Unlimited.char_time(), Duration::ZERO);
    }

    #[test]
    fn reader_byte_display_starts_empty_and_uses_historical_channel_order() {
        let state = Asr33State::new(Instant::now());
        assert_eq!(state.last_reader_byte, None);
        assert_eq!(state.tape_bit_order, TapeBitOrder::Historical8To1);
        assert_eq!(state.reader_control, ReaderControlMode::Manual);
    }

    #[test]
    fn tape_bit_order_toggle_round_trips() {
        let historical = TapeBitOrder::Historical8To1;
        assert_eq!(historical.toggle(), TapeBitOrder::Reversed1To8);
        assert_eq!(historical.toggle().toggle(), historical);
    }

    #[test]
    fn keyboard_distributor_enforces_one_character_time() {
        let start = Instant::now();
        let char_time = Duration::from_millis(100);
        let mut keyboard = Asr33KeyboardState::new(start);

        assert!(keyboard.try_begin_transmission(start, char_time));
        assert!(!keyboard.try_begin_transmission(
            start + Duration::from_millis(99),
            char_time
        ));
        assert!(keyboard.try_begin_transmission(start + char_time, char_time));
    }

    #[test]
    fn instant_mode_has_no_keyboard_lockout() {
        let start = Instant::now();
        let mut keyboard = Asr33KeyboardState::new(start);

        assert!(keyboard.try_begin_transmission(start, Duration::ZERO));
        assert!(keyboard.try_begin_transmission(start, Duration::ZERO));
    }

    #[test]
    fn resetting_keyboard_distributor_releases_lockout() {
        let start = Instant::now();
        let mut keyboard = Asr33KeyboardState::new(start);
        assert!(keyboard.try_begin_transmission(start, Duration::from_millis(100)));
        assert!(!keyboard.try_begin_transmission(
            start + Duration::from_millis(1),
            Duration::from_millis(100)
        ));

        let reset_at = start + Duration::from_millis(1);
        keyboard.reset_distributor(reset_at);
        assert!(keyboard.try_begin_transmission(
            reset_at,
            Duration::from_millis(100)
        ));
    }
}
