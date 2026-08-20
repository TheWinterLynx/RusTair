use std::collections::VecDeque;

pub const IMAGE_W: f32 = 3008.0;
pub const IMAGE_H: f32 = 2983.0;
pub const PRINT_LEFT: f32 = IMAGE_W * 0.25;
pub const PRINT_TOP: f32 = IMAGE_H * 0.34;
pub const PRINT_HEAD_TOP: f32 = IMAGE_H * 0.33;
// Calibrated to the visible paper in the ASR-33 photograph. The previous
// 1644px span let the final columns drift into the dark glass at the right.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Character(&'static str),
    Escape,
    LineFeed,
    CarriageReturn,
    Delete,
    Space,
    Control,
    Shift,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub kind: KeyKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Key {
    pub const fn new(kind: KeyKind, x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { kind, x, y, w, h }
    }

    /// The original Javascript intentionally made the click target a little
    /// larger than the visible keytop: x - 20 .. x + w + 20 and
    /// y .. y + h + 40. We preserve that behaviour here.
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x - 20.0
            && x <= self.x + self.w + 20.0
            && y >= self.y
            && y <= self.y + self.h + 40.0
    }
}

const W: f32 = 114.0;
const H: f32 = 97.0;

// Exact coordinates derived from the original asr33.html keycoords table.
pub const KEYS: &[Key] = &[
    // Number row
    Key::new(KeyKind::Character("1!"), 548.0, 1918.0, W, H),
    Key::new(KeyKind::Character("2\""), 709.0, 1918.0, W, H),
    Key::new(KeyKind::Character("3#"), 870.0, 1918.0, W, H),
    Key::new(KeyKind::Character("4$"), 1031.0, 1918.0, W, H),
    Key::new(KeyKind::Character("5%"), 1192.0, 1918.0, W, H),
    Key::new(KeyKind::Character("6&"), 1353.0, 1918.0, W, H),
    Key::new(KeyKind::Character("7'"), 1514.0, 1918.0, W, H),
    Key::new(KeyKind::Character("8("), 1675.0, 1918.0, W, H),
    Key::new(KeyKind::Character("9)"), 1836.0, 1918.0, W, H),
    Key::new(KeyKind::Character("0"), 1997.0, 1918.0, W, H),
    Key::new(KeyKind::Character(":*"), 2158.0, 1918.0, W, H),
    Key::new(KeyKind::Character("-="), 2319.0, 1918.0, W, H),
    // The original image also has an unassigned HERE IS key at x=2480.

    // QWERTY row
    Key::new(KeyKind::Escape, 468.0, 2093.0, W, H),
    Key::new(KeyKind::Character("Q"), 629.0, 2093.0, W, H),
    Key::new(KeyKind::Character("W"), 790.0, 2093.0, W, H),
    Key::new(KeyKind::Character("E"), 951.0, 2093.0, W, H),
    Key::new(KeyKind::Character("R"), 1112.0, 2093.0, W, H),
    Key::new(KeyKind::Character("T"), 1273.0, 2093.0, W, H),
    Key::new(KeyKind::Character("Y"), 1434.0, 2093.0, W, H),
    Key::new(KeyKind::Character("U"), 1595.0, 2093.0, W, H),
    Key::new(KeyKind::Character("I"), 1758.0, 2093.0, W, H),
    Key::new(KeyKind::Character("O_"), 1919.0, 2093.0, W, H),
    Key::new(KeyKind::Character("P@"), 2082.0, 2091.0, W, H),
    Key::new(KeyKind::LineFeed, 2243.0, 2089.0, W, H),
    Key::new(KeyKind::CarriageReturn, 2404.0, 2089.0, W, H),

    // ASDF row
    Key::new(KeyKind::Character("A"), 670.0, 2265.0, W, H),
    Key::new(KeyKind::Character("S"), 831.0, 2265.0, W, H),
    Key::new(KeyKind::Character("D"), 992.0, 2265.0, W, H),
    Key::new(KeyKind::Character("F"), 1153.0, 2265.0, W, H),
    Key::new(KeyKind::Character("G"), 1314.0, 2265.0, W, H),
    Key::new(KeyKind::Character("H"), 1475.0, 2265.0, W, H),
    Key::new(KeyKind::Character("J"), 1636.0, 2265.0, W, H),
    Key::new(KeyKind::Character("K["), 1803.0, 2269.0, W, H),
    Key::new(KeyKind::Character("L\\"), 1964.0, 2269.0, W, H),
    Key::new(KeyKind::Character(";+"), 2125.0, 2263.0, W, H),
    Key::new(KeyKind::Delete, 2286.0, 2263.0, W, H),

    // ZXCV row
    Key::new(KeyKind::Character("Z"), 744.0, 2443.0, W, H),
    Key::new(KeyKind::Character("X"), 905.0, 2443.0, W, H),
    Key::new(KeyKind::Character("C"), 1066.0, 2443.0, W, H),
    Key::new(KeyKind::Character("V"), 1233.0, 2443.0, W, H),
    Key::new(KeyKind::Character("B"), 1394.0, 2443.0, W, H),
    Key::new(KeyKind::Character("N^"), 1555.0, 2443.0, W, H),
    Key::new(KeyKind::Character("M]"), 1722.0, 2443.0, W, H),
    Key::new(KeyKind::Character(",<"), 1889.0, 2443.0, W, H),
    Key::new(KeyKind::Character(".>"), 2050.0, 2443.0, W, H),
    Key::new(KeyKind::Character("/?"), 2211.0, 2437.0, W, H),

    Key::new(KeyKind::Space, 1240.0, 2627.0, 671.0, 120.0),
    Key::new(KeyKind::Control, 480.0, 2261.0, 150.0, 120.0),
    Key::new(KeyKind::Shift, 548.0, 2439.0, 150.0, 120.0),
    Key::new(KeyKind::Shift, 2370.0, 2421.0, 150.0, 120.0),
];

