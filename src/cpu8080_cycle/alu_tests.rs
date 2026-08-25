use super::*;
use super::alu::{FLAG_AC, FLAG_C, FLAG_Z};

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
fn register_alu_executes_in_m1_and_is_exactly_four_t_states() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x0f;
    r.b = 0x01;
    cpu.set_registers(r);

    let trace = fetch(&mut cpu, 0x80); // ADD B
    assert!(trace[3].instruction_complete);
    assert_eq!(trace[3].instruction_t_states, 4);
    assert_eq!(cpu.registers().a, 0x10);
    assert_ne!(cpu.registers().f & FLAG_AC, 0);
}

#[test]
fn accumulator_only_operations_are_exactly_four_t_states_with_no_extra_cycle() {
    for opcode in [0x07, 0x0f, 0x17, 0x1f, 0x27, 0x2f, 0x37, 0x3f] {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.a = 0x81;
        r.f = 0x12;
        cpu.set_registers(r);

        let trace = fetch(&mut cpu, opcode);
        assert_eq!(trace[0].machine_cycle, MachineCycle::InstructionFetch, "opcode {opcode:02x}");
        assert_eq!(trace[1].machine_cycle, MachineCycle::InstructionFetch, "opcode {opcode:02x}");
        assert_eq!(trace[2].machine_cycle, MachineCycle::InstructionFetch, "opcode {opcode:02x}");
        assert_eq!(trace[3].machine_cycle, MachineCycle::InstructionFetch, "opcode {opcode:02x}");
        assert!(trace[3].instruction_complete, "opcode {opcode:02x}");
        assert_eq!(trace[3].instruction_t_states, 4, "opcode {opcode:02x}");
        assert_eq!(cpu.last_instruction_t_states(), Some(4), "opcode {opcode:02x}");
        assert_eq!(cpu.machine_cycle(), MachineCycle::InstructionFetch, "opcode {opcode:02x}");
        assert_eq!(cpu.t_state(), TState::T1, "opcode {opcode:02x}");
        assert_eq!(cpu.registers().pc, 1, "opcode {opcode:02x}");
    }
}

#[test]
fn memory_alu_is_seven_t_states_and_reads_hl() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x01;
    r.h = 0x12;
    r.l = 0x34;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x86); // ADD M
    let read = read_cycle(&mut cpu, 0x02);
    assert_eq!(read[0].pins.address, Some(0x1234));
    assert!(read[2].instruction_complete);
    assert_eq!(read[2].instruction_t_states, 7);
    assert_eq!(cpu.registers().a, 0x03);
}

#[test]
fn immediate_alu_is_seven_t_states_and_advances_pc() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.a = 0x7f;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xc6); // ADI
    let read = read_cycle(&mut cpu, 0x01);
    assert_eq!(read[0].pins.address, Some(0x2001));
    assert!(read[2].instruction_complete);
    assert_eq!(read[2].instruction_t_states, 7);
    assert_eq!(cpu.registers().pc, 0x2002);
    assert_eq!(cpu.registers().a, 0x80);
}

#[test]
fn cmp_register_preserves_a_and_uses_8080_subtraction_flags() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x03;
    r.b = 0x04;
    cpu.set_registers(r);

    let trace = fetch(&mut cpu, 0xb8); // CMP B
    assert!(trace[3].instruction_complete);
    assert_eq!(cpu.registers().a, 0x03);
    assert_ne!(cpu.registers().f & FLAG_C, 0);
    assert_eq!(cpu.registers().f & FLAG_AC, 0);
}

#[test]
fn inr_and_dcr_register_are_five_t_states_and_preserve_carry() {
    for (opcode, start, expected) in [(0x04, 0x0f, 0x10), (0x05, 0x10, 0x0f)] {
        let mut cpu = Cpu8080Cycle::new();
        let mut r = Registers::default();
        r.b = start;
        r.f |= FLAG_C;
        cpu.set_registers(r);
        fetch(&mut cpu, opcode);
        let t5 = cpu.tick(input(0, true));
        assert!(t5.instruction_complete);
        assert_eq!(t5.instruction_t_states, 5);
        assert_eq!(cpu.registers().b, expected);
        assert_ne!(cpu.registers().f & FLAG_C, 0);
    }
}

#[test]
fn inr_memory_is_ten_t_state_read_modify_write() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0x40;
    r.l = 0x00;
    r.f |= FLAG_C;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x34); // INR M
    let read = read_cycle(&mut cpu, 0xff);
    assert_eq!(read[0].pins.address, Some(0x4000));
    assert!(!read[2].instruction_complete);
    let write = write_cycle(&mut cpu);
    assert_eq!(write[0].pins.address, Some(0x4000));
    assert_eq!(write[1].pins.data_out, Some(0x00));
    assert!(!write[2].pins.wr_n);
    assert!(write[2].instruction_complete);
    assert_eq!(write[2].instruction_t_states, 10);
    assert_ne!(cpu.registers().f & FLAG_C, 0);
    assert_ne!(cpu.registers().f & FLAG_Z, 0);
}

#[test]
fn dcr_memory_wait_state_keeps_rmw_write_bus_stable() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0x50;
    r.l = 0x10;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x35); // DCR M
    read_cycle(&mut cpu, 0x01);
    cpu.tick(input(0, true)); // M3 T1
    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.pins.address, Some(0x5010));
    assert_eq!(t2.pins.data_out, Some(0x00));
    let tw = cpu.tick(input(0, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert_eq!(tw.pins.address, Some(0x5010));
    assert_eq!(tw.pins.data_out, Some(0x00));
    assert!(!tw.pins.wr_n);
    let t3 = cpu.tick(input(0, true));
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 11);
}
