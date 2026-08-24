use super::state::Registers;

pub(super) const FLAG_C: u8 = 0x01;
pub(super) const FLAG_1: u8 = 0x02;
pub(super) const FLAG_P: u8 = 0x04;
pub(super) const FLAG_AC: u8 = 0x10;
pub(super) const FLAG_Z: u8 = 0x40;
pub(super) const FLAG_S: u8 = 0x80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AluOp {
    Add,
    Adc,
    Sub,
    Sbb,
    Ana,
    Xra,
    Ora,
    Cmp,
}

impl AluOp {
    pub(super) const fn from_code(code: u8) -> Self {
        match code & 7 {
            0 => Self::Add,
            1 => Self::Adc,
            2 => Self::Sub,
            3 => Self::Sbb,
            4 => Self::Ana,
            5 => Self::Xra,
            6 => Self::Ora,
            _ => Self::Cmp,
        }
    }
}

#[inline]
fn parity(value: u8) -> bool {
    value.count_ones() & 1 == 0
}

#[inline]
fn set_szp(registers: &mut Registers, value: u8) {
    registers.f &= !(FLAG_S | FLAG_Z | FLAG_P);
    if value & 0x80 != 0 {
        registers.f |= FLAG_S;
    }
    if value == 0 {
        registers.f |= FLAG_Z;
    }
    if parity(value) {
        registers.f |= FLAG_P;
    }
    registers.f |= FLAG_1;
}

pub(super) fn execute(registers: &mut Registers, op: AluOp, rhs: u8) {
    match op {
        AluOp::Add => add(registers, rhs, false),
        AluOp::Adc => add(registers, rhs, true),
        AluOp::Sub => sub(registers, rhs, false, true),
        AluOp::Sbb => sub(registers, rhs, true, true),
        AluOp::Ana => ana(registers, rhs),
        AluOp::Xra => xra(registers, rhs),
        AluOp::Ora => ora(registers, rhs),
        AluOp::Cmp => sub(registers, rhs, false, false),
    }
}

pub(super) fn inr(registers: &mut Registers, value: u8) -> u8 {
    let carry = registers.f & FLAG_C;
    let result = value.wrapping_add(1);
    registers.f &= !(FLAG_C | FLAG_AC);
    if value & 0x0f == 0x0f {
        registers.f |= FLAG_AC;
    }
    set_szp(registers, result);
    registers.f = (registers.f & !FLAG_C) | carry;
    result
}

pub(super) fn dcr(registers: &mut Registers, value: u8) -> u8 {
    let carry = registers.f & FLAG_C;
    let result = value.wrapping_sub(1);
    registers.f &= !(FLAG_C | FLAG_AC);
    // 8080 subtraction AC is the carry out of bit 3 from the internal
    // two's-complement addition, i.e. the inverse of a nibble borrow.
    if value & 0x0f != 0 {
        registers.f |= FLAG_AC;
    }
    set_szp(registers, result);
    registers.f = (registers.f & !FLAG_C) | carry;
    result
}

fn add(registers: &mut Registers, rhs: u8, with_carry: bool) {
    let carry = if with_carry && registers.f & FLAG_C != 0 { 1u16 } else { 0 };
    let lhs = registers.a;
    let sum = lhs as u16 + rhs as u16 + carry;
    let result = sum as u8;

    registers.f &= !(FLAG_C | FLAG_AC);
    if sum > 0xff {
        registers.f |= FLAG_C;
    }
    if ((lhs & 0x0f) as u16 + (rhs & 0x0f) as u16 + carry) > 0x0f {
        registers.f |= FLAG_AC;
    }
    registers.a = result;
    set_szp(registers, result);
}

fn sub(registers: &mut Registers, rhs: u8, with_borrow: bool, store: bool) {
    let borrow = if with_borrow && registers.f & FLAG_C != 0 { 1u16 } else { 0 };
    let lhs = registers.a;
    let rhs16 = rhs as u16 + borrow;
    let result = lhs.wrapping_sub(rhs).wrapping_sub(borrow as u8);

    registers.f &= !(FLAG_C | FLAG_AC);
    if (lhs as u16) < rhs16 {
        registers.f |= FLAG_C;
    }
    let low_rhs = (rhs & 0x0f) as u16 + borrow;
    if (lhs & 0x0f) as u16 >= low_rhs {
        registers.f |= FLAG_AC;
    }
    set_szp(registers, result);
    if store {
        registers.a = result;
    }
}

fn ana(registers: &mut Registers, rhs: u8) {
    let auxiliary_carry = (registers.a | rhs) & 0x08 != 0;
    registers.a &= rhs;
    registers.f &= !(FLAG_C | FLAG_AC);
    if auxiliary_carry {
        registers.f |= FLAG_AC;
    }
    let result = registers.a;
    set_szp(registers, result);
}

fn xra(registers: &mut Registers, rhs: u8) {
    registers.a ^= rhs;
    registers.f &= !(FLAG_C | FLAG_AC);
    let result = registers.a;
    set_szp(registers, result);
}

fn ora(registers: &mut Registers, rhs: u8) {
    registers.a |= rhs;
    registers.f &= !(FLAG_C | FLAG_AC);
    let result = registers.a;
    set_szp(registers, result);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtraction_aux_carry_uses_8080_internal_carry_polarity() {
        let mut r = Registers::default();
        r.a = 0x03;
        execute(&mut r, AluOp::Sub, 0x00);
        assert_eq!(r.a, 0x03);
        assert_ne!(r.f & FLAG_AC, 0);
        assert_eq!(r.f & FLAG_C, 0);

        r.a = 0x03;
        r.f = FLAG_1;
        execute(&mut r, AluOp::Sub, 0x04);
        assert_eq!(r.a, 0xff);
        assert_eq!(r.f & FLAG_AC, 0);
        assert_ne!(r.f & FLAG_C, 0);
    }

    #[test]
    fn cmp_changes_flags_but_not_accumulator() {
        let mut r = Registers::default();
        r.a = 0x03;
        execute(&mut r, AluOp::Cmp, 0x04);
        assert_eq!(r.a, 0x03);
        assert_ne!(r.f & FLAG_C, 0);
        assert_eq!(r.f & FLAG_AC, 0);
    }

    #[test]
    fn inr_and_dcr_preserve_carry() {
        let mut r = Registers::default();
        r.f = FLAG_1 | FLAG_C;
        assert_eq!(inr(&mut r, 0x0f), 0x10);
        assert_ne!(r.f & FLAG_C, 0);
        assert_ne!(r.f & FLAG_AC, 0);

        r.f = FLAG_1 | FLAG_C;
        assert_eq!(dcr(&mut r, 0x10), 0x0f);
        assert_ne!(r.f & FLAG_C, 0);
        assert_eq!(r.f & FLAG_AC, 0);
    }

    #[test]
    fn ana_matches_8080_auxiliary_carry_rule() {
        let mut r = Registers::default();
        r.a = 0x08;
        execute(&mut r, AluOp::Ana, 0x00);
        assert_eq!(r.a, 0x00);
        assert_ne!(r.f & FLAG_AC, 0);
        assert_eq!(r.f & FLAG_C, 0);
    }
}
