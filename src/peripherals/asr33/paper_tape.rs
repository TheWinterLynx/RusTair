use std::collections::VecDeque;

/// Reader/punch state for the ASR-33 paper-tape mechanisms.
#[derive(Default)]
pub(super) struct PaperTape {
    input: VecDeque<u8>,
    output: Vec<u8>,
    capture: bool,
}

impl PaperTape {
    /// Load the physical tape image byte-for-byte.
    ///
    /// Paper tape is binary media. In particular, Altair loaders and BASIC
    /// distribution tapes may use all eight hole positions, so neither ASCII
    /// case conversion nor 7-bit masking belongs in the media layer.
    pub(super) fn load(&mut self, bytes: &[u8]) {
        self.input.clear();
        self.input.extend(bytes.iter().copied());
    }

    pub(super) fn next_byte(&mut self) -> Option<u8> {
        self.input.pop_front()
    }

    pub(super) fn input_len(&self) -> usize {
        self.input.len()
    }

    pub(super) fn input_pending(&self) -> bool {
        !self.input.is_empty()
    }

    pub(super) fn begin_capture(&mut self) {
        self.output.clear();
        self.capture = true;
    }

    pub(super) fn finish_capture(&mut self) {
        self.capture = false;
    }

    pub(super) fn capture_enabled(&self) -> bool {
        self.capture
    }

    pub(super) fn record(&mut self, byte: u8) {
        if self.capture {
            self.output.push(byte & 0x7f);
        }
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
    fn reader_preserves_binary_tape_bytes_exactly() {
        let mut tape = PaperTape::default();
        tape.load(&[b'a', 0x00, 0x80, 0xff]);
        assert_eq!(tape.next_byte(), Some(b'a'));
        assert_eq!(tape.next_byte(), Some(0x00));
        assert_eq!(tape.next_byte(), Some(0x80));
        assert_eq!(tape.next_byte(), Some(0xff));
        assert_eq!(tape.next_byte(), None);
    }

    #[test]
    fn punch_only_records_while_capture_is_enabled() {
        let mut tape = PaperTape::default();
        tape.record(b'A');
        assert!(tape.output().is_empty());

        tape.begin_capture();
        tape.record(b'B');
        tape.finish_capture();
        tape.record(b'C');
        assert_eq!(tape.output(), b"B");
    }
}
