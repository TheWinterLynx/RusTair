use crate::config::{
    RamBoardProfile, RamInit, RamSize, S100HardwareConfig, SerialBoard, SioHardwareConfig,
    TwoSioInterruptWiring, TwoSioStraps,
};
use crate::s100_runtime::{RuntimeMemoryInspection, S100RuntimeFabric};
pub(crate) use crate::s100_runtime::S100_OPEN_BUS_VALUE;

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

/// Transitional memory facade while Cycle and serial are moved onto the live
/// S-100 fabric. RAM bytes themselves already live exclusively in RuntimeRamCard.
pub(super) struct Memory {
    fabric: S100RuntimeFabric,
    ram_size: RamSize,
    init_mode: RamInit,
    board_profile: RamBoardProfile,
    legacy_aggregate: bool,
    read_wait_active: bool,
    read_wait_remaining: u8,
    basic32_probe_guard: bool,
    basic32_probe_write: Option<u8>,
}

impl Default for Memory {
    fn default() -> Self {
        let ram_size = RamSize::K8;
        let init_mode = RamInit::Random;
        let board_profile = RamBoardProfile::FastNoWait;
        Self {
            fabric: Self::legacy_fabric(ram_size, board_profile, init_mode),
            ram_size,
            init_mode,
            board_profile,
            legacy_aggregate: true,
            read_wait_active: false,
            read_wait_remaining: 0,
            basic32_probe_guard: false,
            basic32_probe_write: None,
        }
    }
}

impl Memory {
    fn legacy_hardware(size: RamSize, profile: RamBoardProfile) -> S100HardwareConfig {
        S100HardwareConfig::from_legacy_globals(
            size,
            profile,
            SerialBoard::Sio88,
            SioHardwareConfig::default(),
            TwoSioStraps::default(),
            TwoSioInterruptWiring::default(),
        )
    }

    fn legacy_fabric(
        size: RamSize,
        profile: RamBoardProfile,
        init_mode: RamInit,
    ) -> S100RuntimeFabric {
        S100RuntimeFabric::new(Self::legacy_hardware(size, profile), init_mode)
            .expect("legacy RAM compatibility assembly must be valid")
    }

    pub(super) fn configure(&mut self, size: RamSize, init_mode: RamInit) {
        self.ram_size = size;
        self.init_mode = init_mode;
        self.legacy_aggregate = true;
        self.fabric = Self::legacy_fabric(size, self.board_profile, init_mode);
        self.clear_transient_guards();
        self.reset_timing();
    }

    pub(super) fn configure_hardware(
        &mut self,
        hardware: S100HardwareConfig,
        init_mode: RamInit,
    ) -> Result<(), crate::s100_runtime::S100RuntimeBuildError> {
        self.fabric = S100RuntimeFabric::new(hardware, init_mode)?;
        self.init_mode = init_mode;
        self.legacy_aggregate = false;
        self.clear_transient_guards();
        self.reset_timing();
        Ok(())
    }

    pub(super) fn hardware(&self) -> S100HardwareConfig {
        self.fabric.hardware()
    }

    pub(super) fn inspect(&self, address: u16) -> RuntimeMemoryInspection {
        self.fabric.inspect_memory(address)
    }

    pub(super) fn configure_board_profile(&mut self, profile: RamBoardProfile) {
        self.board_profile = profile;
        self.reset_timing();
    }

