use super::{SimhSession, SimhSessionError, altair_registers};

/// Programmer-visible state exported by the classic Open SIMH `ALTAIR` CPU.
///
/// This deliberately mirrors what SIMH actually exposes. It does not invent a
/// T-state counter or a CPU-HLT latch: those are different questions from the
/// FrontPanel connection's Run/Halt operational state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClassicAltairRegisters {
    pub pc: u16,
    pub a: u8,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    pub carry: bool,
    pub zero: bool,
    pub aux_carry: bool,
    pub sign: bool,
    pub parity: bool,
    pub inte: bool,
    pub switch_register: u16,
}

impl ClassicAltairRegisters {
    pub fn read(session: &SimhSession) -> Result<Self, SimhSessionError> {
        Ok(Self {
            pc: read_u16(session, altair_registers::PC)?,
            a: read_u8(session, altair_registers::A)?,
            bc: read_u16(session, altair_registers::BC)?,
            de: read_u16(session, altair_registers::DE)?,
            hl: read_u16(session, altair_registers::HL)?,
            sp: read_u16(session, altair_registers::SP)?,
            carry: read_bool(session, altair_registers::CARRY)?,
            zero: read_bool(session, altair_registers::ZERO)?,
            aux_carry: read_bool(session, altair_registers::AUX_CARRY)?,
            sign: read_bool(session, altair_registers::SIGN)?,
            parity: read_bool(session, altair_registers::PARITY)?,
            inte: read_bool(session, altair_registers::INTE)?,
            switch_register: read_u16(session, altair_registers::SWITCH_REGISTER)?,
        })
    }

    /// Reconstruct the conventional Intel 8080 PSW flag byte used by RusTair.
    pub const fn flags_8080(self) -> u8 {
        let mut flags = 0x02;
        if self.carry { flags |= 0x01; }
        if self.parity { flags |= 0x04; }
        if self.aux_carry { flags |= 0x10; }
        if self.zero { flags |= 0x40; }
        if self.sign { flags |= 0x80; }
        flags
    }
}

pub fn set_switch_register(
    session: &mut SimhSession,
    value: u16,
) -> Result<(), SimhSessionError> {
    session.deposit_register_u32(altair_registers::SWITCH_REGISTER, u32::from(value))
}

fn read_u16(session: &SimhSession, name: &str) -> Result<u16, SimhSessionError> {
    Ok(session.examine_register_u32(name)? as u16)
}

fn read_u8(session: &SimhSession, name: &str) -> Result<u8, SimhSessionError> {
    Ok(session.examine_register_u32(name)? as u8)
}

fn read_bool(session: &SimhSession, name: &str) -> Result<bool, SimhSessionError> {
    Ok(session.examine_register_u32(name)? != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_byte_matches_rustair_8080_layout() {
        let registers = ClassicAltairRegisters {
            carry: true,
            parity: true,
            aux_carry: true,
            zero: true,
            sign: true,
            ..ClassicAltairRegisters::default()
        };
        assert_eq!(registers.flags_8080(), 0xd7);
    }

    #[test]
    fn reserved_psw_bit_one_is_always_set() {
        assert_eq!(ClassicAltairRegisters::default().flags_8080(), 0x02);
    }
}
