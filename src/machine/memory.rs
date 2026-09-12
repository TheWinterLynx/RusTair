use crate::config::{
    RamBoardProfile, RamInit, RamSize, S100HardwareConfig, SerialBoard, SioHardwareConfig,
};
use crate::cpu8080_cycle::{Cpu8080Inputs, Cpu8080Pins};
use crate::s100::{S100ContactRole, S100Signal};
use crate::s100_backplane::{S100BackplaneError, S100BusSample};
pub(crate) use crate::s100_runtime::S100_OPEN_BUS_VALUE;
use crate::s100_runtime::{DisplayControlLines, RuntimeMemoryInspection, S100RuntimeFabric};
use crate::s100_runtime_ram::RuntimeRamTimingWindow;

pub const MEM_SIZE: usize = 8 * 1024;
pub const MAX_MEM_SIZE: usize = 64 * 1024;
pub const MEMORY_BOARD_SIZE: usize = 1024;
pub const MEMORY_BOARD_COUNT: usize = MAX_MEM_SIZE / MEMORY_BOARD_SIZE;

/// Bus-owned memory/card facade for the live S-100 fabric.
///
/// RAM bytes and serial-card state always live in `S100RuntimeFabric`. The
/// aggregate `RamSize`/`RamBoardProfile` fields below exist only so old tests and
/// migration helpers can request a compatibility assembly; they no longer select
/// a second runtime representation.
pub(super) struct Memory {
    fabric: S100RuntimeFabric,
    ram_size: RamSize,
    init_mode: RamInit,
    board_profile: RamBoardProfile,
    /// True when at least one installed card consumes PHI1, PHI2 or the buffered
    /// CLOC net as a connector input. If false, an edge that changes only those
    /// three physical clock nets cannot affect card state or CPU package inputs.
    phase_edge_requires_settle: bool,
    /// Last package-pin callback seen by the live fabric. Cycle can use this to
    /// prove that a callback changed only clock phase and therefore carries no
    /// new information to the installed card inventory.
    last_cycle_pins: Cpu8080Pins,
    /// Full execution may advance the same physical RAM storage and presentation
    /// bus without replaying every intermediate connector delta. The first return
    /// to Partial must therefore force one real fabric settle before phase-only
    /// edge elision is legal again.
    full_execution_desynced: bool,
    /// Dynamic RAM clock/refresh state is copied once per Full window and lives
    /// here until the Full->Partial boundary. Guest bytes remain in `fabric`.
    full_ram_timing_window: Option<RuntimeRamTimingWindow>,
    basic32_probe_guard: bool,
    basic32_probe_write: Option<u8>,
}

impl Default for Memory {
    fn default() -> Self {
        let ram_size = RamSize::K8;
        let init_mode = RamInit::Random;
        let board_profile = RamBoardProfile::FastNoWait;
        let fabric = S100RuntimeFabric::new(S100HardwareConfig::default(), init_mode)
            .expect("default slot-native S-100 assembly must be valid");
        let phase_edge_requires_settle = Self::phase_edge_listener_present(&fabric);
        Self {
            fabric,
            ram_size,
            init_mode,
            board_profile,
            phase_edge_requires_settle,
            last_cycle_pins: Cpu8080Pins::default(),
            full_execution_desynced: false,
            full_ram_timing_window: None,
            basic32_probe_guard: false,
            basic32_probe_write: None,
        }
    }
}

impl Memory {
    pub(super) fn advance_serial_time(&self, t_states: u64) {
        self.fabric.advance_serial_time(t_states);
    }

    pub(super) fn serial_timing_is_quiet(&self) -> bool {
        self.fabric.serial_timing_is_quiet()
    }

    pub(super) fn serial_receive(&self, port: usize, byte: u8) -> bool {
        self.fabric.serial_receive(port, byte)
    }

