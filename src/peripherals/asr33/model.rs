use super::paper_tape::PaperTape;

pub const IMAGE_W: f32 = 3008.0;
pub const IMAGE_H: f32 = 2983.0;
pub const PRINT_LEFT: f32 = IMAGE_W * 0.25;
pub const PRINT_TOP: f32 = IMAGE_H * 0.34;
pub const PRINT_HEAD_TOP: f32 = IMAGE_H * 0.33;
pub const PRINTABLE_WIDTH: f32 = 1500.0;
pub const PAPER_COLUMNS: usize = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Line,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrintEvent {
    Printable(u8),
    CarriageReturn,
    LineFeed,
    AutomaticReturn,
    Bell,
}

/// Return the Model 33 typewheel's 16-position rotational slot and one of its
/// four vertical character levels.
pub fn typewheel_position(byte: u8) -> (u8, u8) {
    let byte = if byte.is_ascii_lowercase() {
        byte.to_ascii_uppercase()
    } else {
        byte
    };
    let byte = if (0x20..=0x5f).contains(&byte) {
        byte
    } else {
        b'?'
    };
    let index = byte - 0x20;
    (index & 0x0f, index >> 4)
}

pub struct Teletype {
    pub mode: Mode,
    pub output: String,
    pub column: usize,
    pub paper_width: usize,
    pub shift_down: bool,
    pub control_down: bool,
    pub last_key_byte: Option<u8>,
    tape: PaperTape,
    auto_wrap_pending: bool,
    suppress_crlf_after_auto_wrap: bool,
}

impl Default for Teletype {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            output: String::new(),
            column: 0,
            paper_width: PAPER_COLUMNS,
            shift_down: false,
            control_down: false,
            last_key_byte: None,
            tape: PaperTape::default(),
            auto_wrap_pending: false,
            suppress_crlf_after_auto_wrap: false,
        }
    }
}

impl Teletype {
    pub fn char_width_image_px(&self) -> f32 {
        PRINTABLE_WIDTH / self.paper_width.max(1) as f32
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if mode == Mode::Off {
            self.shift_down = false;
            self.control_down = false;
            self.last_key_byte = None;
            self.auto_wrap_pending = false;
            self.suppress_crlf_after_auto_wrap = false;
        }
    }

    pub fn clear_paper(&mut self) {
        self.output.clear();
        self.column = 0;
        self.auto_wrap_pending = false;
        self.suppress_crlf_after_auto_wrap = false;
    }

    pub fn auto_wrap_pending(&self) -> bool {
        self.auto_wrap_pending
    }

    pub fn complete_auto_wrap(&mut self) -> bool {
        if !self.auto_wrap_pending {
            return false;
        }
        self.output.push('\n');
        self.column = 0;
        self.auto_wrap_pending = false;
        self.suppress_crlf_after_auto_wrap = true;
        self.trim_paper_history();
        true
    }

    pub fn print_serial(&mut self, byte: u8) -> Vec<PrintEvent> {
        // Guest UART activity must never move the physical OFF/LINE/LOCAL
        // selector. With the ASR-33 switched OFF the serial line can continue
        // transmitting electrically, but the printer mechanism stays silent.
        if self.mode == Mode::Off {
            return Vec::new();
        }
        self.print(byte)
    }

    pub fn print_local(&mut self, byte: u8) -> Vec<PrintEvent> {
        if self.mode != Mode::Local {
            return Vec::new();
        }
        self.print(byte)
    }

    fn put_at_carriage(&mut self, byte: u8) {
        let line_start = self.output.rfind('\n').map_or(0, |index| index + 1);
        let position = line_start + self.column;
        if position < self.output.len() {
            self.output
                .replace_range(position..position + 1, &(byte as char).to_string());
        } else {
            self.output
                .extend(std::iter::repeat_n(' ', position - self.output.len()));
            self.output.push(byte as char);
        }
    }

    fn trim_paper_history(&mut self) {
        if self.output.len() <= 12_000 {
            return;
        }
        let search_from = 4_000.min(self.output.len());
        if let Some(relative) = self.output[search_from..].find('\n') {
            self.output.drain(..search_from + relative + 1);
        }
    }

