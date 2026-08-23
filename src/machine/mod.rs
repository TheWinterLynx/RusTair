mod front_panel;
mod io_devices;
mod memory;
mod panel_bus;
mod serial;

use std::time::Duration;

use rand::RngCore;

use crate::config::{RamInit, RamSize};
use crate::cpu8080::{Bus, Cpu8080};
use front_panel::FrontPanelPort;
use io_devices::IoDevices;
use memory::Memory;
use panel_bus::{PanelBusMonitor, PanelCycle};

pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};
pub use panel_bus::PanelLampSnapshot;

pub const CLOCK_HZ: u32 = 2_000_000;

pub struct AltairBus {
    memory: Memory,
    io: IoDevices,
    panel: FrontPanelPort,
    panel_bus: PanelBusMonitor,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut s = Self {
            memory: Memory::default(),
            io: IoDevices::default(),
            panel: FrontPanelPort::default(),
            panel_bus: PanelBusMonitor::default(),
        };
        s.initialize_memory();
        s
    }
}

impl AltairBus {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.memory.configure(size, init_mode);
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.memory.installed_size()
    }

    pub fn initialize_memory(&mut self) {
        self.memory.initialize();
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
    }

    pub fn board_index(address: u16) -> Option<usize> {
        Memory::board_index(address)
    }

    pub fn is_protected(&self, address: u16) -> bool {
        self.memory.is_protected(address)
    }

    pub fn set_protected(&mut self, address: u16, protected: bool) {
        self.memory.set_protected(address, protected);
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
        self.panel_bus.snapshot()
    }

    fn panel_address(&self) -> u16 {
        self.panel_bus.live_address()
    }

    fn panel_data(&self) -> u8 {
        self.panel_bus.live_data()
    }

    fn force_panel_lamps(&mut self, address: u16, data: u8) {
        self.panel_bus.force_static(address, data);
    }

    fn freeze_panel_bus(&mut self) {
        self.panel_bus.freeze_live();
    }

    fn commit_panel_activity(&mut self, dt: Duration, dynamic: bool) {
        self.panel_bus.commit_activity(dt, dynamic);
    }

    #[inline]
    fn io_bus_address(port: u8) -> u16 {
        // The 8080 duplicates its 8-bit I/O port number on A15..A8 and A7..A0.
        u16::from(port) * 0x0101
    }
}

impl Bus for AltairBus {
    fn read(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.panel_bus
            .observe(address, value, PanelCycle::MemoryRead);
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.panel_bus
            .observe(address, value, PanelCycle::MemoryWrite);
        self.memory.write(address, value);
    }

    fn input(&mut self, port: u8) -> u8 {
        let value = match port {
            0xff => self.panel.input(),
            _ => self.io.input(port),
        };
        self.panel_bus.observe(
            Self::io_bus_address(port),
            value,
            PanelCycle::InputRead,
        );
        value
    }

    fn output(&mut self, port: u8, value: u8) {
        self.panel_bus.observe(
            Self::io_bus_address(port),
            value,
            PanelCycle::OutputWrite,
        );
        // FFh is the sense-switch input on the Altair front panel, not a
        // latched output display. Other ports are delegated to installed I/O.
        if port != 0xff {
            self.io.output(port, value);
        }
    }

    fn opcode_fetch(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.panel_bus
            .observe(address, value, PanelCycle::InstructionFetch);
        value
    }

    fn stack_read(&mut self, address: u16) -> u8 {
        let value = self.memory.read(address);
        self.panel_bus.observe(address, value, PanelCycle::StackRead);
        value
    }

    fn stack_write(&mut self, address: u16, value: u8) {
        self.panel_bus.observe(address, value, PanelCycle::StackWrite);
        self.memory.write(address, value);
    }

    fn halt_ack(&mut self, address: u16, opcode: u8) {
        self.panel_bus
            .observe(address, opcode, PanelCycle::HaltAcknowledge);
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        let cycle = if while_halted {
            PanelCycle::InterruptAcknowledgeWhileHalted
        } else {
            PanelCycle::InterruptAcknowledge
        };
        self.panel_bus.observe(address, opcode, cycle);
    }
}

