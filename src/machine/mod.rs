mod memory;

use std::collections::VecDeque;

use crate::cpu8080::{Bus, Cpu8080};
use memory::Memory;

pub use memory::{MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};

pub const CLOCK_HZ: u32 = 2_000_000;

pub struct AltairBus {
    memory: Memory,
    pub panel_switches: u16,
    pub data_leds: u8,
    pub serial_rx: VecDeque<u8>,
    /// One-byte transmit holding register. The ASR-33 side deliberately leaves
    /// the byte here until the mechanical print interval has elapsed. That is
    /// how the status ports expose BUSY/READY to software running on the 8080.
    pub serial_tx: VecDeque<u8>,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut s = Self {
            memory: Memory::default(),
            panel_switches: 0,
            data_leds: 0,
            serial_rx: VecDeque::new(),
            serial_tx: VecDeque::new(),
        };
        s.randomize();
        s
    }
}

impl AltairBus {
    pub fn randomize(&mut self) {
        self.memory.randomize();
    }

    /// Programmatic image loading intentionally bypasses the front-panel write
    /// protection latch. POWER ON clears all protection first, matching the
    /// hardware-reset behaviour expected by the emulator.
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

    /// Queue one byte as data arriving from the currently attached terminal.
    /// Endpoint code uses this instead of reaching into the emulated UART queue.
    pub fn serial_receive(&mut self, byte: u8) {
        self.serial_rx.push_back(byte);
    }

    pub fn serial_rx_empty(&self) -> bool {
        self.serial_rx.is_empty()
    }

    pub fn serial_rx_len(&self) -> usize {
        self.serial_rx.len()
    }

    /// Byte currently held for transmission by the emulated serial interface.
    /// The endpoint deliberately leaves it pending until its own timing says the
    /// character has completed, preserving the guest-visible BUSY/READY state.
    pub fn serial_tx_front(&self) -> Option<u8> {
        self.serial_tx.front().copied()
    }

    pub fn serial_tx_complete(&mut self) -> Option<u8> {
        self.serial_tx.pop_front()
    }

    pub fn tx_busy(&self) -> bool {
        !self.serial_tx.is_empty()
    }

    pub fn clear_serial(&mut self) {
        self.serial_rx.clear();
        self.serial_tx.clear();
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
            0xff => (self.panel_switches >> 8) as u8,

            // MITS 88-SIO status convention used by the S2JS reference.
            // Bit 0 is set when the receive buffer is empty, while bits 6/7
            // are set while the transmit holding register is occupied.
            0x00 => {
                let rx_empty = self.serial_rx_empty();
                let tx_busy = self.tx_busy();
                (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
            }
            0x01 => self.serial_rx.pop_front().unwrap_or(0),

            // MITS 2SIO / 8251 convention: bit 0 = RX ready, bit 1 = TX ready.
            0x10 => {
                (if self.serial_rx_empty() { 0 } else { 0x01 })
                    | (if self.tx_busy() { 0 } else { 0x02 })
            }
            0x11 => self.serial_rx.pop_front().unwrap_or(0),
            _ => 0,
        }
    }

    fn output(&mut self, port: u8, value: u8) {
        match port {
            0xff => self.data_leds = value,
            0x01 | 0x11 => {
                // The browser reference effectively has one text-box-sized TX
                // holding register. Correct software polls READY before writing;
                // if it does not, a new write replaces the old pending byte.
                self.serial_tx.clear();
                self.serial_tx.push_back(value);
            }
            _ => {}
        }
    }
}

pub struct AltairMachine {
    pub cpu: Cpu8080,
    pub bus: AltairBus,
    pub powered: bool,
    pub running: bool,
    pub address_leds: u16,
    pub wait_led: bool,
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
    pub fn power(&mut self, on: bool) {
        self.powered = on;
        self.running = false;
        if on {
            self.bus.clear_protection();
            self.reset();
        } else {
            self.wait_led = false;
            self.address_leds = 0;
            self.bus.data_leds = 0;
            self.bus.clear_serial();
            self.bus.randomize();
        }
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.running = false;
        self.wait_led = true;
        self.address_leds = 0;
        self.bus.data_leds = 0;
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
            self.bus.panel_switches
        };
        self.address_leds = address;
        self.bus.data_leds = self.bus.read(address);
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
        let value = self.bus.panel_switches as u8;
        self.bus.write(address, value);
        self.bus.data_leds = self.bus.read(address);
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
    fn protected_board_blocks_cpu_and_front_panel_writes() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.address_leds = 0x0400;
        machine.bus.load(0x0400, &[0x12]);
        machine.protect_current_board(true);

        machine.bus.write(0x0400, 0x34);
        assert_eq!(machine.bus.read(0x0400), 0x12);

        machine.bus.panel_switches = 0x0056;
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
    fn sio_status_tracks_transmit_holding_register() {
        let mut bus = AltairBus::default();

        // Empty TX register: 2SIO says transmitter ready; SIO busy bits clear.
        assert_eq!(bus.input(0x10) & 0x02, 0x02);
        assert_eq!(bus.input(0x00) & 0xc0, 0x00);

        bus.output(0x01, b'A');
        assert!(bus.tx_busy());
        assert_eq!(bus.input(0x10) & 0x02, 0x00);
        assert_eq!(bus.input(0x00) & 0xc0, 0xc0);
        assert_eq!(bus.serial_tx_front(), Some(b'A'));

        bus.serial_tx_complete();
        assert!(!bus.tx_busy());
        assert_eq!(bus.input(0x10) & 0x02, 0x02);
        assert_eq!(bus.input(0x00) & 0xc0, 0x00);
    }

    #[test]
    fn receive_status_matches_reference_ports() {
        let mut bus = AltairBus::default();
        assert_eq!(bus.input(0x00) & 0x01, 0x01); // SIO: 1 means RX empty
        assert_eq!(bus.input(0x10) & 0x01, 0x00); // 2SIO: 0 means no RX data

        bus.serial_receive(b'K');
        assert_eq!(bus.input(0x00) & 0x01, 0x00);
        assert_eq!(bus.input(0x10) & 0x01, 0x01);
        assert_eq!(bus.input(0x01), b'K');
    }
}
