use super::*;

fn input(data_in: u8, ready: bool) -> Cpu8080Inputs {
    Cpu8080Inputs { data_in, ready, ..Cpu8080Inputs::default() }
}

fn irq_input(data_in: u8, ready: bool) -> Cpu8080Inputs {
    Cpu8080Inputs { data_in, ready, interrupt: true, ..Cpu8080Inputs::default() }
}

fn fetch(cpu: &mut Cpu8080Cycle, opcode: u8) -> [TickTrace; 4] {
    [
        cpu.tick(input(0, true)),
        cpu.tick(input(0, true)),
        cpu.tick(input(opcode, true)),
        cpu.tick(input(0, true)),
    ]
}

fn enable_interrupts(cpu: &mut Cpu8080Cycle) {
    fetch(cpu, 0xfb); // EI
    fetch(cpu, 0x00); // delayed instruction
    assert!(cpu.interrupts_enabled());
}

fn complete_rst_after_ack_t3(cpu: &mut Cpu8080Cycle) -> TickTrace {
    cpu.tick(input(0, true)); // inherited M1/INTA T4
    cpu.tick(input(0, true)); // inherited M1/INTA T5
    cpu.tick(input(0, true)); // stack write M2 T1
    cpu.tick(input(0, true)); // stack write M2 T2
    cpu.tick(input(0, true)); // stack write M2 T3
    cpu.tick(input(0, true)); // stack write M3 T1
    cpu.tick(input(0, true)); // stack write M3 T2
    cpu.tick(input(0, true))  // stack write M3 T3
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

#[test]
fn normal_interrupt_ack_executes_external_rst_without_incrementing_pc() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x2000;
    r.sp = 0x4000;
    cpu.set_registers(r);
    enable_interrupts(&mut cpu);

    let return_pc = cpu.registers().pc;
    assert_eq!(return_pc, 0x2002);

    let t1 = cpu.tick(irq_input(0, true));
    assert_eq!(t1.machine_cycle, MachineCycle::InterruptAck);
    assert_eq!(t1.machine_cycle_index, 1);
    assert_eq!(t1.t_state, TState::T1);
    assert_eq!(t1.pins.address, Some(return_pc));
    assert_eq!(t1.pins.data_out, Some(0x23));
    assert!(t1.pins.sync);
    assert!(!cpu.interrupts_enabled());
    assert!(!t1.pins.inte);

    let t2 = cpu.tick(irq_input(0, true));
    assert_eq!(t2.t_state, TState::T2);
    assert!(t2.pins.dbin);

    let t3 = cpu.tick(irq_input(0xcf, true)); // RST 1 from interrupting hardware.
    assert_eq!(t3.t_state, TState::T3);
    assert_eq!(t3.opcode, Some(0xcf));
    assert_eq!(cpu.registers().pc, return_pc);
    assert_eq!(cpu.t_state(), TState::T4);

    cpu.tick(input(0, true)); // T4
    cpu.tick(input(0, true)); // T5
    let high_t1 = cpu.tick(input(0, true));
    let high_t2 = cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    let low_t1 = cpu.tick(input(0, true));
    let low_t2 = cpu.tick(input(0, true));
    let done = cpu.tick(input(0, true));

    assert_eq!(high_t1.machine_cycle, MachineCycle::StackWrite);
    assert_eq!(high_t1.pins.address, Some(0x3fff));
    assert_eq!(high_t2.pins.data_out, Some(0x20));
    assert_eq!(low_t1.pins.address, Some(0x3ffe));
    assert_eq!(low_t2.pins.data_out, Some(0x02));
    assert!(done.instruction_complete);
    assert_eq!(done.instruction_t_states, 11);
    assert_eq!(cpu.last_instruction_t_states(), Some(11));
    assert_eq!(cpu.registers().sp, 0x3ffe);
    assert_eq!(cpu.registers().pc, 0x0008);
}

