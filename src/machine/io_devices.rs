use super::serial::SerialPort;

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
}

impl IoDevices {
    pub(super) fn input(&mut self, port: u8) -> u8 {
        match port {
            // MITS 88-SIO status convention used by the S2JS reference.
            // Bit 0 is set when the receive buffer is empty, while bits 6/7
            // are set while the transmit holding register is occupied.
            SIO_STATUS_PORT => {
                let rx_empty = self.serial.rx_empty();
                let tx_busy = self.serial.tx_busy();
                (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
            }
            SIO_DATA_PORT => self.serial.read_rx().unwrap_or(0),

            // MITS 2SIO / 8251 convention: bit 0 = RX ready, bit 1 = TX ready.
            SIO2_STATUS_PORT => {
                (if self.serial.rx_empty() { 0 } else { 0x01 })
                    | (if self.serial.tx_busy() { 0 } else { 0x02 })
            }
            SIO2_DATA_PORT => self.serial.read_rx().unwrap_or(0),
            _ => 0,
        }
    }

    pub(super) fn output(&mut self, port: u8, value: u8) {
        match port {
            SIO_DATA_PORT | SIO2_DATA_PORT => self.serial.write_tx(value),
            _ => {}
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