    pub(super) fn serial_rx_empty(&self, port: usize) -> bool {
        self.fabric.serial_rx_empty(port)
    }
    pub(super) fn serial_rx_len(&self, port: usize) -> usize {
        self.fabric.serial_rx_len(port)
    }
    pub(super) fn serial_rx_line_idle(&self, port: usize) -> bool {
        self.fabric.serial_rx_line_idle(port)
    }
    pub(super) fn serial_tx_busy(&self, port: usize) -> bool {
        self.fabric.serial_tx_busy(port)
    }
    pub(super) fn serial_tx_front(&self, port: usize) -> Option<u8> {
        self.fabric.serial_tx_front(port)
    }
    pub(super) fn serial_tx_complete(&self, port: usize) -> Option<u8> {
        self.fabric.serial_tx_complete(port)
    }
    pub(super) fn clear_serial(&self) {
        self.fabric.clear_serial();
    }

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

    pub(super) fn peek_io_port(&self, port: u8) -> u8 {
        self.fabric.peek_io_port(port)
    }
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
    pub(super) fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        self.fabric.io_port_activity(port)
    }
    pub(super) fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.fabric.io_trace_snapshot()
    }
    pub(super) fn io_trace_enabled(&self) -> bool {
        self.fabric.io_trace_enabled()
    }
    pub(super) fn set_io_trace_enabled(&self, enabled: bool) {
        self.fabric.set_io_trace_enabled(enabled);
    }
    pub(super) fn clear_io_trace(&self) {
        self.fabric.clear_io_trace();
    }

    fn compatibility_hardware(
        &self,
        size: RamSize,
        profile: RamBoardProfile,
    ) -> S100HardwareConfig {
        let current = self.fabric.hardware();
        let serial_board = current.active_serial_board().unwrap_or(SerialBoard::Sio88);
        let sio_hardware = current.active_sio_hardware().unwrap_or_default();
        let two_sio_straps = current.active_two_sio_straps().unwrap_or_default();
        let two_sio_interrupt_wiring = current
            .active_two_sio_interrupt_wiring()
            .unwrap_or_default();
        S100HardwareConfig::from_legacy_globals(
            size,
            profile,
            serial_board,
            sio_hardware,
            two_sio_straps,
            two_sio_interrupt_wiring,
        )
    }

    fn compatibility_fabric(
        &self,
        size: RamSize,
        profile: RamBoardProfile,
        init_mode: RamInit,
    ) -> S100RuntimeFabric {
        S100RuntimeFabric::new(self.compatibility_hardware(size, profile), init_mode)
            .expect("compatibility S-100 assembly must be valid")
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
        if self.phase_edge_requires_settle || self.full_execution_desynced {
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

    /// Compatibility helper for old fixtures. It now builds a real slot-native
    /// assembly rather than selecting a separate aggregate memory runtime.
    pub(super) fn configure(&mut self, size: RamSize, init_mode: RamInit) {
        self.ram_size = size;
        self.init_mode = init_mode;
        let fabric = self.compatibility_fabric(size, self.board_profile, init_mode);
        self.phase_edge_requires_settle = Self::phase_edge_listener_present(&fabric);
        self.fabric = fabric;
        self.last_cycle_pins = Cpu8080Pins::default();
        self.full_execution_desynced = false;
        self.full_ram_timing_window = None;
        self.clear_transient_guards();
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
        self.last_cycle_pins = Cpu8080Pins::default();
        self.full_execution_desynced = false;
        self.full_ram_timing_window = None;
        self.clear_transient_guards();
        Ok(())
    }

    pub(super) fn hardware(&self) -> S100HardwareConfig {
        self.fabric.hardware()
    }
    pub(super) fn inspect(&self, address: u16) -> RuntimeMemoryInspection {
        self.fabric.inspect_memory(address)
    }

    #[inline(always)]
    fn active_full_ram_timing(&mut self) -> &mut RuntimeRamTimingWindow {
        if self.full_ram_timing_window.is_none() {
            self.full_ram_timing_window = Some(self.fabric.full_ram_timing_window());
        }
        self.full_ram_timing_window
            .as_mut()
            .expect("Full RAM timing window was just initialized")
    }

    fn commit_full_ram_timing_window(&mut self) {
        if let Some(window) = self.full_ram_timing_window.take() {
            window.commit();
        }
    }

    #[inline(always)]
    pub(super) fn full_ram_machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        self.active_full_ram_timing().machine_cycle_timing(
            address,
            memory_access,
            m1,
            base_t_states,
        )
    }

    #[inline(always)]
    pub(super) fn full_ram_internal_t_states(&mut self, t_states: u32) {
        if t_states != 0 {
            self.active_full_ram_timing().internal_t_states(t_states);
        }
    }

    pub(super) fn mark_full_execution_desynced(
        &mut self,
        boundary_pins: Cpu8080Pins,
        latched_status_word: u8,
    ) {
        self.commit_full_ram_timing_window();
        crate::full_boundary_reconcile::FullCpuBoundaryReconcile::reconcile_full_cpu_boundary(
            &mut self.fabric,
            boundary_pins,
            latched_status_word,
        );
        self.last_cycle_pins = boundary_pins;
        self.full_execution_desynced = true;
    }

    pub(super) fn cycle_drive_cpu_edge(
        &mut self,
        pins: Cpu8080Pins,
        display: DisplayControlLines,
    ) -> Result<Cpu8080Inputs, S100BackplaneError> {
        self.commit_full_ram_timing_window();
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
                self.fabric.set_cpu_package_pins(pins);
                return Ok(self.fabric.cpu_package_inputs());
            }
        }
        self.last_cycle_pins = pins;

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
        self.full_execution_desynced = false;
        Ok(self.fabric.cpu_package_inputs())
    }

    /// Re-resolve the current connector graph after host-side card state changes
    /// such as elapsed UART time, modem inputs or debugger injection. No CPU pin
    /// is changed and no interrupt is synthesized: the installed cards refresh
    /// their cached drives and PINT/PRDY reach the CPU package through S-100.
    pub(super) fn cycle_refresh_external_inputs(
        &mut self,
        display: DisplayControlLines,
    ) -> Result<Cpu8080Inputs, S100BackplaneError> {
        self.commit_full_ram_timing_window();
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

    /// Compatibility helper for old aggregate timing fixtures. Rebuild the
    /// compatibility card with the requested physical wait-state value while
    /// copying its current bytes into the replacement card.
    pub(super) fn configure_board_profile(&mut self, profile: RamBoardProfile) {
        let bytes = (0..self.ram_size.bytes())
            .map(|address| {
                self.fabric
                    .peek_unique_memory(address as u16)
                    .unwrap_or(S100_OPEN_BUS_VALUE)
            })
            .collect::<Vec<_>>();
        self.board_profile = profile;
        let fabric = self.compatibility_fabric(self.ram_size, profile, self.init_mode);
        let _ = fabric.load_bytes(0, &bytes);
        self.phase_edge_requires_settle = Self::phase_edge_listener_present(&fabric);
        self.fabric = fabric;
        self.last_cycle_pins = Cpu8080Pins::default();
        self.full_execution_desynced = false;
        self.full_ram_timing_window = None;
        self.clear_transient_guards();
    }

    pub(super) fn board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        (self.fabric.mapped_ram_card_count(address) != 0).then_some(self.board_profile)
    }

    pub(super) fn installed_size(&self) -> usize {
        self.fabric.installed_ram_bytes()
    }

    pub(super) fn initialize(&mut self) {
        self.clear_transient_guards();
        self.fabric.initialize_memory(self.init_mode);
    }

    pub(super) fn randomize(&mut self) {
        self.clear_transient_guards();
        self.fabric.initialize_memory(RamInit::Random);
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
        let _ = self.fabric.set_unique_memory_protection(address, protected);
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

    /// Full executes through the S-100 address decode compiled when cards are
    /// installed. A unique RAM responder is a direct bus dispatch to that
    /// physical card's shared storage, exactly like a decoded TTL chip-select.
    /// No CPU-to-RAM reference exists: Full calls the bus-owned fabric, and
    /// overlapping cards fall back to the generic electrical resolver.
    pub(super) fn read(&mut self, address: u16) -> u8 {
        if let Some(value) = self.compatibility_read_override(address) {
            return value;
        }
        match self.fabric.mapped_ram_card_count(address) {
            0 => S100_OPEN_BUS_VALUE,
            1 => self
                .fabric
                .peek_unique_memory(address)
                .unwrap_or(S100_OPEN_BUS_VALUE),
            _ => self
                .fabric
                .fast_memory_read(address, 0x82)
                .unwrap_or(S100_OPEN_BUS_VALUE),
        }
    }

    /// Full writes use the same bus-owned compiled S-100 decode. A single
    /// selected RAM card receives the write and enforces its physical protection
    /// latch; overlap falls back to the generic electrical transaction so every
    /// selected card observes MWRT/DO.
    pub(super) fn write(&mut self, address: u16, value: u8) {
        if address == u16::MAX && self.basic32_probe_guard {
            self.basic32_probe_write = Some(value);
            return;
        }
        match self.fabric.mapped_ram_card_count(address) {
            0 => {}
            1 => {
                let _ = self.fabric.write_unique_memory(address, value, true);
            }
            _ => {
                let _ = self.fabric.fast_memory_write(address, value, 0x00);
            }
        }
    }

    /// T3 helper used only for the BASIC 3.2 compatibility guard and panel-side
    /// diagnostics. Ordinary Partial memory reads sample live S-100 DI directly.
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
    pub(crate) fn cycle_full_ram_machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        self.memory
            .full_ram_machine_cycle_timing(address, memory_access, m1, base_t_states)
    }
    pub(crate) fn cycle_full_ram_internal_t_states(&mut self, t_states: u32) {
        self.memory.full_ram_internal_t_states(t_states);
    }
    pub(crate) fn cycle_mark_full_execution_desynced(&mut self) {
        let signals = self.s100.signals();
        debug_assert!(!signals.wait && !signals.hlda);
        let boundary_pins = Cpu8080Pins {
            address: Some(signals.address),
            data_out: signals.data_out,
            wr_n: signals.data_out.is_none(),
            inte: signals.inte,
            ..Cpu8080Pins::default()
        };
        let latched_status_word = self.raw_s100_status_word();
        self.memory
            .mark_full_execution_desynced(boundary_pins, latched_status_word);
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
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        self.memory.board_profile(address)
    }

    pub(crate) fn cycle_settle_memory_ready_after_panel_freeze(&mut self) {
        self.s100.set_memory_ready_input(true);
    }

    pub(crate) fn cycle_read_memory(&mut self, address: u16) -> u8 {
        self.memory.cycle_read(address)
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
    fn full_guest_read_and_write_cross_live_backplane() {
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
    fn lazy_full_execution_forces_first_partial_edge_back_through_real_fabric() {
        let mut memory = Memory::default();
        let display = DisplayControlLines {
            ready: true,
            run: true,
            ..DisplayControlLines::default()
        };
        let boundary = Cpu8080Pins {
            address: Some(0x0010),
            wr_n: true,
            ..Cpu8080Pins::default()
        };
        memory.mark_full_execution_desynced(boundary, 0x82);
        assert!(memory.full_execution_desynced);
        assert_eq!(memory.cycle_latched_status_word(), 0x82);
        memory
            .cycle_drive_cpu_edge(
                Cpu8080Pins {
                    phi1: true,
                    address: Some(0x0020),
                    data_out: Some(0x82),
                    sync: true,
                    wr_n: true,
                    ..Cpu8080Pins::default()
                },
                display,
            )
            .unwrap();
        assert!(!memory.full_execution_desynced);
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
