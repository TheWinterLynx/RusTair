use std::sync::OnceLock;

use crate::config::S100InstalledCardConfig;
use crate::cpu8080::Bus;
use crate::cpu8080_cycle::{Cpu8080Cycle, Cpu8080Pins};
use crate::machine::AltairBus;
use crate::s100_memory::S100RamBoardModel;

use super::CycleAccurateMachineBackend;
use super::super::BackendResult;

/// No supported Intel 8080 instruction exceeds 18 T-states without external
/// wait states (XTHL is the longest). The compiled chassis below admits only
/// no-wait static RAM for memory traffic. Serial cards may be present only while
/// their UART timing is electrically quiet, and the full opcode set deliberately
/// excludes IN/OUT, so reserving 18 T-states still guarantees we never overshoot
/// the caller's exact budget.
const FULL_EXECUTION_MAX_T_STATES: u32 = 18;
const FULL_READ_CACHE_ENTRIES: usize = 64;

#[derive(Clone, Copy)]
struct FullReadCacheEntry {
    address: u16,
    value: u8,
    valid: bool,
}

const EMPTY_FULL_READ_CACHE_ENTRY: FullReadCacheEntry = FullReadCacheEntry {
    address: 0,
    value: 0,
    valid: false,
};

/// Compile the Cycle core's authoritative Full/Partial opcode classifier once.
/// The exact same predicate still defines eligibility; hot execution only turns
/// the repeated decoder walk into one indexed byte lookup.
fn full_opcode_table() -> &'static [bool; 256] {
    static TABLE: OnceLock<[bool; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [false; 256];
        let mut opcode = 0usize;
        while opcode < table.len() {
            table[opcode] = Cpu8080Cycle::full_opcode_class_supported(opcode as u8);
            opcode += 1;
        }
        table
    })
}

/// Prepared guest-bus recorder for Cycle Full. Guest memory traffic reaches the
/// same bus-owned S-100 decoder and RuntimeRamCard storage as Partial, while the
/// expensive connector graph remains lazy until an actual synchronization
/// boundary. Front-panel duty is nevertheless part of the externally observable
/// machine, so every semantic machine cycle is projected into the canonical lamp
/// integrator with the exact number of 8080 T-states it represents. This updates
/// presentation only: it never clocks RAM/UART cards or resolves the backplane a
/// second time.
struct FullInstructionBus<'a> {
    bus: &'a mut AltairBus,
    inte: bool,
    boundary_pins: Cpu8080Pins,
    /// Number of visible T-states already reconstructed for the instruction that
    /// is currently executing. Some register-only 8080 instructions have a T4/T5
    /// internal tail after their external fetch cycle; `instruction_complete`
    /// adds those residual states while the last S-100 address/status remain held.
    projected_t_states: u32,
    last_projected_address: Option<u16>,
    /// Tiny read-through cache scoped to this synchronous Full window. The
    /// admitted chassis contains only non-overlapping static RAM, every guest
    /// write crosses this same bus and invalidates its exact address, and no host
    /// mutation can interleave inside the call. It therefore removes repeated
    /// decoder/RefCell reads without changing S-100 ownership or self-modifying
    /// code semantics.
    read_cache: [FullReadCacheEntry; FULL_READ_CACHE_ENTRIES],
    /// Opcode already inspected by the Full dispatcher. Static RAM reads have no
    /// side effects, so the dispatcher may classify the byte once and let the
    /// semantic core consume that same fetch without performing a second memory
    /// lookup. This is still the guest opcode fetch: we publish the same package
    /// boundary when `opcode_fetch` consumes the cached byte.
    prefetched_opcode: Option<(u16, u8)>,
}

impl<'a> FullInstructionBus<'a> {
    fn new(bus: &'a mut AltairBus, inte: bool) -> Self {
        let mut boundary_pins = Cpu8080Pins::default();
        boundary_pins.inte = inte;
        Self {
            bus,
            inte,
            boundary_pins,
            projected_t_states: 0,
            last_projected_address: None,
            read_cache: [EMPTY_FULL_READ_CACHE_ENTRY; FULL_READ_CACHE_ENTRIES],
            prefetched_opcode: None,
        }
    }

