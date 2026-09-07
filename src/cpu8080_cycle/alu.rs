use super::state::Registers;

pub(super) const FLAG_C: u8 = 0x01;
pub(super) const FLAG_1: u8 = 0x02;
pub(super) const FLAG_P: u8 = 0x04;
pub(super) const FLAG_AC: u8 = 0x10;
pub(super) const FLAG_Z: u8 = 0x40;
pub(super) const FLAG_S: u8 = 0x80;

const fn build_szp_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let value = i as u8;
        let mut flags = FLAG_1;
        if value & 0x80 != 0 {
            flags |= FLAG_S;
        }
        if value == 0 {
            flags |= FLAG_Z;
        }
        if value.count_ones() & 1 == 0 {
            flags |= FLAG_P;
        }
        table[i] = flags;
        i += 1;
    }
    table
}

// MAME's mature 8080/8085 core uses a 256-entry Z/S/P lookup table so the
// arithmetic hot path does not repeatedly branch and count parity bits. Keep
// the same idea here, generated at compile time from the 8080 flag definition.
const SZP_TABLE: [u8; 256] = build_szp_table();

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
    Rlc,
    Rrc,
    Ral,
    Rar,
    Daa,
    Cma,
    Stc,
    Cmc,
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
fn set_szp(registers: &mut Registers, value: u8) {
    registers.f = (registers.f & !(FLAG_S | FLAG_Z | FLAG_P)) | SZP_TABLE[value as usize];
}

/// Execute an 8080 ALU/internal accumulator operation.
///
/// `rhs` is used by binary ALU operations and ignored by the eight accumulator-
/// only operations. Keeping them in the same dispatch path is deliberate: all
/// eight complete during M1/T4 just like register ALU operations, with no extra
/// external machine cycle.
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
        AluOp::Rlc => rlc(registers),
        AluOp::Rrc => rrc(registers),
        AluOp::Ral => ral(registers),
        AluOp::Rar => rar(registers),
        AluOp::Daa => daa(registers),
        AluOp::Cma => registers.a = !registers.a,
        AluOp::Stc => registers.f |= FLAG_C,
        AluOp::Cmc => registers.f ^= FLAG_C,
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

fn rlc(registers: &mut Registers) {
    let carry = registers.a >> 7;
    registers.a = registers.a.rotate_left(1);
    registers.f = (registers.f & !FLAG_C) | carry;
}

fn rrc(registers: &mut Registers) {
    let carry = registers.a & 1;
    registers.a = registers.a.rotate_right(1);
    registers.f = (registers.f & !FLAG_C) | carry;
}

fn ral(registers: &mut Registers) {
    let old_carry = if registers.f & FLAG_C != 0 { 1 } else { 0 };
    let new_carry = registers.a >> 7;
    registers.a = (registers.a << 1) | old_carry;
    registers.f = (registers.f & !FLAG_C) | new_carry;
}

fn rar(registers: &mut Registers) {
    let old_carry = if registers.f & FLAG_C != 0 { 0x80 } else { 0 };
    let new_carry = registers.a & 1;
    registers.a = (registers.a >> 1) | old_carry;
    registers.f = (registers.f & !FLAG_C) | new_carry;
}

fn daa(registers: &mut Registers) {
    // Keep this algorithm identical to the already validated production core.
    let old_a = registers.a;
    let old_carry = registers.f & FLAG_C != 0;
    let old_aux = registers.f & FLAG_AC != 0;
    let mut correction = 0u8;
    let mut carry = old_carry;

    if (old_a & 0x0f) > 9 || old_aux {
        correction |= 0x06;
    }
    if old_a > 0x99 || old_carry {
        correction |= 0x60;
        carry = true;
    }

    let result = old_a.wrapping_add(correction);
    registers.f &= !(FLAG_C | FLAG_AC);
    if carry {
        registers.f |= FLAG_C;
    }
    if ((old_a & 0x0f) + (correction & 0x0f)) > 0x0f {
        registers.f |= FLAG_AC;
    }
    registers.a = result;
    set_szp(registers, result);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn szp_table_matches_the_8080_flag_definition_for_all_values() {
        for value in 0u16..=0xff {
            let value = value as u8;
            let expected = FLAG_1
                | if value & 0x80 != 0 { FLAG_S } else { 0 }
                | if value == 0 { FLAG_Z } else { 0 }
                | if value.count_ones() & 1 == 0 { FLAG_P } else { 0 };
            assert_eq!(SZP_TABLE[value as usize], expected, "value {value:02x}");
        }
    }

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

    #[test]
    fn rotate_family_only_changes_accumulator_and_carry() {
        let preserved = FLAG_S | FLAG_Z | FLAG_P | FLAG_AC | FLAG_1;

        let mut r = Registers::default();
        r.a = 0x81;
        r.f = preserved;
        execute(&mut r, AluOp::Rlc, 0);
        assert_eq!(r.a, 0x03);
        assert_eq!(r.f, preserved | FLAG_C);

        r.a = 0x81;
        r.f = preserved;
        execute(&mut r, AluOp::Rrc, 0);
        assert_eq!(r.a, 0xc0);
        assert_eq!(r.f, preserved | FLAG_C);

        r.a = 0x80;
        r.f = preserved | FLAG_C;
        execute(&mut r, AluOp::Ral, 0);
        assert_eq!(r.a, 0x01);
        assert_eq!(r.f, preserved | FLAG_C);

        r.a = 0x01;
        r.f = preserved | FLAG_C;
        execute(&mut r, AluOp::Rar, 0);
        assert_eq!(r.a, 0x80);
        assert_eq!(r.f, preserved | FLAG_C);
    }

    #[test]
    fn cma_stc_and_cmc_leave_unrelated_flags_alone() {
        let preserved = FLAG_S | FLAG_Z | FLAG_P | FLAG_AC | FLAG_1;
        let mut r = Registers::default();
        r.a = 0x55;
        r.f = preserved;

        execute(&mut r, AluOp::Cma, 0);
        assert_eq!(r.a, 0xaa);
        assert_eq!(r.f, preserved);

        execute(&mut r, AluOp::Stc, 0);
        assert_eq!(r.f, preserved | FLAG_C);

        execute(&mut r, AluOp::Cmc, 0);
        assert_eq!(r.f, preserved);
    }

    #[test]
    fn daa_matches_validated_8080_bcd_adjust_cases() {
        let mut r = Registers::default();

        // 09h + 09h = 12h with AC, then DAA -> 18h.
        r.a = 0x12;
        r.f = FLAG_1 | FLAG_AC;
        execute(&mut r, AluOp::Daa, 0);
        assert_eq!(r.a, 0x18);
        assert_eq!(r.f & FLAG_C, 0);

        // 99h + 99h = 32h with carry and AC, DAA -> 98h with carry.
        r.a = 0x32;
        r.f = FLAG_1 | FLAG_C | FLAG_AC;
        execute(&mut r, AluOp::Daa, 0);
        assert_eq!(r.a, 0x98);
        assert_ne!(r.f & FLAG_C, 0);

        // Pure low-nibble correction.
        r.a = 0x0a;
        r.f = FLAG_1;
        execute(&mut r, AluOp::Daa, 0);
        assert_eq!(r.a, 0x10);
        assert_ne!(r.f & FLAG_AC, 0);
    }
}
