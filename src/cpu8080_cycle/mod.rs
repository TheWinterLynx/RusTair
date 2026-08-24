//! Experimental T-state-accurate Intel 8080 core.
//!
//! The existing `crate::cpu8080` implementation remains RusTair's validated
//! production CPU. This module is built alongside it for differential testing.
//!
//! Machine-cycle structure and signal timing are independently implemented in
//! Rust from Intel documentation and informed by Jim Drygiannakis' MIT-licensed
//! `jdryg/8080Emu` edge-level model. See `licenses/8080Emu-MIT.txt`.

mod decode;
mod pins;
mod state;
mod timing;

pub use pins::{Cpu8080Inputs, Cpu8080Pins};
pub use state::Registers;
pub use timing::{MachineCycle, TState};

use decode::{decode, Instruction, Register8, RegisterPair};

const FLAG_C: u8 = 0x01;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cpu8080CycleFault {
    UnsupportedOpcode(u8),
}

/// Snapshot of the T-state that has just been executed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickTrace {
    pub machine_cycle: MachineCycle,
    pub machine_cycle_index: u8,
    pub t_state: TState,
    pub pins: Cpu8080Pins,
    pub opcode: Option<u8>,
    pub instruction_complete: bool,
    pub reset: bool,
    pub fault: Option<Cpu8080CycleFault>,
    pub total_t_states: u64,
    pub instruction_t_states: u32,
}

pub struct Cpu8080Cycle {
    registers: Registers,
    pins: Cpu8080Pins,
    machine_cycle: MachineCycle,
    machine_cycle_index: u8,
    t_state: TState,
    cycle_address: u16,
    cycle_data_out: Option<u8>,
    opcode: Option<u8>,
    instruction: Instruction,
    operand_low: u8,
    effective_address: u16,
    inte: bool,
    total_t_states: u64,
    completed_instructions: u64,
    current_instruction_t_states: u32,
    last_instruction_t_states: Option<u32>,
    reset_asserted: bool,
    fault: Option<Cpu8080CycleFault>,
}

impl Default for Cpu8080Cycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu8080Cycle {
    pub fn new() -> Self {
        Self {
            registers: Registers::default(),
            pins: Cpu8080Pins::default(),
            machine_cycle: MachineCycle::InstructionFetch,
            machine_cycle_index: 1,
            t_state: TState::T1,
            cycle_address: 0,
            cycle_data_out: None,
            opcode: None,
            instruction: Instruction::Nop,
            operand_low: 0,
            effective_address: 0,
            inte: false,
            total_t_states: 0,
            completed_instructions: 0,
            current_instruction_t_states: 0,
            last_instruction_t_states: None,
            reset_asserted: false,
            fault: None,
        }
    }

    pub const fn registers(&self) -> Registers { self.registers }

    /// Seed programmer-visible state for isolated/differential tests.
    /// Call only at an instruction boundary.
    pub fn set_registers(&mut self, registers: Registers) {
        self.registers = registers;
        if self.machine_cycle == MachineCycle::InstructionFetch && self.t_state == TState::T1 {
            self.cycle_address = registers.pc;
        }
    }

    pub const fn pins(&self) -> Cpu8080Pins { self.pins }
    pub const fn machine_cycle(&self) -> MachineCycle { self.machine_cycle }
    pub const fn machine_cycle_index(&self) -> u8 { self.machine_cycle_index }
    pub const fn t_state(&self) -> TState { self.t_state }
    pub const fn total_t_states(&self) -> u64 { self.total_t_states }
    pub const fn completed_instructions(&self) -> u64 { self.completed_instructions }
    pub const fn last_instruction_t_states(&self) -> Option<u32> { self.last_instruction_t_states }
    pub const fn interrupts_enabled(&self) -> bool { self.inte }
    pub const fn fault(&self) -> Option<Cpu8080CycleFault> { self.fault }

