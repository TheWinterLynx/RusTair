use rand::RngCore;

use crate::config::{RamInit, RamSize};

/// Default installed RAM, preserving RusTair's behaviour before configurable RAM.
pub const MEM_SIZE: usize = 8 * 1024;
pub const MAX_MEM_SIZE: usize = 64 * 1024;
pub const MEMORY_BOARD_SIZE: usize = 1024;
pub const MEMORY_BOARD_COUNT: usize = MAX_MEM_SIZE / MEMORY_BOARD_SIZE;

/// Physical RAM backing store plus the front-panel write-protection latches.
///
/// The backing store covers the full 8080 address space, while `installed_size`
/// models how much RAM is actually fitted to the emulated Altair. Reads from
/// uninstalled addresses return zero and writes are ignored.
pub(super) struct Memory {
    bytes: [u8; MAX_MEM_SIZE],
    protected: [bool; MEMORY_BOARD_COUNT],
    installed_size: usize,
    init_mode: RamInit,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            bytes: [0; MAX_MEM_SIZE],
            protected: [false; MEMORY_BOARD_COUNT],
            installed_size: MEM_SIZE,
            init_mode: RamInit::Random,
        }
    }
}

impl Memory {
    pub(super) fn configure(&mut self, size: RamSize, init_mode: RamInit) {
        self.installed_size = size.bytes();
        self.init_mode = init_mode;
        self.clear_protection();
        self.initialize();
    }

    pub(super) fn installed_size(&self) -> usize {
        self.installed_size
    }

    pub(super) fn initialize(&mut self) {
        self.bytes.fill(0);
        if self.init_mode == RamInit::Random {
            rand::rng().fill_bytes(&mut self.bytes[..self.installed_size]);
        }
    }

    /// Force random contents in the installed RAM without changing the chosen
    /// power-on initialization mode.
    pub(super) fn randomize(&mut self) {
        self.bytes.fill(0);
        rand::rng().fill_bytes(&mut self.bytes[..self.installed_size]);
    }

    /// Programmatic image loading intentionally bypasses front-panel write
    /// protection, matching the previous AltairBus behavior.
    pub(super) fn load(&mut self, address: u16, data: &[u8]) {
        let start = address as usize;
        if start >= self.installed_size {
            return;
        }
        let len = data.len().min(self.installed_size - start);
        self.bytes[start..start + len].copy_from_slice(&data[..len]);
    }

    pub(super) fn clear_protection(&mut self) {
        self.protected.fill(false);
    }

    pub(super) fn board_index(address: u16) -> Option<usize> {
        let address = address as usize;
        (address < MAX_MEM_SIZE).then_some(address / MEMORY_BOARD_SIZE)
    }

    pub(super) fn is_protected(&self, address: u16) -> bool {
        if address as usize >= self.installed_size {
            return false;
        }
        Self::board_index(address)
            .map(|index| self.protected[index])
            .unwrap_or(false)
    }

    pub(super) fn set_protected(&mut self, address: u16, protected: bool) {
        if address as usize >= self.installed_size {
            return;
        }
        if let Some(index) = Self::board_index(address) {
            self.protected[index] = protected;
        }
    }

    pub(super) fn read(&self, address: u16) -> u8 {
        if address as usize >= self.installed_size {
            return 0;
        }
        self.bytes[address as usize]
    }

    pub(super) fn write(&mut self, address: u16, value: u8) {
        if address as usize >= self.installed_size || self.is_protected(address) {
            return;
        }
        self.bytes[address as usize] = value;
    }
}
