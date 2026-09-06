use super::alu::AluOp;
use super::control_flow::{Condition, StackPair};

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
            6 => None,
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
    Hlt,
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
    InrRegister(Register8),
    InrMemory,
    DcrRegister(Register8),
    DcrMemory,
    AluRegister { op: AluOp, src: Register8 },
    AluMemory { op: AluOp },
    AluImmediate { op: AluOp },
    Jump,
    JumpConditional(Condition),
    Call,
    CallConditional(Condition),
    Ret,
    RetConditional(Condition),
    Rst(u8),
    Push(StackPair),
    Pop(StackPair),
    In,
    Out,
    Xthl,
    Pchl,
    Xchg,
    Di,
    Sphl,
    Ei,
    Unsupported(u8),
}

/// Canonical decoder used only while building the compile-time 256-entry table.
/// Keeping the existing predicate implementation here makes the table a pure
/// representation change: there is still exactly one source of 8080 opcode
/// semantics to audit.
const fn decode_canonical(opcode: u8) -> Instruction {
    match opcode {
        // The NMOS 8080 silicon treats these seven undocumented holes as NOP.
        0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => {
            return Instruction::Nop;
        }
        0x02 => return Instruction::Stax(RegisterPair::BC),
        0x07 => return Instruction::AluRegister { op: AluOp::Rlc, src: Register8::A },
        0x0a => return Instruction::Ldax(RegisterPair::BC),
        0x0f => return Instruction::AluRegister { op: AluOp::Rrc, src: Register8::A },
        0x12 => return Instruction::Stax(RegisterPair::DE),
        0x17 => return Instruction::AluRegister { op: AluOp::Ral, src: Register8::A },
        0x1a => return Instruction::Ldax(RegisterPair::DE),
        0x1f => return Instruction::AluRegister { op: AluOp::Rar, src: Register8::A },
        0x22 => return Instruction::ShldDirect,
        0x27 => return Instruction::AluRegister { op: AluOp::Daa, src: Register8::A },
        0x2a => return Instruction::LhldDirect,
        0x2f => return Instruction::AluRegister { op: AluOp::Cma, src: Register8::A },
        0x32 => return Instruction::StaDirect,
        0x37 => return Instruction::AluRegister { op: AluOp::Stc, src: Register8::A },
        0x3a => return Instruction::LdaDirect,
        0x3f => return Instruction::AluRegister { op: AluOp::Cmc, src: Register8::A },
        0x76 => return Instruction::Hlt,
        // Undocumented aliases present on original 8080 silicon and already
        // reproduced by RusTair's validated fast core.
        0xc3 | 0xcb => return Instruction::Jump,
        0xc6 => return Instruction::AluImmediate { op: AluOp::Add },
        0xc9 | 0xd9 => return Instruction::Ret,
        0xcd | 0xdd | 0xed | 0xfd => return Instruction::Call,
        0xce => return Instruction::AluImmediate { op: AluOp::Adc },
        0xd3 => return Instruction::Out,
        0xd6 => return Instruction::AluImmediate { op: AluOp::Sub },
        0xdb => return Instruction::In,
        0xde => return Instruction::AluImmediate { op: AluOp::Sbb },
        0xe3 => return Instruction::Xthl,
        0xe6 => return Instruction::AluImmediate { op: AluOp::Ana },
        0xe9 => return Instruction::Pchl,
        0xeb => return Instruction::Xchg,
        0xee => return Instruction::AluImmediate { op: AluOp::Xra },
        0xf3 => return Instruction::Di,
        0xf6 => return Instruction::AluImmediate { op: AluOp::Ora },
        0xf9 => return Instruction::Sphl,
        0xfb => return Instruction::Ei,
        0xfe => return Instruction::AluImmediate { op: AluOp::Cmp },
        _ => {}
    }

    if opcode & 0xcf == 0x01 {
        return Instruction::Lxi(RegisterPair::from_code((opcode >> 4) & 0x03));
    }
    if opcode & 0xcf == 0x03 {
        return Instruction::Inx(RegisterPair::from_code((opcode >> 4) & 0x03));
    }
    if opcode & 0xcf == 0x0b {
        return Instruction::Dcx(RegisterPair::from_code((opcode >> 4) & 0x03));
    }
    if opcode & 0xcf == 0x09 {
        return Instruction::Dad(RegisterPair::from_code((opcode >> 4) & 0x03));
    }

    if opcode & 0xc7 == 0x04 {
        return match Register8::from_code((opcode >> 3) & 0x07) {
            Some(dst) => Instruction::InrRegister(dst),
            None => Instruction::InrMemory,
        };
    }
    if opcode & 0xc7 == 0x05 {
        return match Register8::from_code((opcode >> 3) & 0x07) {
            Some(dst) => Instruction::DcrRegister(dst),
            None => Instruction::DcrMemory,
        };
    }
    if opcode & 0xc7 == 0x06 {
        return match Register8::from_code((opcode >> 3) & 0x07) {
            Some(dst) => Instruction::MviImmediate(dst),
            None => Instruction::MviMemory,
        };
    }

    if opcode >= 0x40 && opcode <= 0x7f {
        let dst = Register8::from_code((opcode >> 3) & 0x07);
        let src = Register8::from_code(opcode & 0x07);
        return match (dst, src) {
            (Some(dst), Some(src)) => Instruction::MovRegister { dst, src },
            (Some(dst), None) => Instruction::MovFromMemory { dst },
            (None, Some(src)) => Instruction::MovToMemory { src },
            (None, None) => Instruction::Unsupported(opcode),
        };
    }

    if opcode >= 0x80 && opcode <= 0xbf {
        let op = AluOp::from_code((opcode >> 3) & 0x07);
        return match Register8::from_code(opcode & 0x07) {
            Some(src) => Instruction::AluRegister { op, src },
            None => Instruction::AluMemory { op },
        };
    }

    if opcode & 0xc7 == 0xc0 {
        return Instruction::RetConditional(Condition::from_code((opcode >> 3) & 7));
    }
    if opcode & 0xc7 == 0xc2 {
        return Instruction::JumpConditional(Condition::from_code((opcode >> 3) & 7));
    }
    if opcode & 0xc7 == 0xc4 {
        return Instruction::CallConditional(Condition::from_code((opcode >> 3) & 7));
    }
    if opcode & 0xc7 == 0xc7 {
        return Instruction::Rst((opcode >> 3) & 7);
    }
    if opcode & 0xcf == 0xc1 {
        return Instruction::Pop(StackPair::from_code((opcode >> 4) & 3));
    }
    if opcode & 0xcf == 0xc5 {
        return Instruction::Push(StackPair::from_code((opcode >> 4) & 3));
    }

    Instruction::Unsupported(opcode)
}

