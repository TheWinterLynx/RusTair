//! MAME-style prepared execution path for the instruction-level 8080.
//!
//! Fast mode is not cycle-exact, so rebuilding synthetic S-100 samples for every
//! opcode/operand byte is presentation work, not guest-visible hardware work.
//! This adapter keeps the real bus-owned S-100 decode and the same physical RAM
//! card storage, but defers front-panel projection until the end of a quiet
//! execution block. I/O, interrupt-enable changes, HALT, active serial timing,
//! HOLD/RESET and accepted PINT force an immediate return to the existing fully
//! synchronized Fast path.

use crate::config::S100InstalledCardConfig;
use crate::cpu8080::Bus;
use crate::s100_memory::S100RamBoardModel;

use super::super::cpu_board::S100Cycle;
use super::super::{AltairBus, AltairMachine};

#[derive(Clone, Copy)]
struct FastVisibleCycle {
    address: u16,
    data: u8,
    cycle: S100Cycle,
}

struct FastExecutionBus<'a> {
    bus: &'a mut AltairBus,
    no_wait_memory: bool,
    last_visible: Option<FastVisibleCycle>,
    synchronization_requested: bool,
    io_touched: bool,
}

impl<'a> FastExecutionBus<'a> {
    fn new(bus: &'a mut AltairBus, no_wait_memory: bool) -> Self {
        Self {
            bus,
            no_wait_memory,
            last_visible: None,
            synchronization_requested: false,
            io_touched: false,
        }
    }

    #[inline]
    fn account_read_wait(&mut self, address: u16) {
        if !self.no_wait_memory {
            self.bus.fast_account_memory_read_wait(address);
        }
    }

    #[inline]
    fn read_memory(&mut self, address: u16, cycle: S100Cycle) -> u8 {
        self.account_read_wait(address);
        // This is the normal guest path owned by Memory/S100RuntimeFabric, not a
        // debugger/peek shortcut. Unique responders use the compiled card decode;
        // overlap still falls back to the generic electrical transaction.
        let value = self.bus.memory.read(address);
        self.last_visible = Some(FastVisibleCycle { address, data: value, cycle });
        value
    }

    #[inline]
    fn write_memory(&mut self, address: u16, value: u8, cycle: S100Cycle) {
        self.bus.memory.write(address, value);
        self.last_visible = Some(FastVisibleCycle { address, data: value, cycle });
    }

    fn request_external_sync(&mut self) {
        self.synchronization_requested = true;
        self.last_visible = None;
    }
}

impl Bus for FastExecutionBus<'_> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        self.read_memory(address, S100Cycle::MemoryRead)
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        self.write_memory(address, value, S100Cycle::MemoryWrite);
    }

    fn input(&mut self, port: u8) -> u8 {
        self.io_touched = true;
        self.request_external_sync();
        <AltairBus as Bus>::input(self.bus, port)
    }

    fn output(&mut self, port: u8, value: u8) {
        self.io_touched = true;
        self.request_external_sync();
        <AltairBus as Bus>::output(self.bus, port, value);
    }

    fn set_inte(&mut self, enabled: bool) {
        // INTE can expose an already asserted PINT at the next instruction
        // boundary. Synchronize there instead of polling PINT on every ordinary
        // memory-only instruction.
        <AltairBus as Bus>::set_inte(self.bus, enabled);
        self.synchronization_requested = true;
    }

    #[inline]
    fn opcode_fetch(&mut self, address: u16) -> u8 {
        self.read_memory(address, S100Cycle::InstructionFetch)
    }

    #[inline]
    fn stack_read(&mut self, address: u16) -> u8 {
        self.read_memory(address, S100Cycle::StackRead)
    }

    #[inline]
    fn stack_write(&mut self, address: u16, value: u8) {
        self.write_memory(address, value, S100Cycle::StackWrite);
    }

    fn halt_ack(&mut self, address: u16, opcode: u8) {
        self.request_external_sync();
        <AltairBus as Bus>::halt_ack(self.bus, address, opcode);
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        self.request_external_sync();
        <AltairBus as Bus>::interrupt_ack(self.bus, address, opcode, while_halted);
    }

    #[inline]
    fn take_wait_states(&mut self) -> u32 {
        self.bus.take_fast_memory_wait_t_states()
    }

    #[inline]
    fn instruction_complete(&mut self, address: u16, opcode: u8, t_states: u32) {
        <AltairBus as Bus>::instruction_complete(self.bus, address, opcode, t_states);
    }
}

