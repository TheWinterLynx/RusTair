use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TerminalSpeed {
    Instant,
    Baud300,
    Baud1200,
    Baud2400,
    Baud9600,
}

impl TerminalSpeed {
    pub(super) const ALL: [Self; 5] = [
        Self::Instant,
        Self::Baud300,
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud9600,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Instant => "Instant",
            Self::Baud300 => "300 baud",
            Self::Baud1200 => "1200 baud",
            Self::Baud2400 => "2400 baud",
            Self::Baud9600 => "9600 baud",
        }
    }

    pub(super) fn char_time(self) -> Duration {
        match self {
            Self::Instant => Duration::ZERO,
            Self::Baud300 => Duration::from_micros(33_333),
            Self::Baud1200 => Duration::from_micros(8_333),
            Self::Baud2400 => Duration::from_micros(4_167),
            Self::Baud9600 => Duration::from_micros(1_042),
        }
    }
}

pub(super) struct TerminalState {
    pub(super) window_open: bool,
    pub(super) output: String,
    pub(super) command: String,
    pub(super) program: String,
    pub(super) uppercase: bool,
    pub(super) last_was_cr: bool,
    pub(super) speed: TerminalSpeed,
    pub(super) input_queue: VecDeque<u8>,
    pub(super) rx_next_at: Option<Instant>,
    pub(super) tx_started: Option<Instant>,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            window_open: false,
            output: String::new(),
            command: String::new(),
            program: String::new(),
            uppercase: true,
            last_was_cr: false,
            speed: TerminalSpeed::Baud9600,
            input_queue: VecDeque::new(),
            rx_next_at: None,
            tx_started: None,
        }
    }
}
