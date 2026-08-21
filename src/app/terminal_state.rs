use std::collections::VecDeque;
use std::time::{Duration, Instant};

const TERMINAL_MAX_CHARS: usize = 200_000;

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
    pub(super) speed: TerminalSpeed,
    pub(super) tx_started: Option<Instant>,
    input_queue: VecDeque<u8>,
    rx_next_at: Option<Instant>,
    last_was_cr: bool,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            window_open: false,
            output: String::new(),
            command: String::new(),
            program: String::new(),
            uppercase: true,
            speed: TerminalSpeed::Baud9600,
            tx_started: None,
            input_queue: VecDeque::new(),
            rx_next_at: None,
            last_was_cr: false,
        }
    }
}

impl TerminalState {
    /// Apply one seven-bit character received from the guest to the terminal
    /// display model. CR/LF are intentionally coalesced for a conventional
    /// text-terminal view; the ASR-33 paper model remains mechanically exact.
    pub(super) fn receive_byte(&mut self, byte: u8) {
        match byte & 0x7f {
            b'\r' => {
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.last_was_cr = true;
            }
            b'\n' => {
                if !self.last_was_cr && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.last_was_cr = false;
            }
            0x08 | 0x7f => {
                if !self.output.ends_with('\n') {
                    self.output.pop();
                }
                self.last_was_cr = false;
            }
            b'\t' => {
                self.output.push('\t');
                self.last_was_cr = false;
            }
            0x20..=0x7e => {
                self.output.push((byte & 0x7f) as char);
                self.last_was_cr = false;
            }
            _ => self.last_was_cr = false,
        }

        if self.output.len() > TERMINAL_MAX_CHARS {
            let excess = self.output.len() - TERMINAL_MAX_CHARS;
            let cut = self.output[excess..]
                .find('\n')
                .map(|offset| excess + offset + 1)
                .unwrap_or(excess);
            self.output.drain(..cut);
        }
    }

    pub(super) fn clear_output(&mut self) {
        self.output.clear();
        self.last_was_cr = false;
    }

    /// Convert host text to the byte stream expected by the Altair terminal
    /// interface and append it to the paced host-side input queue.
    pub(super) fn enqueue_text(
        &mut self,
        text: &str,
        append_final_cr: bool,
        now: Instant,
    ) -> usize {
        let mut bytes = Vec::with_capacity(text.len() + 1);
        let mut previous_was_cr = false;

        for byte in text.bytes() {
            match byte {
                b'\r' => {
                    bytes.push(b'\r');
                    previous_was_cr = true;
                }
                b'\n' => {
                    if !previous_was_cr {
                        bytes.push(b'\r');
                    }
                    previous_was_cr = false;
                }
                0x08 | 0x09 | 0x1b | 0x20..=0x7e => {
                    bytes.push(if self.uppercase {
                        byte.to_ascii_uppercase()
                    } else {
                        byte
                    });
                    previous_was_cr = false;
                }
                _ => previous_was_cr = false,
            }
        }

        if append_final_cr && bytes.last().copied() != Some(b'\r') {
            bytes.push(b'\r');
        }

        let count = bytes.len();
        if count > 0 {
            let was_empty = self.input_queue.is_empty();
            self.input_queue.extend(bytes);
            if was_empty {
                self.rx_next_at = Some(now);
            }
        }
        count
    }

    /// Manual controls take priority over pasted/program input.
    pub(super) fn queue_control(&mut self, byte: u8, now: Instant) {
        self.input_queue.push_front(byte & 0x7f);
        self.rx_next_at = Some(now);
    }

    pub(super) fn input_pending_len(&self) -> usize {
        self.input_queue.len()
    }

    pub(super) fn input_is_empty(&self) -> bool {
        self.input_queue.is_empty()
    }

    pub(super) fn clear_input_timing_if_empty(&mut self) {
        if self.input_queue.is_empty() {
            self.rx_next_at = None;
        }
    }

    pub(super) fn input_due_in(&self, now: Instant) -> Duration {
        self.rx_next_at
            .and_then(|due| due.checked_duration_since(now))
            .unwrap_or(Duration::ZERO)
    }

    /// Release one due byte and arm the following byte at the selected speed.
    pub(super) fn take_due_input(&mut self, now: Instant) -> Option<(u8, Duration)> {
        if self.input_due_in(now) != Duration::ZERO {
            return None;
        }

        let byte = self.input_queue.pop_front()?;
        if self.input_queue.is_empty() {
            self.rx_next_at = None;
            Some((byte, Duration::ZERO))
        } else {
            let delay = self.speed.char_time();
            self.rx_next_at = Some(now + delay);
            Some((byte, delay))
        }
    }

    pub(super) fn restart_input_pacing(&mut self, now: Instant) {
        if !self.input_queue.is_empty() {
            self.rx_next_at = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_coalesces_crlf_and_handles_backspace() {
        let mut terminal = TerminalState::default();
        terminal.receive_byte(b'A');
        terminal.receive_byte(b'B');
        terminal.receive_byte(0x08);
        terminal.receive_byte(b'C');
        terminal.receive_byte(b'\r');
        terminal.receive_byte(b'\n');
        terminal.receive_byte(b'D');
        assert_eq!(terminal.output, "AC\nD");
    }

    #[test]
    fn enqueue_normalizes_newlines_and_uppercases_input() {
        let mut terminal = TerminalState::default();
        let now = Instant::now();
        assert_eq!(terminal.enqueue_text("a\nb\r\nc", true, now), 6);

        let mut bytes = Vec::new();
        while let Some((byte, _)) = terminal.take_due_input(now + Duration::from_secs(1)) {
            bytes.push(byte);
        }
        assert_eq!(bytes, b"A\rB\rC\r");
    }

    #[test]
    fn control_input_takes_priority() {
        let mut terminal = TerminalState::default();
        let now = Instant::now();
        terminal.enqueue_text("ABC", false, now);
        terminal.queue_control(0x03, now);
        assert_eq!(terminal.take_due_input(now).map(|item| item.0), Some(0x03));
    }
}
