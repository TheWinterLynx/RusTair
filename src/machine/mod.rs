mod front_panel;
mod io_devices;
mod memory;
mod panel_bus;
mod serial;

use std::time::Duration;

use rand::RngCore;

use crate::config::{RamInit, RamSize};
use crate::cpu8080::{Bus, Cpu8080};
use front_panel::FrontPanelController;
use io_devices::IoDevices;
use memory::Memory;
use panel_bus::{S100BusState, S100Cycle};

pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};
pub use panel_bus::PanelLampSnapshot;

pub const CLOCK_HZ: u32 = 2_000_000;

pub struct AltairBus {
    memory: Memory,
    io: IoDevices,
    panel: FrontPanelController,
    s100: S100BusState,
    cpu_inte: bool,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut s = Self {
            memory: Memory::default(),
            io: IoDevices::default(),
            panel: FrontPanelController::default(),
            s100: S100BusState::default(),
            cpu_inte: false,
        };
        s.initialize_memory();
        s
    }
}

impl AltairBus {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.memory.configure(size, init_mode);
        self.refresh_protect_line();
    }

    pub fn installed_ram_bytes(&self) -> usize { self.memory.installed_size() }
    pub fn initialize_memory(&mut self) { self.memory.initialize(); self.refresh_protect_line(); }
    pub fn randomize(&mut self) { self.memory.randomize(); }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.memory.arm_basic32_full_memory_probe_guard() }
    pub fn clear_transient_memory_guards(&mut self) { self.memory.clear_transient_guards(); }
    pub fn load(&mut self, address: u16, bytes: &[u8]) { self.memory.load(address, bytes); }
    pub fn clear_protection(&mut self) { self.memory.clear_protection(); self.refresh_protect_line(); }
    pub fn board_index(address: u16) -> Option<usize> { Memory::board_index(address) }
    pub fn is_protected(&self, address: u16) -> bool { self.memory.is_protected(address) }
    pub fn set_protected(&mut self, address: u16, protected: bool) { self.memory.set_protected(address, protected); self.refresh_protect_line(); }
    pub fn serial_receive(&mut self, byte: u8) { self.io.serial_receive(byte); }
    pub fn serial_rx_empty(&self) -> bool { self.io.serial_rx_empty() }
    pub fn serial_rx_len(&self) -> usize { self.io.serial_rx_len() }
    pub fn serial_tx_front(&self) -> Option<u8> { self.io.serial_tx_front() }
    pub fn serial_tx_complete(&mut self) -> Option<u8> { self.io.serial_tx_complete() }
    pub fn tx_busy(&self) -> bool { self.io.serial_tx_busy() }
    pub fn clear_serial(&mut self) { self.io.clear_serial(); }

    fn panel_switches(&self) -> u16 { self.panel.switches() }
    fn toggle_panel_switch(&mut self, bit: usize) { self.panel.toggle_switch(bit); }
    fn panel_lamps(&self) -> PanelLampSnapshot { self.s100.snapshot() }
    fn panel_address(&self) -> u16 { self.s100.signals().address }
    fn panel_data(&self) -> u8 { self.s100.signals().data }

    fn sync_cpu_inte(&mut self, enabled: bool) {
        self.cpu_inte = enabled;
        self.s100.set_inte(enabled);
    }

    fn set_run(&mut self, run: bool) { self.s100.set_run(run); }
    fn set_ready(&mut self, ready: bool) { self.s100.set_ready(ready); }
    fn set_hold(&mut self, hold: bool) { self.s100.set_hold(hold); }
    fn hold_requested(&self) -> bool { self.s100.signals().hold }
    fn set_hlda(&mut self, hlda: bool) { self.s100.set_hlda(hlda); }
    fn hlda(&self) -> bool { self.s100.signals().hlda }
    fn reset_asserted(&self) -> bool { self.s100.signals().reset }
    fn ext_clear_asserted(&self) -> bool { self.s100.signals().ext_clear }
    fn freeze_panel_bus(&mut self) { self.s100.freeze(); }
    fn commit_panel_activity(&mut self, dt: Duration, dynamic: bool) { self.s100.commit(dt, dynamic); }

    fn refresh_protect_line(&mut self) {
        let address = self.s100.signals().address;
        self.s100.refresh_protect(self.memory.is_protected(address));
    }

    fn drive_cpu_cycle(&mut self, address: u16, data: u8, cycle: S100Cycle) {
        let protected = self.memory.is_protected(address);
        self.s100.drive_cpu_cycle(address, data, cycle, protected, self.cpu_inte);
    }

    fn drive_power_on_state(&mut self, address: u16, run: bool) {
        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        self.s100
            .drive_power_on_state(address, data, protected, self.cpu_inte, run);
    }

    fn assert_front_panel_reset_bus(&mut self) {
        self.s100.assert_front_panel_reset();
    }

    fn release_front_panel_reset_bus(&mut self, address: u16, run: bool) {
        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        self.s100
            .release_front_panel_reset(address, data, protected, self.cpu_inte, run);
    }

    fn set_ext_clear(&mut self, asserted: bool) {
        let was_asserted = self.s100.signals().ext_clear;
        self.s100.set_ext_clear(asserted);
        // EXT CLR is a physical S-100 line. Installed I/O boards react to its
        // assertion; the GUI does not clear UART queues directly.
        if asserted && !was_asserted {
            self.io.clear_serial();
        }
    }

    fn front_panel_examine(&mut self, address: u16) -> u8 {
        let data = self.memory.read(address);
        let protected = self.memory.is_protected(address);
        self.s100.drive_front_panel_examine(address, data, protected, self.cpu_inte);
        data
    }

    fn front_panel_deposit(&mut self, address: u16, value: u8) {
        let protected = self.memory.is_protected(address);
        self.s100.drive_front_panel_deposit(address, value, protected, self.cpu_inte);
        self.memory.write(address, value);
        self.refresh_protect_line();
    }

    fn power_off_s100(&mut self) {
        self.s100.power_off();
        self.cpu_inte = false;
    }

    #[inline]
    fn io_bus_address(port: u8) -> u16 { u16::from(port) * 0x0101 }
}