    pub(super) fn board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        (self.fabric.mapped_ram_card_count(address) != 0).then_some(self.board_profile)
    }

    pub(super) fn read_wait_states(&self, address: u16) -> u8 {
        if self.legacy_aggregate {
            return self
                .board_profile(address)
                .map(RamBoardProfile::read_wait_states)
                .unwrap_or(0);
        }
        self.fabric.fast_read_wait_states(address)
    }

    pub(super) fn reset_timing(&mut self) {
        self.read_wait_active = false;
        self.read_wait_remaining = 0;
    }

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
        self.fabric.installed_ram_bytes()
    }

    pub(super) fn initialize(&mut self) {
        self.clear_transient_guards();
        self.fabric.initialize_memory(self.init_mode);
        self.reset_timing();
    }

    pub(super) fn randomize(&mut self) {
        self.clear_transient_guards();
        self.fabric.initialize_memory(RamInit::Random);
        self.reset_timing();
    }

    pub(super) fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.clear_transient_guards();
        if self.fabric.mapped_ram_card_count(u16::MAX) != 1
            || self.fabric.installed_ram_bytes() != MAX_MEM_SIZE
        {
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
        let _ = self.fabric.load_bytes(address, data);
    }

    pub(super) fn peek(&self, address: u16) -> Option<u8> {
        self.fabric.peek_unique_memory(address)
    }

    fn resolved_preview(&self, address: u16) -> u8 {
        let inspection = self.fabric.inspect_memory(address);
        match inspection.drivers.as_slice() {
            [] => S100_OPEN_BUS_VALUE,
            [driver] => driver.value,
            drivers => {
                let first = drivers[0].value;
                if drivers.iter().all(|driver| driver.value == first) {
                    first
                } else {
                    S100_OPEN_BUS_VALUE
                }
            }
        }
    }

    pub(super) fn preview_read(&self, address: u16) -> u8 {
        if address == u16::MAX && self.basic32_probe_guard {
            if let Some(written) = self.basic32_probe_write {
                return written ^ 0xff;
            }
        }
        self.resolved_preview(address)
    }

    pub(super) fn debugger_write(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> bool {
        self.fabric
            .write_unique_memory(address, value, respect_protection)
    }

    pub(super) fn clear_protection(&mut self) {
        self.fabric.clear_memory_protection();
    }

    pub(super) fn board_index(address: u16) -> Option<usize> {
        Some(address as usize / MEMORY_BOARD_SIZE)
    }

    pub(super) fn is_protected(&self, address: u16) -> bool {
        self.fabric.memory_is_protected(address)
    }

    pub(super) fn set_protected(&mut self, address: u16, protected: bool) {
        let _ = self
            .fabric
            .set_unique_memory_protection(address, protected);
    }

    fn compatibility_read_override(&mut self, address: u16) -> Option<u8> {
        if address == u16::MAX && self.basic32_probe_guard {
            if let Some(written) = self.basic32_probe_write.take() {
                self.basic32_probe_guard = false;
                return Some(written ^ 0xff);
            }
        }
        None
    }

    /// Fast guest read: CPU board -> S-100 -> installed RAM card(s) -> DI.
    pub(super) fn read(&mut self, address: u16) -> u8 {
        if let Some(value) = self.compatibility_read_override(address) {
            return value;
        }
        self.fabric
            .fast_memory_read(address, 0x82)
            .unwrap_or(S100_OPEN_BUS_VALUE)
    }

    /// Fast guest write: CPU board pWR/DO -> Display/Control MWRT -> RAM card.
    pub(super) fn write(&mut self, address: u16, value: u8) {
        if address == u16::MAX && self.basic32_probe_guard {
            self.basic32_probe_write = Some(value);
            return;
        }
        let _ = self.fabric.fast_memory_write(address, value, 0x00);
    }

    /// Cycle still performs side effects at its validated T3 boundary in this
    /// cut, but reads/writes the exact same card storage rather than a shadow RAM.
    pub(super) fn cycle_read(&mut self, address: u16) -> u8 {
        self.compatibility_read_override(address)
            .unwrap_or_else(|| self.resolved_preview(address))
    }

    pub(super) fn cycle_write(&mut self, address: u16, value: u8) {
        if address == u16::MAX && self.basic32_probe_guard {
            self.basic32_probe_write = Some(value);
            return;
        }
        let _ = self.fabric.write_unique_memory(address, value, true);
    }
}

impl super::AltairBus {
    pub fn peek_memory(&self, address: u16) -> Option<u8> {
        self.memory.peek(address)
    }

