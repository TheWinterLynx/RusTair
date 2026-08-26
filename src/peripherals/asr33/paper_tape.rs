use std::collections::VecDeque;

/// Reader/punch state for the ASR-33 paper-tape mechanisms.
///
/// The reader keeps the mounted image intact and advances a cursor so a real
/// REWIND operation is possible. The punch has a small mechanical queue: bytes
/// accepted while the punch is running are turned into holes later by the app's
/// transport clock. That lets the reader and punch have independent 1x/5x/10x
/// and unlimited speeds without changing CPU/UART timing.
#[derive(Default)]
pub(super) struct PaperTape {
    input: Vec<u8>,
    input_position: usize,
    output: Vec<u8>,
    punch_pending: VecDeque<u8>,
    capture: bool,
    punch_running: bool,
}

impl PaperTape {
    /// Mount the physical tape image byte-for-byte and park it at the leader.
    /// Paper tape is binary media; no ASCII transformation belongs here.
    pub(super) fn load(&mut self, bytes: &[u8]) {
        self.input.clear();
        self.input.extend_from_slice(bytes);
        self.input_position = 0;
    }

    pub(super) fn next_byte(&mut self) -> Option<u8> {
        let byte = self.input.get(self.input_position).copied()?;
        self.input_position += 1;
        Some(byte)
    }

    pub(super) fn input_len(&self) -> usize {
        self.input.len().saturating_sub(self.input_position)
    }

    pub(super) fn input_total_len(&self) -> usize {
        self.input.len()
    }

    pub(super) fn input_position(&self) -> usize {
        self.input_position.min(self.input.len())
    }

    pub(super) fn input_pending(&self) -> bool {
        self.input_position < self.input.len()
    }

    pub(super) fn rewind_input(&mut self) {
        self.input_position = 0;
    }

    pub(super) fn eject_input(&mut self) {
        self.input.clear();
        self.input_position = 0;
    }

    /// Put a fresh blank tape in the punch without starting the mechanism.
    pub(super) fn prepare_capture(&mut self) {
        self.output.clear();
        self.punch_pending.clear();
        self.capture = true;
        self.punch_running = false;
    }

    /// Historical convenience API: mount a blank tape and immediately run it.
    pub(super) fn begin_capture(&mut self) {
        self.prepare_capture();
        self.punch_running = true;
    }

    pub(super) fn resume_capture(&mut self) {
        if self.capture {
            self.punch_running = true;
        }
    }

    pub(super) fn pause_capture(&mut self) {
        self.punch_running = false;
    }

    /// Finish the physical tape. Bytes already accepted by the punch are
    /// committed before eject/save so a final mechanical queue cannot vanish.
    pub(super) fn finish_capture(&mut self) {
        self.flush_punch_pending();
        self.capture = false;
        self.punch_running = false;
    }

    pub(super) fn capture_enabled(&self) -> bool {
        self.capture
    }

    pub(super) fn capture_running(&self) -> bool {
        self.capture && self.punch_running
    }

    /// Accept one character into the mechanical punch distributor. Keep all
    /// eight data bits: an ASR-33 punch can transparently punch 8-level tape
    /// even though the printer itself displays 7-bit ASCII.
    pub(super) fn record(&mut self, byte: u8) {
        if self.capture_running() {
            self.punch_pending.push_back(byte);
        }
    }

    /// Advance the punch by one physical character position.
    pub(super) fn punch_next(&mut self) -> Option<u8> {
        let byte = self.punch_pending.pop_front()?;
        self.output.push(byte);
        Some(byte)
    }

    pub(super) fn flush_punch_pending(&mut self) {
        self.output.extend(self.punch_pending.drain(..));
    }

    pub(super) fn punch_pending_len(&self) -> usize {
        self.punch_pending.len()
    }

    pub(super) fn output(&self) -> &[u8] {
        &self.output
    }

    pub(super) fn output_len(&self) -> usize {
        self.output.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_preserves_binary_tape_bytes_exactly_and_rewinds() {
        let mut tape = PaperTape::default();
        tape.load(&[b'a', 0x00, 0x80, 0xff]);
        assert_eq!(tape.input_total_len(), 4);
        assert_eq!(tape.next_byte(), Some(b'a'));
        assert_eq!(tape.next_byte(), Some(0x00));
        assert_eq!(tape.input_position(), 2);
        assert_eq!(tape.input_len(), 2);
        tape.rewind_input();
        assert_eq!(tape.input_position(), 0);
        assert_eq!(tape.next_byte(), Some(b'a'));
    }

    #[test]
    fn prepared_punch_waits_for_run_and_preserves_eight_bits() {
        let mut tape = PaperTape::default();
        tape.prepare_capture();
        tape.record(0xff);
        assert_eq!(tape.punch_pending_len(), 0);

        tape.resume_capture();
        tape.record(0xff);
        assert_eq!(tape.punch_pending_len(), 1);
        assert_eq!(tape.punch_next(), Some(0xff));
        assert_eq!(tape.output(), &[0xff]);

        tape.pause_capture();
        tape.record(b'C');
        assert_eq!(tape.output(), &[0xff]);
    }

    #[test]
    fn finish_flushes_already_accepted_punch_bytes() {
        let mut tape = PaperTape::default();
        tape.begin_capture();
        tape.record(b'A');
        tape.record(b'B');
        tape.finish_capture();
        assert_eq!(tape.output(), b"AB");
        assert!(!tape.capture_enabled());
        assert!(!tape.capture_running());
    }
}
