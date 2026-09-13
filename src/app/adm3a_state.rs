pub(super) const ADM3A_COLS: usize = 80;
pub(super) const ADM3A_ROWS: usize = 24;

const ASCII_MASK: u8 = 0x7f;
const CURSOR_ADDRESS_BIAS: u8 = 0x20;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ParserState {
    #[default]
    Normal,
    Escape,
    CursorRow,
    CursorColumn { row: usize },
}

/// Headless Lear Siegler ADM-3A display state.
///
/// This model owns terminal-local screen RAM, cursor and escape parsing only.
/// It deliberately knows nothing about the Altair, S-100 or a MITS serial card:
/// bytes will arrive here only after the selected emulated UART has completed a
/// transmitted frame at the external cable boundary.
pub(super) struct Adm3aState {
    cells: [[u8; ADM3A_COLS]; ADM3A_ROWS],
    cursor_col: usize,
    cursor_row: usize,
    parser: ParserState,
    bell_pending: bool,
}

impl Default for Adm3aState {
    fn default() -> Self {
        Self {
            cells: [[b' '; ADM3A_COLS]; ADM3A_ROWS],
            cursor_col: 0,
            cursor_row: 0,
            parser: ParserState::Normal,
            bell_pending: false,
        }
    }
}

impl Adm3aState {
    pub(super) fn receive_byte(&mut self, byte: u8) {
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
        } else {
            self.cursor_col = 0;
            self.line_down();
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(terminal
            .cells
            .iter()
            .all(|row| row.iter().all(|byte| *byte == b' ')));
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
        assert!(terminal.row(ADM3A_ROWS - 1).iter().all(|byte| *byte == b' '));
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
}
