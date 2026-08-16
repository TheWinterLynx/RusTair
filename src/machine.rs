use std::collections::VecDeque;
use rand::RngCore;
use crate::cpu8080::{Bus, Cpu8080};

pub const MEM_SIZE: usize = 8 * 1024;
pub const CLOCK_HZ: u32 = 2_000_000;

pub struct AltairBus {
    pub memory: [u8; MEM_SIZE],
    pub panel_switches: u16,
    pub data_leds: u8,
    pub serial_rx: VecDeque<u8>,
    pub serial_tx: VecDeque<u8>,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut s = Self {
            memory: [0; MEM_SIZE], panel_switches: 0, data_leds: 0,
            serial_rx: VecDeque::new(), serial_tx: VecDeque::new(),
        };
        s.randomize();
        s
    }
}

impl AltairBus {
    pub fn randomize(&mut self) { rand::rng().fill_bytes(&mut self.memory); }
    pub fn load(&mut self, address: u16, bytes: &[u8]) {
        let start = address as usize;
        if start >= MEM_SIZE { return; }
        let len = bytes.len().min(MEM_SIZE - start);
        self.memory[start..start + len].copy_from_slice(&bytes[..len]);
    }
}

impl Bus for AltairBus {
    fn read(&mut self, address: u16) -> u8 {
        self.memory.get(address as usize).copied().unwrap_or(0)
    }
    fn write(&mut self, address: u16, value: u8) {
        if let Some(b) = self.memory.get_mut(address as usize) { *b = value; }
    }
    fn input(&mut self, port: u8) -> u8 {
        match port {
            0xff => (self.panel_switches >> 8) as u8,
            0x00 => {
                let rx_empty = self.serial_rx.is_empty();
                (if rx_empty { 0x01 } else { 0 }) | 0xc0
            }
            0x01 => self.serial_rx.pop_front().unwrap_or(0),
            0x10 => (if self.serial_rx.is_empty() { 0 } else { 0x01 }) | 0x02,
            0x11 => self.serial_rx.pop_front().unwrap_or(0),
            _ => 0,
        }
    }
    fn output(&mut self, port: u8, value: u8) {
        match port {
            0xff => self.data_leds = value,
            0x01 | 0x11 => self.serial_tx.push_back(value),
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
        Self { cpu: Cpu8080::new(), bus: AltairBus::default(), powered: false,
               running: false, address_leds: 0, wait_led: false }
    }
}

impl AltairMachine {
    pub fn power(&mut self, on: bool) {
        self.powered = on;
        self.running = false;
        if on { self.reset(); }
        else { self.wait_led = false; self.address_leds = 0; self.bus.data_leds = 0; self.bus.randomize(); }
    }
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.running = false;
        self.wait_led = true;
        self.address_leds = 0;
        self.bus.data_leds = 0;
    }
    pub fn set_running(&mut self, run: bool) {
        if !self.powered { return; }
        self.running = run;
        self.wait_led = !run;
    }
    pub fn step(&mut self) {
        if !self.powered { return; }
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
        if !self.powered { return; }
        let a = if next { self.address_leds.wrapping_add(1) } else { self.bus.panel_switches };
        self.address_leds = a;
        self.bus.data_leds = self.bus.read(a);
        self.cpu.pc = a;
    }
    pub fn deposit(&mut self, next: bool) {
        if !self.powered { return; }
        let a = if next { self.address_leds.wrapping_add(1) } else { self.address_leds };
        self.address_leds = a;
        let v = self.bus.panel_switches as u8;
        self.bus.write(a, v);
        self.bus.data_leds = self.bus.read(a);
    }
}
