use super::*;

fn input(data_in: u8, ready: bool, hold: bool, interrupt: bool) -> Cpu8080Inputs {
    Cpu8080Inputs {
        data_in,
        ready,
        hold,
        interrupt,
        ..Cpu8080Inputs::default()
    }
}

fn normal(data_in: u8) -> Cpu8080Inputs {
    input(data_in, true, false, false)
}

fn fetch(cpu: &mut Cpu8080Cycle, opcode: u8) -> [TickTrace; 4] {
    [
        cpu.tick(normal(0)),
        cpu.tick(normal(0)),
        cpu.tick(normal(opcode)),
        cpu.tick(normal(0)),
    ]
}

fn enable_interrupts(cpu: &mut Cpu8080Cycle) {
    fetch(cpu, 0xfb); // EI
    fetch(cpu, 0x00); // delayed instruction
    assert!(cpu.interrupts_enabled());
}

#[test]
fn hold_sampled_in_t2_waits_for_machine_cycle_boundary_and_resumes_exactly() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x1000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x01); // LXI B,d16 -> M2 reads low byte.
    assert_eq!(cpu.machine_cycle(), MachineCycle::MemoryRead);
    assert_eq!(cpu.machine_cycle_index(), 2);

    let m2_t1 = cpu.tick(normal(0));
    assert_eq!(m2_t1.t_state, TState::T1);
    assert_eq!(m2_t1.pins.address, Some(0x1001));

    let m2_t2 = cpu.tick(input(0, true, true, false));
    assert_eq!(m2_t2.t_state, TState::T2);
    assert!(m2_t2.pins.dbin);
    assert!(!m2_t2.pins.hlda);
    assert!(!cpu.is_holding());

    let m2_t3 = cpu.tick(input(0x34, true, true, false));
    assert_eq!(m2_t3.t_state, TState::T3);
    assert!(!m2_t3.pins.hlda);
    assert!(cpu.is_holding());
    assert_eq!(cpu.t_state(), TState::Thold);
    assert_eq!(cpu.machine_cycle(), MachineCycle::MemoryRead);
    assert_eq!(cpu.machine_cycle_index(), 3);

    let total_before = cpu.total_t_states();
    let hold1 = cpu.tick(input(0, true, true, false));
    assert_eq!(hold1.t_state, TState::Thold);
    assert!(hold1.pins.hlda);
    assert_eq!(hold1.pins.address, None);
    assert_eq!(hold1.pins.data_out, None);
    assert!(!hold1.pins.sync);
    assert!(!hold1.pins.dbin);
    assert!(hold1.pins.wr_n);
    assert!(!hold1.pins.wait);
    assert_eq!(hold1.instruction_t_states, 7);
    assert_eq!(cpu.total_t_states(), total_before + 1);

    let hold2 = cpu.tick(input(0, true, true, false));
    assert_eq!(hold2.instruction_t_states, 7);
    assert!(hold2.pins.hlda);

    // Releasing HOLD resumes the already-prepared M3/T1. HOLD dwell time is
    // wall-clock time only and does not inflate the instruction's 10T timing.
    let resume = cpu.tick(input(0, true, false, false));
    assert_eq!(resume.machine_cycle, MachineCycle::MemoryRead);
    assert_eq!(resume.machine_cycle_index, 3);
    assert_eq!(resume.t_state, TState::T1);
    assert_eq!(resume.pins.address, Some(0x1002));
    assert_eq!(resume.pins.data_out, Some(0x82));
    assert!(resume.pins.sync);
    assert!(!resume.pins.hlda);
    assert!(!cpu.is_holding());

    cpu.tick(normal(0));
    let finish = cpu.tick(normal(0x12));
    assert!(finish.instruction_complete);
    assert_eq!(finish.instruction_t_states, 10);
    assert_eq!(cpu.registers().b, 0x12);
    assert_eq!(cpu.registers().c, 0x34);
    assert_eq!(cpu.registers().pc, 0x1003);
}

#[test]
fn ready_wait_finishes_before_hold_grant_and_write_bus_stays_valid() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.h = 0x20;
    r.l = 0x00;
    r.a = 0xa5;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x77); // MOV M,A -> M2 MemoryWrite.
    let t1 = cpu.tick(normal(0));
    assert_eq!(t1.machine_cycle, MachineCycle::MemoryWrite);
    assert_eq!(t1.pins.address, Some(0x2000));

    let t2 = cpu.tick(input(0, false, true, false));
    assert_eq!(t2.t_state, TState::T2);
    assert_eq!(t2.pins.data_out, Some(0xa5));
    assert!(t2.pins.wr_n);
    assert!(!cpu.is_holding());

    let tw = cpu.tick(input(0, true, true, false));
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(!tw.pins.wr_n);
    assert_eq!(tw.pins.address, Some(0x2000));
    assert_eq!(tw.pins.data_out, Some(0xa5));
    assert!(!cpu.is_holding());

    let t3 = cpu.tick(input(0, true, true, false));
    assert_eq!(t3.t_state, TState::T3);
    assert!(!t3.pins.wr_n);
    assert_eq!(t3.pins.address, Some(0x2000));
    assert_eq!(t3.pins.data_out, Some(0xa5));
    assert!(t3.instruction_complete);
    assert_eq!(t3.instruction_t_states, 8);
    assert!(cpu.is_holding());

    let granted = cpu.tick(input(0, true, true, false));
    assert_eq!(granted.t_state, TState::Thold);
    assert!(granted.pins.hlda);
    assert_eq!(granted.pins.address, None);
    assert_eq!(granted.pins.data_out, None);
    assert!(granted.pins.wr_n);
}

