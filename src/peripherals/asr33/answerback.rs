use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MESSAGE: &[u8] = b"\r\nRUSTAIR ASR33";

/// Mechanical Model 33 answer-back drum.
///
/// The component owns the fixed coded message, pending characters and the next
/// mechanical transmit instant. The application only decides when HERE IS/ENQ
/// triggers the drum and forwards due bytes to the active serial line.
#[derive(Default)]
pub struct Answerback {
    queue: VecDeque<u8>,
    next_at: Option<Instant>,
}

impl Answerback {
    /// Start one drum revolution. A second trigger while the mechanism is
    /// already active is ignored, matching the physical device.
    pub fn trigger(&mut self, now: Instant) {
        if self.queue.is_empty() {
            self.queue.extend(MESSAGE.iter().copied());
            self.next_at = Some(now);
        }
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.next_at = None;
    }

    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// Remaining time until the next byte may be transmitted. Returns `None`
    /// when no byte is pending or when the next byte is already due.
    pub fn time_until_next(&self, now: Instant) -> Option<Duration> {
        let next_at = self.next_at?;
        (now < next_at).then(|| next_at.duration_since(now))
    }

    /// Remove one due byte and schedule the next drum character.
    pub fn take_due(&mut self, now: Instant, char_time: Duration) -> Option<u8> {
        if self.next_at.is_some_and(|next_at| now < next_at) {
            return None;
        }

        let byte = self.queue.pop_front()?;
        self.next_at = if self.queue.is_empty() {
            None
        } else {
            Some(now + char_time)
        };
        Some(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answerback_starts_with_required_cr_lf_and_is_paced() {
        let now = Instant::now();
        let char_time = Duration::from_millis(100);
        let mut answerback = Answerback::default();

        answerback.trigger(now);
        assert_eq!(answerback.take_due(now, char_time), Some(b'\r'));
        assert!(answerback.pending());
        assert_eq!(answerback.take_due(now, char_time), None);
        assert_eq!(answerback.take_due(now + char_time, char_time), Some(b'\n'));
    }

    #[test]
    fn repeated_trigger_does_not_restart_active_drum() {
        let now = Instant::now();
        let char_time = Duration::from_millis(100);
        let mut answerback = Answerback::default();

        answerback.trigger(now);
        assert_eq!(answerback.take_due(now, char_time), Some(b'\r'));
        answerback.trigger(now);

        // A restart here would make CR immediately due again. The active drum
        // instead keeps waiting for the original second character.
        assert_eq!(answerback.take_due(now, char_time), None);
    }

    #[test]
    fn clear_stops_the_mechanism() {
        let now = Instant::now();
        let mut answerback = Answerback::default();
        answerback.trigger(now);
        answerback.clear();
        assert!(!answerback.pending());
        assert_eq!(answerback.take_due(now, Duration::from_millis(100)), None);
    }
}
