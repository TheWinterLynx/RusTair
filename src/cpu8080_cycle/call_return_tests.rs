use super::*;
use super::alu::FLAG_Z;

fn input(data_in: u8, ready: bool) -> Cpu8080Inputs {
    Cpu8080Inputs {
        data_in,
        ready,
        ..Cpu8080Inputs::default()
    }
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
fn call_is_seventeen_t_states_and_pushes_return_address_high_then_low() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.sp = 0x4000;
    cpu.set_registers(r);

    let m1 = fetch(&mut cpu, 0xcd);
    assert!(!m1[3].instruction_complete);

    let t5 = cpu.tick(input(0, true));
    assert_eq!(t5.t_state, TState::T5);
    assert_eq!(cpu.registers().sp, 0x3fff);

    let low_target = read_cycle(&mut cpu, 0x34);
    let high_target = read_cycle(&mut cpu, 0x12);
    assert_eq!(low_target[0].pins.address, Some(0x2001));
    assert_eq!(high_target[0].pins.address, Some(0x2002));
    assert_eq!(cpu.registers().pc, 0x2003);

    let high_return = write_cycle(&mut cpu);
    assert_eq!(high_return[0].machine_cycle, MachineCycle::StackWrite);
    assert_eq!(high_return[0].machine_cycle_index, 4);
    assert_eq!(high_return[0].pins.address, Some(0x3fff));
    assert_eq!(high_return[0].pins.data_out, Some(0x04));
    assert_eq!(high_return[1].pins.data_out, Some(0x20));
    assert!(!high_return[2].instruction_complete);

    let low_return = write_cycle(&mut cpu);
    assert_eq!(low_return[0].machine_cycle_index, 5);
    assert_eq!(low_return[0].pins.address, Some(0x3ffe));
    assert_eq!(low_return[1].pins.data_out, Some(0x03));
    assert!(low_return[2].instruction_complete);
    assert_eq!(low_return[2].instruction_t_states, 17);
    assert_eq!(cpu.registers().sp, 0x3ffe);
    assert_eq!(cpu.registers().pc, 0x1234);
}

#[test]
fn conditional_call_is_eleven_t_states_not_taken_and_seventeen_when_taken() {
    let mut not_taken = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.sp = 0x4000;
    r.f |= FLAG_Z;
    not_taken.set_registers(r);

    fetch(&mut not_taken, 0xc4); // CNZ
    let t5 = not_taken.tick(input(0, true));
    assert_eq!(t5.t_state, TState::T5);
    assert_eq!(not_taken.registers().sp, 0x4000);
    read_cycle(&mut not_taken, 0x34);
    let high = read_cycle(&mut not_taken, 0x12);
    assert!(high[2].instruction_complete);
    assert_eq!(high[2].instruction_t_states, 11);
    assert_eq!(not_taken.registers().pc, 0x2003);
    assert_eq!(not_taken.registers().sp, 0x4000);

    let mut taken = Cpu8080Cycle::new();
    r.f &= !FLAG_Z;
    taken.set_registers(r);
    fetch(&mut taken, 0xc4); // CNZ
    taken.tick(input(0, true));
    assert_eq!(taken.registers().sp, 0x3fff);
    read_cycle(&mut taken, 0x78);
    read_cycle(&mut taken, 0x56);
    write_cycle(&mut taken);
    let low_return = write_cycle(&mut taken);
    assert!(low_return[2].instruction_complete);
    assert_eq!(low_return[2].instruction_t_states, 17);
    assert_eq!(taken.registers().pc, 0x5678);
    assert_eq!(taken.registers().sp, 0x3ffe);
}