    fn print(&mut self, raw_byte: u8) -> Vec<PrintEvent> {
        if self.mode == Mode::Off {
            return Vec::new();
        }

        // The paper punch is an 8-level device and sees the incoming character
        // before the printer's 7-bit ASCII/typewheel normalization.
        self.tape.record(raw_byte);

        let byte = raw_byte & 0x7f;
        let byte = if byte.is_ascii_lowercase() {
            byte.to_ascii_uppercase()
        } else {
            byte
        };
        let mut events = Vec::new();

        if self.auto_wrap_pending {
            return events;
        }

        if self.suppress_crlf_after_auto_wrap {
            match byte {
                b'\r' => return events,
                b'\n' => {
                    self.suppress_crlf_after_auto_wrap = false;
                    return events;
                }
                _ => self.suppress_crlf_after_auto_wrap = false,
            }
        }

        if byte == 0x07 {
            events.push(PrintEvent::Bell);
        } else if (0x20..=0x7e).contains(&byte)
            && self.column == self.paper_width.saturating_sub(8)
        {
            events.push(PrintEvent::Bell);
        }

        match byte {
            0x07 | 0x1b => {}
            b'\r' => {
                self.column = 0;
                events.push(PrintEvent::CarriageReturn);
            }
            b'\n' => {
                self.output.push('\n');
                self.output
                    .extend(std::iter::repeat_n(' ', self.column));
                events.push(PrintEvent::LineFeed);
            }
            0x20..=0x7e => {
                if self.column < self.paper_width {
                    self.put_at_carriage(byte);
                    self.column += 1;
                    events.push(PrintEvent::Printable(byte));

                    if self.column == self.paper_width {
                        self.auto_wrap_pending = true;
                        events.push(PrintEvent::AutomaticReturn);
                    }
                }
            }
            _ => {}
        }

        self.trim_paper_history();
        events
    }

    // ---------------- Paper-tape reader ----------------

    pub fn load_tape(&mut self, bytes: &[u8]) {
        self.tape.load(bytes);
    }

    pub fn next_tape_byte(&mut self) -> Option<u8> {
        self.tape.next_byte()
    }

    pub fn tape_input_pending(&self) -> bool {
        self.tape.input_pending()
    }

    pub fn tape_input_len(&self) -> usize {
        self.tape.input_len()
    }

    pub fn tape_input_total_len(&self) -> usize {
        self.tape.input_total_len()
    }

    pub fn tape_input_position(&self) -> usize {
        self.tape.input_position()
    }

    pub fn rewind_tape_reader(&mut self) {
        self.tape.rewind_input();
    }

    pub fn eject_tape_reader(&mut self) {
        self.tape.eject_input();
    }

    // ---------------- Paper-tape punch ----------------

    pub fn tape_capture_enabled(&self) -> bool {
        self.tape.capture_enabled()
    }

    pub fn tape_punch_running(&self) -> bool {
        self.tape.capture_running()
    }

    pub fn prepare_tape_punch(&mut self) {
        self.tape.prepare_capture();
    }

    pub fn start_tape_punch(&mut self) {
        self.tape.begin_capture();
    }

    pub fn resume_tape_punch(&mut self) {
        self.tape.resume_capture();
    }

    pub fn pause_tape_punch(&mut self) {
        self.tape.pause_capture();
    }

    pub fn step_tape_punch(&mut self) -> Option<u8> {
        self.tape.punch_next()
    }

    pub fn tape_punch_pending_len(&self) -> usize {
        self.tape.punch_pending_len()
    }

    pub fn finish_tape_punch(&mut self) {
        self.tape.finish_capture();
    }

    pub fn punched_tape(&self) -> &[u8] {
        self.tape.output()
    }

