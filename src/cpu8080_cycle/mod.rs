//! Experimental T-state-accurate Intel 8080 core.
//!
//! The existing `crate::cpu8080` implementation remains RusTair's validated
//! production CPU. This module is built alongside it for differential testing.
//!
//! Machine-cycle structure and signal timing are independently implemented in
//! Rust from Intel documentation and informed by Jim Drygiannakis' MIT-licensed
//! `jdryg/8080Emu` edge-level model. See `licenses/8080Emu-MIT.txt`.

mod alu;
mod control_flow;
mod decode;
mod pins;
mod state;
mod timing;

#[cfg(test)]
mod alu_tests;
#[cfg(test)]
mod call_return_tests;
#[cfg(test)]
mod control_flow_tests;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod special_transfer_tests;

pub use pins::{Cpu8080Inputs, Cpu8080Pins};
pub use state::Registers;
pub use timing::{MachineCycle, TState};

use decode::{decode, Instruction, Register8, RegisterPair};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cpu8080CycleFault {
    UnsupportedOpcode(u8),
}

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
    temporary_word: u16,
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
            temporary_word: 0,
            inte: false,
            total_t_states: 0,
            completed_instructions: 0,
            current_instruction_t_states: 0,
            last_instruction_t_states: None,
            reset_asserted: false,
            fault: None,
        }
    }

    pub const fn registers(&self) -> Registers {
        self.registers
    }

    pub fn set_registers(&mut self, registers: Registers) {
        self.registers = registers;
        if self.machine_cycle == MachineCycle::InstructionFetch && self.t_state == TState::T1 {
            self.cycle_address = registers.pc;
        }
    }

    pub const fn pins(&self) -> Cpu8080Pins {
        self.pins
    }

    pub const fn machine_cycle(&self) -> MachineCycle {
        self.machine_cycle
    }

    pub const fn machine_cycle_index(&self) -> u8 {
        self.machine_cycle_index
    }

    pub const fn t_state(&self) -> TState {
        self.t_state
    }

    pub const fn total_t_states(&self) -> u64 {
        self.total_t_states
    }

    pub const fn completed_instructions(&self) -> u64 {
        self.completed_instructions
    }

    pub const fn last_instruction_t_states(&self) -> Option<u32> {
        self.last_instruction_t_states
    }

    pub const fn interrupts_enabled(&self) -> bool {
        self.inte
    }

    pub const fn fault(&self) -> Option<Cpu8080CycleFault> {
        self.fault
    }

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
            self.temporary_word = 0;
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
                MachineCycle::StackRead => {
                    instruction_complete = self.finish_stack_read(inputs.data_in);
                }
                MachineCycle::StackWrite => {
                    instruction_complete = self.finish_stack_write();
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
                Instruction::AluRegister { op, src } => {
                    let rhs = self.read_register(src);
                    alu::execute(&mut self.registers, op, rhs);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Xchg => {
                    let de = self.read_pair(RegisterPair::DE);
                    let hl = self.hl();
                    self.write_pair(RegisterPair::DE, hl);
                    self.write_pair(RegisterPair::HL, de);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Xthl => {
                    if machine_cycle == MachineCycle::InstructionFetch && machine_cycle_index == 1 {
                        self.begin_stack_read(self.registers.sp, 2);
                    } else if machine_cycle == MachineCycle::StackWrite && machine_cycle_index == 5 {
                        self.t_state = TState::T5;
                    } else {
                        unreachable!(
                            "unexpected XTHL T4 in {:?} M{}",
                            machine_cycle, machine_cycle_index
                        );
                    }
                }
                Instruction::MviImmediate(_)
                | Instruction::MviMemory
                | Instruction::Lxi(_)
                | Instruction::LdaDirect
                | Instruction::StaDirect
                | Instruction::LhldDirect
                | Instruction::ShldDirect
                | Instruction::AluImmediate { .. }
                | Instruction::Jump
                | Instruction::JumpConditional(_) => {
                    self.begin_memory_read(self.registers.pc, 2);
                }
                Instruction::Call
                | Instruction::CallConditional(_)
                | Instruction::RetConditional(_)
                | Instruction::Rst(_)
                | Instruction::Push(_)
                | Instruction::MovRegister { .. }
                | Instruction::Inx(_)
                | Instruction::Dcx(_)
                | Instruction::InrRegister(_)
                | Instruction::DcrRegister(_)
                | Instruction::Pchl
                | Instruction::Sphl => {
                    self.t_state = TState::T5;
                }
                Instruction::Ret | Instruction::Pop(_) => {
                    self.begin_stack_read(self.registers.sp, 2);
                }
                Instruction::MovFromMemory { .. }
                | Instruction::InrMemory
                | Instruction::DcrMemory
                | Instruction::AluMemory { .. } => {
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
                Instruction::InrRegister(dst) => {
                    let value = self.read_register(dst);
                    let result = alu::inr(&mut self.registers, value);
                    self.write_register(dst, result);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::DcrRegister(dst) => {
                    let value = self.read_register(dst);
                    let result = alu::dcr(&mut self.registers, value);
                    self.write_register(dst, result);
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Push(pair) => {
                    let [high, _] = control_flow::read_stack_pair(&self.registers, pair).to_be_bytes();
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                    self.begin_stack_write(self.registers.sp, high, 2);
                }
                Instruction::Call => {
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                    self.begin_memory_read(self.registers.pc, 2);
                }
                Instruction::CallConditional(condition) => {
                    if control_flow::condition(self.registers.f, condition) {
                        self.registers.sp = self.registers.sp.wrapping_sub(1);
                    }
                    self.begin_memory_read(self.registers.pc, 2);
                }
                Instruction::RetConditional(condition) => {
                    if control_flow::condition(self.registers.f, condition) {
                        self.begin_stack_read(self.registers.sp, 2);
                    } else {
                        instruction_complete = true;
                        self.complete_instruction();
                    }
                }
                Instruction::Rst(_) => {
                    let [high, _] = self.registers.pc.to_be_bytes();
                    self.registers.sp = self.registers.sp.wrapping_sub(1);
                    self.begin_stack_write(self.registers.sp, high, 2);
                }
                Instruction::Pchl => {
                    self.registers.pc = self.hl();
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Sphl => {
                    self.registers.sp = self.hl();
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::Xthl => {
                    self.write_pair(RegisterPair::HL, self.temporary_word);
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
            (Instruction::InrMemory, 2) => {
                let address = self.cycle_address;
                let result = alu::inr(&mut self.registers, data);
                self.begin_memory_write(address, result, 3);
                false
            }
            (Instruction::DcrMemory, 2) => {
                let address = self.cycle_address;
                let result = alu::dcr(&mut self.registers, data);
                self.begin_memory_write(address, result, 3);
                false
            }
            (Instruction::AluMemory { op }, 2) => {
                alu::execute(&mut self.registers, op, data);
                self.complete_instruction();
                true
            }
            (Instruction::AluImmediate { op }, 2) => {
                self.registers.pc = self.registers.pc.wrapping_add(1);
                alu::execute(&mut self.registers, op, data);
                self.complete_instruction();
                true
            }
            (Instruction::Lxi(_), 2) => {
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
            | (Instruction::ShldDirect, 2)
            | (Instruction::Jump, 2)
            | (Instruction::JumpConditional(_), 2)
            | (Instruction::Call, 2)
            | (Instruction::CallConditional(_), 2) => {
                self.operand_low = data;
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.begin_memory_read(self.registers.pc, 3);
                false
            }
            (Instruction::Jump, 3) => {
                self.finish_direct_address(data);
                self.registers.pc = self.effective_address;
                self.complete_instruction();
                true
            }
            (Instruction::JumpConditional(condition), 3) => {
                self.finish_direct_address(data);
                if control_flow::condition(self.registers.f, condition) {
                    self.registers.pc = self.effective_address;
                }
                self.complete_instruction();
                true
            }
            (Instruction::Call, 3) => {
                self.finish_direct_address(data);
                let [high, _] = self.registers.pc.to_be_bytes();
                self.begin_stack_write(self.registers.sp, high, 4);
                false
            }
            (Instruction::CallConditional(condition), 3) => {
                self.finish_direct_address(data);
                if control_flow::condition(self.registers.f, condition) {
                    let [high, _] = self.registers.pc.to_be_bytes();
                    self.begin_stack_write(self.registers.sp, high, 4);
                    false
                } else {
                    self.complete_instruction();
                    true
                }
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
            | (Instruction::StaDirect, 4)
            | (Instruction::InrMemory, 3)
            | (Instruction::DcrMemory, 3) => {
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

    fn finish_stack_read(&mut self, data: u8) -> bool {
        match (self.instruction, self.machine_cycle_index) {
            (Instruction::Pop(_), 2) => {
                self.operand_low = data;
                self.registers.sp = self.registers.sp.wrapping_add(1);
                self.begin_stack_read(self.registers.sp, 3);
                false
            }
            (Instruction::Pop(pair), 3) => {
                self.registers.sp = self.registers.sp.wrapping_add(1);
                control_flow::write_stack_pair(
                    &mut self.registers,
                    pair,
                    u16::from_le_bytes([self.operand_low, data]),
                );
                self.complete_instruction();
                true
            }
            (Instruction::Ret, 2) | (Instruction::RetConditional(_), 2) => {
                self.operand_low = data;
                self.registers.sp = self.registers.sp.wrapping_add(1);
                self.begin_stack_read(self.registers.sp, 3);
                false
            }
            (Instruction::Ret, 3) | (Instruction::RetConditional(_), 3) => {
                self.registers.sp = self.registers.sp.wrapping_add(1);
                self.registers.pc = u16::from_le_bytes([self.operand_low, data]);
                self.complete_instruction();
                true
            }
            (Instruction::Xthl, 2) => {
                self.operand_low = data;
                self.begin_stack_read(self.registers.sp.wrapping_add(1), 3);
                false
            }
            (Instruction::Xthl, 3) => {
                self.temporary_word = u16::from_le_bytes([self.operand_low, data]);
                self.begin_stack_write(self.registers.sp, self.registers.l, 4);
                false
            }
            _ => unreachable!(
                "invalid stack-read cycle M{} for {:?}",
                self.machine_cycle_index, self.instruction
            ),
        }
    }

    fn finish_stack_write(&mut self) -> bool {
        match (self.instruction, self.machine_cycle_index) {
            (Instruction::Push(pair), 2) => {
                let [_, low] = control_flow::read_stack_pair(&self.registers, pair).to_be_bytes();
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                self.begin_stack_write(self.registers.sp, low, 3);
                false
            }
            (Instruction::Push(_), 3) => {
                self.complete_instruction();
                true
            }
            (Instruction::Call, 4) | (Instruction::CallConditional(_), 4) => {
                let [_, low] = self.registers.pc.to_be_bytes();
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                self.begin_stack_write(self.registers.sp, low, 5);
                false
            }
            (Instruction::Call, 5) | (Instruction::CallConditional(_), 5) => {
                self.registers.pc = self.effective_address;
                self.complete_instruction();
                true
            }
            (Instruction::Rst(_), 2) => {
                let [_, low] = self.registers.pc.to_be_bytes();
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                self.begin_stack_write(self.registers.sp, low, 3);
                false
            }
            (Instruction::Rst(vector), 3) => {
                self.registers.pc = u16::from(vector) << 3;
                self.complete_instruction();
                true
            }
            (Instruction::Xthl, 4) => {
                self.begin_stack_write(
                    self.registers.sp.wrapping_add(1),
                    self.registers.h,
                    5,
                );
                false
            }
            (Instruction::Xthl, 5) => {
                // The real 8080 keeps the final StackWrite machine cycle alive
                // for T4/T5; HL is replaced at the end of T5.
                self.t_state = TState::T4;
                false
            }
            _ => unreachable!(
                "invalid stack-write cycle M{} for {:?}",
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
                self.registers.f = (self.registers.f & !alu::FLAG_C)
                    | if sum > 0xffff { alu::FLAG_C } else { 0 };
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
            RegisterPair::BC => {
                self.registers.b = high;
                self.registers.c = low;
            }
            RegisterPair::DE => {
                self.registers.d = high;
                self.registers.e = low;
            }
            RegisterPair::HL => {
                self.registers.h = high;
                self.registers.l = low;
            }
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

    fn begin_stack_read(&mut self, address: u16, machine_cycle_index: u8) {
        self.machine_cycle = MachineCycle::StackRead;
        self.machine_cycle_index = machine_cycle_index;
        self.t_state = TState::T1;
        self.cycle_address = address;
        self.cycle_data_out = None;
    }

    fn begin_stack_write(&mut self, address: u16, data: u8, machine_cycle_index: u8) {
        self.machine_cycle = MachineCycle::StackWrite;
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
        self.registers.f = (self.registers.f & 0xd5) | alu::FLAG_1;
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
        self.temporary_word = 0;
        self.current_instruction_t_states = 0;
        self.last_instruction_t_states = None;
        self.reset_asserted = true;
        self.fault = None;
    }

    fn drive_pins_for_t_state(&mut self, t_state: TState) {
        self.pins.inte = self.inte;
        self.pins.hlda = false;
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
