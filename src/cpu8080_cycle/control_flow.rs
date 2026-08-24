use super::alu::{FLAG_1, FLAG_C, FLAG_P, FLAG_S, FLAG_Z};
use super::state::Registers;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Condition {
    NZ,
    Z,
    NC,
    C,
    PO,
    PE,
    P,
    M,
}

impl Condition {
    pub(super) const fn from_code(code: u8) -> Self {
        match code & 7 {
            0 => Self::NZ,
            1 => Self::Z,
            2 => Self::NC,
            3 => Self::C,
            4 => Self::PO,
            5 => Self::PE,
            6 => Self::P,
            _ => Self::M,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StackPair {
    BC,
    DE,
    HL,
    PSW,
}

impl StackPair {
    pub(super) const fn from_code(code: u8) -> Self {
        match code & 3 {
            0 => Self::BC,
            1 => Self::DE,
            2 => Self::HL,
            _ => Self::PSW,
        }
    }
}

#[inline]
pub(super) fn condition(flags: u8, condition: Condition) -> bool {
    match condition {
        Condition::NZ => flags & FLAG_Z == 0,
        Condition::Z => flags & FLAG_Z != 0,
        Condition::NC => flags & FLAG_C == 0,
        Condition::C => flags & FLAG_C != 0,
        Condition::PO => flags & FLAG_P == 0,
        Condition::PE => flags & FLAG_P != 0,
        Condition::P => flags & FLAG_S == 0,
        Condition::M => flags & FLAG_S != 0,
    }
}

pub(super) fn read_stack_pair(registers: &Registers, pair: StackPair) -> u16 {
    match pair {
        StackPair::BC => u16::from_be_bytes([registers.b, registers.c]),
        StackPair::DE => u16::from_be_bytes([registers.d, registers.e]),
        StackPair::HL => u16::from_be_bytes([registers.h, registers.l]),
        StackPair::PSW => {
            let flags = (registers.f & 0xd5) | FLAG_1;
            u16::from_be_bytes([registers.a, flags])
        }
    }
}

pub(super) fn write_stack_pair(registers: &mut Registers, pair: StackPair, value: u16) {
    let [high, low] = value.to_be_bytes();
    match pair {
        StackPair::BC => {
            registers.b = high;
            registers.c = low;
        }
        StackPair::DE => {
            registers.d = high;
            registers.e = low;
        }
        StackPair::HL => {
            registers.h = high;
            registers.l = low;
        }
        StackPair::PSW => {
            registers.a = high;
            registers.f = (low & 0xd5) | FLAG_1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eight_condition_codes_match_8080_flags() {
        let flags = FLAG_Z | FLAG_C | FLAG_P | FLAG_S | FLAG_1;
        assert!(!condition(flags, Condition::NZ));
        assert!(condition(flags, Condition::Z));
        assert!(!condition(flags, Condition::NC));
        assert!(condition(flags, Condition::C));
        assert!(!condition(flags, Condition::PO));
        assert!(condition(flags, Condition::PE));
        assert!(!condition(flags, Condition::P));
        assert!(condition(flags, Condition::M));
    }

    #[test]
    fn psw_pack_and_unpack_normalize_unused_flag_bits() {
        let mut r = Registers::default();
        r.a = 0xa5;
        r.f = 0xff;
        assert_eq!(read_stack_pair(&r, StackPair::PSW), 0xa5d7);

        write_stack_pair(&mut r, StackPair::PSW, 0x5aff);
        assert_eq!(r.a, 0x5a);
        assert_eq!(r.f, 0xd7);
    }
}
