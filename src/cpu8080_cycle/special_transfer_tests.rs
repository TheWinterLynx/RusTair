use super::*;

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
fn xchg_is_four_t_states_and_swaps_de_with_hl() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.d = 0x12;
    r.e = 0x34;
    r.h = 0xab;
    r.l = 0xcd;
    r.f = 0xd7;
    cpu.set_registers(r);

    let trace = fetch(&mut cpu, 0xeb);
    assert!(trace[3].instruction_complete);
    assert_eq!(trace[3].instruction_t_states, 4);
    assert_eq!(cpu.read_pair(RegisterPair::DE), 0xabcd);
    assert_eq!(cpu.hl(), 0x1234);
    assert_eq!(cpu.registers().f, 0xd7);
}

#[test]
fn pchl_is_five_t_states_and_loads_pc_from_hl() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.h = 0x12;
    r.l = 0x34;
    r.f = 0x46;
    cpu.set_registers(r);

    let m1 = fetch(&mut cpu, 0xe9);
    assert!(!m1[3].instruction_complete);
    let t5 = cpu.tick(input(0, true));
    assert_eq!(t5.t_state, TState::T5);
    assert!(t5.instruction_complete);
    assert_eq!(t5.instruction_t_states, 5);
    assert_eq!(cpu.registers().pc, 0x1234);
    assert_eq!(cpu.registers().f, 0x46);
}

#[test]
fn sphl_is_five_t_states_and_loads_sp_from_hl() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.sp = 0xaaaa;
    r.h = 0xbe;
    r.l = 0xef;
    r.f = 0x46;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xf9);
    let t5 = cpu.tick(input(0, true));
    assert!(t5.instruction_complete);
    assert_eq!(t5.instruction_t_states, 5);
    assert_eq!(cpu.registers().sp, 0xbeef);
    assert_eq!(cpu.registers().f, 0x46);
}

#[test]
fn xthl_is_eighteen_t_states_with_two_reads_two_writes_and_final_internal_states() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.sp = 0x3000;
    r.h = 0xab;
    r.l = 0xcd;
    r.f = 0xd7;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xe3);

    let low_read = read_cycle(&mut cpu, 0x34);
    assert_eq!(low_read[0].machine_cycle, MachineCycle::StackRead);
    assert_eq!(low_read[0].machine_cycle_index, 2);
    assert_eq!(low_read[0].pins.address, Some(0x3000));
    assert_eq!(low_read[0].pins.data_out, Some(0x86));

    let high_read = read_cycle(&mut cpu, 0x12);
    assert_eq!(high_read[0].machine_cycle, MachineCycle::StackRead);
    assert_eq!(high_read[0].machine_cycle_index, 3);
    assert_eq!(high_read[0].pins.address, Some(0x3001));

    let low_write = write_cycle(&mut cpu);
    assert_eq!(low_write[0].machine_cycle, MachineCycle::StackWrite);
    assert_eq!(low_write[0].machine_cycle_index, 4);
    assert_eq!(low_write[0].pins.address, Some(0x3000));
    assert_eq!(low_write[1].pins.data_out, Some(0xcd));
    assert!(!low_write[2].instruction_complete);

    let high_write = write_cycle(&mut cpu);
    assert_eq!(high_write[0].machine_cycle, MachineCycle::StackWrite);
    assert_eq!(high_write[0].machine_cycle_index, 5);
    assert_eq!(high_write[0].pins.address, Some(0x3001));
    assert_eq!(high_write[1].pins.data_out, Some(0xab));
    assert!(!high_write[2].instruction_complete);

    let t4 = cpu.tick(input(0, true));
    assert_eq!(t4.machine_cycle, MachineCycle::StackWrite);
    assert_eq!(t4.machine_cycle_index, 5);
    assert_eq!(t4.t_state, TState::T4);
    assert!(!t4.instruction_complete);
    assert!(t4.pins.wr_n);
    assert_eq!(t4.pins.data_out, None);

    let t5 = cpu.tick(input(0, true));
    assert_eq!(t5.t_state, TState::T5);
    assert!(t5.instruction_complete);
    assert_eq!(t5.instruction_t_states, 18);
    assert_eq!(cpu.hl(), 0x1234);
    assert_eq!(cpu.registers().sp, 0x3000);
    assert_eq!(cpu.registers().f, 0xd7);
}

#[test]
fn xthl_stack_write_wait_extends_timing_and_holds_bus_stable() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.sp = 0x4000;
    r.h = 0xaa;
    r.l = 0x55;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xe3);
    read_cycle(&mut cpu, 0x78);
    read_cycle(&mut cpu, 0x56);

    cpu.tick(input(0, true)); // M4 T1
    let t2 = cpu.tick(input(0, false));
    assert_eq!(t2.pins.address, Some(0x4000));
    assert_eq!(t2.pins.data_out, Some(0x55));
    let tw = cpu.tick(input(0, true));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(!tw.pins.wr_n);
    assert_eq!(tw.pins.address, Some(0x4000));
    assert_eq!(tw.pins.data_out, Some(0x55));

    cpu.tick(input(0, true)); // M4 T3
    write_cycle(&mut cpu);    // M5 T1..T3
    cpu.tick(input(0, true)); // M5 T4
    let t5 = cpu.tick(input(0, true));
    assert!(t5.instruction_complete);
    assert_eq!(t5.instruction_t_states, 19);
    assert_eq!(cpu.hl(), 0x5678);
}
