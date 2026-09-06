use crate::cpu8080::Cpu8080;
#[cfg(test)]
use crate::cpu8080::Bus;

use super::decode::{decode, Instruction};
use super::{Cpu8080Cycle, Cpu8080Pins, MachineCycle, TState};

/// Programmer-visible Intel 8080 register state.
///
/// The shape intentionally mirrors the existing fast core so the future
/// differential harness can compare both engines directly after each
/// instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Registers {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub f: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Default for Registers {
    fn default() -> Self {
        Self {
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            f: 0x02,
            sp: 0,
            pc: 0,
        }
    }
}

impl Cpu8080Cycle {
    /// Seed the undefined processor state that exists immediately after power is
    /// applied, before RESET establishes its documented control state.
    ///
    /// The Altair chassis deliberately randomizes programmer-visible registers
    /// and the 8080 interrupt-enable flip-flop at power-on. The cycle backend
    /// must inherit the same sample so CPU state, its passive fast-core mirror,
    /// and the S-100 INTE line all describe one physical machine.
    pub fn initialize_power_on_state(&mut self, registers: Registers, inte: bool) {
        debug_assert_eq!(self.total_t_states, 0);
        debug_assert_eq!(self.completed_instructions, 0);
        self.set_registers(registers);
        self.inte = inte;
        self.ei_pending = false;
        self.enable_inte_after_instruction = false;
        self.halted = false;
        self.pins.inte = inte;
    }

    /// Complete the zero-time internal transition that the partial core performs
    /// at the top of the first tick after the physical RESET line is released.
    ///
    /// RESET assertion already established PC=0, disabled INTE and returned the
    /// core to InstructionFetch/T1. Releasing RESET does not itself consume an
    /// 8080 T-state; the partial executor merely clears this bookkeeping latch
    /// before driving that T1. Full execution must perform the same transition
    /// explicitly or it would incorrectly reject the first post-RESET opcode.
    pub(crate) fn prepare_full_boundary_after_reset_release(&mut self) -> bool {
        if self.machine_cycle != MachineCycle::InstructionFetch
            || self.t_state != TState::T1
            || self.current_instruction_t_states != 0
            || self.halted
            || self.holding
            || self.hold_pending
            || self.fault.is_some()
        {
            return false;
        }
        self.reset_asserted = false;
        true
    }

    #[inline]
    fn full_execution_boundary_ready(&self) -> bool {
        self.machine_cycle == MachineCycle::InstructionFetch
            && self.t_state == TState::T1
            && self.current_instruction_t_states == 0
            && !self.halted
            && !self.holding
            && !self.hold_pending
            && !self.reset_asserted
            && self.fault.is_none()
            && !self.ei_pending
            && !self.enable_inte_after_instruction
    }

    /// Instruction families whose external machine-cycle schedule is already
    /// representable by the compiled Full bridge. This predicate intentionally
    /// contains no mutable Cycle-core state so one prepared semantic core may run
    /// many consecutive instructions without copying registers back after each
    /// opcode merely to ask the same class question again.
    pub(crate) fn full_opcode_class_supported(opcode: u8) -> bool {
        matches!(
            decode(opcode),
            Instruction::Nop
                | Instruction::MviImmediate(_)
                | Instruction::MviMemory
                | Instruction::MovRegister { .. }
                | Instruction::MovFromMemory { .. }
                | Instruction::MovToMemory { .. }
                | Instruction::Lxi(_)
                | Instruction::Inx(_)
                | Instruction::Dcx(_)
                | Instruction::Dad(_)
                | Instruction::Ldax(_)
                | Instruction::Stax(_)
                | Instruction::LdaDirect
                | Instruction::StaDirect
                | Instruction::LhldDirect
                | Instruction::ShldDirect
                | Instruction::InrRegister(_)
                | Instruction::InrMemory
                | Instruction::DcrRegister(_)
                | Instruction::DcrMemory
                | Instruction::AluRegister { .. }
                | Instruction::AluMemory { .. }
                | Instruction::AluImmediate { .. }
                | Instruction::Jump
                | Instruction::JumpConditional(_)
                | Instruction::Ret
                | Instruction::Pop(_)
                | Instruction::Push(_)
                | Instruction::Pchl
                | Instruction::Xchg
                | Instruction::Sphl
        )
    }

