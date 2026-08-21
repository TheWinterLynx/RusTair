use std::collections::VecDeque;

/// Byte-level state of the emulated Altair serial interface.
///
/// The receive side may buffer host-originated input. The transmit side behaves
/// as a one-byte holding register: guest software observes BUSY until the active
/// endpoint explicitly completes the pending character.
#[derive(Default)]
pub(super) struct SerialPort {
    rx: VecDeque<u8>,
    tx: VecDeque<u8>,
}

impl SerialPort {
    pub(super) fn receive(&mut self, byte: u8) {
        self.rx.push_back(byte);
    }

    pub(super) fn rx_empty(&self) -> bool {
        self.rx.is_empty()
    }

    pub(super) fn rx_len(&self) -> usize {
        self.rx.len()
    }

    pub(super) fn read_rx(&mut self) -> Option<u8> {
        self.rx.pop_front()
    }

    pub(super) fn tx_front(&self) -> Option<u8> {
        self.tx.front().copied()
    }

    pub(super) fn complete_tx(&mut self) -> Option<u8> {
        self.tx.pop_front()
    }

    pub(super) fn tx_busy(&self) -> bool {
        !self.tx.is_empty()
    }

    /// Correct software polls READY before writing. Preserve the previous
    /// hardware model by replacing an outstanding byte on an unchecked write.
    pub(super) fn write_tx(&mut self, byte: u8) {
        self.tx.clear();
        self.tx.push_back(byte);
    }

    pub(super) fn clear(&mut self) {
        self.rx.clear();
        self.tx.clear();
    }
}