#[test]
fn hold_first_asserted_after_t2_is_not_granted_until_a_later_machine_cycle() {
    let mut cpu = Cpu8080Cycle::new();

    let t1 = cpu.tick(normal(0));
    let t2 = cpu.tick(normal(0));
    let t3 = cpu.tick(input(0x00, true, true, false)); // HOLD appears too late for this M1.
    let t4 = cpu.tick(input(0, true, true, false));

    assert_eq!(t1.t_state, TState::T1);
    assert_eq!(t2.t_state, TState::T2);
    assert_eq!(t3.t_state, TState::T3);
    assert!(t4.instruction_complete);
    assert!(!cpu.is_holding());

    // HOLD is still high, so the following M1 samples it in T2 and grants it
    // only after that complete 4T NOP machine cycle.
    cpu.tick(input(0, true, true, false)); // T1
    cpu.tick(input(0, true, true, false)); // T2: sampled
    cpu.tick(input(0x00, true, true, false)); // T3
    let next_t4 = cpu.tick(input(0, true, true, false));
    assert!(next_t4.instruction_complete);
    assert!(cpu.is_holding());

    let hold = cpu.tick(input(0, true, true, false));
    assert!(hold.pins.hlda);
    assert_eq!(hold.t_state, TState::Thold);
}

#[test]
fn output_write_completes_before_hlda_tristates_the_bus() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x5a;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xd3); // OUT d8
    cpu.tick(normal(0));
    cpu.tick(normal(0));
    cpu.tick(normal(0x12)); // immediate -> begin M3 OutputWrite

    let out_t1 = cpu.tick(normal(0));
    assert_eq!(out_t1.machine_cycle, MachineCycle::OutputWrite);
    assert_eq!(out_t1.pins.data_out, Some(0x10));
    assert_eq!(out_t1.pins.address, Some(0x1212));

    let out_t2 = cpu.tick(input(0, true, true, false));
    assert_eq!(out_t2.t_state, TState::T2);
    assert_eq!(out_t2.pins.data_out, Some(0x5a));
    assert!(out_t2.pins.wr_n);
    assert!(!out_t2.pins.hlda);

    let out_t3 = cpu.tick(input(0, true, true, false));
    assert_eq!(out_t3.t_state, TState::T3);
    assert_eq!(out_t3.pins.address, Some(0x1212));
    assert_eq!(out_t3.pins.data_out, Some(0x5a));
    assert!(!out_t3.pins.wr_n);
    assert!(!out_t3.pins.hlda);
    assert!(out_t3.instruction_complete);
    assert!(cpu.is_holding());

    let hold = cpu.tick(input(0, true, true, false));
    assert!(hold.pins.hlda);
    assert_eq!(hold.pins.address, None);
    assert_eq!(hold.pins.data_out, None);
    assert!(hold.pins.wr_n);
}

#[test]
fn hold_has_priority_over_interrupt_while_halted_then_inta_follows_release() {
    let mut cpu = Cpu8080Cycle::new();
    enable_interrupts(&mut cpu);

    fetch(&mut cpu, 0x76); // HLT
    cpu.tick(normal(0));
    cpu.tick(normal(0));
    let halt = cpu.tick(normal(0));
    assert!(halt.instruction_complete);
    assert!(cpu.is_halted());
    assert!(cpu.interrupts_enabled());

    let hold = cpu.tick(input(0xcf, true, true, true));
    assert_eq!(hold.t_state, TState::Thold);
    assert!(hold.pins.hlda);
    assert_eq!(hold.pins.address, None);
    assert!(cpu.is_holding());
    assert!(cpu.is_halted());
    assert!(cpu.interrupts_enabled());

    // On HOLD release, a still-asserted INT is acknowledged immediately using
    // the special interrupt-while-halted status word 2Bh.
    let inta_t1 = cpu.tick(input(0, true, false, true));
    assert_eq!(inta_t1.machine_cycle, MachineCycle::InterruptAckWhileHalt);
    assert_eq!(inta_t1.t_state, TState::T1);
    assert_eq!(inta_t1.pins.data_out, Some(0x2b));
    assert!(inta_t1.pins.sync);
    assert!(!inta_t1.pins.hlda);
    assert!(!cpu.is_holding());
    assert!(!cpu.is_halted());
    assert!(!cpu.interrupts_enabled());
}

#[test]
fn reset_during_hold_reclaims_bus_and_clears_hlda() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.a = 0x5a;
    r.pc = 0x4321;
    cpu.set_registers(r);

    cpu.tick(normal(0)); // M1 T1
    cpu.tick(input(0, true, true, false)); // M1 T2 samples HOLD
    cpu.tick(input(0x00, true, true, false)); // M1 T3
    cpu.tick(input(0, true, true, false)); // M1 T4 -> boundary/grant
    assert!(cpu.is_holding());

    let held = cpu.tick(input(0, true, true, false));
    assert!(held.pins.hlda);
    assert_eq!(held.pins.address, None);

    let reset = cpu.tick(Cpu8080Inputs {
        reset: true,
        hold: true,
        ..Cpu8080Inputs::default()
    });
    assert!(reset.reset);
    assert!(!reset.pins.hlda);
    assert!(!cpu.is_holding());
    assert!(!cpu.is_halted());
    assert_eq!(cpu.t_state(), TState::T1);
    assert_eq!(cpu.machine_cycle(), MachineCycle::InstructionFetch);
    assert_eq!(cpu.registers().pc, 0);
    assert_eq!(cpu.registers().a, 0x5a);
}
