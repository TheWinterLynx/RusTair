use std::time::Instant;

use crate::peripherals::asr33::Answerback;

pub(super) struct Asr33State {
    pub(super) window_open: bool,
    pub(super) tx_started: Option<Instant>,
    pub(super) answerback: Answerback,
    pub(super) last_tape_tick: Instant,
    pub(super) power_flash_until: Option<Instant>,
    pub(super) mechanics: Asr33MechanicsState,
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
            mechanics: Asr33MechanicsState::new(),
            keyboard: Asr33KeyboardState::new(now),
        }
    }
}

#[derive(Default)]
pub(super) struct Asr33MechanicsState {
    pub(super) print_head_raise_until: Option<Instant>,
    pub(super) print_head_impact_at: Option<Instant>,
    pub(super) print_head_auto_return_at: Option<Instant>,
    pub(super) print_head_glyph: u8,
    pub(super) print_head_carriage_return_until: Option<Instant>,
    pub(super) paper_feed_until: Option<Instant>,
}

impl Asr33MechanicsState {
    pub(super) fn new() -> Self {
        Self {
            print_head_glyph: b' ',
            ..Self::default()
        }
    }

    pub(super) fn clear_motion(&mut self) {
        self.print_head_raise_until = None;
        self.print_head_impact_at = None;
        self.print_head_auto_return_at = None;
        self.print_head_carriage_return_until = None;
        self.paper_feed_until = None;
    }

    pub(super) fn printing_active(&self) -> bool {
        self.print_head_impact_at.is_some()
            || self.print_head_auto_return_at.is_some()
            || self.print_head_raise_until.is_some()
            || self.print_head_carriage_return_until.is_some()
            || self.paper_feed_until.is_some()
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