#[test]
fn ret_is_ten_t_states_and_pops_low_then_high_pc() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.sp = 0x3000;
    cpu.set_registers(r);

    let m1 = fetch(&mut cpu, 0xc9);
    assert!(!m1[3].instruction_complete);

    let low = read_cycle(&mut cpu, 0x78);
    assert_eq!(low[0].machine_cycle, MachineCycle::StackRead);
    assert_eq!(low[0].machine_cycle_index, 2);
    assert_eq!(low[0].pins.address, Some(0x3000));

    let high = read_cycle(&mut cpu, 0x56);
    assert_eq!(high[0].machine_cycle_index, 3);
    assert_eq!(high[0].pins.address, Some(0x3001));
    assert!(high[2].instruction_complete);
    assert_eq!(high[2].instruction_t_states, 10);
    assert_eq!(cpu.registers().pc, 0x5678);
    assert_eq!(cpu.registers().sp, 0x3002);
}

#[test]
fn conditional_return_is_five_t_states_not_taken_and_eleven_when_taken() {
    let mut not_taken = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.sp = 0x3000;
    r.f |= FLAG_Z;
    not_taken.set_registers(r);

    let m1 = fetch(&mut not_taken, 0xc0); // RNZ
    assert!(!m1[3].instruction_complete);
    let t5 = not_taken.tick(input(0, true));
    assert!(t5.instruction_complete);
    assert_eq!(t5.instruction_t_states, 5);
    assert_eq!(not_taken.registers().pc, 0x2001);
    assert_eq!(not_taken.registers().sp, 0x3000);

    let mut taken = Cpu8080Cycle::new();
    r.f &= !FLAG_Z;
    taken.set_registers(r);
    fetch(&mut taken, 0xc0); // RNZ
    let t5 = taken.tick(input(0, true));
    assert!(!t5.instruction_complete);
    let low = read_cycle(&mut taken, 0x34);
    let high = read_cycle(&mut taken, 0x12);
    assert_eq!(low[0].pins.address, Some(0x3000));
    assert_eq!(high[0].pins.address, Some(0x3001));
    assert!(high[2].instruction_complete);
    assert_eq!(high[2].instruction_t_states, 11);
    assert_eq!(taken.registers().pc, 0x1234);
    assert_eq!(taken.registers().sp, 0x3002);
}

#[test]
fn all_eight_rst_vectors_are_eleven_t_states_and_push_pc_plus_one() {
    for vector in 0u8..8 {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.pc = 0x1000;
        r.sp = 0x4000;
        cpu.set_registers(r);

        let opcode = 0xc7 | (vector << 3);
        fetch(&mut cpu, opcode);
        let t5 = cpu.tick(input(0, true));
        assert_eq!(t5.t_state, TState::T5);
        assert_eq!(cpu.registers().sp, 0x3fff);

        let high_return = write_cycle(&mut cpu);
        assert_eq!(high_return[0].pins.address, Some(0x3fff));
        assert_eq!(high_return[1].pins.data_out, Some(0x10));

        let low_return = write_cycle(&mut cpu);
        assert_eq!(low_return[0].pins.address, Some(0x3ffe));
        assert_eq!(low_return[1].pins.data_out, Some(0x01));
        assert!(low_return[2].instruction_complete);
        assert_eq!(low_return[2].instruction_t_states, 11);
        assert_eq!(cpu.registers().sp, 0x3ffe);
        assert_eq!(cpu.registers().pc, u16::from(vector) << 3);
    }
}

#[test]
fn call_stack_write_can_be_extended_by_ready_without_losing_return_byte() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.sp = 0x4000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xcd);
    cpu.tick(input(0, true)); // M1 T5
    read_cycle(&mut cpu, 0x34);
    read_cycle(&mut cpu, 0x12);

    cpu.tick(input(0, true)); // M4 T1
    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.pins.address, Some(0x3fff));
    assert_eq!(t2.pins.data_out, Some(0x20));

    let tw = cpu.tick(input(0, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(!tw.pins.wr_n);
    assert_eq!(tw.pins.address, Some(0x3fff));
    assert_eq!(tw.pins.data_out, Some(0x20));
}
