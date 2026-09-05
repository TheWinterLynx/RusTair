use crate::config::{
    RamBoardProfile, RamInit, RamSize, S100HardwareConfig, SerialBoard, SioHardwareConfig,
    TwoSioInterruptWiring, TwoSioStraps,
};
use crate::cpu8080_cycle::{Cpu8080Inputs, Cpu8080Pins};
use crate::s100::{S100ContactRole, S100Signal};
use crate::s100_backplane::{S100BackplaneError, S100BusSample};
use crate::s100_runtime::{
    DisplayControlLines, RuntimeMemoryInspection, S100RuntimeFabric,
};
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
    /// True when at least one installed card consumes PHI1, PHI2 or the buffered
    /// CLOC net as a connector input. If false, an edge that changes only those
    /// three physical clock nets cannot affect card state or CPU package inputs.
    phase_edge_requires_settle: bool,
    /// Last package-pin callback seen by the live fabric. Cycle can use this to
    /// prove that a callback changed only clock phase and therefore carries no
    /// new information to the installed card inventory.
    last_cycle_pins: Cpu8080Pins,
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
            // Legacy tests still consume the old edge projection. Be conservative
            // there; phase-elision is enabled only for explicit physical chassis.
            phase_edge_requires_settle: true,
            last_cycle_pins: Cpu8080Pins::default(),
            read_wait_active: false,
            read_wait_remaining: 0,
            basic32_probe_guard: false,
            basic32_probe_write: None,
        }
    }
}

impl Memory {
    pub(super) fn uses_explicit_hardware(&self) -> bool { !self.legacy_aggregate }

    pub(super) fn advance_serial_time(&self, t_states: u64) {
        self.fabric.advance_serial_time(t_states);
    }

    pub(super) fn serial_receive(&self, port: usize, byte: u8) -> bool {
        self.fabric.serial_receive(port, byte)
    }

