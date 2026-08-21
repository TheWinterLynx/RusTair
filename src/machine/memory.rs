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
    basic32_probe_guard: bool,
    basic32_probe_write: Option<u8>,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            bytes: [0; MAX_MEM_SIZE],
            protected: [false; MEMORY_BOARD_COUNT],
            installed_size: MEM_SIZE,
            init_mode: RamInit::Random,
            basic32_probe_guard: false,
            basic32_probe_write: None,
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
        self.clear_transient_guards();
        self.bytes.fill(0);
        if self.init_mode == RamInit::Random {
            rand::rng().fill_bytes(&mut self.bytes[..self.installed_size]);
        }
    }

    /// Force random contents in the installed RAM without changing the chosen
    /// power-on initialization mode.
    pub(super) fn randomize(&mut self) {
        self.clear_transient_guards();
        self.bytes.fill(0);
        rand::rng().fill_bytes(&mut self.bytes[..self.installed_size]);
    }

    /// Altair BASIC 3.2 probes for the first non-writable address when the user
    /// presses RETURN at `MEMORY SIZE?`. On a completely writable 64 KiB address
    /// space its 16-bit pointer wraps from FFFFh to 0000h and the probe destroys
    /// BASIC itself. Arm a one-shot sentinel at FFFFh only for that bundled
    /// legacy BASIC startup. The first probe write is rejected and its matching
    /// read returns a different value, after which the guard automatically
    /// disappears and FFFFh becomes ordinary RAM again.
    pub(super) fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.clear_transient_guards();
        if self.installed_size != MAX_MEM_SIZE {
            return false;
        }
        self.basic32_probe_guard = true;
        true
    }

    pub(super) fn clear_transient_guards(&mut self) {
        self.basic32_probe_guard = false;
        self.basic32_probe_write = None;
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

    pub(super) fn read(&mut self, address: u16) -> u8 {
        if address as usize >= self.installed_size {
            return 0;
        }

        if address == u16::MAX && self.basic32_probe_guard {
            if let Some(written) = self.basic32_probe_write.take() {
                self.basic32_probe_guard = false;
                return written ^ 0xff;
            }
        }

        self.bytes[address as usize]
    }

    pub(super) fn write(&mut self, address: u16, value: u8) {
        if address as usize >= self.installed_size || self.is_protected(address) {
            return;
        }

        if address == u16::MAX && self.basic32_probe_guard {
            self.basic32_probe_write = Some(value);
            return;
        }

        self.bytes[address as usize] = value;
    }
}