    #[inline]
    fn read_cache_index(address: u16) -> usize {
        let address = address as usize;
        (address ^ (address >> 6)) & (FULL_READ_CACHE_ENTRIES - 1)
    }

    #[inline]
    fn guest_read(&mut self, address: u16) -> u8 {
        let index = Self::read_cache_index(address);
        let cached = self.read_cache[index];
        if cached.valid && cached.address == address {
            return cached.value;
        }
        let value = self.bus.cycle_full_guest_read(address);
        self.read_cache[index] = FullReadCacheEntry {
            address,
            value,
            valid: true,
        };
        value
    }

    #[inline]
    fn invalidate_guest_read(&mut self, address: u16) {
        let index = Self::read_cache_index(address);
        if self.read_cache[index].valid && self.read_cache[index].address == address {
            self.read_cache[index].valid = false;
        }
    }

    /// Project one whole external 8080 machine cycle into the canonical front
    /// panel in one weighted operation. The integrator itself preserves T1 with
    /// the old 8212 status and T2..Tn with the newly latched status, plus the DI
    /// transition after T1 on reads. No physical card or connector is replayed.
    #[inline]
    fn project_machine_cycle(
        &mut self,
        address: u16,
        data: u8,
        status_word: u8,
        t_states: u32,
        reads_data: bool,
        writes_data: bool,
    ) {
        debug_assert!(t_states >= 1);
        debug_assert!(!(reads_data && writes_data));

        self.bus.cycle_full_project_panel_cycle(
            address,
            data,
            status_word,
            t_states,
            reads_data,
            writes_data,
            self.inte,
        );

        self.projected_t_states = self.projected_t_states.saturating_add(t_states);
        self.last_projected_address = Some(address);
    }

    /// Register-only instructions may retain the final fetch/status/address for
    /// one or more internal T-states after the last external bus transfer. The
    /// panel state is constant across that tail, so Full adds the whole interval
    /// as one weighted contribution.
    #[inline]
    fn project_internal_tail(&mut self, t_states: u32) {
        if self.last_projected_address.is_none() {
            debug_assert_eq!(t_states, 0);
            return;
        }
        self.bus
            .cycle_full_project_internal_t_states(t_states, self.inte);
    }

    #[inline]
    fn remember_read_boundary(&mut self, address: u16) {
        self.boundary_pins = Cpu8080Pins {
            phi1: false,
            phi2: false,
            address: Some(address),
            data_out: None,
            sync: false,
            dbin: false,
            wr_n: true,
            inte: self.inte,
            wait: false,
            hlda: false,
        };
    }

    #[inline]
    fn remember_write_boundary(&mut self, address: u16, value: u8) {
        self.boundary_pins = Cpu8080Pins {
            phi1: false,
            phi2: false,
            address: Some(address),
            data_out: Some(value),
            sync: false,
            dbin: false,
            // /WR is still low through T3 and returns high on the following
            // PHI1. If the instruction completed at this write, dead time keeps
            // that T3 package level until the next PHI1.
            wr_n: false,
            inte: self.inte,
            wait: false,
            hlda: false,
        };
    }

    #[inline]
    fn prime_opcode_fetch(&mut self, address: u16, opcode: u8) {
        debug_assert!(self.prefetched_opcode.is_none());
        self.prefetched_opcode = Some((address, opcode));
    }

    fn boundary_pins(&self) -> Cpu8080Pins { self.boundary_pins }
}

