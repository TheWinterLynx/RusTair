use crate::config::S100InstalledCardConfig;
use crate::cpu8080::Bus;
use crate::cpu8080_cycle::Cpu8080Pins;
use crate::machine::AltairBus;
use crate::s100_memory::S100RamBoardModel;

use super::CycleAccurateMachineBackend;
use super::super::BackendResult;

/// No supported Intel 8080 instruction exceeds 18 T-states without external
/// wait states (XTHL is the longest). The compiled chassis below admits only
/// no-wait static RAM and no I/O cards, so reserving 18 T-states before entering
/// full execution guarantees we never overshoot the caller's exact budget.
const FULL_EXECUTION_MAX_T_STATES: u32 = 18;

/// Thin recorder around the existing AltairBus. Guest reads/writes still use
/// the bus-owned compiled S-100 decode and the same RuntimeRamCard storage as the
/// electrical fabric. We retain only the package outputs that physically remain
/// after the instruction's final external machine cycle so the partial core can
/// resume from the exact dead-time boundary later.
struct FullInstructionBus<'a> {
    bus: &'a mut AltairBus,
    inte: bool,
    boundary_pins: Cpu8080Pins,
}

impl<'a> FullInstructionBus<'a> {
    fn new(bus: &'a mut AltairBus, inte: bool) -> Self {
        let mut boundary_pins = Cpu8080Pins::default();
        boundary_pins.inte = inte;
        Self { bus, inte, boundary_pins }
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

    fn boundary_pins(&self) -> Cpu8080Pins { self.boundary_pins }
}

impl Bus for FullInstructionBus<'_> {
    fn read(&mut self, address: u16) -> u8 {
        let value = Bus::read(self.bus, address);
        self.remember_read_boundary(address);
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        Bus::write(self.bus, address, value);
        self.remember_write_boundary(address, value);
    }

    fn input(&mut self, port: u8) -> u8 { Bus::input(self.bus, port) }
    fn output(&mut self, port: u8, value: u8) { Bus::output(self.bus, port, value); }

    fn set_inte(&mut self, enabled: bool) {
        self.inte = enabled;
        Bus::set_inte(self.bus, enabled);
        self.boundary_pins.inte = enabled;
    }

    fn opcode_fetch(&mut self, address: u16) -> u8 {
        let value = Bus::opcode_fetch(self.bus, address);
        self.remember_read_boundary(address);
        value
    }

    fn stack_read(&mut self, address: u16) -> u8 {
        let value = Bus::stack_read(self.bus, address);
        self.remember_read_boundary(address);
        value
    }

    fn stack_write(&mut self, address: u16, value: u8) {
        Bus::stack_write(self.bus, address, value);
        self.remember_write_boundary(address, value);
    }

    fn halt_ack(&mut self, address: u16, opcode: u8) {
        Bus::halt_ack(self.bus, address, opcode);
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        Bus::interrupt_ack(self.bus, address, opcode, while_halted);
    }

    fn take_wait_states(&mut self) -> u32 { Bus::take_wait_states(self.bus) }

    fn instruction_complete(&mut self, address: u16, opcode: u8, t_states: u32) {
        Bus::instruction_complete(self.bus, address, opcode, t_states);
    }
}

impl CycleAccurateMachineBackend {
    /// First compiled chassis class: one MITS 8080 CPU plus non-overlapping
    /// asynchronous/no-wait MITS static RAM boards. Every other real card keeps
    /// the partial engine authoritative until it gets an explicit event model.
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
                _ => return false,
            }
        }
        saw_ram
    }

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

        // Partial clears its internal `reset_asserted` latch at the top of the
        // first T1 after physical RESET is released, before consuming any time.
        // Full has no per-T-state preamble, so perform exactly that zero-time
        // transition here before asking whether this opcode can use the compiled
        // executor. The external RESET line was checked released above.
        if !self.cpu.prepare_full_boundary_after_reset_release() {
            return None;
        }

        // The chassis proof guarantees a unique static-RAM responder wherever
        // memory is populated. `peek_memory` therefore uses the already-compiled
        // S-100 RAM decode instead of the general inspection/resolution path.
        // An unmapped fetch is open bus FFh and stays on Partial (RST 7 is not
        // compiled yet), preserving exact floating-DI behavior.
        let opcode = self
            .machine
            .bus
            .peek_memory(self.cpu.registers().pc)
            .unwrap_or(0xff);
        self.cpu
            .full_execution_opcode_supported(opcode)
            .then_some(opcode)
    }

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

        // Memory bytes and the UI-facing bus have advanced through AltairBus,
        // but the expensive generic connector graph was intentionally left lazy.
        // The first partial edge will force one real settle before any phase-only
        // edge can be elided again.
        self.machine.bus.cycle_mark_full_execution_desynced();
        self.last_teaching_tick = None;
        Some(elapsed)
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
        // synchronous service call. With this first plan there are also no cards
        // with an independent clock/IRQ source, so READY/HOLD/INTR are stable for
        // the entire Full window. If any is already active we simply remain on
        // Partial, where the exact sampling rules continue to apply.
        let full_window = self.compiled_full_chassis_available()
            && lines.ready
            && !lines.hold
            && !(lines.interrupt && self.cpu.interrupts_enabled());

        let mut remaining = t_state_budget;
        while remaining != 0 && self.machine.running {
            if let Some(opcode) = self.compiled_full_opcode(remaining, full_window) {
                if let Some(elapsed) = self.execute_compiled_full_instruction(opcode) {
                    // Static no-wait eligibility plus the 18T reservation above
                    // make this a proof, not a heuristic. Keep saturating_sub as
                    // a release-build guard if future opcode coverage changes.
                    debug_assert!(elapsed <= remaining);
                    remaining = remaining.saturating_sub(elapsed);
                    continue;
                }
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
        // LHLD 0010h = 16T and exercises opcode/operand/data reads through the
        // same physical RuntimeRamCard storage used by the partial fabric.
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

        // The next partial edge must materialize the lazy connector graph and
        // continue from the exact boundary without changing CPU time semantics.
        backend.service_execution_compiled(1).unwrap();
        assert_eq!(backend.cpu.total_t_states(), 17);
    }

    #[test]
    fn compiled_full_rejects_phase_sensitive_serial_and_overlapping_ram_chassis() {
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
        assert!(!serial.compiled_full_chassis_available());

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
