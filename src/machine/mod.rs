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

    pub fn installed_ram_bytes(&self) -> usize {
        self.memory.installed_size()
    }

    pub fn initialize_memory(&mut self) {
        self.memory.initialize();
        self.refresh_protect_line();
    }

    pub fn randomize(&mut self) {
        self.memory.randomize();
    }

    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.memory.arm_basic32_full_memory_probe_guard()
    }

    pub fn clear_transient_memory_guards(&mut self) {
        self.memory.clear_transient_guards();
    }

    pub fn load(&mut self, address: u16, bytes: &[u8]) {
        self.memory.load(address, bytes);
    }

    pub fn clear_protection(&mut self) {
        self.memory.clear_protection();
        self.refresh_protect_line();
    }

    pub fn board_index(address: u16) -> Option<usize> {
        Memory::board_index(address)
    }

    pub fn is_protected(&self, address: u16) -> bool {
        self.memory.is_protected(address)
    }

    pub fn set_protected(&mut self, address: u16, protected: bool) {
        self.memory.set_protected(address, protected);
        self.refresh_protect_line();
    }

    pub fn serial_receive(&mut self, byte: u8) {
        self.io.serial_receive(byte);
    }

    pub fn serial_rx_empty(&self) -> bool {
        self.io.serial_rx_empty()
    }

    pub fn serial_rx_len(&self) -> usize {
        self.io.serial_rx_len()
    }

    pub fn serial_tx_front(&self) -> Option<u8> {
        self.io.serial_tx_front()
    }

    pub fn serial_tx_complete(&mut self) -> Option<u8> {
        self.io.serial_tx_complete()
    }

    pub fn tx_busy(&self) -> bool {
        self.io.serial_tx_busy()
    }

    pub fn clear_serial(&mut self) {
        self.io.clear_serial();
    }

    fn panel_switches(&self) -> u16 {
        self.panel.switches()
    }

    fn toggle_panel_switch(&mut self, bit: usize) {
        self.panel.toggle_switch(bit);
    }

    fn panel_lamps(&self) -> PanelLampSnapshot {
        self.s100.snapshot()
    }

    fn panel_address(&self) -> u16 {
        self.s100.signals().address
    }

    fn panel_data(&self) -> u8 {
        self.s100.signals().data
    }

    fn sync_cpu_inte(&mut self, enabled: bool) {
        self.cpu_inte = enabled;
        self.s100.set_inte(enabled);
    }

    fn set_ready(&mut self, ready: bool) {
        self.s100.set_ready(ready);
    }

    fn set_hold(&mut self, hold: bool) {
        self.s100.set_hold(hold);
    }

    fn hold_requested(&self) -> bool {
        self.s100.signals().hold
    }

    fn set_hlda(&mut self, hlda: bool) {
        self.s100.set_hlda(hlda);
    }

    fn hlda(&self) -> bool {
        self.s100.signals().hlda
    }

    fn freeze_panel_bus(&mut self) {
        self.s100.freeze();
    }

    fn commit_panel_activity(&mut self, dt: Duration, dynamic: bool) {
        self.s100.commit(dt, dynamic);
    }

    fn refresh_protect_line(&mut self) {
        let address = self.s100.signals().address;
        self.s100.refresh_protect(self.memory.is_protected(address));
    }

    fn drive_cpu_cycle(&mut self, address: u16, data: u8, cycle: S100Cycle) {
        let protected = self.memory.is_protected(address);
        self.s100
            .drive_cpu_cycle(address, data, cycle, protected, self.cpu_inte);
    }

    fn front_panel_stopped(&mut self, address: u16) {
        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        self.s100
            .drive_front_panel_reset(address, data, protected, self.cpu_inte);
    }

    fn front_panel_examine(&mut self, address: u16) -> u8 {
        let data = self.memory.read(address);
        let protected = self.memory.is_protected(address);
        self.s100
            .drive_front_panel_examine(address, data, protected, self.cpu_inte);
        data
    }

    fn front_panel_deposit(&mut self, address: u16, value: u8) {
        let protected = self.memory.is_protected(address);
        self.s100
            .drive_front_panel_deposit(address, value, protected, self.cpu_inte);
        self.memory.write(address, value);
        self.refresh_protect_line();
    }

    fn power_off_s100(&mut self) {
        self.s100.power_off();
        self.cpu_inte = false;
    }

    #[inline]
    fn io_bus_address(port: u8) -> u16 {
        // Intel 8080 I/O cycles duplicate the 8-bit port number on both halves
        // of the 16-bit address bus.
        u16::from(port) * 0x0101
    }
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
        let value = match port {
            0xff => self.panel.input(),
            _ => self.io.input(port),
        };
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::InputRead);
        value
    }

    fn output(&mut self, port: u8, value: u8) {
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::OutputWrite);
        if port != 0xff {
            self.io.output(port, value);
        }
    }

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
    pub running: bool,
}