impl Bus for FullInstructionBus<'_> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        let value = self.guest_read(address);
        self.project_machine_cycle(address, value, 0x82, 3, true, false);
        self.remember_read_boundary(address);
        value
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        self.bus.cycle_full_guest_write(address, value);
        self.invalidate_guest_read(address);
        self.project_machine_cycle(address, value, 0x00, 3, false, true);
        self.remember_write_boundary(address, value);
    }

    // These are defensive fallbacks only. Opcode eligibility prevents every
    // currently compiled Full window from reaching an I/O or INTE-changing
    // instruction; such an opcode is a synchronization barrier and runs Partial.
    fn input(&mut self, port: u8) -> u8 { Bus::input(self.bus, port) }
    fn output(&mut self, port: u8, value: u8) { Bus::output(self.bus, port, value); }

    fn set_inte(&mut self, enabled: bool) {
        self.inte = enabled;
        Bus::set_inte(self.bus, enabled);
        self.boundary_pins.inte = enabled;
    }

    #[inline]
    fn opcode_fetch(&mut self, address: u16) -> u8 {
        if let Some((cached_address, opcode)) = self.prefetched_opcode.take() {
            debug_assert_eq!(cached_address, address);
            if cached_address == address {
                self.project_machine_cycle(address, opcode, 0xa2, 4, true, false);
                self.remember_read_boundary(address);
                return opcode;
            }
        }
        let value = self.guest_read(address);
        self.project_machine_cycle(address, value, 0xa2, 4, true, false);
        self.remember_read_boundary(address);
        value
    }

    #[inline]
    fn stack_read(&mut self, address: u16) -> u8 {
        let value = self.guest_read(address);
        self.project_machine_cycle(address, value, 0x86, 3, true, false);
        self.remember_read_boundary(address);
        value
    }

    #[inline]
    fn stack_write(&mut self, address: u16, value: u8) {
        self.bus.cycle_full_guest_write(address, value);
        self.invalidate_guest_read(address);
        self.project_machine_cycle(address, value, 0x04, 3, false, true);
        self.remember_write_boundary(address, value);
    }

    fn halt_ack(&mut self, address: u16, opcode: u8) {
        Bus::halt_ack(self.bus, address, opcode);
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        Bus::interrupt_ack(self.bus, address, opcode, while_halted);
    }

    #[inline]
    fn take_wait_states(&mut self) -> u32 {
        // Full chassis eligibility admits only asynchronous/no-wait static RAM,
        // and IN/OUT are synchronization barriers. A non-zero wait here would be
        // a proof bug, not a dynamic condition to poll.
        0
    }

    #[inline]
    fn instruction_complete(&mut self, address: u16, _opcode: u8, t_states: u32) {
        debug_assert!(self.projected_t_states <= t_states);
        let residual = t_states.saturating_sub(self.projected_t_states);
        self.project_internal_tail(residual);
        self.projected_t_states = 0;
        self.last_projected_address = None;
        self.bus.cycle_full_instruction_complete(address, t_states);
    }
}

impl CycleAccurateMachineBackend {
    /// Compiled chassis class: one MITS 8080 CPU plus non-overlapping,
    /// asynchronous/no-wait MITS static RAM boards. 88-SIO/88-2SIO cards may be
    /// installed because the compiled opcode set cannot perform I/O and the host
    /// window separately proves that their UARTs have no timed transition in
    /// flight. Other real cards stay on Partial until they get an event model.
    ///
    /// This proof is evaluated once per host timeslice, not once per instruction.
    /// That mirrors MAME's prepared address spaces: after POWER-OFF hardware has
    /// been materialized, execution consumes a compact capability fact rather
    /// than repeatedly rediscovering the topology on every opcode.
    fn compiled_full_chassis_available(&self) -> bool {
        let hardware = self.machine.bus.s100_hardware_memory();
        let mut saw_ram = false;
        let mut ranges: Vec<(u32, u32)> = Vec::new();

        for (_, card) in hardware.installed_cards() {
            match card {
                S100InstalledCardConfig::Mits8080Cpu => {}
                S100InstalledCardConfig::Ram(config)
                    if matches!(
                        config.model,
                        S100RamBoardModel::Mits4KStatic88_4Mcs
                            | S100RamBoardModel::Mits16KStatic88_16Mcs
                    ) =>
                {
                    let start = u32::from(config.base_address);
                    let end = start + config.populated_bytes as u32;
                    if ranges
                        .iter()
                        .any(|&(other_start, other_end)| start < other_end && other_start < end)
                    {
                        // Overlap is legal physical S-100 hardware and must retain
                        // the generic resolver so equal drives/contention remain visible.
                        return false;
                    }
                    ranges.push((start, end));
                    saw_ram = true;
                }
                S100InstalledCardConfig::Mits88Sio(_)
                | S100InstalledCardConfig::Mits88TwoSio { .. } => {}
                _ => return false,
            }
        }
        saw_ram
    }