pub struct AltairMachine {
    pub cpu: Cpu8080,
    pub bus: AltairBus,
    pub powered: bool,
    pub running: bool,
    wait_led: bool,
}

impl Default for AltairMachine {
    fn default() -> Self {
        Self {
            cpu: Cpu8080::new(),
            bus: AltairBus::default(),
            powered: false,
            running: false,
            wait_led: false,
        }
    }
}

impl AltairMachine {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.running = false;
        self.bus.configure_memory(size, init_mode);
        self.cpu.reset();
        self.bus.clear_serial();
        self.wait_led = self.powered;
        self.latch_stopped_fetch();
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.bus.installed_ram_bytes()
    }

    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.bus.arm_basic32_full_memory_probe_guard()
    }

    /// Apply/remove power to the original Altair 8800 model.
    ///
    /// The 8800/8800a power-on-clear did not reliably reset the 8080. Real
    /// operators therefore performed STOP + RESET after switching power on.
    /// We preserve that property by giving the CPU an undefined power-on state
    /// instead of silently calling RESET. The control side starts stopped so a
    /// GUI user can perform the documented manual reset deterministically.
    pub fn power(&mut self, on: bool) {
        self.powered = on;
        self.running = false;
        if on {
            self.bus.clear_protection();
            self.bus.clear_transient_memory_guards();
            self.bus.clear_serial();
            self.randomize_power_on_cpu();
            self.wait_led = true;
            self.latch_stopped_fetch();
        } else {
            self.wait_led = false;
            self.bus.force_panel_lamps(0, 0);
            self.bus.clear_serial();
            self.bus.initialize_memory();
        }
    }

    fn randomize_power_on_cpu(&mut self) {
        // The 8080 has no defined register/PC state until RESET is asserted.
        // Keep the core's internal invariants sane, then randomize the externally
        // visible state to model that undefined power-up condition.
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

    fn latch_stopped_fetch(&mut self) {
        if !self.powered {
            self.bus.force_panel_lamps(0, 0);
            return;
        }
        let address = self.cpu.pc;
        let data = self.bus.peek_memory(address).unwrap_or(0);
        self.bus
            .panel_bus
            .observe(address, data, PanelCycle::InstructionFetch);
        self.bus.freeze_panel_bus();
    }

    pub fn reset(&mut self) {
        if !self.powered {
            return;
        }
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        self.running = false;
        self.wait_led = true;
        self.bus.clear_serial();
        self.latch_stopped_fetch();
    }

    /// Front-panel CLR is the I/O clear side of the RESET/CLR switch. It must
    /// not reset the 8080 or change PC; it clears attached emulated I/O state.
    pub fn clear_io(&mut self) {
        if !self.powered {
            return;
        }
        self.bus.clear_serial();
    }

    pub fn set_running(&mut self, run: bool) {
        if !self.powered {
            return;
        }
        self.running = run;
        self.wait_led = !run;
        if !run {
            self.bus.freeze_panel_bus();
        }
    }

    pub fn step(&mut self) {
        if !self.powered || self.running {
            return;
        }
        self.cpu.step(&mut self.bus);
        self.bus.freeze_panel_bus();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        if self.powered && self.running {
            self.cpu.run_cycles(&mut self.bus, cycles);
        }
    }

    pub fn commit_panel_activity(&mut self, dt: Duration) {
        let dynamic = self.powered && self.running && !self.cpu.halted;
        self.bus.commit_panel_activity(dt, dynamic);
    }

    pub fn examine(&mut self, next: bool) {
        if !self.powered || self.running {
            return;
        }
        let address = if next {
            self.bus.panel_address().wrapping_add(1)
        } else {
            self.bus.panel_switches()
        };
        self.cpu.pc = address;
        let _ = self.bus.read(address);
        self.bus.freeze_panel_bus();
    }

    pub fn deposit(&mut self, next: bool) {
        if !self.powered || self.running {
            return;
        }
        let address = if next {
            self.bus.panel_address().wrapping_add(1)
        } else {
            self.bus.panel_address()
        };
        let value = self.bus.panel_switches() as u8;
        self.bus.write(address, value);
        self.bus.freeze_panel_bus();
    }

    pub fn protect_current_board(&mut self, protected: bool) {
        if !self.powered || self.running {
            return;
        }
        self.bus.set_protected(self.bus.panel_address(), protected);
    }

    pub fn current_board_protected(&self) -> bool {
        self.powered && self.bus.is_protected(self.bus.panel_address())
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

    pub fn wait_led(&self) -> bool {
        self.wait_led
    }

    pub fn set_panel_lamps(&mut self, address: u16, data: u8) {
        // Utility/debug presentation override. Never overwrite live bus state
        // while software is running.
        if self.running {
            return;
        }
        self.bus.force_panel_lamps(address, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SerialBoard;

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
    fn protected_board_blocks_cpu_and_front_panel_writes() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.reset();
        machine.set_panel_lamps(0x0400, 0);
        machine.bus.load(0x0400, &[0x12]);
        machine.protect_current_board(true);

        machine.bus.write(0x0400, 0x34);
        assert_eq!(machine.bus.read(0x0400), 0x12);

        for bit in [1, 2, 4, 6] {
            machine.toggle_sense_switch(bit);
        }
        assert_eq!(machine.panel_switches(), 0x0056);
        machine.deposit(false);
        assert_eq!(machine.bus.read(0x0400), 0x12);

        machine.protect_current_board(false);
        machine.deposit(false);
        assert_eq!(machine.bus.read(0x0400), 0x56);
    }

    #[test]
    fn power_on_clears_protection() {
        let mut machine = AltairMachine::default();
        machine.bus.set_protected(0, true);
        machine.power(true);
        assert!(!machine.bus.is_protected(0));
    }

    #[test]
    fn reset_establishes_documented_pc_zero_fetch_state() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.bus.load(0, &[0xa5]);
        machine.reset();
        assert_eq!(machine.cpu.pc, 0);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0xa5);
        let lamps = machine.panel_lamps();
        assert_eq!(lamps.memr, 1.0);
        assert_eq!(lamps.m1, 1.0);
        assert_eq!(lamps.wo, 0.0);
        assert!(machine.wait_led());
    }

    #[test]
    fn clr_preserves_cpu_state_but_clears_serial() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.reset();
        machine.cpu.pc = 0x1234;
        machine.bus.serial_receive(b'X');
        assert_eq!(machine.bus.serial_rx_len(), 1);
        machine.clear_io();
        assert_eq!(machine.cpu.pc, 0x1234);
        assert_eq!(machine.bus.serial_rx_len(), 0);
    }

    #[test]
    fn configured_ram_limits_guest_visible_memory() {
        let mut machine = AltairMachine::default();
        machine.configure_memory(RamSize::Bytes256, RamInit::Zeroed);

        machine.bus.load(0x00ff, &[0xaa, 0xbb]);
        assert_eq!(machine.bus.read(0x00ff), 0xaa);
        assert_eq!(machine.bus.read(0x0100), 0x00);

        machine.bus.write(0x0100, 0x55);
        assert_eq!(machine.bus.read(0x0100), 0x00);
        assert_eq!(machine.installed_ram_bytes(), 256);
    }

    #[test]
    fn zeroed_power_on_mode_is_reapplied_on_power_off() {
        let mut machine = AltairMachine::default();
        machine.configure_memory(RamSize::K1, RamInit::Zeroed);
        machine.bus.write(0x0010, 0x5a);
        assert_eq!(machine.bus.read(0x0010), 0x5a);

        machine.power(false);
        assert_eq!(machine.bus.read(0x0010), 0x00);
    }

    #[test]
    fn basic32_64k_probe_guard_is_one_shot() {
        let mut machine = AltairMachine::default();
        machine.configure_memory(RamSize::K64, RamInit::Zeroed);

        machine.bus.write(0xffff, 0xa5);
        assert_eq!(machine.bus.read(0xffff), 0xa5);
        assert!(machine.arm_basic32_full_memory_probe_guard());

        machine.bus.write(0xffff, 0x37);
        assert_ne!(machine.bus.read(0xffff), 0x37);

        machine.bus.write(0xffff, 0x5a);
        assert_eq!(machine.bus.read(0xffff), 0x5a);
    }

    #[test]
    fn basic32_probe_guard_only_arms_for_full_64k() {
        let mut machine = AltairMachine::default();
        machine.configure_memory(RamSize::K48, RamInit::Zeroed);
        assert!(!machine.arm_basic32_full_memory_probe_guard());
    }

    #[test]
    fn sio_status_tracks_transmit_holding_register() {
        let mut bus = AltairBus::default();
        assert_eq!(bus.serial_board(), SerialBoard::Sio88);

        assert_eq!(bus.input(0x00) & 0xc0, 0x00);
        assert_eq!(bus.input(0x10), 0x00);

        bus.output(0x01, b'A');
        assert!(bus.tx_busy());
        assert_eq!(bus.input(0x00) & 0xc0, 0xc0);
        assert_eq!(bus.serial_tx_front(), Some(b'A'));

        bus.serial_tx_complete();
        assert!(!bus.tx_busy());
        assert_eq!(bus.input(0x00) & 0xc0, 0x00);
    }

    #[test]
    fn two_sio_status_tracks_transmit_holding_register() {
        let mut bus = AltairBus::default();
        bus.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(bus.input(0x10) & 0x02, 0x02);
        assert_eq!(bus.input(0x00), 0xff);

        bus.output(0x11, b'A');
        assert!(bus.tx_busy());
        assert_eq!(bus.input(0x10) & 0x02, 0x00);
        assert_eq!(bus.serial_tx_front(), Some(b'A'));

        bus.serial_tx_complete();
        assert!(!bus.tx_busy());
        assert_eq!(bus.input(0x10) & 0x02, 0x02);
    }

    #[test]
    fn receive_status_matches_selected_serial_board() {
        let mut bus = AltairBus::default();
        assert_eq!(bus.input(0x00) & 0x01, 0x01);

        bus.serial_receive(b'K');
        assert_eq!(bus.input(0x00) & 0x01, 0x00);
        assert_eq!(bus.input(0x01), b'K');

        bus.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(bus.input(0x10) & 0x01, 0x00);

        bus.serial_receive(b'2');
        assert_eq!(bus.input(0x10) & 0x01, 0x01);
        assert_eq!(bus.input(0x11), b'2');
    }

    #[test]
    fn front_panel_sense_port_uses_real_io_bus_addressing() {
        let mut machine = AltairMachine::default();
        machine.toggle_sense_switch(15);
        assert_eq!(machine.panel_switches(), 0x8000);
        assert_eq!(machine.bus.input(0xff), 0x80);
        assert_eq!(machine.address_leds(), 0xffff);

        machine.bus.output(0xff, 0xa5);
        assert_eq!(machine.address_leds(), 0xffff);
        assert_eq!(machine.data_leds(), 0xa5);
    }

    #[test]
    fn io_port_number_is_duplicated_on_the_8080_address_bus() {
        let mut bus = AltairBus::default();
        bus.output(0x11, 0x5a);
        assert_eq!(bus.panel_address(), 0x1111);
        assert_eq!(bus.panel_data(), 0x5a);
    }

    #[test]
    fn front_panel_controls_do_not_modify_running_machine() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.reset();
        machine.set_running(true);
        let pc = machine.cpu.pc;
        machine.step();
        machine.examine(false);
        machine.deposit(false);
        machine.protect_current_board(true);
        assert_eq!(machine.cpu.pc, pc);
        assert!(!machine.current_board_protected());
    }
}