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

#[test]
fn ei_is_four_t_states_but_does_not_enable_interrupts_immediately() {
    let mut cpu = Cpu8080Cycle::new();

    let ei = fetch(&mut cpu, 0xfb);
    assert!(ei[3].instruction_complete);
    assert_eq!(ei[3].instruction_t_states, 4);
    assert!(!cpu.interrupts_enabled());

    let nop = fetch(&mut cpu, 0x00);
    assert!(nop[3].instruction_complete);
    assert_eq!(nop[3].instruction_t_states, 4);
    assert!(cpu.interrupts_enabled());
    assert!(cpu.pins().inte);
}

#[test]
fn di_immediately_after_ei_cancels_the_pending_enable() {
    let mut cpu = Cpu8080Cycle::new();

    fetch(&mut cpu, 0xfb); // EI
    let di = fetch(&mut cpu, 0xf3); // DI is the delayed instruction.
    assert!(di[3].instruction_complete);
    assert_eq!(di[3].instruction_t_states, 4);
    assert!(!cpu.interrupts_enabled());
    assert!(!cpu.pins().inte);

    fetch(&mut cpu, 0x00);
    assert!(!cpu.interrupts_enabled());
}

#[test]
fn di_disables_an_already_enabled_interrupt_flip_flop_in_four_t_states() {
    let mut cpu = Cpu8080Cycle::new();

    fetch(&mut cpu, 0xfb); // EI
    fetch(&mut cpu, 0x00); // delayed instruction -> INTE becomes active.
    assert!(cpu.interrupts_enabled());

    let di = fetch(&mut cpu, 0xf3);
    assert!(di[3].instruction_complete);
    assert_eq!(di[3].instruction_t_states, 4);
    assert!(!cpu.interrupts_enabled());
}

#[test]
fn hlt_uses_a_real_halt_ack_cycle_and_enters_indefinite_halt_dwell() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    cpu.set_registers(r);

    let m1 = fetch(&mut cpu, 0x76);
    assert!(!m1[3].instruction_complete);
    assert_eq!(cpu.machine_cycle(), MachineCycle::HaltAck);

    let t1 = cpu.tick(input(0, true));
    assert_eq!(t1.machine_cycle, MachineCycle::HaltAck);
    assert_eq!(t1.machine_cycle_index, 2);
    assert_eq!(t1.t_state, TState::T1);
    assert_eq!(t1.pins.address, Some(0x2001));
    assert_eq!(t1.pins.data_out, Some(0x8a));
    assert!(t1.pins.sync);
    assert!(!t1.pins.dbin);

    let t2 = cpu.tick(input(0, true));
    assert_eq!(t2.t_state, TState::T2);
    assert!(!t2.pins.dbin);

    let t3 = cpu.tick(input(0, true));
    assert_eq!(t3.t_state, TState::T3);
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 7);
    assert!(cpu.is_halted());
    assert_eq!(cpu.registers().pc, 0x2001);
    assert_eq!(cpu.last_instruction_t_states(), Some(7));

    let total_before = cpu.total_t_states();
    let dwell = cpu.tick(input(0xff, true));
    assert_eq!(dwell.machine_cycle, MachineCycle::HaltAck);
    assert_eq!(dwell.t_state, TState::Thalt);
    assert!(!dwell.instruction_complete);
    assert_eq!(dwell.instruction_t_states, 0);
    assert_eq!(cpu.total_t_states(), total_before + 1);
    assert_eq!(cpu.registers().pc, 0x2001);
    assert!(cpu.is_halted());
}

#[test]
fn ei_followed_by_hlt_enables_inte_when_hlt_completes_and_reset_releases_halt() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x5a;
    r.pc = 0x1000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xfb); // EI
    assert!(!cpu.interrupts_enabled());

    fetch(&mut cpu, 0x76); // HLT is the instruction after EI.
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    let halt = cpu.tick(input(0, true));
    assert!(halt.instruction_complete);
    assert!(cpu.is_halted());
    assert!(cpu.interrupts_enabled());
    assert!(cpu.pins().inte);

    let reset = cpu.tick(Cpu8080Inputs { reset: true, ..Cpu8080Inputs::default() });
    assert!(reset.reset);
    assert!(!cpu.is_halted());
    assert!(!cpu.interrupts_enabled());
    assert_eq!(cpu.registers().pc, 0);
    assert_eq!(cpu.registers().a, 0x5a);
    assert_eq!(cpu.machine_cycle(), MachineCycle::InstructionFetch);
    assert_eq!(cpu.t_state(), TState::T1);
}