impl Bus for AltairBus {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::MemoryRead);
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.drive_cpu_cycle(address, value, S100Cycle::MemoryWrite);
        self.memory.write(address, value);
    }

    fn input(&mut self, port: u8) -> u8 {
        let value = match port { 0xff => self.panel.input(), _ => self.io.input(port) };
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::InputRead);
        value
    }

    fn output(&mut self, port: u8, value: u8) {
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::OutputWrite);
        if port != 0xff { self.io.output(port, value); }
    }

    fn set_inte(&mut self, enabled: bool) { self.sync_cpu_inte(enabled); }

    fn opcode_fetch(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::InstructionFetch);
        value
    }

    fn stack_read(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::StackRead);
        value
    }

    fn stack_write(&mut self, address: u16, value: u8) {
        self.drive_cpu_cycle(address, value, S100Cycle::StackWrite);
        self.memory.write(address, value);
    }

    fn halt_ack(&mut self, address: u16, opcode: u8) {
        self.drive_cpu_cycle(address, opcode, S100Cycle::HaltAcknowledge);
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        let cycle = if while_halted {
            S100Cycle::InterruptAcknowledgeWhileHalted
        } else {
            S100Cycle::InterruptAcknowledge
        };
        self.drive_cpu_cycle(address, opcode, cycle);
    }
}

pub struct AltairMachine {
    pub cpu: Cpu8080,
    pub bus: AltairBus,
    pub powered: bool,
    /// Mirrors the physical RUN/STOP R-S latch, not merely "CPU is executing".
    pub running: bool,
    stop_switch_asserted: bool,
    run_switch_asserted: bool,
}

impl Default for AltairMachine {
    fn default() -> Self {
        Self {
            cpu: Cpu8080::new(),
            bus: AltairBus::default(),
            powered: false,
            running: false,
            stop_switch_asserted: false,
            run_switch_asserted: false,
        }
    }
}

