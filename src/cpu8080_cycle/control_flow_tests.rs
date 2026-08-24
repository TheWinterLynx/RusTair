use super::*;
use super::alu::{FLAG_C, FLAG_Z};

fn input(data_in: u8, ready: bool) -> Cpu8080Inputs {
    Cpu8080Inputs { data_in, ready, ..Cpu8080Inputs::default() }
}

fn fetch(cpu: &mut Cpu8080Cycle, opcode: u8) -> [TickTrace; 4] {
    [
        cpu.tick(input(0, true)),
        cpu.tick(input(0, true)),
        cpu.tick(input(opcode, true)),
        cpu.tick(input(0, true)),
    ]
}

fn read_cycle(cpu: &mut Cpu8080Cycle, data: u8) -> [TickTrace; 3] {
    [
        cpu.tick(input(0, true)),
        cpu.tick(input(0, true)),
        cpu.tick(input(data, true)),
    ]
}

fn write_cycle(cpu: &mut Cpu8080Cycle) -> [TickTrace; 3] {
    [
        cpu.tick(input(0, true)),
        cpu.tick(input(0, true)),
        cpu.tick(input(0, true)),
    ]
}

#[test]
fn jmp_is_ten_t_states_and_loads_little_endian_target() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xc3);
    let lo = read_cycle(&mut cpu, 0x34);
    let hi = read_cycle(&mut cpu, 0x12);
    assert_eq!(lo[0].pins.address, Some(0x2001));
    assert_eq!(hi[0].pins.address, Some(0x2002));
    assert!(hi[2].instruction_complete);
    assert_eq!(hi[2].instruction_t_states, 10);
    assert_eq!(cpu.registers().pc, 0x1234);
}

#[test]
fn conditional_jump_is_always_ten_t_states_taken_or_not() {
    for (flags, expected_pc) in [(0x02, 0x1234), (0x02 | FLAG_Z, 0x2003)] {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.pc = 0x2000;
        r.f = flags;
        cpu.set_registers(r);

        fetch(&mut cpu, 0xc2); // JNZ
        read_cycle(&mut cpu, 0x34);
        let hi = read_cycle(&mut cpu, 0x12);
        assert!(hi[2].instruction_complete);
        assert_eq!(hi[2].instruction_t_states, 10);
        assert_eq!(cpu.registers().pc, expected_pc);
    }
}

#[test]
fn push_bc_is_eleven_t_states_high_byte_then_low_byte() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.b = 0x12;
    r.c = 0x34;
    r.sp = 0x2000;
    cpu.set_registers(r);

    let m1 = fetch(&mut cpu, 0xc5);
    assert!(!m1[3].instruction_complete);
    let t5 = cpu.tick(input(0, true));
    assert_eq!(t5.t_state, TState::T5);

    let high = write_cycle(&mut cpu);
    assert_eq!(high[0].machine_cycle, MachineCycle::StackWrite);
    assert_eq!(high[0].pins.data_out, Some(0x04));
    assert_eq!(high[0].pins.address, Some(0x1fff));
    assert_eq!(high[1].pins.data_out, Some(0x12));
    assert!(!high[2].instruction_complete);

    let low = write_cycle(&mut cpu);
    assert_eq!(low[0].pins.address, Some(0x1ffe));
    assert_eq!(low[1].pins.data_out, Some(0x34));
    assert!(low[2].instruction_complete);
    assert_eq!(low[2].instruction_t_states, 11);
    assert_eq!(cpu.registers().sp, 0x1ffe);
}

#[test]
fn pop_de_is_ten_t_states_low_byte_then_high_byte() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.sp = 0x3000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xd1);
    let low = read_cycle(&mut cpu, 0x78);
    assert_eq!(low[0].machine_cycle, MachineCycle::StackRead);
    assert_eq!(low[0].pins.data_out, Some(0x86));
    assert_eq!(low[0].pins.address, Some(0x3000));
    let high = read_cycle(&mut cpu, 0x56);
    assert_eq!(high[0].pins.address, Some(0x3001));
    assert!(high[2].instruction_complete);
    assert_eq!(high[2].instruction_t_states, 10);
    assert_eq!(cpu.registers().sp, 0x3002);
    assert_eq!(cpu.registers().d, 0x56);
    assert_eq!(cpu.registers().e, 0x78);
}

#[test]
fn push_and_pop_psw_use_a_high_and_normalized_flags_low() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0xa5;
    r.f = 0xff;
    r.sp = 0x4000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xf5);
    cpu.tick(input(0, true));
    let high = write_cycle(&mut cpu);
    let low = write_cycle(&mut cpu);
    assert_eq!(high[1].pins.data_out, Some(0xa5));
    assert_eq!(low[1].pins.data_out, Some(0xd7));

    fetch(&mut cpu, 0xf1);
    read_cycle(&mut cpu, 0xff);
    let pop_high = read_cycle(&mut cpu, 0x5a);
    assert!(pop_high[2].instruction_complete);
    assert_eq!(cpu.registers().a, 0x5a);
    assert_eq!(cpu.registers().f, 0xd7);
}

#[test]
fn stack_write_wait_state_preserves_address_data_and_write_strobe() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.b = 0xaa;
    r.c = 0x55;
    r.sp = 0x1000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xc5);
    cpu.tick(input(0, true)); // M1 T5 -> M2 StackWrite
    cpu.tick(input(0, true)); // M2 T1
    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.pins.address, Some(0x0fff));
    assert_eq!(t2.pins.data_out, Some(0xaa));
    let tw = cpu.tick(input(0, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(!tw.pins.wr_n);
    assert_eq!(tw.pins.address, Some(0x0fff));
    assert_eq!(tw.pins.data_out, Some(0xaa));
}

#[test]
fn stack_read_wait_state_preserves_address_and_dbin() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.sp = 0x1000;
    r.f = FLAG_C;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xc1);
    cpu.tick(input(0, true)); // M2 T1
    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.pins.address, Some(0x1000));
    assert!(t2.pins.dbin);
    let tw = cpu.tick(input(0x34, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(tw.pins.dbin);
    assert_eq!(tw.pins.address, Some(0x1000));
}
