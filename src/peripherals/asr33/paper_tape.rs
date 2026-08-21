use std::collections::VecDeque;

/// Reader/punch state for the ASR-33 paper-tape mechanisms.
#[derive(Default)]
pub(super) struct PaperTape {
    input: VecDeque<u8>,
    output: Vec<u8>,
    capture: bool,
}

impl PaperTape {
    pub(super) fn load(&mut self, bytes: &[u8]) {
        self.input.clear();
        self.input
            .extend(bytes.iter().copied().map(|byte| byte.to_ascii_uppercase()));
    }

    pub(super) fn next_byte(&mut self) -> Option<u8> {
        self.input.pop_front().map(|byte| byte & 0x7f)
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
    fn reader_uppercases_and_emits_seven_bit_data() {
        let mut tape = PaperTape::default();
        tape.load(&[b'a', 0xff]);
        assert_eq!(tape.next_byte(), Some(b'A'));
        assert_eq!(tape.next_byte(), Some(0x7f));
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
