use std::time::Instant;

use crate::peripherals::asr33::{Answerback, MechanicsState};

pub(super) struct Asr33State {
    pub(super) window_open: bool,
    pub(super) tx_started: Option<Instant>,
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
}

impl Asr33KeyboardState {
    fn new(now: Instant) -> Self {
        Self {
            animated_key: None,
            pressed_key: None,
            auto_release_at: None,
            displacement: 0.0,
            anim_tick: now,
        }
    }
}
