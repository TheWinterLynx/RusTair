use crate::config::SerialBoard;

use super::serial::SerialPort;
use super::{AltairBus, AltairMachine};

const SIO_STATUS_PORT: u8 = 0x00;
const SIO_DATA_PORT: u8 = 0x01;
const SIO2_PORT0_STATUS: u8 = 0x10;
const SIO2_PORT0_DATA: u8 = 0x11;
const SIO2_PORT1_STATUS: u8 = 0x12;
const SIO2_PORT1_DATA: u8 = 0x13;

/// I/O devices currently installed in the emulated machine.
///
/// A fully populated MITS 88-2SIO contains two independent 6850 ACIAs. RusTair
/// therefore keeps separate RX/TX state for Port 0 and Port 1 instead of
/// aliasing both guest-visible port pairs to one host-side serial queue.
#[derive(Default)]
pub(super) struct IoDevices {
    serial: [SerialPort; 2],
    serial_board: SerialBoard,
    two_sio_control: [u8; 2],
}

impl IoDevices {
    pub(super) fn configure_serial_board(&mut self, board: SerialBoard) {
        self.serial_board = board;
        self.clear_serial();
    }

    pub(super) fn serial_board(&self) -> SerialBoard {
        self.serial_board
    }

    fn two_sio_status(&self, index: usize) -> u8 {
        let serial = &self.serial[index];
        (if serial.rx_empty() { 0 } else { 0x01 })
            | (if serial.tx_busy() { 0 } else { 0x02 })
    }

    fn write_two_sio_control(&mut self, index: usize, value: u8) {
        self.two_sio_control[index] = value;

        // MC6850 CR1:CR0 = 11 performs a master reset. We do not yet emulate
        // IRQ/RTS/framing electrically, but reset semantics are guest-visible
        // and must remain independent for the two ACIAs.
        if value & 0x03 == 0x03 {
            self.serial[index].clear();
        }
    }

    pub(super) fn input(&mut self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => match port {
                // MITS 88-SIO status convention used by the S2JS reference.
                // Bit 0 is set when the receive buffer is empty, while bits 6/7
                // are set while the transmit holding register is occupied.
                SIO_STATUS_PORT => {
                    let rx_empty = self.serial[0].rx_empty();
                    let tx_busy = self.serial[0].tx_busy();
                    (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
                }
                SIO_DATA_PORT => self.serial[0].read_rx().unwrap_or(0),

                // An absent 88-2SIO must not look TX-ready to software polling
                // either of its status registers.
                SIO2_PORT0_STATUS | SIO2_PORT1_STATUS => 0x00,
                _ => 0,
            },
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.two_sio_status(0),
                SIO2_PORT0_DATA => self.serial[0].read_rx().unwrap_or(0),
                SIO2_PORT1_STATUS => self.two_sio_status(1),
                SIO2_PORT1_DATA => self.serial[1].read_rx().unwrap_or(0),

                // The 88-SIO uses active-low ready flags. Returning all ones
                // for its absent status register keeps software waiting rather
                // than accidentally treating the uninstalled card as ready.
                SIO_STATUS_PORT => 0xff,
                _ => 0,
            },
        }
    }

    pub(super) fn output(&mut self, port: u8, value: u8) {
        match self.serial_board {
            SerialBoard::Sio88 => {
                if port == SIO_DATA_PORT {
                    self.serial[0].write_tx(value);
                }
            }
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.write_two_sio_control(0, value),
                SIO2_PORT0_DATA => self.serial[0].write_tx(value),
                SIO2_PORT1_STATUS => self.write_two_sio_control(1, value),
                SIO2_PORT1_DATA => self.serial[1].write_tx(value),
                _ => {}
            },
        }
    }

    // Port 0 is the legacy/default console path used by the existing ASR-33
    // integration and by the single-port 88-SIO.
    pub(super) fn serial_receive(&mut self, byte: u8) {
        self.serial[0].receive(byte);
    }

    pub(super) fn serial_rx_empty(&self) -> bool {
        self.serial[0].rx_empty()
    }

    pub(super) fn serial_rx_len(&self) -> usize {
        self.serial[0].rx_len()
    }

    pub(super) fn serial_tx_front(&self) -> Option<u8> {
        self.serial[0].tx_front()
    }

    pub(super) fn serial_tx_complete(&mut self) -> Option<u8> {
        self.serial[0].complete_tx()
    }

    pub(super) fn serial_tx_busy(&self) -> bool {
        self.serial[0].tx_busy()
    }

    pub(super) fn port1_receive(&mut self, byte: u8) {
        self.serial[1].receive(byte);
    }

    pub(super) fn port1_rx_empty(&self) -> bool {
        self.serial[1].rx_empty()
    }

    pub(super) fn port1_rx_len(&self) -> usize {
        self.serial[1].rx_len()
    }

    pub(super) fn port1_tx_front(&self) -> Option<u8> {
        self.serial[1].tx_front()
    }

    pub(super) fn port1_tx_complete(&mut self) -> Option<u8> {
        self.serial[1].complete_tx()
    }

    pub(super) fn port1_tx_busy(&self) -> bool {
        self.serial[1].tx_busy()
    }

    pub(super) fn clear_serial(&mut self) {
        self.serial[0].clear();
        self.serial[1].clear();
        self.two_sio_control.fill(0);
    }
}

