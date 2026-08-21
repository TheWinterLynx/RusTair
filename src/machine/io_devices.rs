use crate::config::SerialBoard;

use super::serial::SerialPort;
use super::{AltairBus, AltairMachine};

const SIO_STATUS_PORT: u8 = 0x00;
const SIO_DATA_PORT: u8 = 0x01;
const SIO2_STATUS_PORT: u8 = 0x10;
const SIO2_DATA_PORT: u8 = 0x11;

/// I/O devices currently installed in the emulated machine.
///
/// This layer owns guest-visible port decoding. `AltairBus` only forwards I/O
/// cycles here (apart from the front-panel port), which keeps individual device
/// protocols out of the memory/system-bus implementation and gives future
/// S-100 cards a natural insertion point.
#[derive(Default)]
pub(super) struct IoDevices {
    serial: SerialPort,
    serial_board: SerialBoard,
}

impl IoDevices {
    pub(super) fn configure_serial_board(&mut self, board: SerialBoard) {
        self.serial_board = board;
        self.serial.clear();
    }

    pub(super) fn serial_board(&self) -> SerialBoard {
        self.serial_board
    }

    pub(super) fn input(&mut self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => match port {
                // MITS 88-SIO status convention used by the S2JS reference.
                // Bit 0 is set when the receive buffer is empty, while bits 6/7
                // are set while the transmit holding register is occupied.
                SIO_STATUS_PORT => {
                    let rx_empty = self.serial.rx_empty();
                    let tx_busy = self.serial.tx_busy();
                    (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
                }
                SIO_DATA_PORT => self.serial.read_rx().unwrap_or(0),

                // An absent 88-2SIO must not look TX-ready to software polling
                // its status register.
                SIO2_STATUS_PORT => 0x00,
                _ => 0,
            },
            SerialBoard::TwoSio88 => match port {
                // MITS 88-2SIO / 6850-style convention used by RusTair:
                // bit 0 = RX ready, bit 1 = TX ready.
                SIO2_STATUS_PORT => {
                    (if self.serial.rx_empty() { 0 } else { 0x01 })
                        | (if self.serial.tx_busy() { 0 } else { 0x02 })
                }
                SIO2_DATA_PORT => self.serial.read_rx().unwrap_or(0),

                // The 88-SIO uses active-low ready flags. Returning all ones
                // for its absent status register keeps software waiting rather
                // than accidentally treating the uninstalled card as ready.
                SIO_STATUS_PORT => 0xff,
                _ => 0,
            },
        }
    }

    pub(super) fn output(&mut self, port: u8, value: u8) {
        let selected_data_port = match self.serial_board {
            SerialBoard::Sio88 => SIO_DATA_PORT,
            SerialBoard::TwoSio88 => SIO2_DATA_PORT,
        };

        if port == selected_data_port {
            self.serial.write_tx(value);
        }
    }

    pub(super) fn serial_receive(&mut self, byte: u8) {
        self.serial.receive(byte);
    }

    pub(super) fn serial_rx_empty(&self) -> bool {
        self.serial.rx_empty()
    }

    pub(super) fn serial_rx_len(&self) -> usize {
        self.serial.rx_len()
    }

    pub(super) fn serial_tx_front(&self) -> Option<u8> {
        self.serial.tx_front()
    }

    pub(super) fn serial_tx_complete(&mut self) -> Option<u8> {
        self.serial.complete_tx()
    }

    pub(super) fn serial_tx_busy(&self) -> bool {
        self.serial.tx_busy()
    }

    pub(super) fn clear_serial(&mut self) {
        self.serial.clear();
    }
}

// Keep serial-board configuration next to the device decoder it controls. These
// impl blocks are in a child module of `machine`, so they can update the
// machine's internal reset/lamp state without widening those fields' visibility.
impl AltairBus {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        self.io.configure_serial_board(board);
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.io.serial_board()
    }
}

impl AltairMachine {
    /// Swap the installed serial board and reset the CPU/device state without
    /// modifying RAM or the front-panel sense switches.
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        if self.bus.serial_board() == board {
            return;
        }

        self.running = false;
        self.bus.configure_serial_board(board);
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        self.address_leds = 0;
        self.bus.set_data_leds(0);
        self.wait_led = self.powered;
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.bus.serial_board()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_88_sio_does_not_alias_88_2sio_data_port() {
        let mut io = IoDevices::default();
        assert_eq!(io.serial_board(), SerialBoard::Sio88);

        io.output(SIO2_DATA_PORT, b'X');
        assert!(!io.serial_tx_busy());

        io.output(SIO_DATA_PORT, b'S');
        assert_eq!(io.serial_tx_front(), Some(b'S'));
    }

    #[test]
    fn selected_88_2sio_does_not_alias_88_sio_data_port() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(io.input(SIO2_STATUS_PORT) & 0x02, 0x02);
        assert_eq!(io.input(SIO_STATUS_PORT), 0xff);

        io.output(SIO_DATA_PORT, b'X');
        assert!(!io.serial_tx_busy());

        io.output(SIO2_DATA_PORT, b'2');
        assert_eq!(io.serial_tx_front(), Some(b'2'));
    }

    #[test]
    fn changing_serial_board_preserves_ram() {
        let mut machine = AltairMachine::default();
        machine.bus.load(0x0200, &[0x5a]);

        machine.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(machine.bus.read(0x0200), 0x5a);
        assert_eq!(machine.serial_board(), SerialBoard::TwoSio88);
    }
}
