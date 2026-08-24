use std::time::{Duration, Instant};

use crate::config::TerminalDuplex;
use crate::peripherals::asr33::{Answerback, MechanicsState};

pub(super) struct Asr33State {
    pub(super) window_open: bool,
    pub(super) tx_started: Option<Instant>,
    pub(super) duplex: TerminalDuplex,
    pub(super) answerback: Answerback,
    pub(super) last_tape_tick: Instant,
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
            last_tape_tick: now,
            power_flash_until: None,
            mechanics: MechanicsState::new(),
            keyboard: Asr33KeyboardState::new(now),
        }
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