const fn build_decode_table() -> [Instruction; 256] {
    let mut table = [Instruction::Nop; 256];
    let mut index = 0usize;
    while index < table.len() {
        table[index] = decode_canonical(index as u8);
        index += 1;
    }
    table
}

const DECODE_TABLE: [Instruction; 256] = build_decode_table();

#[inline(always)]
pub(super) const fn decode(opcode: u8) -> Instruction {
    DECODE_TABLE[opcode as usize]
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
    fn decodes_accumulator_only_operations_on_the_four_t_state_alu_path() {
        for (opcode, op) in [
            (0x07, AluOp::Rlc), (0x0f, AluOp::Rrc), (0x17, AluOp::Ral),
            (0x1f, AluOp::Rar), (0x27, AluOp::Daa), (0x2f, AluOp::Cma),
            (0x37, AluOp::Stc), (0x3f, AluOp::Cmc),
        ] {
            assert_eq!(decode(opcode), Instruction::AluRegister { op, src: Register8::A });
        }
    }

    #[test]
    fn decodes_inr_dcr_and_alu_families() {
        assert_eq!(decode(0x04), Instruction::InrRegister(Register8::B));
        assert_eq!(decode(0x34), Instruction::InrMemory);
        assert_eq!(decode(0x3d), Instruction::DcrRegister(Register8::A));
        assert_eq!(decode(0x35), Instruction::DcrMemory);

        for alu_code in 0u8..8 {
            let op = AluOp::from_code(alu_code);
            let base = 0x80 | (alu_code << 3);
            assert_eq!(decode(base), Instruction::AluRegister { op, src: Register8::B });
            assert_eq!(decode(base | 6), Instruction::AluMemory { op });
        }
    }

    #[test]
    fn decodes_jumps_calls_returns_restarts_and_stack_pairs() {
        assert_eq!(decode(0xc3), Instruction::Jump);
        assert_eq!(decode(0xcd), Instruction::Call);
        assert_eq!(decode(0xc9), Instruction::Ret);

        for code in 0u8..8 {
            let condition = Condition::from_code(code);
            assert_eq!(decode(0xc2 | (code << 3)), Instruction::JumpConditional(condition));
            assert_eq!(decode(0xc4 | (code << 3)), Instruction::CallConditional(condition));
            assert_eq!(decode(0xc0 | (code << 3)), Instruction::RetConditional(condition));
            assert_eq!(decode(0xc7 | (code << 3)), Instruction::Rst(code));
        }
        for code in 0u8..4 {
            let pair = StackPair::from_code(code);
            assert_eq!(decode(0xc1 | (code << 4)), Instruction::Pop(pair));
            assert_eq!(decode(0xc5 | (code << 4)), Instruction::Push(pair));
        }
    }

    #[test]
    fn decodes_io_special_transfer_and_control_instructions() {
        assert_eq!(decode(0x76), Instruction::Hlt);
        assert_eq!(decode(0xd3), Instruction::Out);
        assert_eq!(decode(0xdb), Instruction::In);
        assert_eq!(decode(0xe3), Instruction::Xthl);
        assert_eq!(decode(0xe9), Instruction::Pchl);
        assert_eq!(decode(0xeb), Instruction::Xchg);
        assert_eq!(decode(0xf3), Instruction::Di);
        assert_eq!(decode(0xf9), Instruction::Sphl);
        assert_eq!(decode(0xfb), Instruction::Ei);
    }

    #[test]
    fn undocumented_silicon_aliases_match_the_validated_fast_core() {
        for opcode in [0x08, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38] {
            assert_eq!(decode(opcode), Instruction::Nop);
        }
        assert_eq!(decode(0xcb), Instruction::Jump);
        assert_eq!(decode(0xd9), Instruction::Ret);
        for opcode in [0xdd, 0xed, 0xfd] {
            assert_eq!(decode(opcode), Instruction::Call);
        }
    }

    #[test]
    fn all_256_opcode_values_have_8080_silicon_behavior() {
        for opcode in 0u16..=0xff {
            assert!(
                !matches!(decode(opcode as u8), Instruction::Unsupported(_)),
                "opcode {:02x} has no cycle-core behavior",
                opcode
            );
        }
    }

    #[test]
    fn transfer_paths_are_preserved() {
        assert_eq!(decode(0x02), Instruction::Stax(RegisterPair::BC));
        assert_eq!(decode(0x22), Instruction::ShldDirect);
        assert_eq!(decode(0x36), Instruction::MviMemory);
        assert_eq!(decode(0x46), Instruction::MovFromMemory { dst: Register8::B });
        assert_eq!(decode(0x70), Instruction::MovToMemory { src: Register8::B });
    }
}