    pub(crate) fn inspect_memory_mapping(&self, address: u16) -> RuntimeMemoryInspection {
        self.memory.inspect(address)
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

    pub(crate) fn configure_s100_hardware_memory(
        &mut self,
        hardware: S100HardwareConfig,
        init: RamInit,
    ) -> Result<(), crate::s100_runtime::S100RuntimeBuildError> {
        self.memory.configure_hardware(hardware, init)
    }

    pub(crate) fn s100_hardware_memory(&self) -> S100HardwareConfig {
        self.memory.hardware()
    }

    pub(crate) fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.memory.configure_board_profile(profile);
        self.fast_wait_t_states = 0;
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn fast_account_memory_read_wait(&mut self, address: u16) {
        self.fast_wait_t_states = self
            .fast_wait_t_states
            .saturating_add(u32::from(self.memory.read_wait_states(address)));
    }

    pub(crate) fn take_fast_memory_wait_t_states(&mut self) -> u32 {
        std::mem::take(&mut self.fast_wait_t_states)
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
        let memory_ready = self.memory.ready_for_t_state(address, memory_read, phase);
        let signals = self.s100.signals();
        let inp_about_to_latch = !memory_read
            && phase == MemoryReadyPhase::T2
            && signals.psync
            && signals.data_out.map_or(false, |word| word & 0x40 != 0);
        let input_read = !memory_read
            && match phase {
                MemoryReadyPhase::T1 => false,
                MemoryReadyPhase::T2 => signals.inp || inp_about_to_latch,
                _ => signals.inp,
            };
        let io_wait_selected = input_read && self.io.input_wait_states(address as u8) != 0;
        let io_ready = self
            .io
            .ready_for_input_t_state(address as u8, input_read, phase);
        let ready = memory_ready && io_ready;
        let phi1_owned_2sio_transition = io_wait_selected
            && matches!(phase, MemoryReadyPhase::T2 | MemoryReadyPhase::Tw);
        if !phi1_owned_2sio_transition {
            self.s100.set_memory_ready_input(ready);
        }
        ready
    }

    pub(crate) fn cycle_settle_memory_ready_after_panel_freeze(&mut self) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn cycle_read_memory(&mut self, address: u16) -> u8 {
        self.memory.cycle_read(address)
    }

    pub(crate) fn cycle_peek_memory(&self, address: u16) -> u8 {
        self.memory.preview_read(address)
    }

    pub(crate) fn cycle_write_memory(&mut self, address: u16, value: u8) {
        self.memory.cycle_write(address, value);
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
    fn peek_distinguishes_uninstalled_memory_from_open_bus() {
        let mut memory = Memory::default();
        memory.configure(RamSize::Bytes256, RamInit::Zeroed);
        assert_eq!(memory.peek(0x00ff), Some(0));
        assert_eq!(memory.peek(0x0100), None);
        assert_eq!(memory.preview_read(0x0100), S100_OPEN_BUS_VALUE);
        assert_eq!(memory.read(0x0100), S100_OPEN_BUS_VALUE);
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
    fn debugger_write_can_respect_or_override_card_protection() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        memory.set_protected(0x0010, true);
        assert!(!memory.debugger_write(0x0010, 0x12, true));
        assert_eq!(memory.peek(0x0010), Some(0x00));
        assert!(memory.debugger_write(0x0010, 0x34, false));
        assert_eq!(memory.peek(0x0010), Some(0x34));
    }

    #[test]
    fn fast_guest_read_and_write_cross_live_backplane() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        memory.write(0x0010, 0x5a);
        assert_eq!(memory.peek(0x0010), Some(0x5a));
        assert_eq!(memory.read(0x0010), 0x5a);
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    #[test]
    fn legacy_mits_1k_profile_still_yields_two_wait_cycles_without_replacing_bytes() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        memory.write(0x0010, 0x5a);
        memory.configure_board_profile(RamBoardProfile::Mits1KStatic1975);
        assert_eq!(memory.peek(0x0010), Some(0x5a));
        assert!(!memory.ready_for_t_state(0, true, MemoryReadyPhase::T1));
        assert!(!memory.ready_for_t_state(0, true, MemoryReadyPhase::T2));
        assert!(!memory.ready_for_t_state(0, true, MemoryReadyPhase::Tw));
        assert!(memory.ready_for_t_state(0, true, MemoryReadyPhase::Tw));
        assert!(memory.ready_for_t_state(0, true, MemoryReadyPhase::T3));
    }
}
