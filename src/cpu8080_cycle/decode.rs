#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Instruction {
    Nop,
    Unsupported(u8),
}

pub(super) const fn decode(opcode: u8) -> Instruction {
    match opcode {
        0x00 => Instruction::Nop,
        opcode => Instruction::Unsupported(opcode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_one_decodes_only_nop() {
        assert_eq!(decode(0x00), Instruction::Nop);
        assert_eq!(decode(0x3e), Instruction::Unsupported(0x3e));
    }
}
