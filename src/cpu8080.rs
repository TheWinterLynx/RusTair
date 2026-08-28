// Intel 8080 core for RusTair.
//
// This is a Rust port/re-expression of the behaviour used by the original
// `8080.js` bundled with the Ian Davies Altair simulator. The original core
// is Copyright (C) 2013, 2014 Martin Maly, based on BSD-licensed work by
// Chris Double. See THIRD_PARTY.md.

pub const FLAG_C: u8 = 0x01;
pub const FLAG_1: u8 = 0x02;
pub const FLAG_P: u8 = 0x04;
pub const FLAG_AC: u8 = 0x10;
pub const FLAG_Z: u8 = 0x40;
pub const FLAG_S: u8 = 0x80;

pub trait Bus {
    fn read(&mut self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);
    fn input(&mut self, _port: u8) -> u8 { 0xff }
    fn output(&mut self, _port: u8, _value: u8) {}
    fn set_inte(&mut self, _enabled: bool) {}

    fn opcode_fetch(&mut self, address: u16) -> u8 { self.read(address) }
    fn stack_read(&mut self, address: u16) -> u8 { self.read(address) }
    fn stack_write(&mut self, address: u16, value: u8) { self.write(address, value); }
    fn halt_ack(&mut self, _address: u16, _opcode: u8) {}
    fn interrupt_ack(&mut self, _address: u16, _opcode: u8, _while_halted: bool) {}
    fn instruction_complete(&mut self, _address: u16, _opcode: u8, _t_states: u32) {}
}

#[derive(Clone, Debug)]
pub struct Cpu8080 {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub f: u8,
    pub pc: u16,
    pub sp: u16,
    pub inte: bool,
    pub halted: bool,
    pub cycles: u64,
    ei_pending: bool,
}

impl Default for Cpu8080 {
    fn default() -> Self { Self::new() }
}

impl Cpu8080 {
    pub fn new() -> Self {
        Self {
            a: 0, b: 0, c: 0, d: 0, e: 0, h: 0, l: 0,
            f: FLAG_1,
            pc: 0,
            sp: 0xf000,
            inte: false,
            halted: false,
            cycles: 0,
            ei_pending: false,
        }
    }

    /// Apply the real Intel 8080 RESET semantics. RESET clears the program
    /// counter and interrupt/control state, but does not clear A, flags, the
    /// general registers, or SP. Keeping those values is important after the
    /// Altair's deliberately undefined/random power-on state.
    pub fn reset(&mut self) {
        self.pc = 0;
        self.inte = false;
        self.halted = false;
        self.ei_pending = false;
    }

    #[inline] pub fn af(&self) -> u16 { ((self.a as u16) << 8) | self.flags_for_stack() as u16 }
    #[inline] pub fn bc(&self) -> u16 { ((self.b as u16) << 8) | self.c as u16 }
    #[inline] pub fn de(&self) -> u16 { ((self.d as u16) << 8) | self.e as u16 }
    #[inline] pub fn hl(&self) -> u16 { ((self.h as u16) << 8) | self.l as u16 }
    #[inline] pub fn set_bc(&mut self, v: u16) { self.b = (v >> 8) as u8; self.c = v as u8; }
    #[inline] pub fn set_de(&mut self, v: u16) { self.d = (v >> 8) as u8; self.e = v as u8; }
    #[inline] pub fn set_hl(&mut self, v: u16) { self.h = (v >> 8) as u8; self.l = v as u8; }
    #[inline] fn set_af(&mut self, v: u16) { self.a = (v >> 8) as u8; self.f = (v as u8 & 0xd5) | FLAG_1; }

    #[inline]
    fn flags_for_stack(&self) -> u8 { (self.f & 0xd5) | FLAG_1 }

    #[inline]
    fn parity(v: u8) -> bool { v.count_ones() & 1 == 0 }

    #[inline]
    fn set_szp(&mut self, v: u8) {
        self.f &= !(FLAG_S | FLAG_Z | FLAG_P);
        if v & 0x80 != 0 { self.f |= FLAG_S; }
        if v == 0 { self.f |= FLAG_Z; }
        if Self::parity(v) { self.f |= FLAG_P; }
        self.f |= FLAG_1;
    }