    /// Advance exactly one Intel 8080 T-state.
    ///
    /// This milestone adds 16-bit register-pair transfers and addressing while
    /// preserving the previously validated fetch/MOV/MVI/STA memory engine.
    pub fn tick(&mut self, inputs: Cpu8080Inputs) -> TickTrace {
        if inputs.reset {
            self.apply_reset();
            return TickTrace {
                machine_cycle: self.machine_cycle,
                machine_cycle_index: self.machine_cycle_index,
                t_state: self.t_state,
                pins: self.pins,
                opcode: None,
                instruction_complete: false,
                reset: true,
                fault: None,
                total_t_states: self.total_t_states,
                instruction_t_states: 0,
            };
        }

        if self.reset_asserted {
            self.reset_asserted = false;
        }

        if let Some(fault) = self.fault {
            return TickTrace {
                machine_cycle: self.machine_cycle,
                machine_cycle_index: self.machine_cycle_index,
                t_state: self.t_state,
                pins: self.pins,
                opcode: self.opcode,
                instruction_complete: false,
                reset: false,
                fault: Some(fault),
                total_t_states: self.total_t_states,
                instruction_t_states: self.current_instruction_t_states,
            };
        }

        let machine_cycle = self.machine_cycle;
        let machine_cycle_index = self.machine_cycle_index;
        let t_state = self.t_state;

        if machine_cycle == MachineCycle::InstructionFetch && t_state == TState::T1 {
            self.opcode = None;
            self.instruction = Instruction::Nop;
            self.operand_low = 0;
            self.effective_address = 0;
            self.cycle_address = self.registers.pc;
            self.cycle_data_out = None;
        }

        self.drive_pins_for_t_state(t_state);
        self.total_t_states = self.total_t_states.saturating_add(1);
        self.current_instruction_t_states = self.current_instruction_t_states.saturating_add(1);
        let instruction_t_states = self.current_instruction_t_states;

        let mut instruction_complete = false;
        let mut fault = None;

        match t_state {
            TState::T1 => self.t_state = TState::T2,
            TState::T2 | TState::Tw => {
                self.t_state = if self.cycle_uses_ready() && !inputs.ready {
                    TState::Tw
                } else {
                    TState::T3
                };
            }
            TState::T3 => match machine_cycle {
                MachineCycle::InstructionFetch => {
                    self.opcode = Some(inputs.data_in);
                    self.instruction = decode(inputs.data_in);
                    self.registers.pc = self.registers.pc.wrapping_add(1);
                    self.t_state = TState::T4;
                }
                MachineCycle::MemoryRead => {
                    instruction_complete = self.finish_memory_read(inputs.data_in);
                }
                MachineCycle::MemoryWrite => {
                    instruction_complete = self.finish_memory_write();
                }
                MachineCycle::Internal => {
                    instruction_complete = self.finish_internal_cycle();
                }
                _ => unreachable!("unsupported machine cycle {:?}", machine_cycle),
            },
            TState::T4 => match self.instruction {
                Instruction::Nop => {
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::MviImmediate(_)
                | Instruction::MviMemory
                | Instruction::Lxi(_)
                | Instruction::LdaDirect
                | Instruction::StaDirect
                | Instruction::LhldDirect
                | Instruction::ShldDirect => {
                    self.begin_memory_read(self.registers.pc, 2);
                }
                Instruction::MovRegister { .. } | Instruction::Inx(_) | Instruction::Dcx(_) => {
                    self.t_state = TState::T5;
                }
                Instruction::MovFromMemory { .. } => {
                    self.begin_memory_read(self.hl(), 2);
                }
                Instruction::MovToMemory { src } => {
                    self.begin_memory_write(self.hl(), self.read_register(src), 2);
                }
                Instruction::Ldax(pair) => {
                    self.begin_memory_read(self.read_pair(pair), 2);
                }
                Instruction::Stax(pair) => {
                    self.begin_memory_write(self.read_pair(pair), self.registers.a, 2);
                }
                Instruction::Dad(_) => {
                    // jdryg's edge-level model likewise uses two 3T Internal
                    // machine cycles after M1 for DAD: 4 + 3 + 3 = 10T.
                    self.begin_internal_cycle(2);
                }
                Instruction::Unsupported(opcode) => {
                    let unsupported = Cpu8080CycleFault::UnsupportedOpcode(opcode);
                    self.fault = Some(unsupported);
                    fault = Some(unsupported);
                }
            },
            TState::T5 => match self.instruction {
                Instruction::MovRegister { dst, src } => {
                    let value = self.read_register(src);
                    self.write_register(dst, value);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Inx(pair) => {
                    let value = self.read_pair(pair).wrapping_add(1);
                    self.write_pair(pair, value);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Dcx(pair) => {
                    let value = self.read_pair(pair).wrapping_sub(1);
                    self.write_pair(pair, value);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                _ => unreachable!("unexpected T5 for {:?}", self.instruction),
            },
        }

        TickTrace {
            machine_cycle,
            machine_cycle_index,
            t_state,
            pins: self.pins,
            opcode: self.opcode,
            instruction_complete,
            reset: false,
            fault,
            total_t_states: self.total_t_states,
            instruction_t_states,
        }
    }

    fn finish_memory_read(&mut self, data: u8) -> bool {
        match (self.instruction, self.machine_cycle_index) {
            (Instruction::MviImmediate(dst), 2) => {
                self.write_register(dst, data);
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.complete_instruction();
                true
            }
            (Instruction::MviMemory, 2) => {
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.begin_memory_write(self.hl(), data, 3);
                false
            }
            (Instruction::MovFromMemory { dst }, 2) => {
                self.write_register(dst, data);
                self.complete_instruction();
                true
            }
            (Instruction::Lxi(pair), 2) => {
                self.operand_low = data;
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.begin_memory_read(self.registers.pc, 3);
                false
            }
            (Instruction::Lxi(pair), 3) => {
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.write_pair(pair, u16::from_le_bytes([self.operand_low, data]));
                self.complete_instruction();
                true
            }
            (Instruction::Ldax(_), 2) => {
                self.registers.a = data;
                self.complete_instruction();
                true
            }
            (Instruction::StaDirect, 2)
            | (Instruction::LdaDirect, 2)
            | (Instruction::LhldDirect, 2)
            | (Instruction::ShldDirect, 2) => {
                self.operand_low = data;
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.begin_memory_read(self.registers.pc, 3);
                false
            }
            (Instruction::StaDirect, 3) => {
                self.finish_direct_address(data);
                self.begin_memory_write(self.effective_address, self.registers.a, 4);
                false
            }
            (Instruction::LdaDirect, 3) => {
                self.finish_direct_address(data);
                self.begin_memory_read(self.effective_address, 4);
                false
            }
            (Instruction::LdaDirect, 4) => {
                self.registers.a = data;
                self.complete_instruction();
                true
            }
            (Instruction::LhldDirect, 3) => {
                self.finish_direct_address(data);
                self.begin_memory_read(self.effective_address, 4);
                false
            }
            (Instruction::LhldDirect, 4) => {
                self.registers.l = data;
                self.begin_memory_read(self.effective_address.wrapping_add(1), 5);
                false
            }
            (Instruction::LhldDirect, 5) => {
                self.registers.h = data;
                self.complete_instruction();
                true
            }
            (Instruction::ShldDirect, 3) => {
                self.finish_direct_address(data);
                self.begin_memory_write(self.effective_address, self.registers.l, 4);
                false
            }
            _ => unreachable!(
                "invalid memory-read cycle M{} for {:?}",
                self.machine_cycle_index, self.instruction
            ),
        }
    }

    fn finish_memory_write(&mut self) -> bool {
        match (self.instruction, self.machine_cycle_index) {
            (Instruction::MviMemory, 3)
            | (Instruction::MovToMemory { .. }, 2)
            | (Instruction::Stax(_), 2)
            | (Instruction::StaDirect, 4) => {
                self.complete_instruction();
                true
            }
            (Instruction::ShldDirect, 4) => {
                self.begin_memory_write(
                    self.effective_address.wrapping_add(1),
                    self.registers.h,
                    5,
                );
                false
            }
            (Instruction::ShldDirect, 5) => {
                self.complete_instruction();
                true
            }
            _ => unreachable!(
                "invalid memory-write cycle M{} for {:?}",
                self.machine_cycle_index, self.instruction
            ),
        }
    }

    fn finish_internal_cycle(&mut self) -> bool {
        match (self.instruction, self.machine_cycle_index) {
            (Instruction::Dad(_), 2) => {
                self.begin_internal_cycle(3);
                false
            }
            (Instruction::Dad(pair), 3) => {
                let lhs = self.hl() as u32;
                let rhs = self.read_pair(pair) as u32;
                let sum = lhs + rhs;
                self.write_pair(RegisterPair::HL, sum as u16);
                self.registers.f = (self.registers.f & !FLAG_C)
                    | if sum > 0xffff { FLAG_C } else { 0 };
                self.complete_instruction();
                true
            }
            _ => unreachable!(
                "invalid internal cycle M{} for {:?}",
                self.machine_cycle_index, self.instruction
            ),
        }
    }

    fn finish_direct_address(&mut self, high: u8) {
        self.registers.pc = self.registers.pc.wrapping_add(1);
        self.effective_address = u16::from_le_bytes([self.operand_low, high]);
    }

    fn hl(&self) -> u16 {
        u16::from_be_bytes([self.registers.h, self.registers.l])
    }

    fn read_pair(&self, pair: RegisterPair) -> u16 {
        match pair {
            RegisterPair::BC => u16::from_be_bytes([self.registers.b, self.registers.c]),
            RegisterPair::DE => u16::from_be_bytes([self.registers.d, self.registers.e]),
            RegisterPair::HL => self.hl(),
            RegisterPair::SP => self.registers.sp,
        }
    }

    fn write_pair(&mut self, pair: RegisterPair, value: u16) {
        let [high, low] = value.to_be_bytes();
        match pair {
            RegisterPair::BC => { self.registers.b = high; self.registers.c = low; }
            RegisterPair::DE => { self.registers.d = high; self.registers.e = low; }
            RegisterPair::HL => { self.registers.h = high; self.registers.l = low; }
            RegisterPair::SP => self.registers.sp = value,
        }
    }

    fn read_register(&self, register: Register8) -> u8 {
        match register {
            Register8::B => self.registers.b,
            Register8::C => self.registers.c,
            Register8::D => self.registers.d,
            Register8::E => self.registers.e,
            Register8::H => self.registers.h,
            Register8::L => self.registers.l,
            Register8::A => self.registers.a,
        }
    }

    fn write_register(&mut self, register: Register8, value: u8) {
        match register {
            Register8::B => self.registers.b = value,
            Register8::C => self.registers.c = value,
            Register8::D => self.registers.d = value,
            Register8::E => self.registers.e = value,
            Register8::H => self.registers.h = value,
            Register8::L => self.registers.l = value,
            Register8::A => self.registers.a = value,
        }
    }

    fn cycle_uses_ready(&self) -> bool {
        self.machine_cycle != MachineCycle::Internal
    }

    fn begin_memory_read(&mut self, address: u16, machine_cycle_index: u8) {
        self.machine_cycle = MachineCycle::MemoryRead;
        self.machine_cycle_index = machine_cycle_index;
        self.t_state = TState::T1;
        self.cycle_address = address;
        self.cycle_data_out = None;
    }

    fn begin_memory_write(&mut self, address: u16, data: u8, machine_cycle_index: u8) {
        self.machine_cycle = MachineCycle::MemoryWrite;
        self.machine_cycle_index = machine_cycle_index;
        self.t_state = TState::T1;
        self.cycle_address = address;
        self.cycle_data_out = Some(data);
    }

    fn begin_internal_cycle(&mut self, machine_cycle_index: u8) {
        self.machine_cycle = MachineCycle::Internal;
        self.machine_cycle_index = machine_cycle_index;
        self.t_state = TState::T1;
        self.cycle_data_out = None;
    }

    fn begin_instruction_fetch(&mut self) {
        self.machine_cycle = MachineCycle::InstructionFetch;
        self.machine_cycle_index = 1;
        self.t_state = TState::T1;
        self.cycle_address = self.registers.pc;
        self.cycle_data_out = None;
    }

    fn complete_instruction(&mut self) {
        self.completed_instructions = self.completed_instructions.saturating_add(1);
        self.last_instruction_t_states = Some(self.current_instruction_t_states);
        self.current_instruction_t_states = 0;
        self.begin_instruction_fetch();
    }

    fn apply_reset(&mut self) {
        self.registers.pc = 0;
        self.inte = false;
        self.pins = Cpu8080Pins::default();
        self.machine_cycle = MachineCycle::InstructionFetch;
        self.machine_cycle_index = 1;
        self.t_state = TState::T1;
        self.cycle_address = 0;
        self.cycle_data_out = None;
        self.opcode = None;
        self.instruction = Instruction::Nop;
        self.operand_low = 0;
        self.effective_address = 0;
        self.current_instruction_t_states = 0;
        self.last_instruction_t_states = None;
        self.reset_asserted = true;
        self.fault = None;
    }

    fn drive_pins_for_t_state(&mut self, t_state: TState) {
        self.pins.inte = self.inte;
        self.pins.hlda = false;
        // Internal DAD cycles have no externally valid bus transaction. Their
        // exact sub-T-state address behaviour will be pinned down during the
        // later Phi1/Phi2 trace-validation phase.
        self.pins.address = if self.machine_cycle == MachineCycle::Internal {
            None
        } else {
            Some(self.cycle_address)
        };

        let input_cycle = matches!(
            self.machine_cycle,
            MachineCycle::InstructionFetch
                | MachineCycle::MemoryRead
                | MachineCycle::StackRead
                | MachineCycle::InputRead
                | MachineCycle::InterruptAck
                | MachineCycle::InterruptAckWhileHalt
        );
        let output_cycle = matches!(
            self.machine_cycle,
            MachineCycle::MemoryWrite | MachineCycle::StackWrite | MachineCycle::OutputWrite
        );

        match t_state {
            TState::T1 => {
                self.pins.data_out = self.machine_cycle.status_word();
                self.pins.sync = self.machine_cycle.status_word().is_some();
                self.pins.dbin = false;
                self.pins.wr_n = true;
                self.pins.wait = false;
            }
            TState::T2 => {
                self.pins.data_out = if output_cycle { self.cycle_data_out } else { None };
                self.pins.sync = false;
                self.pins.dbin = input_cycle;
                self.pins.wr_n = true;
                self.pins.wait = false;
            }
            TState::Tw => {
                self.pins.data_out = if output_cycle { self.cycle_data_out } else { None };
                self.pins.sync = false;
                self.pins.dbin = input_cycle;
                self.pins.wr_n = !output_cycle;
                self.pins.wait = true;
            }
            TState::T3 => {
                self.pins.data_out = if output_cycle { self.cycle_data_out } else { None };
                self.pins.sync = false;
                self.pins.dbin = false;
                self.pins.wr_n = !output_cycle;
                self.pins.wait = false;
            }
            TState::T4 | TState::T5 => {
                self.pins.data_out = None;
                self.pins.sync = false;
                self.pins.dbin = false;
                self.pins.wr_n = true;
                self.pins.wait = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(data_in: u8, ready: bool) -> Cpu8080Inputs {
        Cpu8080Inputs { data_in, ready, ..Cpu8080Inputs::default() }
    }

    fn fetch(cpu: &mut Cpu8080Cycle, opcode: u8) -> [TickTrace; 4] {
        [
            cpu.tick(input(0, true)),
            cpu.tick(input(0, true)),
            cpu.tick(input(opcode, true)),
            cpu.tick(input(0, true)),
        ]
    }

    fn read_cycle(cpu: &mut Cpu8080Cycle, data: u8) -> [TickTrace; 3] {
        [
            cpu.tick(input(0, true)),
            cpu.tick(input(0, true)),
            cpu.tick(input(data, true)),
        ]
    }

    fn write_cycle(cpu: &mut Cpu8080Cycle) -> [TickTrace; 3] {
        [
            cpu.tick(input(0, true)),
            cpu.tick(input(0, true)),
            cpu.tick(input(0, true)),
        ]
    }

    #[test]
    fn nop_remains_four_t_states() {
        let mut cpu = Cpu8080Cycle::new();
        let trace = fetch(&mut cpu, 0x00);
        assert!(trace[3].instruction_complete);
        assert_eq!(trace[3].instruction_t_states, 4);
        assert_eq!(cpu.registers().pc, 1);
    }

    #[test]
    fn lxi_all_pairs_are_ten_t_states_little_endian_and_preserve_flags() {
        let cases = [(0x01, RegisterPair::BC), (0x11, RegisterPair::DE), (0x21, RegisterPair::HL), (0x31, RegisterPair::SP)];
        for (opcode, pair) in cases {
            let mut cpu = Cpu8080Cycle::new();
            let mut r = Registers::default();
            r.pc = 0x2000;
            r.f = 0xd7;
            cpu.set_registers(r);
            fetch(&mut cpu, opcode);
            let lo = read_cycle(&mut cpu, 0x34);
            assert_eq!(lo[0].pins.address, Some(0x2001));
            assert!(!lo[2].instruction_complete);
            let hi = read_cycle(&mut cpu, 0x12);
            assert_eq!(hi[0].pins.address, Some(0x2002));
            assert!(hi[2].instruction_complete);
            assert_eq!(hi[2].instruction_t_states, 10);
            assert_eq!(cpu.read_pair(pair), 0x1234);
            assert_eq!(cpu.registers().pc, 0x2003);
            assert_eq!(cpu.registers().f, 0xd7);
        }
    }

    #[test]
    fn inx_and_dcx_are_five_t_states_and_wrap_without_touching_flags() {
        for (opcode, pair, start, expected) in [
            (0x03, RegisterPair::BC, 0xffff, 0x0000),
            (0x13, RegisterPair::DE, 0x1234, 0x1235),
            (0x2b, RegisterPair::HL, 0x0000, 0xffff),
            (0x3b, RegisterPair::SP, 0x1000, 0x0fff),
        ] {
            let mut cpu = Cpu8080Cycle::new();
            let mut r = Registers::default();
            r.f = 0xd7;
            cpu.set_registers(r);
            cpu.write_pair(pair, start);
            fetch(&mut cpu, opcode);
            let t5 = cpu.tick(input(0, true));
            assert!(t5.instruction_complete);
            assert_eq!(t5.instruction_t_states, 5);
            assert_eq!(cpu.read_pair(pair), expected);
            assert_eq!(cpu.registers().f, 0xd7);
        }
    }

    #[test]
    fn dad_uses_two_internal_cycles_for_exactly_ten_t_states() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.h = 0xff;
        r.l = 0xff;
        r.b = 0x00;
        r.c = 0x01;
        r.f = 0xd6; // all representative non-carry bits set, carry clear.
        cpu.set_registers(r);
        fetch(&mut cpu, 0x09); // DAD B

        let m2 = [
            cpu.tick(input(0, false)),
            cpu.tick(input(0, false)),
            cpu.tick(input(0, false)),
        ];
        assert_eq!(m2[0].machine_cycle, MachineCycle::Internal);
        assert_eq!(m2[0].machine_cycle_index, 2);
        assert!(!m2[0].pins.sync);
        assert!(!m2[1].pins.dbin);
        assert!(!m2[1].pins.wait); // READY cannot stretch an internal DAD cycle.

        let m3 = [
            cpu.tick(input(0, false)),
            cpu.tick(input(0, false)),
            cpu.tick(input(0, false)),
        ];
        assert!(m3[2].instruction_complete);
        assert_eq!(m3[2].instruction_t_states, 10);
        assert_eq!(cpu.hl(), 0x0000);
        assert_eq!(cpu.registers().f, 0xd7); // only carry changed.
    }

    #[test]
    fn ldax_and_stax_use_bc_or_de_as_latched_effective_address() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.b = 0x12; r.c = 0x34; r.d = 0x56; r.e = 0x78; r.a = 0xa5; r.f = 0x46;
        cpu.set_registers(r);

        fetch(&mut cpu, 0x0a); // LDAX B
        let read = read_cycle(&mut cpu, 0x5a);
        assert_eq!(read[0].pins.address, Some(0x1234));
        assert!(read[2].instruction_complete);
        assert_eq!(read[2].instruction_t_states, 7);
        assert_eq!(cpu.registers().a, 0x5a);

        fetch(&mut cpu, 0x12); // STAX D
        let write = write_cycle(&mut cpu);
        assert_eq!(write[0].pins.address, Some(0x5678));
        assert_eq!(write[1].pins.data_out, Some(0x5a));
        assert!(!write[2].pins.wr_n);
        assert!(write[2].instruction_complete);
        assert_eq!(write[2].instruction_t_states, 7);
        assert_eq!(cpu.registers().f, 0x46);
    }

    #[test]
    fn lda_and_sta_are_thirteen_t_states() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.a = 0xa5; r.f = 0x46;
        cpu.set_registers(r);

        fetch(&mut cpu, 0x3a);
        read_cycle(&mut cpu, 0x34);
        read_cycle(&mut cpu, 0x12);
        let data = read_cycle(&mut cpu, 0x5a);
        assert_eq!(data[0].pins.address, Some(0x1234));
        assert!(data[2].instruction_complete);
        assert_eq!(data[2].instruction_t_states, 13);
        assert_eq!(cpu.registers().a, 0x5a);

        fetch(&mut cpu, 0x32);
        read_cycle(&mut cpu, 0x78);
        read_cycle(&mut cpu, 0x56);
        let write = write_cycle(&mut cpu);
        assert_eq!(write[0].pins.address, Some(0x5678));
        assert_eq!(write[1].pins.data_out, Some(0x5a));
        assert!(!write[2].pins.wr_n);
        assert!(write[2].instruction_complete);
        assert_eq!(write[2].instruction_t_states, 13);
    }

    #[test]
    fn lhld_and_shld_are_sixteen_t_states_and_use_consecutive_addresses() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.h = 0xaa; r.l = 0xbb; r.f = 0xd7;
        cpu.set_registers(r);

        fetch(&mut cpu, 0x2a);
        read_cycle(&mut cpu, 0x00);
        read_cycle(&mut cpu, 0x40);
        let low = read_cycle(&mut cpu, 0x34);
        assert_eq!(low[0].pins.address, Some(0x4000));
        let high = read_cycle(&mut cpu, 0x12);
        assert_eq!(high[0].pins.address, Some(0x4001));
        assert!(high[2].instruction_complete);
        assert_eq!(high[2].instruction_t_states, 16);
        assert_eq!(cpu.hl(), 0x1234);
        assert_eq!(cpu.registers().f, 0xd7);

        fetch(&mut cpu, 0x22);
        read_cycle(&mut cpu, 0x00);
        read_cycle(&mut cpu, 0x50);
        let low_write = write_cycle(&mut cpu);
        assert_eq!(low_write[0].pins.address, Some(0x5000));
        assert_eq!(low_write[1].pins.data_out, Some(0x34));
        assert!(!low_write[2].instruction_complete);
        let high_write = write_cycle(&mut cpu);
        assert_eq!(high_write[0].pins.address, Some(0x5001));
        assert_eq!(high_write[1].pins.data_out, Some(0x12));
        assert!(high_write[2].instruction_complete);
        assert_eq!(high_write[2].instruction_t_states, 16);
    }

    #[test]
    fn previous_hl_addressed_mvi_and_mov_paths_still_work() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.h = 0x20; r.l = 0x10; r.b = 0x77; r.f = 0x46;
        cpu.set_registers(r);

        fetch(&mut cpu, 0x70); // MOV M,B
        let write = write_cycle(&mut cpu);
        assert_eq!(write[0].pins.address, Some(0x2010));
        assert_eq!(write[1].pins.data_out, Some(0x77));
        assert_eq!(write[2].instruction_t_states, 7);

        fetch(&mut cpu, 0x36); // MVI M,d8
        read_cycle(&mut cpu, 0xa5);
        let write = write_cycle(&mut cpu);
        assert_eq!(write[0].pins.address, Some(0x2010));
        assert_eq!(write[1].pins.data_out, Some(0xa5));
        assert_eq!(write[2].instruction_t_states, 10);
    }

    #[test]
    fn ready_wait_still_extends_external_write_and_keeps_bus_stable() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.h = 0x12; r.l = 0x34; r.b = 0xa5;
        cpu.set_registers(r);
        fetch(&mut cpu, 0x70);
        cpu.tick(input(0, true)); // M2 T1
        let t2 = cpu.tick(input(0, false));
        assert_eq!(t2.pins.address, Some(0x1234));
        assert_eq!(t2.pins.data_out, Some(0xa5));
        let tw = cpu.tick(input(0, true));
        assert_eq!(tw.t_state, TState::Tw);
        assert!(tw.pins.wait);
        assert!(!tw.pins.wr_n);
        assert_eq!(tw.pins.address, Some(0x1234));
        assert_eq!(tw.pins.data_out, Some(0xa5));
        let t3 = cpu.tick(input(0, true));
        assert!(t3.instruction_complete);
        assert_eq!(t3.instruction_t_states, 8);
    }

    #[test]
    fn unsupported_hlt_remains_an_explicit_fault() {
        let mut cpu = Cpu8080Cycle::new();
        cpu.tick(input(0, true));
        cpu.tick(input(0, true));
        cpu.tick(input(0x76, true));
        let t4 = cpu.tick(input(0, true));
        assert_eq!(t4.fault, Some(Cpu8080CycleFault::UnsupportedOpcode(0x76)));
    }

    #[test]
    fn reset_preserves_general_registers_but_restarts_at_zero() {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.a = 0x5a; r.b = 0xa5; r.pc = 0x4321;
        cpu.set_registers(r);
        let reset = cpu.tick(Cpu8080Inputs { reset: true, ..Cpu8080Inputs::default() });
        assert!(reset.reset);
        assert_eq!(cpu.registers().pc, 0);
        assert_eq!(cpu.registers().a, 0x5a);
        assert_eq!(cpu.registers().b, 0xa5);
    }
}