    pub(super) fn serial_rx_empty(&self, port: usize) -> bool { self.fabric.serial_rx_empty(port) }
    pub(super) fn serial_rx_len(&self, port: usize) -> usize { self.fabric.serial_rx_len(port) }
    pub(super) fn serial_rx_line_idle(&self, port: usize) -> bool { self.fabric.serial_rx_line_idle(port) }
    pub(super) fn serial_tx_busy(&self, port: usize) -> bool { self.fabric.serial_tx_busy(port) }
    pub(super) fn serial_tx_front(&self, port: usize) -> Option<u8> { self.fabric.serial_tx_front(port) }
    pub(super) fn serial_tx_complete(&self, port: usize) -> Option<u8> { self.fabric.serial_tx_complete(port) }
    pub(super) fn clear_serial(&self) { self.fabric.clear_serial(); }
    pub(super) fn serial_modem_lines(&self, port: usize) -> Option<(bool, bool, bool, bool)> {
        self.fabric.serial_modem_lines(port)
    }
    pub(super) fn set_serial_modem_inputs(&self, port: usize, cts: bool, dcd: bool) -> bool {
        self.fabric.set_serial_modem_inputs(port, cts, dcd)
    }
    pub(super) fn set_serial_receive_break(&self, port: usize, active: bool) -> bool {
        self.fabric.set_serial_receive_break(port, active)
    }
    pub(super) fn sio_handshake_lines(&self) -> Option<(bool, bool, bool, bool, bool, bool)> {
        self.fabric.sio_handshake_lines()
    }
    pub(super) fn pulse_sio_input_device_ready(&self) -> bool {
        self.fabric.pulse_sio_input_device_ready()
    }
    pub(super) fn pulse_sio_output_device_ready(&self) -> bool {
        self.fabric.pulse_sio_output_device_ready()
    }
    pub(super) fn debugger_inject_serial_rx(&self, port: u8, byte: u8) -> bool {
        self.fabric.debugger_inject_serial_rx(port, byte)
    }
    pub(super) fn debugger_clear_serial_rx(&self, port: u8) -> bool {
        self.fabric.debugger_clear_serial_rx(port)
    }
    pub(super) fn debugger_clear_serial_tx(&self, port: u8) -> bool {
        self.fabric.debugger_clear_serial_tx(port)
    }
    pub(super) fn debugger_complete_serial_tx(&self, port: u8) -> Option<u8> {
        self.fabric.debugger_complete_serial_tx(port)
    }
    pub(super) fn peek_io_port(&self, port: u8) -> u8 { self.fabric.peek_io_port(port) }
    pub(super) fn debugger_input_port(&self, port: u8) -> u8 {
        self.fabric.debugger_input_port(port)
    }
    pub(super) fn debugger_output_port(&self, port: u8, value: u8) {
        self.fabric.debugger_output_port(port, value);
    }
    pub(super) fn serial_vector_interrupt_requests(&self) -> u8 {
        self.fabric.serial_vector_interrupt_requests()
    }
    pub(super) fn primary_serial_board(&self) -> Option<SerialBoard> {
        self.fabric.primary_serial_board()
    }
    pub(super) fn primary_sio_hardware(&self) -> Option<SioHardwareConfig> {
        self.fabric.primary_sio_hardware()
    }
    pub(super) fn primary_two_sio_straps(&self) -> Option<TwoSioStraps> {
        self.fabric.primary_two_sio_straps()
    }
    pub(super) fn primary_two_sio_interrupt_wiring(&self) -> Option<TwoSioInterruptWiring> {
        self.fabric.primary_two_sio_interrupt_wiring()
    }
    pub(super) fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        self.fabric.io_port_activity(port)
    }
    pub(super) fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.fabric.io_trace_snapshot()
    }
    pub(super) fn io_trace_enabled(&self) -> bool { self.fabric.io_trace_enabled() }
    pub(super) fn set_io_trace_enabled(&self, enabled: bool) {
        self.fabric.set_io_trace_enabled(enabled);
    }
    pub(super) fn clear_io_trace(&self) { self.fabric.clear_io_trace(); }

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

    fn phase_edge_listener_present(fabric: &S100RuntimeFabric) -> bool {
        fabric.backplane().slots().iter().any(|slot| {
            slot.descriptor().is_some_and(|descriptor| {
                descriptor.contacts.iter().any(|contact| {
                    contact.role == S100ContactRole::Input
                        && matches!(
                            contact.signal,
                            S100Signal::Phi1 | S100Signal::Phi2 | S100Signal::Clock
                        )
                })
            })
        })
    }

    fn phase_only_edge_is_unobserved(&self, pins: Cpu8080Pins) -> bool {
        if self.legacy_aggregate || self.phase_edge_requires_settle {
            return false;
        }
        let previous = self.last_cycle_pins;
        let same_non_phase = previous.address == pins.address
            && previous.data_out == pins.data_out
            && previous.sync == pins.sync
            && previous.dbin == pins.dbin
            && previous.wr_n == pins.wr_n
            && previous.inte == pins.inte
            && previous.wait == pins.wait
            && previous.hlda == pins.hlda;
        if !same_non_phase {
            return false;
        }

        let phi1_rising = !previous.phi1 && pins.phi1 && !previous.phi2 && !pins.phi2;
        let phi1_falling = previous.phi1 && !pins.phi1 && !previous.phi2 && !pins.phi2;
        let phi2_rising = !previous.phi1 && !pins.phi1 && !previous.phi2 && pins.phi2;
        let phi2_falling = previous.phi2 && !pins.phi2 && !previous.phi1 && !pins.phi1;

        phi1_rising || phi1_falling || phi2_rising || phi2_falling
    }

    pub(super) fn configure(&mut self, size: RamSize, init_mode: RamInit) {
        self.ram_size = size;
        self.init_mode = init_mode;
        self.legacy_aggregate = true;
        self.fabric = Self::legacy_fabric(size, self.board_profile, init_mode);
        self.phase_edge_requires_settle = true;
        self.last_cycle_pins = Cpu8080Pins::default();
        self.clear_transient_guards();
        self.reset_timing();
    }

    pub(super) fn configure_hardware(
        &mut self,
        hardware: S100HardwareConfig,
        init_mode: RamInit,
    ) -> Result<(), crate::s100_runtime::S100RuntimeBuildError> {
        let fabric = S100RuntimeFabric::new(hardware, init_mode)?;
        self.phase_edge_requires_settle = Self::phase_edge_listener_present(&fabric);
        self.fabric = fabric;
        self.init_mode = init_mode;
        self.legacy_aggregate = false;
        self.last_cycle_pins = Cpu8080Pins::default();
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

    pub(super) fn cycle_drive_cpu_edge(
        &mut self,
        pins: Cpu8080Pins,
        display: DisplayControlLines,
    ) -> Result<Cpu8080Inputs, S100BackplaneError> {
        // Cpu8080Cycle still emits all four historical edges. If the installed
        // connector inventory proves nobody consumes PHI1/PHI2/CLOC, a pure
        // clock transition can be folded. Falling/PHI2 edges are already safe
        // after the same T-state's synchronization point. PHI1 rising gets the
        // stronger proof below: Display/Control unchanged, no externally changed
        // serial connector, and no pending 8212 status-latch transition.
        if self.phase_only_edge_is_unobserved(pins) {
            let previous = self.last_cycle_pins;
            let phi1_rising = !previous.phi1 && pins.phi1 && !previous.phi2 && !pins.phi2;
            let status_latch_changes = phi1_rising
                && pins.sync
                && pins
                    .data_out
                    .is_some_and(|word| word != self.fabric.cpu_latched_status_word());
            let can_elide = if phi1_rising {
                !status_latch_changes && self.fabric.can_elide_phase_only_rising(display)?
            } else {
                true
            };

            if can_elide {
                self.last_cycle_pins = pins;
                // Keep Intel-package phase, buffered CLOC, and the CPU board's
                // own 8212 state exact. Only propagation of an electrically
                // unobservable connector transition is deferred; the next real
                // edge folds the cached CPU drive into the backplane.
                self.fabric.set_cpu_package_pins(pins);
                return Ok(self.fabric.cpu_package_inputs());
            }
        }
        self.last_cycle_pins = pins;

        // BASIC 3.2's optional full-memory compatibility guard is deliberately
        // host-side and non-historical. Suppress only its probe write before the
        // live compatibility RAM sees MWRT; ordinary Cycle writes, including
        // every historical RAM card, continue through the physical bus path.
        let mut physical_pins = pins;
        if self.basic32_probe_guard
            && physical_pins.address == Some(u16::MAX)
            && !physical_pins.wr_n
        {
            if let Some(value) = physical_pins.data_out {
                self.basic32_probe_write = Some(value);
            }
            physical_pins.wr_n = true;
        }
        self.fabric.set_cpu_package_pins(physical_pins);
        self.fabric.settle(display, &[])?;
        Ok(self.fabric.cpu_package_inputs())
    }

    pub(super) fn cycle_live_inputs(&self) -> Cpu8080Inputs {
        self.fabric.cpu_package_inputs()
    }

    pub(super) fn cycle_live_sample(&self) -> &S100BusSample {
        self.fabric.sample()
    }

    pub(super) fn cycle_latched_status_word(&self) -> u8 {
        self.fabric.cpu_latched_status_word()
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

    pub(super) fn clear_protection(&self) {
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

    /// Transitional T3 read helper retained for serial/front-panel migration
    /// tests. Guest Cycle memory reads now sample the live CPU-board DI input.
    pub(super) fn cycle_read(&mut self, address: u16) -> u8 {
        self.compatibility_read_override(address)
            .unwrap_or_else(|| self.resolved_preview(address))
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

    pub(crate) fn cycle_live_s100_inputs(&self) -> Cpu8080Inputs {
        self.memory.cycle_live_inputs()
    }

    pub(crate) fn cycle_live_s100_sample(&self) -> &S100BusSample {
        self.memory.cycle_live_sample()
    }

    pub(crate) fn cycle_live_s100_status_word(&self) -> u8 {
        self.memory.cycle_latched_status_word()
    }

    fn cycle_display_control_lines(&self) -> DisplayControlLines {
        let signals = self.s100.signals();
        DisplayControlLines {
            ready: signals.front_panel_ready,
            run: signals.run,
            hold: signals.hold,
            reset: signals.reset,
            external_clear: signals.ext_clear,
            protect: false,
            unprotect: false,
        }
    }

    pub(crate) fn cycle_drive_live_s100_edge(
        &mut self,
        pins: Cpu8080Pins,
    ) -> Result<Cpu8080Inputs, S100BackplaneError> {
        let display = self.cycle_display_control_lines();
        self.memory.cycle_drive_cpu_edge(pins, display)
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
        // Explicit chassis hardware already owns PRDY through the installed RAM
        // and I/O cards. Re-running aggregate wait-state logic here would create
        // a second, software-only READY authority in parallel with the bus.
        if !self.memory.legacy_aggregate {
            return true;
        }

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

    #[test]
    fn cycle_edge_path_and_debugger_share_the_same_physical_ram_bytes() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K1, RamInit::Zeroed);
        assert!(memory.debugger_write(0x0010, 0x5a, false));
        let display = DisplayControlLines {
            ready: true,
            run: true,
            ..DisplayControlLines::default()
        };

        memory
            .cycle_drive_cpu_edge(
                Cpu8080Pins {
                    phi1: true,
                    address: Some(0x0010),
                    data_out: Some(0x82),
                    sync: true,
                    wr_n: true,
                    ..Cpu8080Pins::default()
                },
                display,
            )
            .unwrap();
        let inputs = memory
            .cycle_drive_cpu_edge(
                Cpu8080Pins {
                    phi2: true,
                    address: Some(0x0010),
                    dbin: true,
                    wr_n: true,
                    ..Cpu8080Pins::default()
                },
                display,
            )
            .unwrap();

        assert_eq!(inputs.data_in, 0x5a);
        assert_eq!(memory.peek(0x0010), Some(0x5a));
        assert_eq!(memory.cycle_live_sample().data_in(), Some(0x5a));
    }

    #[test]
    fn basic32_probe_guard_suppresses_only_the_live_cycle_probe_write() {
        let mut memory = Memory::default();
        memory.configure(RamSize::K64, RamInit::Zeroed);
        assert!(memory.arm_basic32_full_memory_probe_guard());
        let display = DisplayControlLines {
            ready: true,
            run: true,
            ..DisplayControlLines::default()
        };
        memory
            .cycle_drive_cpu_edge(
                Cpu8080Pins {
                    phi1: true,
                    address: Some(u16::MAX),
                    data_out: Some(0x37),
                    wr_n: false,
                    ..Cpu8080Pins::default()
                },
                display,
            )
            .unwrap();
        assert_eq!(memory.peek(u16::MAX), Some(0));
        assert_eq!(memory.preview_read(u16::MAX), 0xc8);
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