    #[inline] fn condition(&self, code: u8) -> bool {
        match code & 7 {
            0 => self.f & FLAG_Z == 0,
            1 => self.f & FLAG_Z != 0,
            2 => self.f & FLAG_C == 0,
            3 => self.f & FLAG_C != 0,
            4 => self.f & FLAG_P == 0,
            5 => self.f & FLAG_P != 0,
            6 => self.f & FLAG_S == 0,
            _ => self.f & FLAG_S != 0,
        }
    }

    #[inline]
    fn next_byte<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    #[inline]
    fn next_word<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.next_byte(bus) as u16;
        let hi = self.next_byte(bus) as u16;
        lo | (hi << 8)
    }

    #[inline]
    fn read_word<B: Bus>(&mut self, bus: &mut B, address: u16) -> u16 {
        let lo = bus.read(address) as u16;
        let hi = bus.read(address.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    #[inline]
    fn write_word<B: Bus>(&mut self, bus: &mut B, address: u16, value: u16) {
        bus.write(address, value as u8);
        bus.write(address.wrapping_add(1), (value >> 8) as u8);
    }

    #[inline]
    fn push<B: Bus>(&mut self, bus: &mut B, value: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.stack_write(self.sp, (value >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.stack_write(self.sp, value as u8);
    }

    #[inline]
    fn pop<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = bus.stack_read(self.sp) as u16;
        let hi = bus.stack_read(self.sp.wrapping_add(1)) as u16;
        self.sp = self.sp.wrapping_add(2);
        lo | (hi << 8)
    }

    #[inline]
    fn read_reg<B: Bus>(&mut self, bus: &mut B, code: u8) -> u8 {
        match code & 7 {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            _ => self.a,
        }
    }

    #[inline]
    fn write_reg<B: Bus>(&mut self, bus: &mut B, code: u8, value: u8) {
        match code & 7 {
            0 => self.b = value,
            1 => self.c = value,
            2 => self.d = value,
            3 => self.e = value,
            4 => self.h = value,
            5 => self.l = value,
            6 => bus.write(self.hl(), value),
            _ => self.a = value,
        }
    }

    #[inline]
    fn rp(&self, code: u8) -> u16 {
        match code & 3 { 0 => self.bc(), 1 => self.de(), 2 => self.hl(), _ => self.sp }
    }

    #[inline]
    fn set_rp(&mut self, code: u8, value: u16) {
        match code & 3 { 0 => self.set_bc(value), 1 => self.set_de(value), 2 => self.set_hl(value), _ => self.sp = value }
    }

    #[inline]
    fn add(&mut self, rhs: u8, carry: bool) {
        let c = if carry && self.f & FLAG_C != 0 { 1u16 } else { 0 };
        let a = self.a;
        let sum = a as u16 + rhs as u16 + c;
        let result = sum as u8;
        self.f &= !(FLAG_C | FLAG_AC);
        if sum > 0xff { self.f |= FLAG_C; }
        if ((a & 0x0f) as u16 + (rhs & 0x0f) as u16 + c) > 0x0f { self.f |= FLAG_AC; }
        self.a = result;
        self.set_szp(result);
    }

    #[inline]
    fn sub(&mut self, rhs: u8, borrow: bool, store: bool) {
        let b = if borrow && self.f & FLAG_C != 0 { 1u16 } else { 0 };
        let a = self.a;
        let rhs16 = rhs as u16 + b;
        let result = a.wrapping_sub(rhs).wrapping_sub(b as u8);
        self.f &= !(FLAG_C | FLAG_AC);
        if (a as u16) < rhs16 { self.f |= FLAG_C; }
        // Intel 8080 AC on subtraction is the carry out of bit 3 from the
        // internal two's-complement addition, i.e. the inverse of a nibble
        // borrow. This is deliberately not Z80-style half-borrow semantics.
        let low_rhs = (rhs & 0x0f) as u16 + b;
        if (a & 0x0f) as u16 >= low_rhs { self.f |= FLAG_AC; }
        self.set_szp(result);
        if store { self.a = result; }
    }

    #[inline]
    fn ana(&mut self, rhs: u8) {
        let ac = (self.a | rhs) & 0x08 != 0;
        self.a &= rhs;
        self.f &= !(FLAG_C | FLAG_AC);
        if ac { self.f |= FLAG_AC; }
        self.set_szp(self.a);
    }

    #[inline]
    fn xra(&mut self, rhs: u8) {
        self.a ^= rhs;
        self.f &= !(FLAG_C | FLAG_AC);
        self.set_szp(self.a);
    }

    #[inline]
    fn ora(&mut self, rhs: u8) {
        self.a |= rhs;
        self.f &= !(FLAG_C | FLAG_AC);
        self.set_szp(self.a);
    }

    #[inline]
    fn inr(&mut self, value: u8) -> u8 {
        let carry = self.f & FLAG_C;
        let result = value.wrapping_add(1);
        self.f &= !(FLAG_C | FLAG_AC);
        if value & 0x0f == 0x0f { self.f |= FLAG_AC; }
        self.set_szp(result);
        self.f = (self.f & !FLAG_C) | carry;
        result
    }

    #[inline]
    fn dcr(&mut self, value: u8) -> u8 {
        let carry = self.f & FLAG_C;
        let result = value.wrapping_sub(1);
        self.f &= !(FLAG_C | FLAG_AC);
        if value & 0x0f != 0 { self.f |= FLAG_AC; }
        self.set_szp(result);
        self.f = (self.f & !FLAG_C) | carry;
        result
    }

    fn daa(&mut self) {
        let old_a = self.a;
        let old_c = self.f & FLAG_C != 0;
        let old_ac = self.f & FLAG_AC != 0;
        let mut correction = 0u8;
        let mut carry = old_c;
        if (old_a & 0x0f) > 9 || old_ac { correction |= 0x06; }
        if old_a > 0x99 || old_c { correction |= 0x60; carry = true; }
        let result = old_a.wrapping_add(correction);
        self.f &= !(FLAG_C | FLAG_AC);
        if carry { self.f |= FLAG_C; }
        if ((old_a & 0x0f) + (correction & 0x0f)) > 0x0f { self.f |= FLAG_AC; }
        self.a = result;
        self.set_szp(result);
    }

    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        if self.halted {
            self.cycles += 4;
            return 4;
        }
        let enable_after = self.ei_pending;
        self.ei_pending = false;
        let opcode_address = self.pc;
        let opcode = bus.opcode_fetch(opcode_address);
        self.pc = self.pc.wrapping_add(1);
        let t = self.execute(bus, opcode);
        // EI enables interrupts only after the following instruction. If that
        // following instruction is DI, DI's immediate disable must win over the
        // pending enable rather than being undone at the end of the same step.
        if enable_after && opcode != 0xf3 {
            self.inte = true;
            bus.set_inte(true);
        }
        self.f = (self.f & 0xd5) | FLAG_1;
        self.cycles += t as u64;
        bus.instruction_complete(opcode_address, opcode, t);
        t
    }

    pub fn run_cycles<B: Bus>(&mut self, bus: &mut B, budget: u32) -> u32 {
        let mut used = 0;
        while used < budget { used += self.step(bus); }
        used
    }

    pub fn interrupt<B: Bus>(&mut self, bus: &mut B, opcode: u8) -> bool {
        if !self.inte { return false; }
        let while_halted = self.halted;
        bus.interrupt_ack(self.pc, opcode, while_halted);
        self.inte = false;
        bus.set_inte(false);
        self.halted = false;
        let t = self.execute(bus, opcode);
        self.cycles += t as u64;
        true
    }

    fn execute<B: Bus>(&mut self, bus: &mut B, op: u8) -> u32 {
        if (0x40..=0x7f).contains(&op) {
            if op == 0x76 {
                self.halted = true;
                bus.halt_ack(self.pc, op);
                return 7;
            }
            let dst = (op >> 3) & 7;
            let src = op & 7;
            let v = self.read_reg(bus, src);
            self.write_reg(bus, dst, v);
            return if src == 6 || dst == 6 { 7 } else { 5 };
        }

        if (0x80..=0xbf).contains(&op) {
            let alu = (op >> 3) & 7;
            let src = op & 7;
            let v = self.read_reg(bus, src);
            match alu {
                0 => self.add(v, false),
                1 => self.add(v, true),
                2 => self.sub(v, false, true),
                3 => self.sub(v, true, true),
                4 => self.ana(v),
                5 => self.xra(v),
                6 => self.ora(v),
                _ => self.sub(v, false, false),
            }
            return if src == 6 { 7 } else { 4 };
        }

        if op & 0xc7 == 0x04 {
            let r = (op >> 3) & 7;
            let v = self.read_reg(bus, r);
            let n = self.inr(v);
            self.write_reg(bus, r, n);
            return if r == 6 { 10 } else { 5 };
        }
        if op & 0xc7 == 0x05 {
            let r = (op >> 3) & 7;
            let v = self.read_reg(bus, r);
            let n = self.dcr(v);
            self.write_reg(bus, r, n);
            return if r == 6 { 10 } else { 5 };
        }
        if op & 0xc7 == 0x06 {
            let r = (op >> 3) & 7;
            let v = self.next_byte(bus);
            self.write_reg(bus, r, v);
            return if r == 6 { 10 } else { 7 };
        }

        if op & 0xcf == 0x01 {
            let rp = (op >> 4) & 3;
            let v = self.next_word(bus);
            self.set_rp(rp, v);
            return 10;
        }
        if op & 0xcf == 0x03 {
            let rp = (op >> 4) & 3;
            let v = self.rp(rp).wrapping_add(1);
            self.set_rp(rp, v);
            return 5;
        }
        if op & 0xcf == 0x0b {
            let rp = (op >> 4) & 3;
            let v = self.rp(rp).wrapping_sub(1);
            self.set_rp(rp, v);
            return 5;
        }
        if op & 0xcf == 0x09 {
            let rp = (op >> 4) & 3;
            let lhs = self.hl() as u32;
            let rhs = self.rp(rp) as u32;
            let sum = lhs + rhs;
            self.set_hl(sum as u16);
            self.f &= !FLAG_C;
            if sum > 0xffff { self.f |= FLAG_C; }
            return 10;
        }

        if op & 0xc7 == 0xc0 {
            let cond = (op >> 3) & 7;
            if self.condition(cond) { self.pc = self.pop(bus); 11 } else { 5 }
        } else if op & 0xc7 == 0xc2 {
            let cond = (op >> 3) & 7;
            let target = self.next_word(bus);
            if self.condition(cond) { self.pc = target; }
            10
        } else if op & 0xc7 == 0xc4 {
            let cond = (op >> 3) & 7;
            let target = self.next_word(bus);
            if self.condition(cond) { self.push(bus, self.pc); self.pc = target; 17 } else { 11 }
        } else if op & 0xcf == 0xc1 {
            let rp = (op >> 4) & 3;
            let v = self.pop(bus);
            if rp == 3 { self.set_af(v); } else { self.set_rp(rp, v); }
            10
        } else if op & 0xcf == 0xc5 {
            let rp = (op >> 4) & 3;
            let v = if rp == 3 { self.af() } else { self.rp(rp) };
            self.push(bus, v);
            11
        } else if op & 0xc7 == 0xc7 {
            let vector = (op & 0x38) as u16;
            self.push(bus, self.pc);
            self.pc = vector;
            11
        } else {
            match op {
                0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => 4,
                0x02 => { bus.write(self.bc(), self.a); 7 }
                0x0a => { self.a = bus.read(self.bc()); 7 }
                0x12 => { bus.write(self.de(), self.a); 7 }
                0x1a => { self.a = bus.read(self.de()); 7 }
                0x07 => { let c = self.a >> 7; self.a = self.a.rotate_left(1); self.f = (self.f & !FLAG_C) | c; 4 }
                0x0f => { let c = self.a & 1; self.a = self.a.rotate_right(1); self.f = (self.f & !FLAG_C) | c; 4 }
                0x17 => {
                    let old_c = if self.f & FLAG_C != 0 { 1 } else { 0 };
                    let new_c = self.a >> 7;
                    self.a = (self.a << 1) | old_c;
                    self.f = (self.f & !FLAG_C) | new_c;
                    4
                }
                0x1f => {
                    let old_c = if self.f & FLAG_C != 0 { 0x80 } else { 0 };
                    let new_c = self.a & 1;
                    self.a = (self.a >> 1) | old_c;
                    self.f = (self.f & !FLAG_C) | new_c;
                    4
                }
                0x22 => { let a = self.next_word(bus); self.write_word(bus, a, self.hl()); 16 }
                0x2a => { let a = self.next_word(bus); let v = self.read_word(bus, a); self.set_hl(v); 16 }
                0x27 => { self.daa(); 4 }
                0x2f => { self.a = !self.a; 4 }
                0x32 => { let a = self.next_word(bus); bus.write(a, self.a); 13 }
                0x37 => { self.f |= FLAG_C; 4 }
                0x3a => { let a = self.next_word(bus); self.a = bus.read(a); 13 }
                0x3f => { self.f ^= FLAG_C; 4 }
                0xc3 | 0xcb => { self.pc = self.next_word(bus); 10 }
                0xc6 => { let v = self.next_byte(bus); self.add(v, false); 7 }
                0xc9 | 0xd9 => { self.pc = self.pop(bus); 10 }
                0xcd | 0xdd | 0xed | 0xfd => { let a = self.next_word(bus); self.push(bus, self.pc); self.pc = a; 17 }
                0xce => { let v = self.next_byte(bus); self.add(v, true); 7 }
                0xd3 => { let p = self.next_byte(bus); bus.output(p, self.a); 10 }
                0xd6 => { let v = self.next_byte(bus); self.sub(v, false, true); 7 }
                0xdb => { let p = self.next_byte(bus); self.a = bus.input(p); 10 }
                0xde => { let v = self.next_byte(bus); self.sub(v, true, true); 7 }
                0xe3 => {
                    let lo = bus.stack_read(self.sp);
                    let hi = bus.stack_read(self.sp.wrapping_add(1));
                    bus.stack_write(self.sp, self.l);
                    bus.stack_write(self.sp.wrapping_add(1), self.h);
                    self.l = lo; self.h = hi;
                    18
                }
                0xe6 => { let v = self.next_byte(bus); self.ana(v); 7 }
                0xe9 => { self.pc = self.hl(); 5 }
                0xeb => { core::mem::swap(&mut self.d, &mut self.h); core::mem::swap(&mut self.e, &mut self.l); 4 }
                0xee => { let v = self.next_byte(bus); self.xra(v); 7 }
                0xf3 => { self.inte = false; self.ei_pending = false; bus.set_inte(false); 4 }
                0xf6 => { let v = self.next_byte(bus); self.ora(v); 7 }
                0xf9 => { self.sp = self.hl(); 5 }
                0xfb => { self.ei_pending = true; 4 }
                0xfe => { let v = self.next_byte(bus); self.sub(v, false, false); 7 }
                _ => 4,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus { mem: [u8; 65536] }
    impl Default for TestBus { fn default() -> Self { Self { mem: [0; 65536] } } }
    impl Bus for TestBus {
        fn read(&mut self, a: u16) -> u8 { self.mem[a as usize] }
        fn write(&mut self, a: u16, v: u8) { self.mem[a as usize] = v; }
    }

    #[test]
    fn reset_preserves_programmer_visible_registers() {
        let mut cpu = Cpu8080::new();
        cpu.a = 0x11;
        cpu.b = 0x22;
        cpu.c = 0x33;
        cpu.d = 0x44;
        cpu.e = 0x55;
        cpu.h = 0x66;
        cpu.l = 0x77;
        cpu.f = 0xd7;
        cpu.pc = 0x1234;
        cpu.sp = 0x5678;
        cpu.inte = true;
        cpu.halted = true;
        cpu.cycles = 0x1234;
        cpu.ei_pending = true;

        cpu.reset();

        assert_eq!(cpu.a, 0x11);
        assert_eq!(cpu.b, 0x22);
        assert_eq!(cpu.c, 0x33);
        assert_eq!(cpu.d, 0x44);
        assert_eq!(cpu.e, 0x55);
        assert_eq!(cpu.h, 0x66);
        assert_eq!(cpu.l, 0x77);
        assert_eq!(cpu.f, 0xd7);
        assert_eq!(cpu.sp, 0x5678);
        assert_eq!(cpu.cycles, 0x1234);
        assert_eq!(cpu.pc, 0);
        assert!(!cpu.inte);
        assert!(!cpu.halted);
        assert!(!cpu.ei_pending);
    }

    #[test]
    fn lxi_mov_add_halt() {
        let mut bus = TestBus::default();
        bus.mem[..8].copy_from_slice(&[0x06, 2, 0x0e, 3, 0x78, 0x81, 0x76, 0]);
        let mut cpu = Cpu8080::new();
        while !cpu.halted { cpu.step(&mut bus); }
        assert_eq!(cpu.a, 5);
        assert_eq!(cpu.b, 2);
        assert_eq!(cpu.c, 3);
    }

    #[test]
    fn call_and_ret() {
        let mut bus = TestBus::default();
        bus.mem[0] = 0xcd; bus.mem[1] = 0x06; bus.mem[2] = 0x00;
        bus.mem[3] = 0x76;
        bus.mem[6] = 0x3e; bus.mem[7] = 0x42; bus.mem[8] = 0xc9;
        let mut cpu = Cpu8080::new(); cpu.sp = 0x1000;
        while !cpu.halted { cpu.step(&mut bus); }
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cpu.sp, 0x1000);
    }

    #[test]
    fn xchg_uses_four_t_states() {
        let mut bus = TestBus::default();
        bus.mem[0] = 0xeb;
        let mut cpu = Cpu8080::new();
        cpu.d = 0x12;
        cpu.e = 0x34;
        cpu.h = 0xab;
        cpu.l = 0xcd;

        let t = cpu.step(&mut bus);

        assert_eq!(t, 4);
        assert_eq!(cpu.cycles, 4);
        assert_eq!(cpu.de(), 0xabcd);
        assert_eq!(cpu.hl(), 0x1234);
    }

    #[test]
    fn ei_is_delayed_one_instruction_but_di_wins_when_it_is_that_instruction() {
        let mut bus = TestBus::default();
        bus.mem[0] = 0xfb; // EI
        bus.mem[1] = 0xf3; // DI
        let mut cpu = Cpu8080::new();

        assert_eq!(cpu.step(&mut bus), 4);
        assert!(!cpu.inte, "EI must not enable interrupts immediately");
        assert_eq!(cpu.step(&mut bus), 4);
        assert!(!cpu.inte, "DI immediately after EI must leave interrupts disabled");
        assert!(!cpu.interrupt(&mut bus, 0xcf));
    }

    #[test]
    fn subtraction_aux_carry_uses_8080_internal_carry_polarity() {
        let mut bus = TestBus::default();
        let mut cpu = Cpu8080::new();

        // 03h - 00h: no nibble borrow, therefore Intel 8080 AC is set.
        cpu.a = 0x03;
        cpu.b = 0x00;
        bus.mem[0] = 0x90; // SUB B
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0x03);
        assert_ne!(cpu.f & FLAG_AC, 0);
        assert_eq!(cpu.f & FLAG_C, 0);

        // 03h - 04h: nibble borrow, therefore AC is clear and C is set.
        cpu.pc = 0;
        cpu.a = 0x03;
        cpu.b = 0x04;
        cpu.f = FLAG_1;
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0xff);
        assert_eq!(cpu.f & FLAG_AC, 0);
        assert_ne!(cpu.f & FLAG_C, 0);
    }

    #[test]
    fn sbb_and_compare_share_subtraction_aux_carry_rules() {
        let mut bus = TestBus::default();
        let mut cpu = Cpu8080::new();

        cpu.a = 0x03;
        cpu.b = 0x00;
        cpu.f = FLAG_1 | FLAG_C;
        bus.mem[0] = 0x98; // SBB B: 03h - 00h - 1 = 02h
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0x02);
        assert_ne!(cpu.f & FLAG_AC, 0);

        cpu.pc = 0;
        cpu.a = 0x03;
        cpu.f = FLAG_1;
        bus.mem[0] = 0xfe; // CPI 00h: A unchanged, same subtraction flags
        bus.mem[1] = 0x00;
        cpu.step(&mut bus);
        assert_eq!(cpu.a, 0x03);
        assert_ne!(cpu.f & FLAG_AC, 0);
    }
}
