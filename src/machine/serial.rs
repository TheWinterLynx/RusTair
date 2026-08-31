use std::collections::VecDeque;

// Keep the audited COM2502 implementation next to the legacy byte-level
// SerialPort while the 88-SIO production path is migrated in controlled steps.
// Exposing it only to the parent machine module prevents application code from
// bypassing the S-100 board wrapper.
#[path = "sio.rs"]
pub(super) mod sio;

/// Byte-level state retained temporarily for serial paths that have not yet
/// migrated to a board-owned UART implementation.
///
/// The audited 88-SIO no longer belongs conceptually to this type; its finite
/// COM2502 implementation is `serial::sio::SioPort` above. Keeping this legacy
/// helper during the staged migration avoids mixing a core-chip rewrite with a
/// large S-100 decoder change in one untestable commit.
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

    /// Observe the next receive byte without consuming it. Debugger/inspector
    /// code must use this rather than `read_rx`, because a real DATA-port read
    /// has the guest-visible side effect of removing the character.
    pub(super) fn rx_front(&self) -> Option<u8> {
        self.rx.front().copied()
    }

    pub(super) fn read_rx(&mut self) -> Option<u8> {
        self.rx.pop_front()
    }

    pub(super) fn clear_rx(&mut self) {
        self.rx.clear();
    }

    pub(super) fn tx_front(&self) -> Option<u8> {
        self.tx.front().copied()
    }

    pub(super) fn complete_tx(&mut self) -> Option<u8> {
        self.tx.pop_front()
    }

    pub(super) fn clear_tx(&mut self) {
        self.tx.clear();
    }

    pub(super) fn tx_busy(&self) -> bool {
        !self.tx.is_empty()
    }

    /// Correct software polls READY before writing. Preserve the previous
    /// byte-level model by replacing an outstanding byte on an unchecked write.
    pub(super) fn write_tx(&mut self, byte: u8) {
        self.tx.clear();
        self.tx.push_back(byte);
    }

    pub(super) fn clear(&mut self) {
        self.rx.clear();
        self.tx.clear();
    }
}
