use rand::RngCore;

pub const MEM_SIZE: usize = 8 * 1024;
pub const MEMORY_BOARD_SIZE: usize = 1024;
pub const MEMORY_BOARD_COUNT: usize = MEM_SIZE / MEMORY_BOARD_SIZE;

/// Installed RAM and the front-panel write-protection latches associated with
/// the emulated 1 KiB MITS-style memory boards.
pub(super) struct Memory {
    bytes: [u8; MEM_SIZE],
    protected: [bool; MEMORY_BOARD_COUNT],
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            bytes: [0; MEM_SIZE],
            protected: [false; MEMORY_BOARD_COUNT],
        }
    }
}

impl Memory {
    pub(super) fn randomize(&mut self) {
        rand::rng().fill_bytes(&mut self.bytes);
    }

    /// Programmatic image loading intentionally bypasses front-panel write
    /// protection, matching the previous AltairBus behavior.
    pub(super) fn load(&mut self, address: u16, data: &[u8]) {
        let start = address as usize;
        if start >= MEM_SIZE {
            return;
        }
        let len = data.len().min(MEM_SIZE - start);
        self.bytes[start..start + len].copy_from_slice(&data[..len]);
    }

    pub(super) fn clear_protection(&mut self) {
        self.protected.fill(false);
    }

    pub(super) fn board_index(address: u16) -> Option<usize> {
        let address = address as usize;
        (address < MEM_SIZE).then_some(address / MEMORY_BOARD_SIZE)
    }

    pub(super) fn is_protected(&self, address: u16) -> bool {
        Self::board_index(address)
            .map(|index| self.protected[index])
            .unwrap_or(false)
    }

    pub(super) fn set_protected(&mut self, address: u16, protected: bool) {
        if let Some(index) = Self::board_index(address) {
            self.protected[index] = protected;
        }
    }

    pub(super) fn read(&self, address: u16) -> u8 {
        self.bytes.get(address as usize).copied().unwrap_or(0)
    }

    pub(super) fn write(&mut self, address: u16, value: u8) {
        if self.is_protected(address) {
            return;
        }
        if let Some(byte) = self.bytes.get_mut(address as usize) {
            *byte = value;
        }
    }
}
