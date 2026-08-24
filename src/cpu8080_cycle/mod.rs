//! Experimental T-state-accurate Intel 8080 core.
//!
//! The existing `crate::cpu8080` implementation remains RusTair's validated
//! production CPU. This module is being built alongside it so every completed
//! instruction can eventually be checked against the fast core.
//!
//! The machine-cycle structure and signal timing are independently implemented
//! in Rust using Intel 8080 documentation and informed by Jim Drygiannakis'
//! MIT-licensed `jdryg/8080Emu` edge-level model. See
//! `licenses/8080Emu-MIT.txt` for the upstream license notice.

mod decode;
mod pins;
mod state;
mod timing;

pub use pins::{Cpu8080Inputs, Cpu8080Pins};
pub use state::Registers;
pub use timing::{MachineCycle, TState};

use decode::{decode, Instruction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cpu8080CycleFault {
    UnsupportedOpcode(u8),
}

/// Snapshot of the T-state that has just been executed.
///
/// `Cpu8080Cycle::tick` returns the externally visible pins for that state and
/// then advances the core to the next T-state. This makes traces read naturally
/// as `T1, T2, T3, T4, ...` while `Cpu8080Cycle::t_state()` reports what will be
/// executed by the next call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickTrace {
    pub machine_cycle: MachineCycle,
    /// Intel documentation numbers instruction cycles M1, M2, M3...
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
    /// Address latched for the active machine cycle.
    cycle_address: u16,
    /// Data held stable by an active write machine cycle.
    cycle_data_out: Option<u8>,
    opcode: Option<u8>,
    instruction: Instruction,
    operand_low: u8,
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

    /// Seed programmer-visible state for isolated/differential tests.
    ///
    /// Call this only at an instruction boundary; the cycle engine deliberately
    /// does not try to make a mid-machine-cycle register replacement meaningful.
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

    /// Advance exactly one Intel 8080 T-state.
    ///
    /// Milestone 2 supports M1 opcode fetch, memory-read and memory-write
    /// machine cycles, READY/TW insertion, RESET, NOP, MVI A,d8 and STA addr.
    /// Unsupported opcodes stop with an explicit fault instead of silently
    /// behaving like NOPs.
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
            // RESET is asynchronous. Once it is released the next clocked state
            // starts a fresh M1/T1 fetch from address 0000h.
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
            TState::T1 => {
                self.t_state = TState::T2;
            }
            TState::T2 => {
                self.t_state = if inputs.ready { TState::T3 } else { TState::Tw };
            }
            TState::Tw => {
                self.t_state = if inputs.ready { TState::T3 } else { TState::Tw };
            }
            TState::T3 => match machine_cycle {
                MachineCycle::InstructionFetch => {
                    // DBIN falls at the T3 boundary while the incoming byte is
                    // latched into IR. PC advances after the opcode fetch.
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
                _ => unreachable!(
                    "Milestone-2 core entered unsupported machine cycle {:?}",
                    machine_cycle
                ),
            },
            TState::T4 => match self.instruction {
                Instruction::Nop => {
                    instruction_complete = true;
                    self.complete_instruction();
                }
                Instruction::MviAImmediate | Instruction::StaDirect => {
                    // The fourth T-state of M1 is internal decode/setup. Operand
                    // access begins as a separate three-T-state memory cycle.
                    self.begin_memory_read(self.registers.pc, 2);
                }
                Instruction::Unsupported(opcode) => {
                    let unsupported = Cpu8080CycleFault::UnsupportedOpcode(opcode);
                    self.fault = Some(unsupported);
                    fault = Some(unsupported);
                }
            },
            TState::T5 => {
                // No Milestone-2 instruction reaches T5. Keeping the public
                // state lets later opcodes add their documented timing without
                // changing trace types.
                unreachable!("Milestone-2 core entered T5")
            }
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
            (Instruction::MviAImmediate, 2) => {
                self.registers.a = data;
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.complete_instruction();
                true
            }
            (Instruction::StaDirect, 2) => {
                self.operand_low = data;
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.begin_memory_read(self.registers.pc, 3);
                false
            }
            (Instruction::StaDirect, 3) => {
                self.registers.pc = self.registers.pc.wrapping_add(1);
                let address = u16::from_le_bytes([self.operand_low, data]);
                self.begin_memory_write(address, self.registers.a, 4);
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
            (Instruction::StaDirect, 4) => {
                self.complete_instruction();
                true
            }
            _ => unreachable!(
                "invalid memory-write cycle M{} for {:?}",
                self.machine_cycle_index, self.instruction
            ),
        }
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
        // Intel RESET forces execution to resume at address zero and disables
        // interrupts. General-purpose registers are intentionally not cleared:
        // their post-reset contents are not architecturally guaranteed.
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
        self.current_instruction_t_states = 0;
        self.last_instruction_t_states = None;
        self.reset_asserted = true;
        self.fault = None;
    }

    fn drive_pins_for_t_state(&mut self, t_state: TState) {
        self.pins.inte = self.inte;
        self.pins.hlda = false;
        self.pins.address = Some(self.cycle_address);

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
                // /WR is asserted after T2 and remains asserted through any TW
                // states until the write cycle completes at T3.
                self.pins.wr_n = !output_cycle;
                self.pins.wait = true;
            }
            TState::T3 => {
                self.pins.data_out = if output_cycle { self.cycle_data_out } else { None };
                self.pins.sync = false;
                // At our T-state snapshot boundary DBIN has fallen while data_in
                // is latched. /WR is active for write completion in T3.
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
        Cpu8080Inputs {
            data_in,
            ready,
            ..Cpu8080Inputs::default()
        }
    }

    fn fetch_opcode(cpu: &mut Cpu8080Cycle, opcode: u8) -> [TickTrace; 4] {
        [
            cpu.tick(input(0, true)),
            cpu.tick(input(0, true)),
            cpu.tick(input(opcode, true)),
            cpu.tick(input(0, true)),
        ]
    }

    #[test]
    fn nop_is_a_four_t_state_m1_fetch() {
        let mut cpu = Cpu8080Cycle::new();

        let [t1, t2, t3, t4] = fetch_opcode(&mut cpu, 0x00);
        assert_eq!(t1.t_state, TState::T1);
        assert_eq!(t1.machine_cycle, MachineCycle::InstructionFetch);
        assert_eq!(t1.machine_cycle_index, 1);
        assert_eq!(t1.pins.address, Some(0x0000));
        assert_eq!(t1.pins.data_out, Some(0xA2));
        assert!(t1.pins.sync);
        assert!(!t1.pins.dbin);
        assert!(t1.pins.wr_n);
        assert!(!t1.instruction_complete);

        assert_eq!(t2.t_state, TState::T2);
        assert_eq!(t2.pins.address, Some(0x0000));
        assert_eq!(t2.pins.data_out, None);
        assert!(!t2.pins.sync);
        assert!(t2.pins.dbin);

        assert_eq!(t3.t_state, TState::T3);
        assert!(!t3.pins.dbin);
        assert_eq!(t3.opcode, Some(0x00));
        assert_eq!(cpu.registers().pc, 0x0001);

        assert_eq!(t4.t_state, TState::T4);
        assert!(t4.instruction_complete);
        assert_eq!(t4.instruction_t_states, 4);
        assert_eq!(cpu.last_instruction_t_states(), Some(4));
        assert_eq!(cpu.completed_instructions(), 1);
        assert_eq!(cpu.total_t_states(), 4);
        assert_eq!(cpu.t_state(), TState::T1);
    }

    #[test]
    fn three_nops_are_exactly_twelve_t_states() {
        let mut cpu = Cpu8080Cycle::new();
        let mut completions = 0;

        for _ in 0..12 {
            if cpu.tick(input(0x00, true)).instruction_complete {
                completions += 1;
            }
        }

        assert_eq!(completions, 3);
        assert_eq!(cpu.completed_instructions(), 3);
        assert_eq!(cpu.total_t_states(), 12);
        assert_eq!(cpu.registers().pc, 0x0003);
        assert_eq!(cpu.last_instruction_t_states(), Some(4));
    }

    #[test]
    fn mvi_a_immediate_is_exactly_seven_t_states() {
        let mut cpu = Cpu8080Cycle::new();
        let mut registers = Registers::default();
        registers.pc = 0x2000;
        registers.a = 0x11;
        registers.f = 0xd7;
        cpu.set_registers(registers);

        let fetch = fetch_opcode(&mut cpu, 0x3e);
        assert_eq!(fetch[0].pins.address, Some(0x2000));
        assert_eq!(fetch[3].t_state, TState::T4);
        assert!(!fetch[3].instruction_complete);
        assert_eq!(cpu.machine_cycle(), MachineCycle::MemoryRead);
        assert_eq!(cpu.machine_cycle_index(), 2);

        let m2t1 = cpu.tick(input(0, true));
        assert_eq!(m2t1.machine_cycle, MachineCycle::MemoryRead);
        assert_eq!(m2t1.machine_cycle_index, 2);
        assert_eq!(m2t1.t_state, TState::T1);
        assert_eq!(m2t1.pins.address, Some(0x2001));
        assert_eq!(m2t1.pins.data_out, Some(0x82));
        assert!(m2t1.pins.sync);
        assert!(!m2t1.pins.dbin);

        let m2t2 = cpu.tick(input(0, true));
        assert_eq!(m2t2.t_state, TState::T2);
        assert!(m2t2.pins.dbin);
        assert!(m2t2.pins.wr_n);

        let m2t3 = cpu.tick(input(0x42, true));
        assert_eq!(m2t3.t_state, TState::T3);
        assert!(!m2t3.pins.dbin);
        assert!(m2t3.instruction_complete);
        assert_eq!(m2t3.instruction_t_states, 7);

        assert_eq!(cpu.registers().a, 0x42);
        assert_eq!(cpu.registers().f, 0xd7);
        assert_eq!(cpu.registers().pc, 0x2002);
        assert_eq!(cpu.last_instruction_t_states(), Some(7));
        assert_eq!(cpu.total_t_states(), 7);
        assert_eq!(cpu.completed_instructions(), 1);
        assert_eq!(cpu.machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(cpu.t_state(), TState::T1);
    }

    #[test]
    fn mvi_memory_read_honors_ready_and_tw() {
        let mut cpu = Cpu8080Cycle::new();
        fetch_opcode(&mut cpu, 0x3e);

        cpu.tick(input(0, true)); // M2 T1
        let t2 = cpu.tick(input(0, false));
        assert_eq!(t2.t_state, TState::T2);
        assert!(t2.pins.dbin);
        assert_eq!(cpu.t_state(), TState::Tw);

        let tw = cpu.tick(input(0x99, true));
        assert_eq!(tw.t_state, TState::Tw);
        assert!(tw.pins.wait);
        assert!(tw.pins.dbin);
        assert_eq!(cpu.t_state(), TState::T3);

        let t3 = cpu.tick(input(0x5a, true));
        assert!(t3.instruction_complete);
        assert_eq!(t3.instruction_t_states, 8);
        assert_eq!(cpu.registers().a, 0x5a);
        assert_eq!(cpu.last_instruction_t_states(), Some(8));
    }

    #[test]
    fn sta_direct_is_thirteen_t_states_and_drives_a_real_write_cycle() {
        let mut cpu = Cpu8080Cycle::new();
        let mut registers = Registers::default();
        registers.pc = 0x0100;
        registers.a = 0x5a;
        registers.f = 0x46;
        cpu.set_registers(registers);

        fetch_opcode(&mut cpu, 0x32);

        let m2t1 = cpu.tick(input(0, true));
        assert_eq!(m2t1.machine_cycle, MachineCycle::MemoryRead);
        assert_eq!(m2t1.machine_cycle_index, 2);
        assert_eq!(m2t1.pins.address, Some(0x0101));
        assert_eq!(m2t1.pins.data_out, Some(0x82));
        cpu.tick(input(0, true)); // M2 T2
        let m2t3 = cpu.tick(input(0x34, true));
        assert!(!m2t3.instruction_complete);
        assert_eq!(cpu.registers().pc, 0x0102);

        let m3t1 = cpu.tick(input(0, true));
        assert_eq!(m3t1.machine_cycle, MachineCycle::MemoryRead);
        assert_eq!(m3t1.machine_cycle_index, 3);
        assert_eq!(m3t1.pins.address, Some(0x0102));
        cpu.tick(input(0, true)); // M3 T2
        let m3t3 = cpu.tick(input(0x12, true));
        assert!(!m3t3.instruction_complete);
        assert_eq!(cpu.registers().pc, 0x0103);
        assert_eq!(cpu.machine_cycle(), MachineCycle::MemoryWrite);
        assert_eq!(cpu.machine_cycle_index(), 4);

        let m4t1 = cpu.tick(input(0, true));
        assert_eq!(m4t1.machine_cycle, MachineCycle::MemoryWrite);
        assert_eq!(m4t1.machine_cycle_index, 4);
        assert_eq!(m4t1.pins.address, Some(0x1234));
        assert_eq!(m4t1.pins.data_out, Some(0x00));
        assert!(m4t1.pins.sync);
        assert!(m4t1.pins.wr_n);
        assert!(!m4t1.pins.dbin);

        let m4t2 = cpu.tick(input(0, true));
        assert_eq!(m4t2.t_state, TState::T2);
        assert_eq!(m4t2.pins.address, Some(0x1234));
        assert_eq!(m4t2.pins.data_out, Some(0x5a));
        assert!(m4t2.pins.wr_n);
        assert!(!m4t2.pins.dbin);

        let m4t3 = cpu.tick(input(0, true));
        assert_eq!(m4t3.t_state, TState::T3);
        assert_eq!(m4t3.pins.address, Some(0x1234));
        assert_eq!(m4t3.pins.data_out, Some(0x5a));
        assert!(!m4t3.pins.wr_n);
        assert!(m4t3.instruction_complete);
        assert_eq!(m4t3.instruction_t_states, 13);

        assert_eq!(cpu.registers().a, 0x5a);
        assert_eq!(cpu.registers().f, 0x46);
        assert_eq!(cpu.registers().pc, 0x0103);
        assert_eq!(cpu.total_t_states(), 13);
        assert_eq!(cpu.last_instruction_t_states(), Some(13));
        assert_eq!(cpu.completed_instructions(), 1);
    }

    #[test]
    fn sta_write_wait_keeps_address_and_data_stable_and_extends_wr() {
        let mut cpu = Cpu8080Cycle::new();
        let mut registers = Registers::default();
        registers.a = 0xa5;
        cpu.set_registers(registers);

        fetch_opcode(&mut cpu, 0x32);
        cpu.tick(input(0, true)); // M2 T1
        cpu.tick(input(0, true)); // M2 T2
        cpu.tick(input(0x78, true)); // M2 T3
        cpu.tick(input(0, true)); // M3 T1
        cpu.tick(input(0, true)); // M3 T2
        cpu.tick(input(0x56, true)); // M3 T3
        cpu.tick(input(0, true)); // M4 T1

        let t2 = cpu.tick(input(0, false));
        assert_eq!(t2.t_state, TState::T2);
        assert_eq!(t2.pins.address, Some(0x5678));
        assert_eq!(t2.pins.data_out, Some(0xa5));
        assert!(t2.pins.wr_n);
        assert_eq!(cpu.t_state(), TState::Tw);

        let tw = cpu.tick(input(0, true));
        assert_eq!(tw.t_state, TState::Tw);
        assert_eq!(tw.pins.address, Some(0x5678));
        assert_eq!(tw.pins.data_out, Some(0xa5));
        assert!(!tw.pins.wr_n);
        assert!(tw.pins.wait);

        let t3 = cpu.tick(input(0, true));
        assert_eq!(t3.t_state, TState::T3);
        assert_eq!(t3.pins.data_out, Some(0xa5));
        assert!(!t3.pins.wr_n);
        assert!(t3.instruction_complete);
        assert_eq!(t3.instruction_t_states, 14);
        assert_eq!(cpu.last_instruction_t_states(), Some(14));
    }

    #[test]
    fn ready_low_inserts_tw_without_consuming_the_opcode() {
        let mut cpu = Cpu8080Cycle::new();

        cpu.tick(input(0x00, true)); // T1
        let t2 = cpu.tick(input(0x5a, false));
        assert_eq!(t2.t_state, TState::T2);
        assert!(t2.pins.dbin);
        assert_eq!(cpu.t_state(), TState::Tw);

        let tw1 = cpu.tick(input(0x5a, false));
        assert_eq!(tw1.t_state, TState::Tw);
        assert!(tw1.pins.wait);
        assert!(tw1.pins.dbin);
        assert_eq!(cpu.registers().pc, 0x0000);

        let tw2 = cpu.tick(input(0x5a, true));
        assert_eq!(tw2.t_state, TState::Tw);
        assert!(tw2.pins.wait);
        assert_eq!(cpu.t_state(), TState::T3);

        let t3 = cpu.tick(input(0x00, true));
        assert_eq!(t3.opcode, Some(0x00));
        assert_eq!(cpu.registers().pc, 0x0001);

        let t4 = cpu.tick(input(0x00, true));
        assert!(t4.instruction_complete);
        assert_eq!(t4.instruction_t_states, 6);
        assert_eq!(cpu.last_instruction_t_states(), Some(6));
    }

    #[test]
    fn reset_restarts_fetch_at_zero_without_clearing_general_registers() {
        let mut cpu = Cpu8080Cycle::new();
        let mut registers = Registers::default();
        registers.a = 0x5a;
        registers.b = 0xa5;
        registers.pc = 0x4321;
        cpu.set_registers(registers);

        let reset = cpu.tick(Cpu8080Inputs {
            reset: true,
            ..Cpu8080Inputs::default()
        });
        assert!(reset.reset);
        assert_eq!(cpu.registers().pc, 0x0000);
        assert_eq!(cpu.registers().a, 0x5a);
        assert_eq!(cpu.registers().b, 0xa5);
        assert!(!cpu.interrupts_enabled());
        assert_eq!(cpu.t_state(), TState::T1);
        assert_eq!(cpu.pins(), Cpu8080Pins::default());

        let t1 = cpu.tick(Cpu8080Inputs::default());
        assert_eq!(t1.t_state, TState::T1);
        assert_eq!(t1.pins.address, Some(0x0000));
        assert_eq!(t1.pins.data_out, Some(0xA2));
    }

    #[test]
    fn unsupported_opcode_faults_instead_of_becoming_an_implicit_nop() {
        let mut cpu = Cpu8080Cycle::new();

        cpu.tick(input(0xff, true)); // T1
        cpu.tick(input(0xff, true)); // T2
        cpu.tick(input(0xff, true)); // T3, latch FFh
        let t4 = cpu.tick(input(0x00, true));

        assert_eq!(
            t4.fault,
            Some(Cpu8080CycleFault::UnsupportedOpcode(0xff))
        );
        assert!(!t4.instruction_complete);
        assert_eq!(cpu.completed_instructions(), 0);
        assert_eq!(cpu.total_t_states(), 4);
    }
}