fn all_memory_reads_are_no_wait(bus: &AltairBus) -> bool {
    bus.s100_hardware_memory()
        .installed_cards()
        .all(|(_, card)| match card {
            S100InstalledCardConfig::Ram(config) => matches!(
                config.model,
                S100RamBoardModel::Mits4KStatic88_4Mcs
                    | S100RamBoardModel::Mits16KStatic88_16Mcs
            ),
            S100InstalledCardConfig::FastRamCompatibility(config) => {
                config.read_wait_states == 0
            }
            _ => true,
        })
}

/// Conservative proof that advancing chassis time cannot change the legacy Fast
/// UART state. A completed-but-unread RX byte intentionally keeps us out of this
/// path even though its shift clock is idle; this first version prefers a false
/// negative over delaying a possible guest-visible serial transition.
fn serial_timing_is_quiet(bus: &AltairBus) -> bool {
    bus.io.serial_rx_len() == 0
        && bus.io.serial_rx_line_idle()
        && !bus.io.serial_tx_busy()
        && bus.io.port1_rx_len() == 0
        && bus.io.port1_rx_line_idle()
        && !bus.io.port1_tx_busy()
}

impl AltairMachine {
    /// Execute a host budget through the prepared Fast path when the chassis has
    /// no asynchronous event capable of changing CPU inputs inside the block.
    ///
    /// The loop remains instruction-level (Fast mode's advertised contract), but
    /// the CPU sees a monomorphized memory accessor rather than AltairBus's
    /// presentation-heavy synthetic S-100 cycle adapter on every byte access.
    pub(crate) fn run_cycles_compiled_fast(&mut self, cycles: u32) {
        if cycles == 0 || !self.powered || !self.running {
            return;
        }

        self.bus.refresh_interrupt_request_line();
        let lines = self.bus.cpu_control_lines();
        if lines.reset
            || lines.hold
            || !lines.ready
            || self.cpu.halted
            || !serial_timing_is_quiet(&self.bus)
            || (lines.interrupt && self.cpu.inte)
        {
            self.run_cycles(cycles);
            return;
        }

        let no_wait_memory = all_memory_reads_are_no_wait(&self.bus);
        let mut used = 0u32;
        let mut sync_elapsed = 0u32;
        let mut synchronization_requested = false;
        let mut io_touched = false;
        let mut last_visible = None;

        {
            let cpu = &mut self.cpu;
            let bus = &mut self.bus;
            let mut fast_bus = FastExecutionBus::new(bus, no_wait_memory);

            while used < cycles {
                let elapsed = cpu.step(&mut fast_bus);
                used = used.saturating_add(elapsed);
                last_visible = fast_bus.last_visible;

                if fast_bus.synchronization_requested || cpu.halted {
                    synchronization_requested = fast_bus.synchronization_requested;
                    io_touched = fast_bus.io_touched;
                    sync_elapsed = elapsed;
                    break;
                }
            }
        }

        // Fast mode advertises reconstructed, not exact, bus activity. Publish
        // one representative final memory cycle for panel/address/data state
        // instead of replaying every synthetic T-state that just got skipped.
        if let Some(last) = last_visible {
            self.bus.drive_cpu_cycle(last.address, last.data, last.cycle);
        }
        self.bus.sync_cpu_inte(self.cpu.inte);

        if synchronization_requested {
            // An IN/OUT instruction may have started a timed UART operation. The
            // legacy Fast engine advances the UART by that instruction's elapsed
            // time before testing PINT again, so preserve exactly that boundary.
            if io_touched {
                self.bus
                    .advance_serial_hardware_time(u64::from(sync_elapsed));
            }
            let remaining = cycles.saturating_sub(used);
            if remaining != 0 {
                self.run_cycles(remaining);
            }
        } else if self.cpu.halted {
            let remaining = cycles.saturating_sub(used);
            if remaining != 0 {
                self.run_cycles(remaining);
            }
        }
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
    fn compiled_fast_memory_loop_matches_legacy_cpu_state_and_timing() {
        let mut compiled = AltairMachine::default();
        let mut legacy = AltairMachine::default();
        for machine in [&mut compiled, &mut legacy] {
            machine
                .bus
                .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
                .unwrap();
            machine.power(true);
            machine.front_panel_reset();
            machine.bus.load(0, &[0x00, 0xc3, 0x00, 0x00]);
            machine.set_running(true);
        }

        compiled.run_cycles_compiled_fast(14_000);
        legacy.run_cycles(14_000);

        assert_eq!(compiled.cpu.pc, legacy.cpu.pc);
        assert_eq!(compiled.cpu.sp, legacy.cpu.sp);
        assert_eq!(compiled.cpu.f, legacy.cpu.f);
        assert_eq!(compiled.cpu.cycles, legacy.cpu.cycles);
        assert_eq!(compiled.cpu.inte, legacy.cpu.inte);
        assert_eq!(compiled.cpu.halted, legacy.cpu.halted);
    }
}