    /// MAME-style `full` execution is legal only at a clean instruction
    /// boundary and only for instruction families whose external machine-cycle
    /// schedule can currently be reconstructed without a mid-instruction event.
    ///
    /// The stateful T-state engine remains the `partial` oracle. I/O, HLT,
    /// delayed interrupt-enable transitions and instruction families with extra
    /// non-bus cycles interleaved between external cycles stay on that path until
    /// their compiled schedules are added explicitly.
    #[cfg(test)]
    pub(crate) fn full_execution_opcode_supported(&self, opcode: u8) -> bool {
        self.full_execution_boundary_ready() && Self::full_opcode_class_supported(opcode)
    }

    /// Export one clean Cycle boundary into the validated instruction-level 8080
    /// semantic core. A Full execution window may keep this core authoritative
    /// for multiple supported instructions and import it only once when a real
    /// synchronization boundary is reached.
    pub(crate) fn begin_full_execution_window(&self) -> Option<Cpu8080> {
        if !self.full_execution_boundary_ready() {
            return None;
        }

        let mut full = Cpu8080::new();
        full.a = self.registers.a;
        full.b = self.registers.b;
        full.c = self.registers.c;
        full.d = self.registers.d;
        full.e = self.registers.e;
        full.h = self.registers.h;
        full.l = self.registers.l;
        full.f = self.registers.f;
        full.pc = self.registers.pc;
        full.sp = self.registers.sp;
        full.inte = self.inte;
        full.halted = false;
        full.cycles = self.total_t_states;
        Some(full)
    }

    /// Import one completed Full window at the next exact InstructionFetch/T1
    /// boundary. No guest-visible state is skipped here: every instruction in
    /// the window was executed by the same semantic core and every memory access
    /// already went through the bus-owned compiled S-100 decoder.
    pub(crate) fn commit_full_execution_window(
        &mut self,
        full: &Cpu8080,
        completed: u64,
        last_elapsed: u32,
    ) {
        debug_assert!(completed != 0);
        debug_assert!(!full.halted);
        self.registers = Registers {
            a: full.a,
            b: full.b,
            c: full.c,
            d: full.d,
            e: full.e,
            h: full.h,
            l: full.l,
            f: full.f,
            sp: full.sp,
            pc: full.pc,
        };
        self.inte = full.inte;
        self.ei_pending = false;
        self.enable_inte_after_instruction = false;
        self.halted = false;
        self.hold_pending = false;
        self.holding = false;
        self.total_t_states = full.cycles;
        self.completed_instructions = self.completed_instructions.saturating_add(completed);
        self.current_instruction_t_states = 0;
        self.last_instruction_t_states = Some(last_elapsed);
        self.opcode = None;
        self.instruction = Instruction::Nop;
        self.operand_low = 0;
        self.effective_address = 0;
        self.temporary_word = 0;
        self.reset_asserted = false;
        self.fault = None;
        self.begin_instruction_fetch();
        self.pins.inte = self.inte;
    }

    /// Execute one whole instruction using RusTair's validated instruction-level
    /// 8080 semantics, then import the programmer-visible result back into this
    /// exact core at the next clean fetch boundary.
    ///
    /// This remains as the narrow one-instruction bridge used by focused tests;
    /// production Full execution uses `begin_full_execution_window` and
    /// `commit_full_execution_window` so the register set is copied only at real
    /// synchronization boundaries rather than once per opcode.
    #[cfg(test)]
    pub(crate) fn execute_full_instruction<B: Bus>(
        &mut self,
        bus: &mut B,
        opcode: u8,
    ) -> Option<u32> {
        if !self.full_execution_opcode_supported(opcode) {
            return None;
        }

        let mut full = self.begin_full_execution_window()?;
        let before = full.cycles;
        let elapsed = full.step(bus);
        debug_assert_eq!(full.cycles.saturating_sub(before), u64::from(elapsed));
        self.commit_full_execution_window(&full, 1, elapsed);
        Some(elapsed)
    }