impl AltairMachine {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.running = false;
        self.bus.set_run(false);
        self.bus.configure_memory(size, init_mode);
        self.cpu.reset();
        self.bus.clear_serial();
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        if self.powered {
            self.front_panel_reset();
        } else {
            self.bus.power_off_s100();
        }
    }

    pub fn installed_ram_bytes(&self) -> usize { self.bus.installed_ram_bytes() }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.bus.arm_basic32_full_memory_probe_guard() }

    /// Safe default power operation. The CPU power-on state remains undefined,
    /// but the RUN/STOP latch is forced to STOP unless historical mode is
    /// explicitly selected by the caller.
    pub fn power(&mut self, on: bool) {
        self.power_with_historical_run_latch(on, false);
    }

    /// Apply/remove power with optional reproduction of the original undefined
    /// RUN/STOP flip-flop power-on state. Historical mode is intentionally
    /// opt-in because a random RUN state can start executing immediately.
    pub fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) {
        self.powered = on;
        self.stop_switch_asserted = false;
        self.run_switch_asserted = false;
        if on {
            self.bus.clear_protection();
            self.bus.clear_transient_memory_guards();
            self.bus.clear_serial();
            self.randomize_power_on_cpu();
            let run = historical && (rand::rng().next_u32() & 1 != 0);
            self.running = run;
            self.bus.set_run(run);
            self.bus.sync_cpu_inte(self.cpu.inte);
            self.bus.set_hlda(false);
            self.bus.panel.set_address_latch(self.cpu.pc);
            self.bus.drive_power_on_state(self.cpu.pc, run);
        } else {
            self.running = false;
            self.bus.clear_serial();
            self.bus.initialize_memory();
            self.bus.power_off_s100();
        }
    }

    fn randomize_power_on_cpu(&mut self) {
        self.cpu.reset();
        let mut rng = rand::rng();
        self.cpu.a = rng.next_u32() as u8;
        self.cpu.b = rng.next_u32() as u8;
        self.cpu.c = rng.next_u32() as u8;
        self.cpu.d = rng.next_u32() as u8;
        self.cpu.e = rng.next_u32() as u8;
        self.cpu.h = rng.next_u32() as u8;
        self.cpu.l = rng.next_u32() as u8;
        self.cpu.f = ((rng.next_u32() as u8) & 0xd5) | 0x02;
        self.cpu.pc = rng.next_u32() as u16;
        self.cpu.sp = rng.next_u32() as u16;
        self.cpu.inte = rng.next_u32() & 1 != 0;
        self.cpu.halted = false;
        self.cpu.cycles = 0;
    }

    /// Assert one side of the physical RUN/STOP switch. RUN sets the R-S latch
    /// immediately. STOP is captured at the next available machine-cycle
    /// boundary; with the 8080 halted there is no useful PSYNC, reproducing the
    /// original case where STOP alone cannot leave HLT.
    pub fn assert_run_stop(&mut self, run: bool) {
        if !self.powered { return; }
        self.run_switch_asserted = run;
        self.stop_switch_asserted = !run;

        if run {
            if !self.bus.reset_asserted() {
                self.set_running(true);
            }
        } else if self.bus.reset_asserted() || !self.cpu.halted {
            self.set_running(false);
        }
    }

    pub fn release_run_stop(&mut self, run: bool) {
        if run {
            self.run_switch_asserted = false;
        } else {
            self.stop_switch_asserted = false;
        }
    }

    /// Assert the physical RESET switch and keep it asserted. RESET clears the
    /// 8080 but does not itself alter the RUN/STOP latch. If STOP is being held,
    /// however, RESET supplies the condition needed for the pending STOP to be
    /// captured, reproducing the documented STOP+RESET recovery from HLT.
    pub fn assert_front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        if self.stop_switch_asserted {
            self.running = false;
            self.bus.set_run(false);
        }
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.assert_front_panel_reset_bus();
    }

    /// Release RESET at 0000h. READY follows the independent RUN/STOP latch:
    /// a machine that was RUN continues from zero, while STOP remains in WAIT.
    pub fn release_front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.cpu.reset();
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.release_front_panel_reset_bus(address, self.running);
    }

    /// Programmatic momentary RESET pulse used by loaders and tests. Physical
    /// semantics are preserved: the RUN/STOP latch is not implicitly changed.
    pub fn front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.assert_front_panel_reset();
        self.release_front_panel_reset();
    }

    pub fn reset(&mut self) {
        if !self.powered { return; }
        self.front_panel_reset();
        self.bus.clear_serial();
    }

    /// Assert the physical EXT CLR line (S-100 pin 54). I/O cards react to the
    /// bus signal; CPU registers and RUN/STOP state are untouched.
    pub fn assert_front_panel_clear(&mut self) {
        if !self.powered { return; }
        self.bus.set_ext_clear(true);
    }

    pub fn release_front_panel_clear(&mut self) {
        if !self.powered { return; }
        self.bus.set_ext_clear(false);
    }

    /// Convenience pulse used by non-interactive callers.
    pub fn clear_io(&mut self) {
        if !self.powered { return; }
        self.assert_front_panel_clear();
        self.release_front_panel_clear();
    }

    /// Programmatic latch control used by loaders/configuration. The physical
    /// front-panel switch uses `assert_run_stop`/`release_run_stop` instead.
    pub fn set_running(&mut self, run: bool) {
        if !self.powered || self.bus.reset_asserted() { return; }
        self.running = run;
        self.bus.set_run(run);
        if run {
            self.bus.set_ready(true);
        } else {
            let address = self.bus.panel_address();
            self.bus.panel.set_address_latch(address);
            self.bus.set_ready(false);
            self.bus.set_hlda(false);
            self.bus.freeze_panel_bus();
        }
    }

    pub fn step(&mut self) {
        if !self.powered || self.running || self.bus.reset_asserted() { return; }
        if self.bus.hold_requested() {
            self.bus.set_hlda(true);
            self.bus.freeze_panel_bus();
            return;
        }
        self.bus.set_hlda(false);
        self.bus.set_ready(true);
        self.bus.sync_cpu_inte(self.cpu.inte);
        // Limitation retained by design: the CPU core is instruction-level, so
        // this executes one instruction rather than one physical machine cycle.
        self.cpu.step(&mut self.bus);
        self.bus.sync_cpu_inte(self.cpu.inte);
        let address = self.bus.panel_address();
        self.bus.panel.set_address_latch(address);
        self.bus.set_ready(false);
        self.bus.freeze_panel_bus();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        if !self.powered || !self.running || self.bus.reset_asserted() { return; }
        self.bus.set_ready(true);
        if self.bus.hold_requested() {
            self.bus.set_hlda(true);
            return;
        }
        self.bus.set_hlda(false);
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.cpu.run_cycles(&mut self.bus, cycles);
        self.bus.sync_cpu_inte(self.cpu.inte);
    }

    pub fn request_hold(&mut self, hold: bool) {
        self.bus.set_hold(hold);
        if !hold { self.bus.set_hlda(false); }
    }

    pub fn commit_panel_activity(&mut self, dt: Duration) {
        let dynamic = self.powered
            && self.running
            && !self.cpu.halted
            && !self.bus.hlda()
            && !self.bus.reset_asserted();
        self.bus.commit_panel_activity(dt, dynamic);
    }

    pub fn examine(&mut self, next: bool) {
        if !self.powered || self.running || self.bus.reset_asserted() { return; }
        let address = if next {
            self.bus.panel.examine_next_address()
        } else {
            self.bus.panel.examine_address()
        };
        self.cpu.pc = address;
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_ready(false);
        let _ = self.bus.front_panel_examine(address);
    }

    pub fn deposit(&mut self, next: bool) {
        if !self.powered || self.running || self.bus.reset_asserted() { return; }
        let address = if next {
            self.bus.panel.deposit_next_address()
        } else {
            self.bus.panel.deposit_address()
        };
        if next { self.cpu.pc = address; }
        let value = self.bus.panel_switches() as u8;
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_ready(false);
        self.bus.front_panel_deposit(address, value);
    }

    pub fn protect_current_board(&mut self, protected: bool) {
        if !self.powered || self.running || self.bus.reset_asserted() { return; }
        let address = self.bus.panel.address_latch();
        self.bus.set_protected(address, protected);
        self.bus.refresh_protect_line();
        self.bus.freeze_panel_bus();
    }

    pub fn current_board_protected(&self) -> bool { self.powered && self.bus.s100.signals().prot }
    pub fn panel_switches(&self) -> u16 { self.bus.panel_switches() }
    pub fn toggle_sense_switch(&mut self, bit: usize) { self.bus.toggle_panel_switch(bit); }
    pub fn address_leds(&self) -> u16 { self.bus.panel_address() }
    pub fn data_leds(&self) -> u8 { self.bus.panel_data() }
    pub fn panel_lamps(&self) -> PanelLampSnapshot { self.bus.panel_lamps() }
    pub fn wait_led(&self) -> bool { self.powered && self.bus.s100.signals().wait }
    pub fn ext_clear_asserted(&self) -> bool { self.powered && self.bus.ext_clear_asserted() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protection_is_per_1k_board() {
        let mut bus = AltairBus::default();
        bus.set_protected(0x0410, true);
        assert!(!bus.is_protected(0x03ff));
        assert!(bus.is_protected(0x0400));
        assert!(bus.is_protected(0x07ff));
        assert!(!bus.is_protected(0x0800));
    }

    #[test]
    fn reset_held_and_released_match_mits_checkout_sequence_when_stopped() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.bus.load(0, &[0xa5]);

        machine.assert_front_panel_reset();
        assert_eq!(machine.address_leds(), 0xffff);
        assert_eq!(machine.data_leds(), 0xff);
        let held = machine.panel_lamps();
        assert_eq!(held.inte, 0.0);
        assert_eq!(held.memr, 0.0);
        assert_eq!(held.m1, 0.0);
        assert_eq!(held.wo, 0.0);
        assert_eq!(held.wait, 0.0);

        machine.release_front_panel_reset();
        assert_eq!(machine.cpu.pc, 0);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0xa5);
        let released = machine.panel_lamps();
        assert_eq!(released.inte, 0.0);
        assert_eq!(released.memr, 1.0);
        assert_eq!(released.m1, 1.0);
        assert_eq!(released.wo, 0.0);
        assert_eq!(released.wait, 1.0);
    }

    #[test]
    fn physical_reset_preserves_run_latch() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.set_running(true);
        machine.assert_front_panel_reset();
        assert!(machine.running);
        machine.release_front_panel_reset();
        assert!(machine.running);
        assert!(!machine.wait_led());
        assert_eq!(machine.cpu.pc, 0);
    }

    #[test]
    fn stop_while_halted_requires_stop_plus_reset_recovery() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.set_running(true);
        machine.cpu.halted = true;

        machine.assert_run_stop(false);
        assert!(machine.running, "STOP cannot latch without PSYNC while halted");
        machine.assert_front_panel_reset();
        assert!(!machine.running, "held STOP must latch when RESET supplies recovery");
        machine.release_front_panel_reset();
        machine.release_run_stop(false);
        assert!(machine.wait_led());
    }

    #[test]
    fn front_panel_reset_preserves_serial_io_state() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.bus.serial_receive(b'R');
        machine.front_panel_reset();
        assert_eq!(machine.cpu.pc, 0);
        assert_eq!(machine.bus.serial_rx_len(), 1);
    }

    #[test]
    fn ext_clear_is_held_bus_signal_and_clears_io_without_touching_cpu() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.cpu.pc = 0x1234;
        machine.bus.serial_receive(b'X');

        machine.assert_front_panel_clear();
        assert!(machine.ext_clear_asserted());
        assert_eq!(machine.cpu.pc, 0x1234);
        assert_eq!(machine.bus.serial_rx_len(), 0);

        machine.release_front_panel_clear();
        assert!(!machine.ext_clear_asserted());
        assert_eq!(machine.cpu.pc, 0x1234);
    }

    #[test]
    fn safe_power_on_defaults_run_latch_to_stop() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        assert!(!machine.running);
        assert!(!machine.bus.s100.signals().run);
    }

    #[test]
    fn hold_request_drives_hlda_through_bus_arbitration() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.set_running(true);
        machine.request_hold(true);
        machine.run_cycles(10);
        machine.commit_panel_activity(Duration::from_secs(1));
        assert_eq!(machine.panel_lamps().hlda, 1.0);
        machine.request_hold(false);
        assert!(!machine.bus.s100.signals().hlda);
    }

    #[test]
    fn examine_and_deposit_drive_front_panel_bus_with_mits_wo_semantics() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.bus.load(0, &[0x12]);
        machine.examine(false);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0x12);
        assert_eq!(machine.panel_lamps().memr, 1.0);
        assert_eq!(machine.panel_lamps().wo, 0.0);

        for bit in [1, 2, 4, 6] { machine.toggle_sense_switch(bit); }
        machine.deposit(false);
        assert_eq!(machine.bus.peek_memory(0), Some(0x56));
        assert_eq!(machine.panel_lamps().wo, 1.0);
    }
}
