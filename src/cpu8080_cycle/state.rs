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
            // Intel 8080 PSW bit 1 is conventionally held high.
            f: 0x02,
            sp: 0,
            pc: 0,
        }
    }
}

impl super::Cpu8080Cycle {
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
}