    fn compiled_full_chassis_has_serial(&self) -> bool {
        self.machine
            .bus
            .s100_hardware_memory()
            .installed_cards()
            .any(|(_, card)| {
                matches!(
                    card,
                    S100InstalledCardConfig::Mits88Sio(_)
                        | S100InstalledCardConfig::Mits88TwoSio { .. }
                )
            })
    }

    /// While all physical serial shift paths are idle, advancing their baud
    /// oscillators cannot change S-100 PRDY/PINT/VI or UART register state. The
    /// elapsed phase may therefore be folded in one exact batch immediately
    /// before re-entering Partial. A pending/completed endpoint TX byte is kept
    /// conservative via the busy predicates even though it has no future bit
    /// edge of its own.
    fn compiled_serial_timing_is_quiet(&self) -> bool {
        self.machine.bus.serial_rx_line_idle()
            && !self.machine.bus.tx_busy()
            && self.machine.bus.serial_port1_rx_line_idle()
            && !self.machine.bus.serial_port1_tx_busy()
    }

    #[cfg(test)]
    #[inline]
    fn compiled_full_opcode(&mut self, remaining: u32, full_window: bool) -> Option<u8> {
        if !full_window
            || remaining < FULL_EXECUTION_MAX_T_STATES
            || !self.at_instruction_boundary()
            || self.stop_wait_park_pending
            || self.cpu_fault.is_some()
            || self.machine.bus.cpu_control_lines().reset
        {
            return None;
        }

        if !self.cpu.prepare_full_boundary_after_reset_release() {
            return None;
        }

        let opcode = self
            .machine
            .bus
            .peek_memory(self.cpu.registers().pc)
            .unwrap_or(0xff);
        self.cpu
            .full_execution_opcode_supported(opcode)
            .then_some(opcode)
    }

    /// Focused one-instruction bridge retained for exact unit tests. Production
    /// execution below uses a semantic window so this expensive state transfer is
    /// not repeated once per opcode.
    #[cfg(test)]
    fn execute_compiled_full_instruction(&mut self, opcode: u8) -> Option<u32> {
        self.instruction_address = self.cpu.registers().pc;
        let inte = self.cpu.interrupts_enabled();

        let (elapsed, boundary_pins) = {
            let cpu = &mut self.cpu;
            let bus = &mut self.machine.bus;
            let mut full_bus = FullInstructionBus::new(bus, inte);
            let elapsed = cpu.execute_full_instruction(&mut full_bus, opcode)?;
            (elapsed, full_bus.boundary_pins())
        };

        debug_assert!(elapsed <= FULL_EXECUTION_MAX_T_STATES);
        self.cpu.set_full_boundary_pins(boundary_pins);
        self.machine.bus.cycle_mark_full_execution_desynced();
        self.last_teaching_tick = None;
        Some(elapsed)
    }