// Keep serial-board configuration and Port 1 access next to the device decoder
// they control. Port 0 remains available through AltairBus's existing serial
// API so the ASR-33 path and the 88-SIO keep their established semantics.
impl AltairBus {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        self.io.configure_serial_board(board);
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.io.serial_board()
    }

    pub fn serial_port1_receive(&mut self, byte: u8) {
        self.io.port1_receive(byte);
    }

    pub fn serial_port1_rx_empty(&self) -> bool {
        self.io.port1_rx_empty()
    }

    pub fn serial_port1_rx_len(&self) -> usize {
        self.io.port1_rx_len()
    }

    pub fn serial_port1_tx_front(&self) -> Option<u8> {
        self.io.port1_tx_front()
    }

    pub fn serial_port1_tx_complete(&mut self) -> Option<u8> {
        self.io.port1_tx_complete()
    }

    pub fn serial_port1_tx_busy(&self) -> bool {
        self.io.port1_tx_busy()
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
    use crate::cpu8080::Bus;

    #[test]
    fn default_88_sio_does_not_alias_88_2sio_data_ports() {
        let mut io = IoDevices::default();
        assert_eq!(io.serial_board(), SerialBoard::Sio88);

        io.output(SIO2_PORT0_DATA, b'X');
        io.output(SIO2_PORT1_DATA, b'Y');
        assert!(!io.serial_tx_busy());
        assert!(!io.port1_tx_busy());

        io.output(SIO_DATA_PORT, b'S');
        assert_eq!(io.serial_tx_front(), Some(b'S'));
    }

    #[test]
    fn selected_88_2sio_exposes_two_independent_serial_ports() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO_STATUS_PORT), 0xff);

        io.output(SIO2_PORT0_DATA, b'0');
        io.output(SIO2_PORT1_DATA, b'1');
        assert_eq!(io.serial_tx_front(), Some(b'0'));
        assert_eq!(io.port1_tx_front(), Some(b'1'));

        io.port1_receive(b'B');
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x01, 0x01);
        assert_eq!(io.port1_rx_len(), 1);
        assert_eq!(io.input(SIO2_PORT1_DATA), b'B');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x01, 0x00);
    }

    #[test]
    fn two_sio_master_reset_is_per_port() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_DATA, b'0');
        io.output(SIO2_PORT1_DATA, b'1');

        io.output(SIO2_PORT0_STATUS, 0x03);

        assert!(!io.serial_tx_busy());
        assert!(io.port1_tx_busy());
        assert_eq!(io.port1_tx_front(), Some(b'1'));
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
