use super::*;

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

fn read_operand(cpu: &mut Cpu8080Cycle, operand: u8) -> [TickTrace; 3] {
    [
        cpu.tick(input(0, true)),
        cpu.tick(input(0, true)),
        cpu.tick(input(operand, true)),
    ]
}

#[test]
fn out_is_ten_t_states_with_repeated_port_address_and_output_status() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.a = 0xa5;
    r.f = 0xd7;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xd3);
    let operand = read_operand(&mut cpu, 0x12);
    assert_eq!(operand[0].machine_cycle, MachineCycle::MemoryRead);
    assert_eq!(operand[0].pins.address, Some(0x2001));

    let t1 = cpu.tick(input(0, true));
    assert_eq!(t1.machine_cycle, MachineCycle::OutputWrite);
    assert_eq!(t1.machine_cycle_index, 3);
    assert_eq!(t1.pins.address, Some(0x1212));
    assert_eq!(t1.pins.data_out, Some(0x10));
    assert!(t1.pins.sync);

    let t2 = cpu.tick(input(0, true));
    assert_eq!(t2.pins.address, Some(0x1212));
    assert_eq!(t2.pins.data_out, Some(0xa5));
    assert!(t2.pins.wr_n);

    let t3 = cpu.tick(input(0, true));
    assert!(!t3.pins.wr_n);
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 10);
    assert_eq!(cpu.registers().pc, 0x2002);
    assert_eq!(cpu.registers().a, 0xa5);
    assert_eq!(cpu.registers().f, 0xd7);
}

#[test]
fn in_is_ten_t_states_with_repeated_port_address_and_input_status() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x3000;
    r.a = 0x11;
    r.f = 0x46;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xdb);
    read_operand(&mut cpu, 0x34);

    let t1 = cpu.tick(input(0, true));
    assert_eq!(t1.machine_cycle, MachineCycle::InputRead);
    assert_eq!(t1.machine_cycle_index, 3);
    assert_eq!(t1.pins.address, Some(0x3434));
    assert_eq!(t1.pins.data_out, Some(0x42));
    assert!(t1.pins.sync);

    let t2 = cpu.tick(input(0, true));
    assert!(t2.pins.dbin);
    assert_eq!(t2.pins.address, Some(0x3434));

    let t3 = cpu.tick(input(0x5a, true));
    assert!(!t3.pins.dbin);
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 10);
    assert_eq!(cpu.registers().pc, 0x3002);
    assert_eq!(cpu.registers().a, 0x5a);
    assert_eq!(cpu.registers().f, 0x46);
}

#[test]
fn input_read_ready_wait_keeps_port_and_dbin_stable() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.f = 0xd7;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xdb);
    read_operand(&mut cpu, 0x7f);

    let t1 = cpu.tick(input(0, true));
    assert_eq!(t1.pins.address, Some(0x7f7f));

    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.t_state, TState::T2);
    assert!(t2.pins.dbin);
    assert_eq!(t2.pins.address, Some(0x7f7f));

    let tw = cpu.tick(input(0, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(tw.pins.dbin);
    assert_eq!(tw.pins.address, Some(0x7f7f));

    let t3 = cpu.tick(input(0xa5, true));
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 11);
    assert_eq!(cpu.registers().a, 0xa5);
    assert_eq!(cpu.registers().f, 0xd7);
}

#[test]
fn output_write_ready_wait_keeps_port_data_and_write_strobe_stable() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x5a;
    r.f = 0x46;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xd3);
    read_operand(&mut cpu, 0x08);

    cpu.tick(input(0, true)); // M3 T1
    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.pins.address, Some(0x0808));
    assert_eq!(t2.pins.data_out, Some(0x5a));
    assert!(t2.pins.wr_n);

    let tw = cpu.tick(input(0, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(!tw.pins.wr_n);
    assert_eq!(tw.pins.address, Some(0x0808));
    assert_eq!(tw.pins.data_out, Some(0x5a));

    let t3 = cpu.tick(input(0, true));
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 11);
    assert!(!t3.pins.wr_n);
    assert_eq!(cpu.registers().f, 0x46);
}
