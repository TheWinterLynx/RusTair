#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Instruction {
    Nop,
    MviAImmediate,
    StaDirect,
    Unsupported(u8),
}

pub(super) const fn decode(opcode: u8) -> Instruction {
    match opcode {
        0x00 => Instruction::Nop,
        0x32 => Instruction::StaDirect,
        0x3e => Instruction::MviAImmediate,
        opcode => Instruction::Unsupported(opcode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_two_decodes_nop_mvi_a_and_sta() {
        assert_eq!(decode(0x00), Instruction::Nop);
        assert_eq!(decode(0x32), Instruction::StaDirect);
        assert_eq!(decode(0x3e), Instruction::MviAImmediate);
        assert_eq!(decode(0xff), Instruction::Unsupported(0xff));
    }
}
