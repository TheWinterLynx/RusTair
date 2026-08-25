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

    /// Inspect a physical RAM byte without triggering any guest-visible read
    /// side effects. This is intentionally separate from `read`: debugger and
    /// visualizer tools must not consume transient compatibility guards such as
    /// the BASIC 3.2 FFFFh probe sentinel.
    pub(super) fn peek(&self, address: u16) -> Option<u8> {
        let index = address as usize;
        (index < self.installed_size).then_some(self.bytes[index])
    }

    /// Debugger/editor write to the physical RAM backing store.
    ///
    /// This never writes into uninstalled address space and intentionally does
    /// not participate in guest-visible compatibility guards. When
    /// `respect_protection` is true it also honors RusTair's current 1 KiB
    /// front-panel protection granularity; false is an explicit debugger
    /// override.
    pub(super) fn debugger_write(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> bool {
        let index = address as usize;
        if index >= self.installed_size {
            return false;
        }
        if respect_protection && self.is_protected(address) {
            return false;
        }
        self.bytes[index] = value;
        true
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

impl super::AltairBus {
    /// Non-invasive debugger read. `None` means that address is outside the
    /// physically installed RAM rather than a stored zero byte.
    pub fn peek_memory(&self, address: u16) -> Option<u8> {
        self.memory.peek(address)
    }

    /// Edit installed RAM from the debugger. This bypasses guest-visible memory
    /// side effects; callers choose whether the current protection latch is
    /// honored or deliberately overridden.
    pub fn debugger_write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> bool {
        self.memory
            .debugger_write(address, value, respect_protection)
    }

    // Raw accessors used only by the T-state CPU backend. Unlike the legacy
    // `Bus` trait implementation these perform the functional RAM/I/O action
    // without synthesizing an aggregate S-100 machine cycle. The caller drives
    // the actual per-T-state electrical sample separately through
    // `cycle_drive_s100_t_state`, preventing duplicate panel activity.
    pub(crate) fn cycle_read_memory(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }

    pub(crate) fn cycle_peek_memory(&self, address: u16) -> u8 {
        self.memory.peek(address).unwrap_or(0)
    }

    pub(crate) fn cycle_write_memory(&mut self, address: u16, value: u8) {
        self.memory.write(address, value);
    }

    pub(crate) fn cycle_input_port(&mut self, port: u8) -> u8 {
        if port == 0xff {
            self.panel.input()
        } else {
            self.io.input(port)
        }
    }

    pub(crate) fn cycle_peek_input_port(&self, port: u8) -> u8 {
        self.peek_io_port(port)
    }

    pub(crate) fn cycle_output_port(&mut self, port: u8, value: u8) {
        if port != 0xff {
            self.io.output(port, value);
        }
    }

    pub(crate) fn cycle_drive_s100_t_state(
        &mut self,
        address: Option<u16>,
        data: Option<u8>,
        status_word: Option<u8>,
        inte: bool,
        ready: bool,
        wait: bool,
        hlda: bool,
    ) {
        let protected = address
            .map(|address| self.memory.is_protected(address))
            .unwrap_or(false);
        self.cpu_inte = inte;
        self.s100.drive_cpu_t_state(
            address,
            data,
            status_word,
            protected,
            inte,
            ready,
            wait,
            hlda,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_distinguishes_uninstalled_memory() {
        let mut memory = Memory::default();
        memory.configure(RamSize::Bytes256, RamInit::Zeroed);
        assert_eq!(memory.peek(0x00ff), Some(0));
        assert_eq!(memory.peek(0x0100), None);
    }

    #[test]
    fn peek_does_not_consume_basic32_probe_guard() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K64, RamInit::Zeroed);
        assert!(memory.arm_basic32_full_memory_probe_guard());

        memory.write(0xffff, 0x37);
        assert_eq!(memory.peek(0xffff), Some(0));
        assert_ne!(memory.read(0xffff), 0x37);
    }

    #[test]
    fn debugger_write_never_creates_uninstalled_ram() {
        let mut memory = Memory::default();
        memory.configure(RamSize::Bytes256, RamInit::Zeroed);
        assert!(!memory.debugger_write(0x0100, 0x5a, false));
        assert_eq!(memory.peek(0x0100), None);
    }

    #[test]
    fn debugger_write_can_respect_or_override_protection() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        memory.set_protected(0x0010, true);

        assert!(!memory.debugger_write(0x0010, 0x12, true));
        assert_eq!(memory.peek(0x0010), Some(0x00));

        assert!(memory.debugger_write(0x0010, 0x34, false));
        assert_eq!(memory.peek(0x0010), Some(0x34));
    }

    #[test]
    fn cycle_raw_memory_path_does_not_synthesize_panel_activity() {
        let mut bus = super::super::AltairBus::default();
        bus.load(0x0010, &[0x5a]);
        let before = bus.s100.signals();
        assert_eq!(bus.cycle_read_memory(0x0010), 0x5a);
        bus.cycle_write_memory(0x0011, 0xa5);
        assert_eq!(bus.peek_memory(0x0011), Some(0xa5));
        let after = bus.s100.signals();
        assert_eq!(after.address, before.address);
        assert_eq!(after.data, before.data);
        assert_eq!(after.memr, before.memr);
        assert_eq!(after.m1, before.m1);
    }
}