    /// Restore the package-output state that physically remained on the CPU
    /// board after a compiled full instruction. The semantic core is already at
    /// the following fetch boundary; these are only the previous cycle's held
    /// package pins during the dead time before the next PHI1.
    pub(crate) fn set_full_boundary_pins(&mut self, mut pins: Cpu8080Pins) {
        debug_assert_eq!(self.machine_cycle, MachineCycle::InstructionFetch);
        debug_assert_eq!(self.t_state, TState::T1);
        pins.phi1 = false;
        pins.phi2 = false;
        pins.inte = self.inte;
        self.pins = pins;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu8080_cycle::Cpu8080Cycle;

    #[test]
    fn power_on_seed_carries_interrupt_flip_flop_without_advancing_time() {
        let registers = Registers {
            a: 0x11,
            b: 0x22,
            c: 0x33,
            d: 0x44,
            e: 0x55,
            h: 0x66,
            l: 0x77,
            f: 0xd7,
            sp: 0x1234,
            pc: 0xabcd,
        };
        let mut cpu = Cpu8080Cycle::new();

        cpu.initialize_power_on_state(registers, true);

        assert_eq!(cpu.registers(), registers);
        assert!(cpu.interrupts_enabled());
        assert!(cpu.pins().inte);
        assert_eq!(cpu.total_t_states(), 0);
        assert_eq!(cpu.completed_instructions(), 0);
        assert!(!cpu.is_halted());
    }

    #[test]
    fn full_executor_rejoins_partial_core_at_clean_fetch_boundary() {
        struct TestBus {
            memory: [u8; 16],
        }
        impl Bus for TestBus {
            fn read(&mut self, address: u16) -> u8 { self.memory[address as usize] }
            fn write(&mut self, address: u16, value: u8) { self.memory[address as usize] = value; }
        }

        let mut cpu = Cpu8080Cycle::new();
        let mut bus = TestBus { memory: [0; 16] };
        bus.memory[0] = 0x3e; // MVI A,5Ah
        bus.memory[1] = 0x5a;

        assert_eq!(cpu.execute_full_instruction(&mut bus, 0x3e), Some(7));
        assert_eq!(cpu.registers().a, 0x5a);
        assert_eq!(cpu.registers().pc, 2);
        assert_eq!(cpu.total_t_states(), 7);
        assert_eq!(cpu.completed_instructions(), 1);
        assert_eq!(cpu.machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(cpu.t_state(), TState::T1);
    }

    #[test]
    fn full_window_imports_many_semantic_instructions_once() {
        struct TestBus {
            memory: [u8; 16],
        }
        impl Bus for TestBus {
            fn read(&mut self, address: u16) -> u8 { self.memory[address as usize] }
            fn write(&mut self, address: u16, value: u8) { self.memory[address as usize] = value; }
        }

        let mut cpu = Cpu8080Cycle::new();
        let mut bus = TestBus { memory: [0; 16] };
        bus.memory[..4].copy_from_slice(&[0x04, 0x04, 0x05, 0x00]); // INR B, INR B, DCR B, NOP
        let mut full = cpu.begin_full_execution_window().unwrap();
        let mut last = 0;
        for _ in 0..4 {
            let opcode = bus.memory[full.pc as usize];
            assert!(Cpu8080Cycle::full_opcode_class_supported(opcode));
            last = full.step(&mut bus);
        }
        cpu.commit_full_execution_window(&full, 4, last);

        assert_eq!(cpu.registers().b, 1);
        assert_eq!(cpu.registers().pc, 4);
        assert_eq!(cpu.total_t_states(), 19);
        assert_eq!(cpu.completed_instructions(), 4);
        assert_eq!(cpu.machine_cycle(), MachineCycle::InstructionFetch);
        assert_eq!(cpu.t_state(), TState::T1);
    }

    #[test]
    fn full_executor_rejects_io_halt_and_pending_ei_without_touching_bus() {
        struct CountingBus(usize);
        impl Bus for CountingBus {
            fn read(&mut self, _address: u16) -> u8 { self.0 += 1; 0 }
            fn write(&mut self, _address: u16, _value: u8) { self.0 += 1; }
        }

        for opcode in [0xdb, 0xd3, 0x76, 0xfb, 0xf3] {
            let mut cpu = Cpu8080Cycle::new();
            let mut bus = CountingBus(0);
            assert_eq!(cpu.execute_full_instruction(&mut bus, opcode), None);
            assert_eq!(bus.0, 0);
            assert_eq!(cpu.total_t_states(), 0);
        }
    }
}