pub fn hit_test(x: f32, y: f32) -> Option<&'static Key> {
    KEYS.iter().find(|key| key.contains(x, y))
}

pub fn key_to_byte(kind: KeyKind, shifted: bool, control: bool) -> Option<u8> {
    let mut ch = match kind {
        KeyKind::Character(chars) => {
            let mut iter = chars.chars();
            let first = iter.next()?;
            if shifted {
                iter.next().unwrap_or(first)
            } else {
                first
            }
        }
        KeyKind::Escape => return Some(0x1b),
        KeyKind::LineFeed => return Some(b'\n'),
        KeyKind::CarriageReturn => return Some(b'\r'),
        KeyKind::Delete => return Some(0x7f),
        KeyKind::Space => return Some(b' '),
        KeyKind::Control | KeyKind::Shift => return None,
    };

    ch = ch.to_ascii_uppercase();
    let mut byte = ch as u8;
    if control && byte.is_ascii_uppercase() {
        byte -= 64;
    }
    Some(byte)
}

/// Return the Model 33 typewheel's 16-position rotational slot and one of its
/// four vertical character levels. The ASR-33 printable set occupies the four
/// 16-code ASCII blocks 0x20..=0x5f, so the low nibble selects rotation and
/// the next two bits select height.
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
    pub capture_to_tape: bool,
    pub tape_out: Vec<u8>,
    pub tape_in: VecDeque<u8>,
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
            capture_to_tape: false,
            tape_out: Vec::new(),
            tape_in: VecDeque::new(),
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

    /// Finish the automatic end-of-line mechanism after the final character
    /// has struck the ribbon. Keeping this separate from `print()` leaves the
    /// carriage visibly parked at column 72 until the strike has completed.
    pub fn complete_auto_wrap(&mut self) -> bool {
        if !self.auto_wrap_pending {
            return false;
        }
        self.output.push('\n');
        self.column = 0;
        self.auto_wrap_pending = false;
        // Software commonly emits its own CR/LF. Swallow one immediately
        // following pair so the mechanical auto-return does not create a blank
        // line when both behaviours coincide at the right margin.
        self.suppress_crlf_after_auto_wrap = true;
        self.trim_paper_history();
        true
    }

    /// Print one seven-bit character arriving from the Altair.
    /// Returns sound/mechanical events that the native layer can render.
    pub fn print_serial(&mut self, byte: u8) -> Vec<PrintEvent> {
        if self.mode == Mode::Off {
            // The web original did this deliberately as a convenience.
            self.mode = Mode::Line;
        }
        self.print(byte)
    }

    /// Local echo, used only in LOCAL mode.
    pub fn print_local(&mut self, byte: u8) -> Vec<PrintEvent> {
        if self.mode != Mode::Local {
            return Vec::new();
        }
        self.print(byte)
    }

    fn put_at_carriage(&mut self, byte: u8) {
        // The paper model is ASCII-only, so byte offsets and character offsets
        // are identical. This lets a bare CR genuinely overprint the current
        // line instead of pretending that CR also fed the paper.
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

    fn print(&mut self, byte: u8) -> Vec<PrintEvent> {
        if self.mode == Mode::Off {
            return Vec::new();
        }

        let byte = byte & 0x7f;
        let byte = if byte.is_ascii_lowercase() {
            byte.to_ascii_uppercase()
        } else {
            byte
        };
        let mut events = Vec::new();

        if self.capture_to_tape {
            self.tape_out.push(byte);
        }

        // While the automatic return is physically in progress the typebox is
        // unavailable. Serial input is normally held by the controller; this
        // also protects LOCAL-mode input from printing beyond the hard stop.
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

                    // The ASR-33 automatic margin mechanism is triggered by the
                    // final printable position. It waits for this impact to
                    // finish, then the controller performs CR+LF mechanically.
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

    pub fn load_tape(&mut self, bytes: &[u8]) {
        self.tape_in.clear();
        self.tape_in
            .extend(bytes.iter().copied().map(|b| b.to_ascii_uppercase()));
    }

    pub fn next_tape_byte(&mut self) -> Option<u8> {
        self.tape_in.pop_front().map(|b| b & 0x7f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_hitbox_finds_a_key() {
        let k = hit_test(670.0, 2265.0).unwrap();
        assert_eq!(key_to_byte(k.kind, false, false), Some(b'A'));
    }

    #[test]
    fn shift_and_control_match_asr33() {
        assert_eq!(
            key_to_byte(KeyKind::Character("K["), true, false),
            Some(b'[')
        );
        assert_eq!(
            key_to_byte(KeyKind::Character("A"), false, true),
            Some(1)
        );
    }

    #[test]
    fn default_paper_is_72_columns() {
        assert_eq!(Teletype::default().paper_width, PAPER_COLUMNS);
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

        // Nothing can print beyond the hard stop while the carriage mechanism
        // is waiting to return.
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
}
