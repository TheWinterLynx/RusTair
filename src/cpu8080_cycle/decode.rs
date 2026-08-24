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
            6 => None, // M: memory addressed through HL, implemented next.
            7 => Some(Self::A),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Instruction {
    Nop,
    MviImmediate(Register8),
    MovRegister { dst: Register8, src: Register8 },
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

    // MVI r,d8: 00DDD110. DDD=110 is M and deliberately remains unsupported
    // until the HL-addressed memory form is added.
    if opcode & 0xc7 == 0x06 {
        return match Register8::from_code((opcode >> 3) & 0x07) {
            Some(dst) => Instruction::MviImmediate(dst),
            None => Instruction::Unsupported(opcode),
        };
    }

    // MOV d,s: 01DDDSSS. Any encoding containing M (code 110) requires a
    // memory cycle and is kept unsupported for this milestone. 76h is HLT and
    // therefore also excluded naturally because both operands encode M.
    if opcode >= 0x40 && opcode <= 0x7f {
        let dst = Register8::from_code((opcode >> 3) & 0x07);
        let src = Register8::from_code(opcode & 0x07);
        return match (dst, src) {
            (Some(dst), Some(src)) => Instruction::MovRegister { dst, src },
            _ => Instruction::Unsupported(opcode),
        };
    }

    Instruction::Unsupported(opcode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_three_decodes_all_register_mvi_forms() {
        let cases = [
            (0x06, Register8::B),
            (0x0e, Register8::C),
            (0x16, Register8::D),
            (0x1e, Register8::E),
            (0x26, Register8::H),
            (0x2e, Register8::L),
            (0x3e, Register8::A),
        ];

        for (opcode, register) in cases {
            assert_eq!(decode(opcode), Instruction::MviImmediate(register));
        }

        assert_eq!(decode(0x36), Instruction::Unsupported(0x36)); // MVI M,d8
    }

    #[test]
    fn milestone_three_decodes_all_register_to_register_mov_forms() {
        for dst_code in 0u8..8 {
            for src_code in 0u8..8 {
                let opcode = 0x40 | (dst_code << 3) | src_code;
                let dst = Register8::from_code(dst_code);
                let src = Register8::from_code(src_code);
                match (dst, src) {
                    (Some(dst), Some(src)) => {
                        assert_eq!(decode(opcode), Instruction::MovRegister { dst, src });
                    }
                    _ => assert_eq!(decode(opcode), Instruction::Unsupported(opcode)),
                }
            }
        }
    }

    #[test]
    fn existing_nop_and_sta_decodes_are_preserved() {
        assert_eq!(decode(0x00), Instruction::Nop);
        assert_eq!(decode(0x32), Instruction::StaDirect);
        assert_eq!(decode(0xff), Instruction::Unsupported(0xff));
    }
}
