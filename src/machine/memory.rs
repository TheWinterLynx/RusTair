use rand::RngCore;

use crate::config::{RamBoardProfile, RamInit, RamSize};

/// Default installed RAM, preserving RusTair's behaviour before configurable RAM.
pub const MEM_SIZE: usize = 8 * 1024;
pub const MAX_MEM_SIZE: usize = 64 * 1024;
pub const MEMORY_BOARD_SIZE: usize = 1024;
pub const MEMORY_BOARD_COUNT: usize = MAX_MEM_SIZE / MEMORY_BOARD_SIZE;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryReadyPhase {
    T1,
    T2,
    Tw,
    T3,
    Other,
}

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
    board_profiles: [RamBoardProfile; MEMORY_BOARD_COUNT],
    read_wait_active: bool,
    read_wait_remaining: u8,
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
            board_profiles: [RamBoardProfile::FastNoWait; MEMORY_BOARD_COUNT],
            read_wait_active: false,
            read_wait_remaining: 0,
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
        self.reset_timing();
        self.initialize();
    }

    pub(super) fn configure_board_profile(&mut self, profile: RamBoardProfile) {
        // Storage is per 1 KiB slot even though the current UI applies one
        // profile to all installed slots. This deliberately leaves room for
        // mixed-card Altair configurations without another memory rewrite.
        self.board_profiles.fill(profile);
        self.reset_timing();
    }

    pub(super) fn board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        if address as usize >= self.installed_size {
            return None;
        }
        Self::board_index(address).map(|index| self.board_profiles[index])
    }

    fn read_wait_states(&self, address: u16) -> u8 {
        self.board_profile(address)
            .map(RamBoardProfile::read_wait_states)
            .unwrap_or(0)
    }

    pub(super) fn reset_timing(&mut self) {
        self.read_wait_active = false;
        self.read_wait_remaining = 0;
    }

    /// Return the memory-card PRDY contribution for the current 8080 T-state.
    /// The MITS 1K board starts its slowdown pulse with PSYNC and produces two
    /// actual TW cycles on reads. Writes and uninstalled addresses never wait.
    pub(super) fn ready_for_t_state(
        &mut self,
        address: u16,
        memory_read: bool,
        phase: MemoryReadyPhase,
    ) -> bool {
        if !memory_read {
            self.reset_timing();
            return true;
        }

        match phase {
            MemoryReadyPhase::T1 => {
                self.read_wait_remaining = self.read_wait_states(address);
                self.read_wait_active = self.read_wait_remaining != 0;
                !self.read_wait_active
            }
            MemoryReadyPhase::T2 => !self.read_wait_active,
            MemoryReadyPhase::Tw if self.read_wait_active => {
                if self.read_wait_remaining > 1 {
                    self.read_wait_remaining -= 1;
                    false
                } else {
                    self.reset_timing();
                    true
                }
            }
            MemoryReadyPhase::Tw => true,
            MemoryReadyPhase::T3 | MemoryReadyPhase::Other => {
                self.reset_timing();
                true
            }
        }
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

    pub(super) fn randomize(&mut self) {
        self.clear_transient_guards();
        self.bytes.fill(0);
        rand::rng().fill_bytes(&mut self.bytes[..self.installed_size]);
    }

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

    pub(super) fn load(&mut self, address: u16, data: &[u8]) {
        let start = address as usize;
        if start >= self.installed_size {
            return;
        }
        let len = data.len().min(self.installed_size - start);
        self.bytes[start..start + len].copy_from_slice(&data[..len]);
    }

    /// Inspect a physical RAM byte without triggering guest-visible read side
    /// effects. `None` means no RAM is physically installed at that address.
    pub(super) fn peek(&self, address: u16) -> Option<u8> {
        let index = address as usize;
        (index < self.installed_size).then_some(self.bytes[index])
    }

    /// Preview exactly the byte a guest memory read would receive without
    /// consuming any transient hardware/compatibility state. This differs from
    /// `peek`: uninstalled RAM reads as 00h and the BASIC 3.2 FFFFh sentinel is
    /// represented exactly as the next real guest read would see it.
    pub(super) fn preview_read(&self, address: u16) -> u8 {
        if address as usize >= self.installed_size {
            return 0;
        }
        if address == u16::MAX && self.basic32_probe_guard {
            if let Some(written) = self.basic32_probe_write {
                return written ^ 0xff;
            }
        }
        self.bytes[address as usize]
    }

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
    pub fn peek_memory(&self, address: u16) -> Option<u8> {
        self.memory.peek(address)
    }

    pub(crate) fn preview_guest_memory(&self, address: u16) -> u8 {
        self.memory.preview_read(address)
    }

    pub fn debugger_write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> bool {
        self.memory
            .debugger_write(address, value, respect_protection)
    }

    pub(crate) fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.memory.configure_board_profile(profile);
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        self.memory.board_profile(address)
    }

    pub(crate) fn cycle_memory_ready(
        &mut self,
        address: u16,
        memory_read: bool,
        phase: MemoryReadyPhase,
    ) -> bool {
        let ready = self.memory.ready_for_t_state(address, memory_read, phase);
        self.s100.set_memory_ready_input(ready);
        ready
    }

    /// Host freezes physical STOP at the first TW instead of burning millions of
    /// identical wait clocks. A real memory-board one-shot would expire during
    /// the operator pause, so settle that transient PRDY source before resume.
    pub(crate) fn cycle_settle_memory_ready_after_panel_freeze(&mut self) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
    }

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

    pub(crate) fn cycle_output_port(&mut self, port: u8, value: u8) {
        if port != 0xff {
            self.io.output(port, value);
        }
    }

    /// Canonical raw S-100 status latch. Debugger/teaching code must read this
    /// rather than reverse-engineering electrical state from optical LED duty.
    pub(crate) fn raw_s100_status_word(&self) -> u8 {
        let s = self.s100.signals();
        (u8::from(s.memr) << 7)
            | (u8::from(s.inp) << 6)
            | (u8::from(s.m1) << 5)
            | (u8::from(s.out) << 4)
            | (u8::from(s.hlta) << 3)
            | (u8::from(s.stack) << 2)
            | (u8::from(s.wo) << 1)
            | u8::from(s.int_ack)
    }

    pub(crate) fn raw_s100_inte(&self) -> bool {
        self.s100.signals().inte
    }

    pub(crate) fn raw_s100_prot(&self) -> bool {
        self.s100.signals().prot
    }

    pub(crate) fn raw_s100_wait(&self) -> bool {
        self.s100.signals().wait
    }

    pub(crate) fn raw_s100_hlda(&self) -> bool {
        self.s100.signals().hlda
    }

    pub(crate) fn raw_s100_data_in(&self) -> Option<u8> {
        self.s100.signals().data_in
    }

    pub(crate) fn raw_s100_data_out(&self) -> Option<u8> {
        self.s100.signals().data_out
    }

    pub(crate) fn raw_cpu_data(&self) -> Option<u8> {
        self.s100.signals().cpu_data
    }

    pub(crate) fn raw_panel_data(&self) -> u8 {
        self.s100.signals().panel_data
    }

    pub(crate) fn cycle_drive_s100_t_state(
        &mut self,
        address: Option<u16>,
        cpu_data: Option<u8>,
        data_in: Option<u8>,
        data_out: Option<u8>,
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
            cpu_data,
            data_in,
            data_out,
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
        assert_eq!(memory.preview_read(0x0100), 0x00);
    }

    #[test]
    fn preview_does_not_consume_basic32_probe_guard() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K64, RamInit::Zeroed);
        assert!(memory.arm_basic32_full_memory_probe_guard());

        memory.write(0xffff, 0x37);
        assert_eq!(memory.peek(0xffff), Some(0));
        assert_eq!(memory.preview_read(0xffff), 0xc8);
        assert_eq!(memory.preview_read(0xffff), 0xc8);
        assert_eq!(memory.read(0xffff), 0xc8);
        assert_eq!(memory.read(0xffff), 0x00);
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
        assert_eq!(after.cpu_data, before.cpu_data);
        assert_eq!(after.data_in, before.data_in);
        assert_eq!(after.data_out, before.data_out);
        assert_eq!(after.panel_data, before.panel_data);
        assert_eq!(after.memr, before.memr);
        assert_eq!(after.m1, before.m1);
    }

    #[test]
    fn raw_s100_status_and_data_domains_read_electrical_state_not_led_persistence() {
        let mut bus = super::super::AltairBus::default();
        bus.cycle_drive_s100_t_state(
            Some(0x1234),
            Some(0xa2),
            None,
            Some(0xa2),
            Some(0xa2),
            false,
            true,
            false,
            false,
        );
        assert_eq!(bus.raw_s100_status_word(), 0xa2);
        assert_eq!(bus.raw_cpu_data(), Some(0xa2));
        assert_eq!(bus.raw_s100_data_in(), None);
        assert_eq!(bus.raw_s100_data_out(), Some(0xa2));
        assert_eq!(bus.raw_panel_data(), 0x00);
        assert!(!bus.raw_s100_inte());
        assert!(!bus.raw_s100_prot());
        assert!(!bus.raw_s100_wait());
        assert!(!bus.raw_s100_hlda());
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn mits_1k_read_timing_yields_two_wait_cycles() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        memory.configure_board_profile(RamBoardProfile::Mits1KStatic1975);

        assert!(!memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T1));
        assert!(!memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T2));
        assert!(!memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::Tw));
        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::Tw));
        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T3));
    }

    #[test]
    fn mits_1k_write_and_fast_profile_do_not_stretch_ready() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        memory.configure_board_profile(RamBoardProfile::Mits1KStatic1975);
        assert!(memory.ready_for_t_state(0x0000, false, MemoryReadyPhase::T1));
        memory.configure_board_profile(RamBoardProfile::FastNoWait);
        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T1));
        assert!(memory.ready_for_t_state(0x0000, true, MemoryReadyPhase::T2));
    }
}
