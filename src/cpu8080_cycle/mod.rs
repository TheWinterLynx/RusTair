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
    t_state: TState,
    opcode: Option<u8>,
    instruction: Instruction,
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
            t_state: TState::T1,
            opcode: None,
            instruction: Instruction::Nop,
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
    }

    pub const fn pins(&self) -> Cpu8080Pins {
        self.pins
    }

    pub const fn machine_cycle(&self) -> MachineCycle {
        self.machine_cycle
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
    /// Milestone 1 implements the M1 opcode-fetch cycle, READY/TW insertion,
    /// asynchronous RESET handling and opcode 00h (NOP). Unsupported opcodes
    /// stop at their execution state with an explicit fault instead of silently
    /// behaving like NOPs.
    pub fn tick(&mut self, inputs: Cpu8080Inputs) -> TickTrace {
        if inputs.reset {
            self.apply_reset();
            return TickTrace {
                machine_cycle: self.machine_cycle,
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
        let t_state = self.t_state;

        if machine_cycle == MachineCycle::InstructionFetch && t_state == TState::T1 {
            self.opcode = None;
            self.instruction = Instruction::Nop;
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
            TState::T3 => {
                // DBIN falls at the T3 boundary while the incoming byte is
                // latched into IR. PC is incremented as part of the completed
                // opcode fetch before the instruction's internal T-state(s).
                self.opcode = Some(inputs.data_in);
                self.instruction = decode(inputs.data_in);
                self.registers.pc = self.registers.pc.wrapping_add(1);
                self.t_state = TState::T4;
            }
            TState::T4 => match self.instruction {
                Instruction::Nop => {
                    instruction_complete = true;
                    self.completed_instructions = self.completed_instructions.saturating_add(1);
                    self.last_instruction_t_states = Some(self.current_instruction_t_states);
                    self.current_instruction_t_states = 0;
                    self.machine_cycle = MachineCycle::InstructionFetch;
                    self.t_state = TState::T1;
                }
                Instruction::Unsupported(opcode) => {
                    let unsupported = Cpu8080CycleFault::UnsupportedOpcode(opcode);
                    self.fault = Some(unsupported);
                    fault = Some(unsupported);
                }
            },
            TState::T5 => {
                // No Milestone-1 instruction reaches T5. Keeping the state in
                // the public enum lets later instructions add their true timing
                // without changing trace types.
                unreachable!("Milestone-1 core entered T5")
            }
        }

        TickTrace {
            machine_cycle,
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

    fn apply_reset(&mut self) {
        // Intel RESET forces execution to resume at address zero and disables
        // interrupts. General-purpose registers are intentionally not cleared:
        // their post-reset contents are not architecturally guaranteed.
        self.registers.pc = 0;
        self.inte = false;
        self.pins = Cpu8080Pins::default();
        self.machine_cycle = MachineCycle::InstructionFetch;
        self.t_state = TState::T1;
        self.opcode = None;
        self.instruction = Instruction::Nop;
        self.current_instruction_t_states = 0;
        self.last_instruction_t_states = None;
        self.reset_asserted = true;
        self.fault = None;
    }

    fn drive_pins_for_t_state(&mut self, t_state: TState) {
        self.pins.inte = self.inte;
        self.pins.hlda = false;
        self.pins.wr_n = true;

        match t_state {
            TState::T1 => {
                self.pins.address = Some(self.registers.pc);
                self.pins.data_out = self.machine_cycle.status_word();
                self.pins.sync = true;
                self.pins.dbin = false;
                self.pins.wait = false;
            }
            TState::T2 => {
                self.pins.data_out = None;
                self.pins.sync = false;
                self.pins.dbin = true;
                self.pins.wait = false;
            }
            TState::Tw => {
                self.pins.data_out = None;
                self.pins.sync = false;
                self.pins.dbin = true;
                self.pins.wait = true;
            }
            TState::T3 | TState::T4 | TState::T5 => {
                self.pins.data_out = None;
                self.pins.sync = false;
                self.pins.dbin = false;
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

    #[test]
    fn nop_is_a_four_t_state_m1_fetch() {
        let mut cpu = Cpu8080Cycle::new();

        let t1 = cpu.tick(input(0x00, true));
        assert_eq!(t1.t_state, TState::T1);
        assert_eq!(t1.machine_cycle, MachineCycle::InstructionFetch);
        assert_eq!(t1.pins.address, Some(0x0000));
        assert_eq!(t1.pins.data_out, Some(0xA2));
        assert!(t1.pins.sync);
        assert!(!t1.pins.dbin);
        assert!(t1.pins.wr_n);
        assert!(!t1.instruction_complete);

        let t2 = cpu.tick(input(0x00, true));
        assert_eq!(t2.t_state, TState::T2);
        assert_eq!(t2.pins.address, Some(0x0000));
        assert_eq!(t2.pins.data_out, None);
        assert!(!t2.pins.sync);
        assert!(t2.pins.dbin);

        let t3 = cpu.tick(input(0x00, true));
        assert_eq!(t3.t_state, TState::T3);
        assert!(!t3.pins.dbin);
        assert_eq!(t3.opcode, Some(0x00));
        assert_eq!(cpu.registers().pc, 0x0001);

        let t4 = cpu.tick(input(0x00, true));
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

        cpu.tick(input(0x3e, true)); // T1
        cpu.tick(input(0x3e, true)); // T2
        cpu.tick(input(0x3e, true)); // T3, latch 3Eh
        let t4 = cpu.tick(input(0x00, true));

        assert_eq!(
            t4.fault,
            Some(Cpu8080CycleFault::UnsupportedOpcode(0x3e))
        );
        assert!(!t4.instruction_complete);
        assert_eq!(cpu.completed_instructions(), 0);
        assert_eq!(cpu.total_t_states(), 4);
    }
}
