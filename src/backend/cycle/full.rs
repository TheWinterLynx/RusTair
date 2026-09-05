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
    /// First compiled chassis class: one MITS 8080 CPU plus any number of the
    /// asynchronous/no-wait MITS static RAM boards. Every other real card keeps
    /// the partial engine authoritative until it gets an explicit event model.
    ///
    /// This whitelist is intentionally physical rather than performance-based:
    /// 88-MCS needs READY waits, dynamic/synchronous RAM observes clocks, and
    /// SIO/2SIO can change state from independent serial clocks.
    fn compiled_full_chassis_available(&self) -> bool {
        let mut saw_ram = false;
        for (_, card) in self.machine.bus.s100_hardware_memory().installed_cards() {
            match card {
                S100InstalledCardConfig::Mits8080Cpu => {}
                S100InstalledCardConfig::Ram(config)
                    if matches!(
                        config.model,
                        S100RamBoardModel::Mits4KStatic88_4Mcs
                            | S100RamBoardModel::Mits16KStatic88_16Mcs
                    ) =>
                {
                    saw_ram = true;
                }
                _ => return false,
            }
        }
        saw_ram
    }

    fn compiled_full_opcode(&mut self, remaining: u32) -> Option<u8> {
        if remaining < FULL_EXECUTION_MAX_T_STATES
            || !self.at_instruction_boundary()
            || self.stop_wait_park_pending
            || self.cpu_fault.is_some()
            || !self.compiled_full_chassis_available()
        {
            return None;
        }

        // Full is entered only at the same boundary where the partial core would
        // arbitrate INTR/HOLD/READY. No host-side event can arrive in the middle
        // of this synchronous call, and this first compiled chassis class has no
        // independent-clock card, so the accepted instruction is deterministic.
        self.machine.bus.refresh_interrupt_request_line();
        let lines = self.machine.bus.cpu_control_lines();
        if !lines.ready
            || lines.reset
            || lines.hold
            || (lines.interrupt && self.cpu.interrupts_enabled())
        {
            return None;
        }

        let opcode = self
            .machine
            .bus
            .preview_guest_memory(self.cpu.registers().pc);
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
        self.machine.bus.refresh_interrupt_request_line();
        Some(elapsed)
    }

    pub(super) fn service_execution_compiled(
        &mut self,
        t_state_budget: u32,
    ) -> BackendResult<()> {
        let lines = self.machine.bus.cpu_control_lines();
        if t_state_budget == 0
            || !self.machine.powered
            || !self.machine.running
            || lines.reset
        {
            return self.fail_if_cpu_fault("service execution");
        }

        let mut remaining = t_state_budget;
        while remaining != 0 && self.machine.running {
            if let Some(opcode) = self.compiled_full_opcode(remaining) {
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
        self.fail_if_cpu_fault("service execution")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

        let opcode = backend
            .compiled_full_opcode(FULL_EXECUTION_MAX_T_STATES)
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
    fn compiled_full_rejects_phase_sensitive_or_serial_chassis() {
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
    }
}