    pub fn punched_tape_len(&self) -> usize {
        self.tape.output_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paper_is_72_columns() {
        assert_eq!(Teletype::default().paper_width, PAPER_COLUMNS);
    }

    #[test]
    fn serial_output_does_not_change_physical_off_mode() {
        let mut tty = Teletype::default();
        assert_eq!(tty.mode, Mode::Off);
        assert!(tty.print_serial(b'A').is_empty());
        assert_eq!(tty.mode, Mode::Off);
        assert!(tty.output.is_empty());
    }

    #[test]
    fn carriage_return_and_line_feed_are_independent() {
        let mut tty = Teletype::default();
        tty.set_mode(Mode::Line);
        tty.print_serial(b'A');
        tty.print_serial(b'B');
        tty.print_serial(b'\r');
        assert_eq!(tty.output, "AB");
        assert_eq!(tty.column, 0);
        tty.print_serial(b'\n');
        assert_eq!(tty.output, "AB\n");
        tty.print_serial(b'C');
        assert_eq!(tty.output, "AB\nC");
    }

    #[test]
    fn line_feed_keeps_horizontal_carriage_position() {
        let mut tty = Teletype::default();
        tty.set_mode(Mode::Line);
        tty.print_serial(b'A');
        tty.print_serial(b'B');
        tty.print_serial(b'\n');
        tty.print_serial(b'C');
        assert_eq!(tty.output, "AB\n  C");
        assert_eq!(tty.column, 3);
    }

    #[test]
    fn bare_carriage_return_overprints_current_line() {
        let mut tty = Teletype::default();
        tty.set_mode(Mode::Line);
        tty.print_serial(b'A');
        tty.print_serial(b'B');
        tty.print_serial(b'\r');
        tty.print_serial(b'C');
        assert_eq!(tty.output, "CB");
    }

    #[test]
    fn right_margin_requests_and_completes_automatic_return() {
        let mut tty = Teletype::default();
        tty.set_mode(Mode::Line);
        let mut last_events = Vec::new();
        for _ in 0..PAPER_COLUMNS {
            last_events = tty.print_serial(b'X');
        }
        assert_eq!(tty.column, PAPER_COLUMNS);
        assert_eq!(tty.output.len(), PAPER_COLUMNS);
        assert!(tty.auto_wrap_pending());
        assert!(last_events.contains(&PrintEvent::AutomaticReturn));

        tty.print_serial(b'Y');
        assert_eq!(tty.output.len(), PAPER_COLUMNS);

        assert!(tty.complete_auto_wrap());
        assert_eq!(tty.column, 0);
        assert_eq!(tty.output.len(), PAPER_COLUMNS + 1);
        assert!(tty.output.ends_with('\n'));

        tty.print_serial(b'Z');
        assert!(tty.output.ends_with("\nZ"));
    }

    #[test]
    fn explicit_crlf_after_auto_wrap_is_not_double_spaced() {
        let mut tty = Teletype::default();
        tty.set_mode(Mode::Line);
        for _ in 0..PAPER_COLUMNS {
            tty.print_serial(b'X');
        }
        tty.complete_auto_wrap();
        tty.print_serial(b'\r');
        tty.print_serial(b'\n');
        tty.print_serial(b'Z');
        assert_eq!(tty.output.matches('\n').count(), 1);
        assert!(tty.output.ends_with("\nZ"));
    }

    #[test]
    fn typewheel_uses_16_rotations_and_four_levels() {
        assert_eq!(typewheel_position(b' '), (0, 0));
        assert_eq!(typewheel_position(b'?'), (15, 1));
        assert_eq!(typewheel_position(b'@'), (0, 2));
        assert_eq!(typewheel_position(b'P'), (0, 3));
    }

    #[test]
    fn punch_transport_is_explicit_and_keeps_raw_eight_bit_data() {
        let mut tty = Teletype::default();
        tty.set_mode(Mode::Line);
        tty.prepare_tape_punch();
        tty.print_serial(0xff);
        assert_eq!(tty.tape_punch_pending_len(), 0);

        tty.resume_tape_punch();
        tty.print_serial(0xff);
        assert_eq!(tty.tape_punch_pending_len(), 1);
        assert_eq!(tty.step_tape_punch(), Some(0xff));
        tty.finish_tape_punch();
        assert_eq!(tty.punched_tape(), &[0xff]);
    }
}
