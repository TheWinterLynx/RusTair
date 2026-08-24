#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Register8 {
    B,
    C,
    D,
    E,
    H,
    L,
    A,
}

impl Register8 {
    pub(super) const fn from_code(code: u8) -> Option<Self> {
        match code & 0x07 {
            0 => Some(Self::B),
            1 => Some(Self::C),
            2 => Some(Self::D),
            3 => Some(Self::E),
            4 => Some(Self::H),
            5 => Some(Self::L),
            6 => None, // M: memory addressed through HL.
            7 => Some(Self::A),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RegisterPair {
    BC,
    DE,
    HL,
    SP,
}

impl RegisterPair {
    pub(super) const fn from_code(code: u8) -> Self {
        match code & 0x03 {
            0 => Self::BC,
            1 => Self::DE,
            2 => Self::HL,
            _ => Self::SP,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Instruction {
    Nop,
    MviImmediate(Register8),
    MviMemory,
    MovRegister { dst: Register8, src: Register8 },
    MovFromMemory { dst: Register8 },
    MovToMemory { src: Register8 },
    Lxi(RegisterPair),
    Inx(RegisterPair),
    Dcx(RegisterPair),
    Dad(RegisterPair),
    Ldax(RegisterPair),
    Stax(RegisterPair),
    LdaDirect,
    StaDirect,
    LhldDirect,
    ShldDirect,
    Unsupported(u8),
}

pub(super) const fn decode(opcode: u8) -> Instruction {
    match opcode {
        0x00 => return Instruction::Nop,
        0x02 => return Instruction::Stax(RegisterPair::BC),
        0x0a => return Instruction::Ldax(RegisterPair::BC),
        0x12 => return Instruction::Stax(RegisterPair::DE),
        0x1a => return Instruction::Ldax(RegisterPair::DE),
        0x22 => return Instruction::ShldDirect,
        0x2a => return Instruction::LhldDirect,
        0x32 => return Instruction::StaDirect,
        0x3a => return Instruction::LdaDirect,
        _ => {}
    }

    // LXI rp,d16: 00RP0001.
    if opcode & 0xcf == 0x01 {
        return Instruction::Lxi(RegisterPair::from_code((opcode >> 4) & 0x03));
    }

    // INX rp / DCX rp / DAD rp.
    if opcode & 0xcf == 0x03 {
        return Instruction::Inx(RegisterPair::from_code((opcode >> 4) & 0x03));
    }
    if opcode & 0xcf == 0x0b {
        return Instruction::Dcx(RegisterPair::from_code((opcode >> 4) & 0x03));
    }
    if opcode & 0xcf == 0x09 {
        return Instruction::Dad(RegisterPair::from_code((opcode >> 4) & 0x03));
    }

    // MVI r,d8 / MVI M,d8: 00DDD110.
    if opcode & 0xc7 == 0x06 {
        return match Register8::from_code((opcode >> 3) & 0x07) {
            Some(dst) => Instruction::MviImmediate(dst),
            None => Instruction::MviMemory,
        };
    }

    // MOV d,s: 01DDDSSS. 76h is HLT, not MOV M,M.
    if opcode >= 0x40 && opcode <= 0x7f {
        if opcode == 0x76 {
            return Instruction::Unsupported(opcode);
        }
        let dst = Register8::from_code((opcode >> 3) & 0x07);
        let src = Register8::from_code(opcode & 0x07);
        return match (dst, src) {
            (Some(dst), Some(src)) => Instruction::MovRegister { dst, src },
            (Some(dst), None) => Instruction::MovFromMemory { dst },
            (None, Some(src)) => Instruction::MovToMemory { src },
            (None, None) => Instruction::Unsupported(opcode),
        };
    }

    Instruction::Unsupported(opcode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_register_pair_families() {
        let pairs = [RegisterPair::BC, RegisterPair::DE, RegisterPair::HL, RegisterPair::SP];
        for (index, pair) in pairs.into_iter().enumerate() {
            let base = (index as u8) << 4;
            assert_eq!(decode(base | 0x01), Instruction::Lxi(pair));
            assert_eq!(decode(base | 0x03), Instruction::Inx(pair));
            assert_eq!(decode(base | 0x0b), Instruction::Dcx(pair));
            assert_eq!(decode(base | 0x09), Instruction::Dad(pair));
        }
    }

    #[test]
    fn decodes_indirect_and_direct_transfer_family() {
        assert_eq!(decode(0x02), Instruction::Stax(RegisterPair::BC));
        assert_eq!(decode(0x12), Instruction::Stax(RegisterPair::DE));
        assert_eq!(decode(0x0a), Instruction::Ldax(RegisterPair::BC));
        assert_eq!(decode(0x1a), Instruction::Ldax(RegisterPair::DE));
        assert_eq!(decode(0x22), Instruction::ShldDirect);
        assert_eq!(decode(0x2a), Instruction::LhldDirect);
        assert_eq!(decode(0x32), Instruction::StaDirect);
        assert_eq!(decode(0x3a), Instruction::LdaDirect);
    }

    #[test]
    fn mov_and_mvi_memory_forms_remain_supported_but_hlt_does_not() {
        assert_eq!(decode(0x36), Instruction::MviMemory);
        assert_eq!(decode(0x46), Instruction::MovFromMemory { dst: Register8::B });
        assert_eq!(decode(0x70), Instruction::MovToMemory { src: Register8::B });
        assert_eq!(decode(0x76), Instruction::Unsupported(0x76));
    }

    #[test]
    fn unrelated_opcode_remains_explicitly_unsupported() {
        assert_eq!(decode(0xff), Instruction::Unsupported(0xff));
    }
}
