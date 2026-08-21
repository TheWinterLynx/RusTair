mod front_panel;
mod io_devices;
mod memory;
mod serial;

use crate::config::{RamInit, RamSize};
use crate::cpu8080::{Bus, Cpu8080};
use front_panel::FrontPanelPort;
use io_devices::IoDevices;
use memory::Memory;

pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};

pub const CLOCK_HZ: u32 = 2_000_000;

pub struct AltairBus {
    memory: Memory,
    io: IoDevices,
    panel: FrontPanelPort,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut s = Self {
            memory: Memory::default(),
            io: IoDevices::default(),
            panel: FrontPanelPort::default(),
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

    fn data_leds(&self) -> u8 {
        self.panel.data_leds()
    }

    fn set_data_leds(&mut self, value: u8) {
        self.panel.set_data_leds(value);
    }
}

impl Bus for AltairBus {
    fn read(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }

    fn write(&mut self, address: u16, value: u8) {
        self.memory.write(address, value);
    }

    fn input(&mut self, port: u8) -> u8 {
        match port {
            0xff => self.panel.input(),
            _ => self.io.input(port),
        }
    }

    fn output(&mut self, port: u8, value: u8) {
        match port {
            0xff => self.panel.output(value),
            _ => self.io.output(port, value),
        }
    }
}

pub struct AltairMachine {
    pub cpu: Cpu8080,
    pub bus: AltairBus,
    pub powered: bool,
    pub running: bool,
    address_leds: u16,
    wait_led: bool,
}

impl Default for AltairMachine {
    fn default() -> Self {
        Self {
            cpu: Cpu8080::new(),
            bus: AltairBus::default(),
            powered: false,
            running: false,
            address_leds: 0,
            wait_led: false,
        }
    }
}

impl AltairMachine {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.running = false;
        self.bus.configure_memory(size, init_mode);
        self.cpu.reset();
        self.address_leds = 0;
        self.bus.set_data_leds(0);
        self.bus.clear_serial();
        self.wait_led = self.powered;
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.bus.installed_ram_bytes()
    }

    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.bus.arm_basic32_full_memory_probe_guard()
    }

    pub fn power(&mut self, on: bool) {
        self.powered = on;
        self.running = false;
        if on {
            self.bus.clear_protection();
            self.reset();
        } else {
            self.wait_led = false;
            self.address_leds = 0;
            self.bus.set_data_leds(0);
            self.bus.clear_serial();
            self.bus.initialize_memory();
        }
    }

    pub fn reset(&mut self) {
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        self.running = false;
        self.wait_led = true;
        self.address_leds = 0;
        self.bus.set_data_leds(0);
        self.bus.clear_serial();
    }

    pub fn set_running(&mut self, run: bool) {
        if !self.powered {
            return;
        }
        self.running = run;
        self.wait_led = !run;
    }

    pub fn step(&mut self) {
        if !self.powered {
            return;
        }
        self.cpu.step(&mut self.bus);
        self.address_leds = self.cpu.pc;
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        if self.powered && self.running {
            self.cpu.run_cycles(&mut self.bus, cycles);
            self.address_leds = self.cpu.pc;
        }
    }

    pub fn examine(&mut self, next: bool) {
        if !self.powered {
            return;
        }
        let address = if next {
            self.address_leds.wrapping_add(1)
        } else {
            self.bus.panel_switches()
        };
        self.address_leds = address;
        let value = self.bus.read(address);
        self.bus.set_data_leds(value);
        self.cpu.pc = address;
    }

    pub fn deposit(&mut self, next: bool) {
        if !self.powered {
            return;
        }
        let address = if next {
            self.address_leds.wrapping_add(1)
        } else {
            self.address_leds
        };
        self.address_leds = address;
        let value = self.bus.panel_switches() as u8;
        self.bus.write(address, value);
        let displayed = self.bus.read(address);
        self.bus.set_data_leds(displayed);
    }

    pub fn protect_current_board(&mut self, protected: bool) {
        if !self.powered {
            return;
        }
        self.bus.set_protected(self.address_leds, protected);
    }

    pub fn current_board_protected(&self) -> bool {
        self.powered && self.bus.is_protected(self.address_leds)
    }

    pub fn panel_switches(&self) -> u16 {
        self.bus.panel_switches()
    }

    pub fn toggle_sense_switch(&mut self, bit: usize) {
        self.bus.toggle_panel_switch(bit);
    }

    pub fn address_leds(&self) -> u16 {
        self.address_leds
    }

    pub fn data_leds(&self) -> u8 {
        self.bus.data_leds()
    }

    pub fn wait_led(&self) -> bool {
        self.wait_led
    }

    pub fn set_panel_lamps(&mut self, address: u16, data: u8) {
        self.address_leds = address;
        self.bus.set_data_leds(data);
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
    fn front_panel_port_is_encapsulated_by_machine_api() {
        let mut machine = AltairMachine::default();
        machine.toggle_sense_switch(15);
        assert_eq!(machine.panel_switches(), 0x8000);
        assert_eq!(machine.bus.input(0xff), 0x80);

        machine.bus.output(0xff, 0xa5);
        assert_eq!(machine.data_leds(), 0xa5);
    }
}
