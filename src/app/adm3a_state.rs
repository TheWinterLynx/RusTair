use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(super) const ADM3A_COLS: usize = 80;
pub(super) const ADM3A_ROWS: usize = 24;

const ASCII_MASK: u8 = 0x7f;
const CURSOR_ADDRESS_BIAS: u8 = 0x20;

/// Physical communication-rate selector offered by the Lear Siegler ADM-3A.
///
/// This belongs to the terminal, not to the Altair or the MITS serial card. A
/// cable therefore has two independently configured clocks, just like the real
/// equipment; matching/mismatch behavior is handled at their physical boundary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Adm3aBaudRate {
    Baud75,
    Baud110,
    Baud150,
    Baud300,
    Baud600,
    Baud1200,
    Baud1800,
    Baud2400,
    Baud4800,
    #[default]
    Baud9600,
    Baud19200,
}

impl Adm3aBaudRate {
    pub(super) const ALL: [Self; 11] = [
        Self::Baud75,
        Self::Baud110,
        Self::Baud150,
        Self::Baud300,
        Self::Baud600,
        Self::Baud1200,
        Self::Baud1800,
        Self::Baud2400,
        Self::Baud4800,
        Self::Baud9600,
        Self::Baud19200,
    ];

    pub(super) const fn baud(self) -> u32 {
        match self {
            Self::Baud75 => 75,
            Self::Baud110 => 110,
            Self::Baud150 => 150,
            Self::Baud300 => 300,
            Self::Baud600 => 600,
            Self::Baud1200 => 1_200,
            Self::Baud1800 => 1_800,
            Self::Baud2400 => 2_400,
            Self::Baud4800 => 4_800,
            Self::Baud9600 => 9_600,
            Self::Baud19200 => 19_200,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Baud75 => "75",
            Self::Baud110 => "110",
            Self::Baud150 => "150",
            Self::Baud300 => "300",
            Self::Baud600 => "600",
            Self::Baud1200 => "1200",
            Self::Baud1800 => "1800",
            Self::Baud2400 => "2400",
            Self::Baud4800 => "4800",
            Self::Baud9600 => "9600",
            Self::Baud19200 => "19200",
        }
    }
}

/// ADM-3A DATA 7/8 configuration switch (S3-3).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Adm3aDataBits {
    Seven,
    #[default]
    Eight,
}

impl Adm3aDataBits {
    pub(super) const ALL: [Self; 2] = [Self::Seven, Self::Eight];

    pub(super) const fn bits(self) -> u8 {
        match self {
            Self::Seven => 7,
            Self::Eight => 8,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Seven => "7",
            Self::Eight => "8",
        }
    }
}

/// ADM-3A PARITY/INH plus ODD/EVEN configuration switches (S3-2/S3-5).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Adm3aParity {
    #[default]
    None,
    Even,
    Odd,
}

impl Adm3aParity {
    pub(super) const ALL: [Self; 3] = [Self::None, Self::Even, Self::Odd];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::None => "N",
            Self::Even => "E",
            Self::Odd => "O",
        }
    }
}

/// ADM-3A STOP 1/2 configuration switch (S3-4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Adm3aStopBits {
    #[default]
    One,
    Two,
}

impl Adm3aStopBits {
    pub(super) const ALL: [Self; 2] = [Self::One, Self::Two];

    pub(super) const fn bits(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Two => "2",
        }
    }
}

/// Wire framing selected by the ADM-3A switches. There was no single universal
/// factory word format: ordering/customer setup selected these switches, so the
/// emulator exposes them independently while retaining 8N1 as the prior RusTair
/// compatibility default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Adm3aWordFormat {
    pub(super) data_bits: Adm3aDataBits,
    pub(super) parity: Adm3aParity,
    pub(super) stop_bits: Adm3aStopBits,
}

impl Adm3aWordFormat {
    pub(super) const fn frame_bits(self) -> u8 {
        1 + self.data_bits.bits()
            + match self.parity {
                Adm3aParity::None => 0,
                Adm3aParity::Even | Adm3aParity::Odd => 1,
            }
            + self.stop_bits.bits()
    }

