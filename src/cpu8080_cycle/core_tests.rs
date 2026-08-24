use super::*;
use super::decode::RegisterPair;

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
fn nop_remains_four_t_states() {
    let mut cpu = Cpu8080Cycle::new();
    let trace = fetch(&mut cpu, 0x00);
    assert!(trace[3].instruction_complete);
    assert_eq!(trace[3].instruction_t_states, 4);
    assert_eq!(cpu.registers().pc, 1);
}

#[test]
fn lxi_all_pairs_are_ten_t_states_little_endian_and_preserve_flags() {
    let cases = [(0x01, RegisterPair::BC), (0x11, RegisterPair::DE), (0x21, RegisterPair::HL), (0x31, RegisterPair::SP)];
    for (opcode, pair) in cases {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.pc = 0x2000;
        r.f = 0xd7;
        cpu.set_registers(r);
        fetch(&mut cpu, opcode);
        let lo = read_cycle(&mut cpu, 0x34);
        assert_eq!(lo[0].pins.address, Some(0x2001));
        let hi = read_cycle(&mut cpu, 0x12);
        assert!(hi[2].instruction_complete);
        assert_eq!(hi[2].instruction_t_states, 10);
        assert_eq!(cpu.read_pair(pair), 0x1234);
        assert_eq!(cpu.registers().pc, 0x2003);
        assert_eq!(cpu.registers().f, 0xd7);
    }
}

#[test]
fn inx_and_dcx_are_five_t_states_and_wrap_without_touching_flags() {
    for (opcode, pair, start, expected) in [
        (0x03, RegisterPair::BC, 0xffff, 0x0000),
        (0x13, RegisterPair::DE, 0x1234, 0x1235),
        (0x2b, RegisterPair::HL, 0x0000, 0xffff),
        (0x3b, RegisterPair::SP, 0x1000, 0x0fff),
    ] {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.f = 0xd7;
        cpu.set_registers(r);
        cpu.write_pair(pair, start);
        fetch(&mut cpu, opcode);
        let t5 = cpu.tick(input(0, true));
        assert!(t5.instruction_complete);
        assert_eq!(t5.instruction_t_states, 5);
        assert_eq!(cpu.read_pair(pair), expected);
        assert_eq!(cpu.registers().f, 0xd7);
    }
}

#[test]
fn dad_uses_two_internal_cycles_for_exactly_ten_t_states() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0xff; r.l = 0xff; r.b = 0; r.c = 1; r.f = 0xd6;
    cpu.set_registers(r);
    fetch(&mut cpu, 0x09);
    for _ in 0..3 { cpu.tick(input(0, false)); }
    let m3 = [cpu.tick(input(0, false)), cpu.tick(input(0, false)), cpu.tick(input(0, false))];
    assert!(m3[2].instruction_complete);
    assert_eq!(m3[2].instruction_t_states, 10);
    assert_eq!(cpu.hl(), 0);
    assert_eq!(cpu.registers().f, 0xd7);
}

#[test]
fn ldax_stax_and_direct_transfers_preserve_exact_timings() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.b = 0x12; r.c = 0x34; r.d = 0x56; r.e = 0x78; r.a = 0xa5; r.f = 0x46;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x0a);
    let ldax = read_cycle(&mut cpu, 0x5a);
    assert_eq!(ldax[0].pins.address, Some(0x1234));
    assert_eq!(ldax[2].instruction_t_states, 7);

    fetch(&mut cpu, 0x12);
    let stax = write_cycle(&mut cpu);
    assert_eq!(stax[0].pins.address, Some(0x5678));
    assert_eq!(stax[1].pins.data_out, Some(0x5a));
    assert_eq!(stax[2].instruction_t_states, 7);

    fetch(&mut cpu, 0x32);
    read_cycle(&mut cpu, 0x78);
    read_cycle(&mut cpu, 0x56);
    let sta = write_cycle(&mut cpu);
    assert_eq!(sta[0].pins.address, Some(0x5678));
    assert_eq!(sta[2].instruction_t_states, 13);
}

#[test]
fn lhld_and_shld_are_sixteen_t_states_and_use_consecutive_addresses() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0xaa; r.l = 0xbb; r.f = 0xd7;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x2a);
    read_cycle(&mut cpu, 0x00);
    read_cycle(&mut cpu, 0x40);
    let low = read_cycle(&mut cpu, 0x34);
    let high = read_cycle(&mut cpu, 0x12);
    assert_eq!(low[0].pins.address, Some(0x4000));
    assert_eq!(high[0].pins.address, Some(0x4001));
    assert_eq!(high[2].instruction_t_states, 16);
    assert_eq!(cpu.hl(), 0x1234);

    fetch(&mut cpu, 0x22);
    read_cycle(&mut cpu, 0x00);
    read_cycle(&mut cpu, 0x50);
    let low_write = write_cycle(&mut cpu);
    let high_write = write_cycle(&mut cpu);
    assert_eq!(low_write[0].pins.address, Some(0x5000));
    assert_eq!(low_write[1].pins.data_out, Some(0x34));
    assert_eq!(high_write[0].pins.address, Some(0x5001));
    assert_eq!(high_write[1].pins.data_out, Some(0x12));
    assert_eq!(high_write[2].instruction_t_states, 16);
}

#[test]
fn hl_addressed_mvi_mov_and_wait_state_paths_still_work() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0x20; r.l = 0x10; r.b = 0x77;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x70);
    let write = write_cycle(&mut cpu);
    assert_eq!(write[0].pins.address, Some(0x2010));
    assert_eq!(write[1].pins.data_out, Some(0x77));
    assert_eq!(write[2].instruction_t_states, 7);

    fetch(&mut cpu, 0x36);
    read_cycle(&mut cpu, 0xa5);
    let write = write_cycle(&mut cpu);
    assert_eq!(write[1].pins.data_out, Some(0xa5));
    assert_eq!(write[2].instruction_t_states, 10);
}

#[test]
fn ready_wait_extends_external_write_and_keeps_bus_stable() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0x12; r.l = 0x34; r.b = 0xa5;
    cpu.set_registers(r);
    fetch(&mut cpu, 0x70);
    cpu.tick(input(0, true));
    let t2 = cpu.tick(input(0, false));
    let tw = cpu.tick(input(0, true));
    assert_eq!(t2.pins.address, Some(0x1234));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(!tw.pins.wr_n);
    assert_eq!(tw.pins.data_out, Some(0xa5));
    let t3 = cpu.tick(input(0, true));
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 8);
}

#[test]
fn unsupported_hlt_remains_an_explicit_fault() {
    let mut cpu = Cpu8080Cycle::new();
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    cpu.tick(input(0x76, true));
    let t4 = cpu.tick(input(0, true));
    assert_eq!(t4.fault, Some(Cpu8080CycleFault::UnsupportedOpcode(0x76)));
}

#[test]
fn reset_preserves_general_registers_but_restarts_at_zero() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x5a; r.b = 0xa5; r.pc = 0x4321;
    cpu.set_registers(r);
    let reset = cpu.tick(Cpu8080Inputs { reset: true, ..Cpu8080Inputs::default() });
    assert!(reset.reset);
    assert_eq!(cpu.registers().pc, 0);
    assert_eq!(cpu.registers().a, 0x5a);
    assert_eq!(cpu.registers().b, 0xa5);
}
