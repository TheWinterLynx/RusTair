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
pub(super) enum Instruction {
    Nop,
    MviImmediate(Register8),
    MviMemory,
    MovRegister { dst: Register8, src: Register8 },
    MovFromMemory { dst: Register8 },
    MovToMemory { src: Register8 },
    StaDirect,
    Unsupported(u8),
}

pub(super) const fn decode(opcode: u8) -> Instruction {
    if opcode == 0x00 {
        return Instruction::Nop;
    }

    if opcode == 0x32 {
        return Instruction::StaDirect;
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
    fn milestone_four_decodes_all_mvi_forms_including_memory() {
        let cases = [
            (0x06, Some(Register8::B)),
            (0x0e, Some(Register8::C)),
            (0x16, Some(Register8::D)),
            (0x1e, Some(Register8::E)),
            (0x26, Some(Register8::H)),
            (0x2e, Some(Register8::L)),
            (0x36, None),
            (0x3e, Some(Register8::A)),
        ];

        for (opcode, register) in cases {
            match register {
                Some(register) => {
                    assert_eq!(decode(opcode), Instruction::MviImmediate(register));
                }
                None => assert_eq!(decode(opcode), Instruction::MviMemory),
            }
        }
    }

    #[test]
    fn milestone_four_decodes_register_and_hl_memory_mov_forms() {
        for dst_code in 0u8..8 {
            for src_code in 0u8..8 {
                let opcode = 0x40 | (dst_code << 3) | src_code;
                let dst = Register8::from_code(dst_code);
                let src = Register8::from_code(src_code);

                if opcode == 0x76 {
                    assert_eq!(decode(opcode), Instruction::Unsupported(0x76));
                    continue;
                }

                match (dst, src) {
                    (Some(dst), Some(src)) => {
                        assert_eq!(decode(opcode), Instruction::MovRegister { dst, src });
                    }
                    (Some(dst), None) => {
                        assert_eq!(decode(opcode), Instruction::MovFromMemory { dst });
                    }
                    (None, Some(src)) => {
                        assert_eq!(decode(opcode), Instruction::MovToMemory { src });
                    }
                    (None, None) => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn existing_nop_sta_and_unrelated_unsupported_decodes_are_preserved() {
        assert_eq!(decode(0x00), Instruction::Nop);
        assert_eq!(decode(0x32), Instruction::StaDirect);
        assert_eq!(decode(0x76), Instruction::Unsupported(0x76));
        assert_eq!(decode(0xff), Instruction::Unsupported(0xff));
    }
}