    pub(super) fn label(self) -> String {
        format!(
            "{}{}{}",
            self.data_bits.label(),
            self.parity.label(),
            self.stop_bits.label()
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ParserState {
    #[default]
    Normal,
    Escape,
    CursorRow,
    CursorColumn { row: usize },
}

/// Headless Lear Siegler ADM-3A display and keyboard state.
///
/// This model owns terminal-local screen RAM, cursor, escape parsing and the
/// keyboard transmitter queue only. It deliberately knows nothing about the
/// Altair, S-100 or a MITS serial card: bytes cross that boundary only through
/// the selected emulated UART and its physical receive/transmit paths.
pub(super) struct Adm3aState {
    pub(super) window_open: bool,
    powered: bool,
    auto_new_line: bool,
    baud_rate: Adm3aBaudRate,
    word_format: Adm3aWordFormat,
    /// S3-6 BIT 8 0/1. It applies only while 8-bit words are selected and fixes
    /// the keyboard transmitter's eighth data bit independently from 7-bit ASCII.
    bit8_one: bool,
    cells: [[u8; ADM3A_COLS]; ADM3A_ROWS],
    cursor_col: usize,
    cursor_row: usize,
    parser: ParserState,
    bell_pending: bool,
    keyboard_queue: VecDeque<u8>,
    keyboard_next_at: Option<Instant>,
}

impl Default for Adm3aState {
    fn default() -> Self {
        Self {
            window_open: false,
            powered: false,
            auto_new_line: false,
            baud_rate: Adm3aBaudRate::default(),
            word_format: Adm3aWordFormat::default(),
            bit8_one: false,
            cells: [[b' '; ADM3A_COLS]; ADM3A_ROWS],
            cursor_col: 0,
            cursor_row: 0,
            parser: ParserState::Normal,
            bell_pending: false,
            keyboard_queue: VecDeque::new(),
            keyboard_next_at: None,
        }
    }
}

impl Adm3aState {
    pub(super) const fn powered(&self) -> bool {
        self.powered
    }

    pub(super) fn set_powered(&mut self, powered: bool) {
        if self.powered == powered {
            return;
        }
        self.powered = powered;
        self.clear_screen();
        self.bell_pending = false;
        self.keyboard_queue.clear();
        self.keyboard_next_at = None;
    }

    pub(super) const fn baud_rate(&self) -> Adm3aBaudRate {
        self.baud_rate
    }

    pub(super) fn set_baud_rate(&mut self, baud_rate: Adm3aBaudRate) {
        if self.baud_rate == baud_rate {
            return;
        }
        self.baud_rate = baud_rate;
        // The selector changes the terminal's transmitter clock immediately.
        // Keep already typed keys, but restart the pacing epoch at the next
        // presentation instead of carrying timing from the old oscillator rate.
        self.keyboard_next_at = None;
    }

    pub(super) const fn word_format(&self) -> Adm3aWordFormat {
        self.word_format
    }

    pub(super) fn set_word_format(&mut self, word_format: Adm3aWordFormat) {
        if self.word_format == word_format {
            return;
        }
        self.word_format = word_format;
        // Word length/parity/stop changes alter the physical frame duration.
        self.keyboard_next_at = None;
    }

    pub(super) const fn bit8_one(&self) -> bool {
        self.bit8_one
    }

    pub(super) fn set_bit8_one(&mut self, bit8_one: bool) {
        self.bit8_one = bit8_one;
    }

    pub(super) fn receive_byte(&mut self, byte: u8) {
        // ADM-3A display/control decoding is US-ASCII. The optional eighth data
        // bit is a communications bit and never becomes an extra glyph address.
        let byte = byte & ASCII_MASK;
        match self.parser {
            ParserState::Normal => self.receive_normal(byte),
            ParserState::Escape => {
                self.parser = if byte == b'=' {
                    ParserState::CursorRow
                } else {
                    ParserState::Normal
                };
            }
            ParserState::CursorRow => {
                self.parser = ParserState::CursorColumn {
                    row: Self::decode_cursor_coordinate(byte, ADM3A_ROWS),
                };
            }
            ParserState::CursorColumn { row } => {
                self.cursor_row = row;
                self.cursor_col = Self::decode_cursor_coordinate(byte, ADM3A_COLS);
                self.parser = ParserState::Normal;
            }
        }
    }

    fn receive_normal(&mut self, byte: u8) {
        match byte {
            0x07 => self.bell_pending = true,
            0x08 => self.cursor_col = self.cursor_col.saturating_sub(1),
            0x0a => self.line_down(),
            0x0b => self.cursor_row = self.cursor_row.saturating_sub(1),
            0x0c => self.cursor_col = (self.cursor_col + 1).min(ADM3A_COLS - 1),
            b'\r' => self.cursor_col = 0,
            0x1a => self.clear_screen(),
            0x1b => self.parser = ParserState::Escape,
            0x20..=0x7e => self.write_printable(byte),
            _ => {}
        }
    }

    fn write_printable(&mut self, byte: u8) {
        self.cells[self.cursor_row][self.cursor_col] = byte;
        if self.cursor_col + 1 < ADM3A_COLS {
            self.cursor_col += 1;
        } else if self.auto_new_line {
            self.cursor_col = 0;
            self.line_down();
        } else {
            // With AUTO NL disabled, real ADM-3A overflow reloads column 79.
            // Further printable characters overwrite the last cell until an
            // explicit CR/LF (or cursor-control command) moves the cursor.
            self.cursor_col = ADM3A_COLS - 1;
        }
    }

    fn line_down(&mut self) {
        if self.cursor_row + 1 < ADM3A_ROWS {
            self.cursor_row += 1;
        } else {
            self.scroll_up();
        }
    }

    fn scroll_up(&mut self) {
        for row in 1..ADM3A_ROWS {
            self.cells[row - 1] = self.cells[row];
        }
        self.cells[ADM3A_ROWS - 1] = [b' '; ADM3A_COLS];
        self.cursor_row = ADM3A_ROWS - 1;
    }

    fn decode_cursor_coordinate(byte: u8, limit: usize) -> usize {
        usize::from(byte.saturating_sub(CURSOR_ADDRESS_BIAS)).min(limit - 1)
    }

    pub(super) fn clear_screen(&mut self) {
        self.cells = [[b' '; ADM3A_COLS]; ADM3A_ROWS];
        self.cursor_col = 0;
        self.cursor_row = 0;
        self.parser = ParserState::Normal;
    }

    pub(super) fn row(&self, row: usize) -> &[u8; ADM3A_COLS] {
        &self.cells[row]
    }

    pub(super) const fn cursor(&self) -> (usize, usize) {
        (self.cursor_col, self.cursor_row)
    }

    pub(super) fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell_pending)
    }

    pub(super) fn queue_keyboard_byte(&mut self, byte: u8, now: Instant) -> bool {
        if !self.powered {
            return false;
        }
        let ascii = byte & ASCII_MASK;
        let byte = if self.word_format.data_bits == Adm3aDataBits::Eight && self.bit8_one {
            ascii | 0x80
        } else {
            ascii
        };
        self.keyboard_queue.push_back(byte);
        self.keyboard_next_at.get_or_insert(now);
        true
    }

    pub(super) fn keyboard_pending_len(&self) -> usize {
        self.keyboard_queue.len()
    }

    pub(super) fn keyboard_due_in(&self, now: Instant) -> Duration {
        self.keyboard_next_at
            .map(|due| due.saturating_duration_since(now))
            .unwrap_or(Duration::ZERO)
    }

    pub(super) fn take_due_keyboard_byte(&mut self, now: Instant) -> Option<u8> {
        if !self.keyboard_due_in(now).is_zero() {
            return None;
        }

        let byte = self.keyboard_queue.pop_front()?;
        self.keyboard_next_at = if self.keyboard_queue.is_empty() {
            None
        } else {
            Some(now + self.keyboard_char_time())
        };
        Some(byte)
    }

    fn keyboard_char_time(&self) -> Duration {
        Duration::from_secs_f64(
            f64::from(self.word_format.frame_bits()) / f64::from(self.baud_rate.baud()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_starts_off_and_transition_resets_screen() {
        let mut terminal = Adm3aState::default();
        assert!(!terminal.powered());
        assert!(!terminal.auto_new_line);
        assert_eq!(terminal.baud_rate(), Adm3aBaudRate::Baud9600);
        assert_eq!(terminal.word_format(), Adm3aWordFormat::default());
        terminal.receive_byte(b'X');
        terminal.set_powered(true);
        assert!(terminal.powered());
        assert_eq!(terminal.row(0)[0], b' ');
        assert_eq!(terminal.cursor(), (0, 0));
        terminal.receive_byte(b'Y');
        terminal.set_powered(false);
        assert!(!terminal.powered());
        assert_eq!(terminal.row(0)[0], b' ');
        assert_eq!(terminal.cursor(), (0, 0));
        assert_eq!(terminal.keyboard_pending_len(), 0);
    }

    #[test]
    fn printable_input_and_cr_lf_are_independent() {
        let mut terminal = Adm3aState::default();
        for byte in b"ABC\rD" {
            terminal.receive_byte(*byte);
        }
        assert_eq!(&terminal.row(0)[..3], b"DBC");
        assert_eq!(terminal.cursor(), (1, 0));

        terminal.receive_byte(b'\n');
        assert_eq!(terminal.cursor(), (1, 1));
        terminal.receive_byte(b'E');
        assert_eq!(terminal.row(1)[1], b'E');
    }

    #[test]
    fn auto_new_line_switch_controls_column_80_overflow() {
        let mut terminal = Adm3aState::default();
        for _ in 0..ADM3A_COLS {
            terminal.receive_byte(b'X');
        }
        assert_eq!(terminal.cursor(), (ADM3A_COLS - 1, 0));
        assert_eq!(terminal.row(0)[ADM3A_COLS - 1], b'X');

        terminal.receive_byte(b'Y');
        assert_eq!(terminal.cursor(), (ADM3A_COLS - 1, 0));
        assert_eq!(terminal.row(0)[ADM3A_COLS - 1], b'Y');

        terminal.receive_byte(b'\r');
        terminal.receive_byte(b'\n');
        assert_eq!(terminal.cursor(), (0, 1));

        terminal.auto_new_line = true;
        for _ in 0..ADM3A_COLS {
            terminal.receive_byte(b'Z');
        }
        assert_eq!(terminal.cursor(), (0, 2));
    }

    #[test]
    fn four_direction_cursor_controls_stop_at_screen_edges() {
        let mut terminal = Adm3aState::default();
        terminal.receive_byte(0x08);
        terminal.receive_byte(0x0b);
        assert_eq!(terminal.cursor(), (0, 0));

        terminal.receive_byte(0x0c);
        terminal.receive_byte(0x0a);
        assert_eq!(terminal.cursor(), (1, 1));

        for _ in 0..200 {
            terminal.receive_byte(0x0c);
        }
        assert_eq!(terminal.cursor().0, ADM3A_COLS - 1);
    }

    #[test]
    fn escape_equals_addresses_cursor_with_space_bias() {
        let mut terminal = Adm3aState::default();
        for byte in [0x1b, b'=', 0x20 + 7, 0x20 + 42] {
            terminal.receive_byte(byte);
        }
        assert_eq!(terminal.cursor(), (42, 7));
        terminal.receive_byte(b'X');
        assert_eq!(terminal.row(7)[42], b'X');
    }

    #[test]
    fn cursor_addressing_clamps_out_of_range_coordinates() {
        let mut terminal = Adm3aState::default();
        for byte in [0x1b, b'=', 0x7f, 0x7f] {
            terminal.receive_byte(byte);
        }
        assert_eq!(terminal.cursor(), (ADM3A_COLS - 1, ADM3A_ROWS - 1));
    }

    #[test]
    fn sub_clears_screen_and_homes_cursor() {
        let mut terminal = Adm3aState::default();
        for byte in b"HELLO" {
            terminal.receive_byte(*byte);
        }
        terminal.receive_byte(0x1a);
        assert_eq!(terminal.cursor(), (0, 0));
        assert!(
            terminal
                .cells
                .iter()
                .all(|row| row.iter().all(|byte| *byte == b' '))
        );
    }

    #[test]
    fn bottom_line_feed_scrolls_without_changing_column() {
        let mut terminal = Adm3aState::default();
        for row in 0..ADM3A_ROWS {
            for byte in [0x1b, b'=', 0x20 + row as u8, 0x20] {
                terminal.receive_byte(byte);
            }
            terminal.receive_byte(b'A' + (row % 26) as u8);
        }
        for byte in [0x1b, b'=', 0x20 + (ADM3A_ROWS - 1) as u8, 0x20 + 5] {
            terminal.receive_byte(byte);
        }
        terminal.receive_byte(0x0a);
        assert_eq!(terminal.cursor(), (5, ADM3A_ROWS - 1));
        assert_eq!(terminal.row(0)[0], b'B');
        assert!(
            terminal
                .row(ADM3A_ROWS - 1)
                .iter()
                .all(|byte| *byte == b' ')
        );
    }

    #[test]
    fn bell_is_latched_until_consumed() {
        let mut terminal = Adm3aState::default();
        terminal.receive_byte(0x07);
        assert!(terminal.take_bell());
        assert!(!terminal.take_bell());
    }

    #[test]
    fn unknown_escape_recovers_to_normal_input() {
        let mut terminal = Adm3aState::default();
        terminal.receive_byte(0x1b);
        terminal.receive_byte(b'X');
        terminal.receive_byte(b'A');
        assert_eq!(terminal.row(0)[0], b'A');
        assert_eq!(terminal.cursor(), (1, 0));
    }

    #[test]
    fn keyboard_transmitter_uses_selected_baud_and_word_format() {
        let mut terminal = Adm3aState::default();
        let now = Instant::now();
        assert!(!terminal.queue_keyboard_byte(b'A', now));

        terminal.set_powered(true);
        terminal.set_baud_rate(Adm3aBaudRate::Baud110);
        terminal.set_word_format(Adm3aWordFormat {
            data_bits: Adm3aDataBits::Seven,
            parity: Adm3aParity::Even,
            stop_bits: Adm3aStopBits::Two,
        });
        assert_eq!(terminal.word_format().frame_bits(), 11);
        assert!(terminal.queue_keyboard_byte(b'A', now));
        assert!(terminal.queue_keyboard_byte(b'B', now));
        assert_eq!(terminal.keyboard_pending_len(), 2);
        assert_eq!(terminal.take_due_keyboard_byte(now), Some(b'A'));
        assert_eq!(terminal.take_due_keyboard_byte(now), None);

        let later = now + terminal.keyboard_char_time();
        assert_eq!(terminal.take_due_keyboard_byte(later), Some(b'B'));
        assert_eq!(terminal.keyboard_pending_len(), 0);
        assert_eq!(terminal.baud_rate().baud(), 110);
        assert_eq!(terminal.keyboard_char_time(), Duration::from_millis(100));
    }

    #[test]
    fn bit8_switch_only_changes_eight_bit_keyboard_words() {
        let mut terminal = Adm3aState::default();
        terminal.set_powered(true);
        let now = Instant::now();

        terminal.set_bit8_one(true);
        assert!(terminal.queue_keyboard_byte(b'A', now));
        assert_eq!(terminal.take_due_keyboard_byte(now), Some(0xc1));

        terminal.set_word_format(Adm3aWordFormat {
            data_bits: Adm3aDataBits::Seven,
            ..Adm3aWordFormat::default()
        });
        assert!(terminal.queue_keyboard_byte(b'B', now));
        assert_eq!(terminal.take_due_keyboard_byte(now), Some(b'B'));
    }

    #[test]
    fn adm3a_exposes_all_eleven_hardware_baud_choices() {
        let rates = Adm3aBaudRate::ALL.map(Adm3aBaudRate::baud);
        assert_eq!(
            rates,
            [
                75, 110, 150, 300, 600, 1_200, 1_800, 2_400, 4_800, 9_600, 19_200
            ]
        );
    }
}