impl Default for AltairMachine {
    fn default() -> Self {
        Self {
            cpu: Cpu8080::new(),
            bus: AltairBus::default(),
            powered: false,
            running: false,
        }
    }
}

impl AltairMachine {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.running = false;
        self.bus.configure_memory(size, init_mode);
        self.cpu.reset();
        self.bus.clear_serial();
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        if self.powered {
            self.bus.set_ready(false);
            self.bus.front_panel_stopped(0);
        } else {
            self.bus.power_off_s100();
        }
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.bus.installed_ram_bytes()
    }

    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.bus.arm_basic32_full_memory_probe_guard()
    }

    /// Apply/remove power. The original 8800 did not guarantee a CPU reset at
    /// power-on, so the processor starts in an undefined state and the front
    /// panel holds READY low until the operator performs RESET/RUN.
    pub fn power(&mut self, on: bool) {
        self.powered = on;
        self.running = false;
        if on {
            self.bus.clear_protection();
            self.bus.clear_transient_memory_guards();
            self.bus.clear_serial();
            self.randomize_power_on_cpu();
            self.bus.sync_cpu_inte(self.cpu.inte);
            self.bus.set_ready(false);
            self.bus.set_hlda(false);
            self.bus.panel.set_address_latch(self.cpu.pc);
            self.bus.front_panel_stopped(self.cpu.pc);
        } else {
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

    /// Physical RESET position: reset only the CPU/front-panel address control;
    /// CLR remains a separate I/O signal on the opposite switch position.
    pub fn front_panel_reset(&mut self) {
        if !self.powered {
            return;
        }
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        self.running = false;
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.set_ready(false);
        self.bus.front_panel_stopped(address);
    }

    /// Convenience reset for loaders/reconfiguration. This intentionally adds
    /// I/O clear to the physical RESET operation.
    pub fn reset(&mut self) {
        if !self.powered {
            return;
        }
        self.front_panel_reset();
        self.bus.clear_serial();
    }

    pub fn clear_io(&mut self) {
        if self.powered {
            self.bus.clear_serial();
        }
    }

    pub fn set_running(&mut self, run: bool) {
        if !self.powered {
            return;
        }
        self.running = run;
        if run {
            self.bus.set_ready(true);
        } else {
            self.bus.panel.set_address_latch(self.bus.panel_address());
            self.bus.set_ready(false);
            self.bus.set_hlda(false);
            self.bus.freeze_panel_bus();
        }
    }

    pub fn step(&mut self) {
        if !self.powered || self.running {
            return;
        }
        if self.bus.hold_requested() {
            self.bus.set_hlda(true);
            self.bus.freeze_panel_bus();
            return;
        }
        self.bus.set_hlda(false);
        self.bus.set_ready(true);
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.cpu.step(&mut self.bus);
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.panel.set_address_latch(self.bus.panel_address());
        self.bus.set_ready(false);
        self.bus.freeze_panel_bus();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        if !self.powered || !self.running {
            return;
        }
        self.bus.set_ready(true);
        if self.bus.hold_requested() {
            // With no cycle-level CPU the request is acknowledged at the next
            // instruction boundary, which is the strongest faithful guarantee
            // available without changing the processor core.
            self.bus.set_hlda(true);
            return;
        }
        self.bus.set_hlda(false);
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.cpu.run_cycles(&mut self.bus, cycles);
        self.bus.sync_cpu_inte(self.cpu.inte);
    }

    /// Future DMA/bus-master peripherals assert HOLD here. HLDA is generated by
    /// the CPU-side arbitration path; the renderer never owns either signal.
    pub fn request_hold(&mut self, hold: bool) {
        self.bus.set_hold(hold);
        if !hold {
            self.bus.set_hlda(false);
        }
    }

    pub fn commit_panel_activity(&mut self, dt: Duration) {
        let dynamic = self.powered
            && self.running
            && !self.cpu.halted
            && !self.bus.hlda();
        self.bus.commit_panel_activity(dt, dynamic);
    }

    pub fn examine(&mut self, next: bool) {
        if !self.powered || self.running {
            return;
        }
        let address = if next {
            self.bus.panel.examine_next_address()
        } else {
            self.bus.panel.examine_address()
        };
        // The original EXAMINE logic injects JMP/NOP control bytes and leaves
        // the processor stopped at the selected address. We model that hardware
        // transaction atomically because the 8080 core is instruction-level.
        self.cpu.pc = address;
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_ready(false);
        let _ = self.bus.front_panel_examine(address);
    }

    pub fn deposit(&mut self, next: bool) {
        if !self.powered || self.running {
            return;
        }
        let address = if next {
            self.bus.panel.deposit_next_address()
        } else {
            self.bus.panel.deposit_address()
        };
        if next {
            self.cpu.pc = address;
        }
        let value = self.bus.panel_switches() as u8;
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_ready(false);
        self.bus.front_panel_deposit(address, value);
    }

    pub fn protect_current_board(&mut self, protected: bool) {
        if !self.powered || self.running {
            return;
        }
        let address = self.bus.panel.address_latch();
        self.bus.set_protected(address, protected);
        self.bus.refresh_protect_line();
        self.bus.freeze_panel_bus();
    }

    /// The PROT lamp reads the emulated S-100 PS line, not the memory model
    /// directly. `set_protected` is responsible for updating PS.
    pub fn current_board_protected(&self) -> bool {
        self.powered && self.bus.s100.signals().prot
    }

    pub fn panel_switches(&self) -> u16 {
        self.bus.panel_switches()
    }

    pub fn toggle_sense_switch(&mut self, bit: usize) {
        self.bus.toggle_panel_switch(bit);
    }

    pub fn address_leds(&self) -> u16 {
        self.bus.panel_address()
    }

    pub fn data_leds(&self) -> u8 {
        self.bus.panel_data()
    }

    pub fn panel_lamps(&self) -> PanelLampSnapshot {
        self.bus.panel_lamps()
    }

    /// Backward-compatible query; WAIT itself is now the S-100 PWAIT signal.
    pub fn wait_led(&self) -> bool {
        self.powered && self.bus.s100.signals().wait
    }
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
    fn reset_drives_front_panel_bus_without_inventing_m1() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.bus.load(0, &[0xa5]);
        machine.front_panel_reset();
        assert_eq!(machine.cpu.pc, 0);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0xa5);
        let lamps = machine.panel_lamps();
        assert_eq!(lamps.wait, 1.0);
        assert_eq!(lamps.memr, 0.0);
        assert_eq!(lamps.m1, 0.0);
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
    fn clr_preserves_cpu_state_but_clears_serial() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.cpu.pc = 0x1234;
        machine.bus.serial_receive(b'X');
        machine.clear_io();
        assert_eq!(machine.cpu.pc, 0x1234);
        assert_eq!(machine.bus.serial_rx_len(), 0);
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
        machine.commit_panel_activity(Duration::ZERO);
        assert_eq!(machine.panel_lamps().hlda, 0.0);
    }

    #[test]
    fn examine_and_deposit_are_front_panel_bus_transactions() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.bus.load(0, &[0x12]);
        machine.examine(false);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0x12);
        assert_eq!(machine.panel_lamps().memr, 1.0);

        for bit in [1, 2, 4, 6] {
            machine.toggle_sense_switch(bit);
        }
        machine.deposit(false);
        assert_eq!(machine.bus.peek_memory(0), Some(0x56));
        assert_eq!(machine.panel_lamps().wo, 1.0);
    }
}
