use super::{SimhSession, SimhSessionError};
use crate::backend::{CpuState, Intel8080State, Z80State};

/// CPU personality selected inside Open-SIMH `altairz80`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AltairZ80CpuMode {
    Intel8080,
    #[default]
    Z80,
}

impl AltairZ80CpuMode {
    /// Exact Open-SIMH monitor modifier (`SET CPU ...`).
    pub const fn simh_modifier(self) -> &'static str {
        match self {
            Self::Intel8080 => "8080",
            Self::Z80 => "Z80",
        }
    }
}

/// Snapshot of the 8080/Z80-visible register bank exported by `altairz80`.
/// Register names come directly from `AltairZ80/altairz80_cpu.c::cpu_reg[]`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AltairZ80Registers {
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub pc: u16,
    pub sp: u16,
    pub ix: u16,
    pub iy: u16,
    pub af_alt: u16,
    pub bc_alt: u16,
    pub de_alt: u16,
    pub hl_alt: u16,
    pub iff: u8,
    pub interrupt_mode: u8,
    pub ir: u16,
    /// AltairZ80 exposes only an 8-bit SR pseudo-register.
    pub switch_register_low: u8,
    /// Instruction-model T-state accounting exported by SIMH. This is useful
    /// for diagnostics but does not imply pin/T-state-accurate simulation.
    pub total_t_states: u64,
}

impl AltairZ80Registers {
    pub fn read(session: &SimhSession) -> Result<Self, SimhSessionError> {
        Ok(Self {
            af: read_u16(session, "AF")?,
            bc: read_u16(session, "BC")?,
            de: read_u16(session, "DE")?,
            hl: read_u16(session, "HL")?,
            pc: read_u16(session, "PC")?,
            sp: read_u16(session, "SP")?,
            ix: read_u16(session, "IX")?,
            iy: read_u16(session, "IY")?,
            af_alt: read_u16(session, "AF1")?,
            bc_alt: read_u16(session, "BC1")?,
            de_alt: read_u16(session, "DE1")?,
            hl_alt: read_u16(session, "HL1")?,
            iff: read_u8(session, "IFF")?,
            interrupt_mode: read_u8(session, "IM")?,
            ir: read_u16(session, "IR")?,
            switch_register_low: read_u8(session, "SR")?,
            total_t_states: u64::from(session.examine_register_u32("TSTATES")?),
        })
    }

    pub const fn accumulator(self) -> u8 { (self.af >> 8) as u8 }
    pub const fn flags(self) -> u8 { self.af as u8 }

    pub const fn to_cpu_state(self, mode: AltairZ80CpuMode) -> CpuState {
        match mode {
            AltairZ80CpuMode::Intel8080 => {
                let flags = (self.flags() & 0xd5) | 0x02;
                CpuState::Intel8080(Intel8080State {
                    a: self.accumulator(),
                    b: (self.bc >> 8) as u8,
                    c: self.bc as u8,
                    d: (self.de >> 8) as u8,
                    e: self.de as u8,
                    h: (self.hl >> 8) as u8,
                    l: self.hl as u8,
                    flags,
                    pc: self.pc,
                    sp: self.sp,
                    inte: self.iff & 0x01 != 0,
                    halted: None,
                    total_t_states: Some(self.total_t_states),
                })
            }
            AltairZ80CpuMode::Z80 => CpuState::Z80(Z80State {
                a: self.accumulator(),
                flags: self.flags(),
                bc: self.bc,
                de: self.de,
                hl: self.hl,
                pc: self.pc,
                sp: self.sp,
                ix: self.ix,
                iy: self.iy,
                af_alt: self.af_alt,
                bc_alt: self.bc_alt,
                de_alt: self.de_alt,
                hl_alt: self.hl_alt,
                iff: self.iff,
                interrupt_mode: self.interrupt_mode,
                ir: self.ir,
                halted: None,
                total_t_states: Some(self.total_t_states),
            }),
        }
    }
}

pub fn set_altairz80_switch_register_low(
    session: &mut SimhSession,
    value: u8,
) -> Result<(), SimhSessionError> {
    session.deposit_register_u32("SR", u32::from(value))
}

fn read_u16(session: &SimhSession, name: &str) -> Result<u16, SimhSessionError> {
    Ok(session.examine_register_u32(name)? as u16)
}

fn read_u8(session: &SimhSession, name: &str) -> Result<u8, SimhSessionError> {
    Ok(session.examine_register_u32(name)? as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AltairZ80Registers {
        AltairZ80Registers {
            af: 0x12d7,
            bc: 0x3456,
            de: 0x789a,
            hl: 0xbcde,
            pc: 0x1020,
            sp: 0x3040,
            ix: 0x5060,
            iy: 0x7080,
            af_alt: 0x90a0,
            bc_alt: 0xb0c0,
            de_alt: 0xd0e0,
            hl_alt: 0xf001,
            iff: 0x01,
            interrupt_mode: 2,
            ir: 0x2233,
            switch_register_low: 0xa5,
            total_t_states: 123_456,
        }
    }

    #[test]
    fn z80_snapshot_preserves_extended_registers_and_tstates() {
        let CpuState::Z80(state) = sample().to_cpu_state(AltairZ80CpuMode::Z80) else {
            panic!("expected Z80 snapshot")
        };
        assert_eq!(state.a, 0x12);
        assert_eq!(state.flags, 0xd7);
        assert_eq!(state.ix, 0x5060);
        assert_eq!(state.iy, 0x7080);
        assert_eq!(state.af_alt, 0x90a0);
        assert_eq!(state.total_t_states, Some(123_456));
    }

    #[test]
    fn 8080_mode_normalizes_psw_and_iff() {
        let CpuState::Intel8080(state) = sample().to_cpu_state(AltairZ80CpuMode::Intel8080) else {
            panic!("expected 8080 snapshot")
        };
        assert_eq!(state.a, 0x12);
        assert_eq!(state.bc(), 0x3456);
        assert_eq!(state.flags, 0xd7);
        assert!(state.inte);
        assert_eq!(state.total_t_states, Some(123_456));
    }
}