    /// Execute as many consecutive compiled instructions as the remaining exact
    /// host budget permits, keeping one instruction-level semantic core live for
    /// the entire block. Cycle state is exported once at entry and imported once
    /// at the final clean fetch boundary; memory still uses the bus-owned S-100
    /// decoder on every guest access.
    fn execute_compiled_full_window(
        &mut self,
        remaining: &mut u32,
        full_window: bool,
    ) -> Option<u64> {
        if !full_window
            || *remaining < FULL_EXECUTION_MAX_T_STATES
            || !self.at_instruction_boundary()
            || self.stop_wait_park_pending
            || self.cpu_fault.is_some()
            || self.machine.bus.cpu_control_lines().reset
        {
            return None;
        }

        if !self.cpu.prepare_full_boundary_after_reset_release() {
            return None;
        }

        let opcode_table = full_opcode_table();
        let first_opcode = self
            .machine
            .bus
            .peek_memory(self.cpu.registers().pc)
            .unwrap_or(0xff);
        if !opcode_table[first_opcode as usize] {
            return None;
        }

        let mut full = self.cpu.begin_full_execution_window()?;
        let inte = full.inte;
        let start_cycles = full.cycles;
        let mut completed = 0u64;
        let mut last_elapsed = 0u32;
        let mut last_address = full.pc;

        let boundary_pins = {
            let bus = &mut self.machine.bus;
            let mut full_bus = FullInstructionBus::new(bus, inte);

            while *remaining >= FULL_EXECUTION_MAX_T_STATES {
                let opcode_address = full.pc;
                let opcode = full_bus.guest_read(opcode_address);
                if !opcode_table[opcode as usize] {
                    break;
                }

                // The classification load above is the byte the CPU would fetch
                // from this proven side-effect-free static RAM chassis. Reuse it
                // as the semantic opcode fetch instead of looking up the same
                // address a second time inside Cpu8080::step().
                full_bus.prime_opcode_fetch(opcode_address, opcode);
                last_address = opcode_address;
                let elapsed = full.step(&mut full_bus);
                debug_assert!(full_bus.prefetched_opcode.is_none());
                debug_assert!(elapsed <= FULL_EXECUTION_MAX_T_STATES);
                debug_assert!(elapsed <= *remaining);
                *remaining -= elapsed;
                completed = completed.saturating_add(1);
                last_elapsed = elapsed;
            }

            full_bus.boundary_pins()
        };

        if completed == 0 {
            return None;
        }

        let elapsed_total = full.cycles.saturating_sub(start_cycles);
        self.instruction_address = last_address;
        self.cpu
            .commit_full_execution_window(&full, completed, last_elapsed);
        self.cpu.set_full_boundary_pins(boundary_pins);

        // Materialize the expensive generic connector graph only when Partial or
        // an external observer actually needs it. A whole Full window therefore
        // creates one lazy-desync marker instead of one per instruction.
        self.machine.bus.cycle_mark_full_execution_desynced();
        self.last_teaching_tick = None;
        Some(elapsed_total)
    }

    pub(super) fn service_execution_compiled(
        &mut self,
        t_state_budget: u32,
    ) -> BackendResult<()> {
        self.machine.bus.refresh_interrupt_request_line();
        let lines = self.machine.bus.cpu_control_lines();
        if t_state_budget == 0
            || !self.machine.powered
            || !self.machine.running
            || lines.reset
        {
            return self.fail_if_cpu_fault("service execution");
        }

        // Host input cannot mutate the chassis concurrently inside this
        // synchronous service call. Static RAM has no asynchronous source. A
        // serial card is also safe for Full while both UART shift paths are idle:
        // only its free-running baud phase advances, which is folded below before
        // the first Partial edge. Active RX/TX/BREAK immediately keeps the entire
        // window on Partial where exact edge timing remains authoritative.
        let serial_clocked = self.compiled_full_chassis_has_serial();
        let full_window = self.compiled_full_chassis_available()
            && (!serial_clocked || self.compiled_serial_timing_is_quiet())
            && lines.ready
            && !lines.hold
            && !(lines.interrupt && self.cpu.interrupts_enabled());

        let mut remaining = t_state_budget;
        let mut deferred_serial_t_states = 0u64;
        while remaining != 0 && self.machine.running {
            if let Some(elapsed) = self.execute_compiled_full_window(&mut remaining, full_window) {
                if serial_clocked {
                    deferred_serial_t_states = deferred_serial_t_states.saturating_add(elapsed);
                }
                continue;
            }

            // No serial state transition could occur while the quiet Full block
            // ran, but its independent baud oscillator never stopped. Materialize
            // the exact accumulated phase before any Partial T-state can observe
            // or mutate the serial card (especially an IN/OUT instruction).
            if deferred_serial_t_states != 0 {
                self.machine
                    .bus
                    .advance_serial_hardware_time(deferred_serial_t_states);
                deferred_serial_t_states = 0;
            }

            let ready = self.machine.bus.cycle_front_panel_ready_input();
            let trace = self.tick_once(ready);
            remaining -= 1;
            if trace.fault.is_some() {
                return self.fail_if_cpu_fault("service execution");
            }
            if self.stop_wait_park_pending {
                self.park_physical_stop_at_first_tw();
                break;
            }
        }

        if deferred_serial_t_states != 0 {
            self.machine
                .bus
                .advance_serial_hardware_time(deferred_serial_t_states);
        }
        self.machine.bus.refresh_interrupt_request_line();
        self.fail_if_cpu_fault("service execution")
    }