#[test]
fn ready_low_in_interrupt_ack_inserts_tw_before_sampling_external_opcode() {
    let mut cpu = Cpu8080Cycle::new();
    enable_interrupts(&mut cpu);

    let t1 = cpu.tick(irq_input(0, true));
    let t2 = cpu.tick(irq_input(0, false));
    let tw = cpu.tick(irq_input(0, true));
    let t3 = cpu.tick(irq_input(0xc7, true)); // RST 0

    assert_eq!(t1.machine_cycle, MachineCycle::InterruptAck);
    assert_eq!(t2.t_state, TState::T2);
    assert!(t2.pins.dbin);
    assert_eq!(tw.t_state, TState::Tw);
    assert!(tw.pins.wait);
    assert!(tw.pins.dbin);
    assert_eq!(tw.pins.address, t1.pins.address);
    assert_eq!(t3.opcode, Some(0xc7));

    let done = complete_rst_after_ack_t3(&mut cpu);
    assert!(done.instruction_complete);
    assert_eq!(done.instruction_t_states, 12);
    assert_eq!(cpu.registers().pc, 0x0000);
}

#[test]
fn interrupt_while_halted_uses_2b_status_and_wakes_into_external_rst() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x3000;
    r.sp = 0x5000;
    cpu.set_registers(r);

    fetch(&mut cpu, 0xfb); // EI
    fetch(&mut cpu, 0x76); // HLT is the delayed instruction.
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    assert!(cpu.is_halted());
    assert!(cpu.interrupts_enabled());
    assert_eq!(cpu.registers().pc, 0x3002);

    let t1 = cpu.tick(irq_input(0, true));
    assert_eq!(t1.machine_cycle, MachineCycle::InterruptAckWhileHalt);
    assert_eq!(t1.machine_cycle_index, 1);
    assert_eq!(t1.t_state, TState::T1);
    assert_eq!(t1.pins.address, Some(0x3002));
    assert_eq!(t1.pins.data_out, Some(0x2b));
    assert!(t1.pins.sync);
    assert!(!cpu.is_halted());
    assert!(!cpu.interrupts_enabled());

    cpu.tick(irq_input(0, true));
    cpu.tick(irq_input(0xff, true)); // RST 7
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    let high_t1 = cpu.tick(input(0, true));
    let high_t2 = cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    let low_t1 = cpu.tick(input(0, true));
    let low_t2 = cpu.tick(input(0, true));
    let done = cpu.tick(input(0, true));

    assert_eq!(high_t1.pins.address, Some(0x4fff));
    assert_eq!(high_t2.pins.data_out, Some(0x30));
    assert_eq!(low_t1.pins.address, Some(0x4ffe));
    assert_eq!(low_t2.pins.data_out, Some(0x02));
    assert!(done.instruction_complete);
    assert_eq!(done.instruction_t_states, 11);
    assert_eq!(cpu.registers().sp, 0x4ffe);
    assert_eq!(cpu.registers().pc, 0x0038);
}

#[test]
fn masked_interrupt_does_not_release_halt() {
    let mut cpu = Cpu8080Cycle::new();
    let mut r = Registers::default();
    r.pc = 0x4100;
    cpu.set_registers(r);

    fetch(&mut cpu, 0x76);
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    cpu.tick(input(0, true));
    assert!(cpu.is_halted());
    assert!(!cpu.interrupts_enabled());

    let dwell = cpu.tick(irq_input(0xff, true));
    assert_eq!(dwell.machine_cycle, MachineCycle::HaltAck);
    assert_eq!(dwell.t_state, TState::Thalt);
    assert!(cpu.is_halted());
    assert_eq!(cpu.registers().pc, 0x4101);
}

#[test]
fn interrupt_request_is_only_accepted_at_an_instruction_boundary() {
    let mut cpu = Cpu8080Cycle::new();
    enable_interrupts(&mut cpu);

    // Start MVI A,d8 with no interrupt request during M1.
    fetch(&mut cpu, 0x3e);
    assert_eq!(cpu.machine_cycle(), MachineCycle::MemoryRead);

    let read_t1 = cpu.tick(irq_input(0, true));
    let read_t2 = cpu.tick(irq_input(0, true));
    let read_t3 = cpu.tick(irq_input(0x5a, true));
    assert_eq!(read_t1.machine_cycle, MachineCycle::MemoryRead);
    assert_eq!(read_t2.machine_cycle, MachineCycle::MemoryRead);
    assert!(read_t3.instruction_complete);
    assert_eq!(cpu.registers().a, 0x5a);
    assert!(cpu.interrupts_enabled());

    // The same still-asserted INT line is accepted on the next boundary.
    let ack_t1 = cpu.tick(irq_input(0, true));
    assert_eq!(ack_t1.machine_cycle, MachineCycle::InterruptAck);
    assert_eq!(ack_t1.t_state, TState::T1);
    assert!(!cpu.interrupts_enabled());
}