    /// Normal host execution enters the compiled dispatcher. This inherent
    /// method intentionally shadows the `MachineBackend` trait method for direct
    /// calls from `CycleHostBackend`; debugger/observer execution still calls the
    /// separate partial observer path and therefore retains exact T-state stops.
    pub(crate) fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        self.service_execution_compiled(t_state_budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MachineBackend;
    use crate::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
    use crate::s100_chassis::S100ChassisConfig;
    use crate::s100_memory::S100RamCardConfig;

    fn static_4k_hardware() -> S100HardwareConfig {
        let mut hardware =
            S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits4KStatic88_4Mcs,
                        0,
                    ),
                )),
            )
            .unwrap();
        hardware.validate().unwrap()
    }

    fn prepare_static_backend(program: &[u8]) -> CycleAccurateMachineBackend {
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.load_bytes(0, program).unwrap();
        backend.release_reset().unwrap();
        backend.run().unwrap();
        backend
    }

    #[test]
    fn compiled_full_static_ram_executes_whole_instruction_and_rejoins_partial_boundary() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend
            .load_bytes(0, &[0x2a, 0x10, 0x00])
            .unwrap();
        backend.load_bytes(0x0010, &[0x5a, 0xa5]).unwrap();
        backend.run().unwrap();
        assert!(backend.compiled_full_chassis_available());
        let opcode = backend
            .compiled_full_opcode(FULL_EXECUTION_MAX_T_STATES, true)
            .expect("static 4K chassis must compile");
        assert_eq!(opcode, 0x2a);
        assert_eq!(backend.execute_compiled_full_instruction(opcode), Some(16));
        let registers = backend.cpu.registers();
        assert_eq!((registers.h, registers.l), (0xa5, 0x5a));
        assert_eq!(registers.pc, 3);
        assert_eq!(backend.cpu.total_t_states(), 16);
        assert_eq!(backend.cpu.machine_cycle(), crate::cpu8080_cycle::MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu.t_state(), crate::cpu8080_cycle::TState::T1);
        assert!(backend.last_teaching_tick.is_none());

        backend.service_execution_compiled(1).unwrap();
        assert_eq!(backend.cpu.total_t_states(), 17);
    }

    #[test]
    fn compiled_full_read_cache_invalidates_on_guest_write() {
        let mut backend = prepare_static_backend(&[0x00]);
        backend.machine.bus.debugger_write_memory(0x0020, 0x11, false);
        let mut full_bus = FullInstructionBus::new(&mut backend.machine.bus, false);
        assert_eq!(full_bus.guest_read(0x0020), 0x11);
        full_bus.write(0x0020, 0x5a);
        assert_eq!(full_bus.guest_read(0x0020), 0x5a);
    }

    #[test]
    fn compiled_full_window_executes_many_instructions_before_rejoining_partial() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]).unwrap();
        backend.run().unwrap();

        let before = backend.cpu.completed_instructions();
        backend.service_execution_compiled(14_000).unwrap();
        assert_eq!(backend.cpu.total_t_states(), 14_000);
        assert!(backend.cpu.completed_instructions().saturating_sub(before) > 1_000);
        assert_eq!(backend.cpu.machine_cycle(), crate::cpu8080_cycle::MachineCycle::InstructionFetch);
    }

    #[test]
    fn compiled_full_front_panel_duty_matches_forced_partial_including_internal_t5() {
        // MOV B,C is a five-T-state register-only instruction: the external fetch
        // ends after four T-states and the exact core still exposes one internal
        // T5 with the fetch address/status held. JMP then exercises two ordinary
        // memory-read cycles. Repeating the pair forces a long Full window while
        // covering both reconstruction forms.
        const BUDGET: u32 = 1_500;
        let program = [0x41, 0xc3, 0x00, 0x00];
        let mut compiled = prepare_static_backend(&program);
        let mut partial = prepare_static_backend(&program);

        compiled.service_execution_compiled(BUDGET).unwrap();
        for _ in 0..BUDGET {
            let ready = partial.machine.bus.cycle_front_panel_ready_input();
            let trace = partial.tick_once(ready);
            assert!(trace.fault.is_none());
        }

        assert_eq!(compiled.cpu.total_t_states(), partial.cpu.total_t_states());
        assert_eq!(compiled.cpu.registers().pc, partial.cpu.registers().pc);
        assert_eq!(
            compiled.machine.bus.raw_panel_lamp_duty(),
            partial.machine.bus.raw_panel_lamp_duty(),
            "Cycle Full must preserve the exact raw front-panel duty of Partial"
        );
    }

    #[test]
    fn compiled_full_clocks_idle_two_sio_exactly_once() {
        let hardware = S100HardwareConfig::historical_8800b_18_slot_starter();
        let mut compiled = CycleAccurateMachineBackend::default();
        let mut reference = CycleAccurateMachineBackend::default();

        for backend in [&mut compiled, &mut reference] {
            backend
                .machine
                .bus
                .configure_s100_hardware_memory(hardware, RamInit::Zeroed)
                .unwrap();
            backend.power(true).unwrap();
            backend.assert_reset().unwrap();
            backend.release_reset().unwrap();
            backend.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]).unwrap();
            backend.run().unwrap();
        }

        compiled.service_execution_compiled(14_000).unwrap();
        assert_eq!(compiled.cpu.total_t_states(), 14_000);
        reference.machine.bus.advance_serial_hardware_time(14_000);

        // Port 1 is strapped to 9600 baud by the historical starter. Programming
        // the same /16 mode after the elapsed idle interval turns its first TX
        // completion into a precise probe of baud-generator phase.
        for backend in [&mut compiled, &mut reference] {
            backend.machine.bus.debugger_output_port(0x12, 0x15);
            backend.machine.bus.debugger_output_port(0x13, b'P');
        }

        let mut compiled_done = None;
        let mut reference_done = None;
        for elapsed in 1..=5_000u64 {
            compiled.machine.bus.advance_serial_hardware_time(1);
            reference.machine.bus.advance_serial_hardware_time(1);
            if compiled_done.is_none() && compiled.machine.bus.serial_port1_tx_front().is_some() {
                compiled_done = Some(elapsed);
            }
            if reference_done.is_none() && reference.machine.bus.serial_port1_tx_front().is_some() {
                reference_done = Some(elapsed);
            }
            if compiled_done.is_some() && reference_done.is_some() {
                break;
            }
        }

        assert_eq!(compiled_done, reference_done);
        assert!(compiled_done.is_some());
    }

    #[test]
    fn compiled_full_allows_idle_serial_but_rejects_wait_ram_and_overlap() {
        let mut wait_hardware =
            S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        wait_hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        wait_hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits1KStatic88Mcs,
                        0,
                    ),
                )),
            )
            .unwrap();

        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(wait_hardware, RamInit::Zeroed)
            .unwrap();
        assert!(!backend.compiled_full_chassis_available());

        let mut serial = CycleAccurateMachineBackend::default();
        serial
            .machine
            .bus
            .configure_s100_hardware_memory(
                S100HardwareConfig::historical_8800b_18_slot_starter(),
                RamInit::Zeroed,
            )
            .unwrap();
        assert!(serial.compiled_full_chassis_available());
        assert!(serial.compiled_full_chassis_has_serial());
        assert!(serial.compiled_serial_timing_is_quiet());

        let mut overlap = static_4k_hardware();
        overlap
            .set_slot(
                3,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits4KStatic88_4Mcs,
                        0,
                    ),
                )),
            )
            .unwrap();
        let mut overlapped = CycleAccurateMachineBackend::default();
        overlapped
            .machine
            .bus
            .configure_s100_hardware_memory(overlap, RamInit::Zeroed)
            .unwrap();
        assert!(!overlapped.compiled_full_chassis_available());
    }
}
