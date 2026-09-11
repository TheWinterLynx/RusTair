use rustair::cpu8080_cycle::{
    Cpu8080Cycle, Cpu8080Inputs, MachineCycle, Registers, TState, TickTrace,
};

fn tick_memory(cpu: &mut Cpu8080Cycle, memory: &[u8; 65_536], interrupt: bool) -> TickTrace {
    let data_in = memory[cpu.cycle_address() as usize];
    cpu.tick(Cpu8080Inputs {
        data_in,
        ready: true,
        interrupt,
        ..Cpu8080Inputs::default()
    })
}

#[test]
fn ei_lhld_enables_inte_only_on_the_final_lhld_t_state() {
    let mut memory = [0u8; 65_536];
    memory[0x0000] = 0xfb; // EI
    memory[0x0001] = 0x2a; // LHLD 0010h
    memory[0x0002] = 0x10;
    memory[0x0003] = 0x00;
    memory[0x0010] = 0x5a;
    memory[0x0011] = 0xa5;

    let mut cpu = Cpu8080Cycle::new();
    cpu.set_registers(Registers {
        f: 0x02,
        sp: 0x8000,
        pc: 0,
        ..Registers::default()
    });

    // Keep INT asserted throughout. Since INTE starts low, a real 8080 must
    // still execute EI and the complete following instruction before accepting it.
    for t in 1..=4 {
        let trace = tick_memory(&mut cpu, &memory, true);
        assert_eq!(trace.machine_cycle, MachineCycle::InstructionFetch);
        assert!(!trace.pins.inte, "EI must keep INTE low through T{t}");
    }
    assert_eq!(cpu.registers().pc, 0x0001);
    assert!(!cpu.interrupts_enabled());

    let mut penultimate = None;
    let mut final_trace = None;
    for t in 1..=16 {
        let trace = tick_memory(&mut cpu, &memory, true);
        if t == 15 {
            penultimate = Some(trace);
        }
        if t == 16 {
            final_trace = Some(trace);
        }
    }

    let penultimate = penultimate.expect("LHLD penultimate T-state");
    let final_trace = final_trace.expect("LHLD final T-state");
    assert_eq!(penultimate.machine_cycle, MachineCycle::MemoryRead);
    assert_eq!(penultimate.machine_cycle_index, 5);
    assert_eq!(penultimate.t_state, TState::T2);
    assert!(!penultimate.pins.inte);

    assert_eq!(final_trace.machine_cycle, MachineCycle::MemoryRead);
    assert_eq!(final_trace.machine_cycle_index, 5);
    assert_eq!(final_trace.t_state, TState::T3);
    assert!(final_trace.instruction_complete);
    assert!(
        final_trace.pins.inte,
        "delayed EI must become visible on the exact final T-state of LHLD"
    );
    assert!(cpu.interrupts_enabled());
    assert_eq!((cpu.registers().h, cpu.registers().l), (0xa5, 0x5a));
    assert_eq!(cpu.registers().pc, 0x0004);

    // The same continuously asserted INT is accepted only at the following
    // instruction boundary. It must begin a real INTA cycle, not fetch 0004h.
    let inta_t1 = tick_memory(&mut cpu, &memory, true);
    assert_eq!(inta_t1.machine_cycle, MachineCycle::InterruptAck);
    assert_eq!(inta_t1.machine_cycle_index, 1);
    assert_eq!(inta_t1.t_state, TState::T1);
    assert!(inta_t1.pins.sync);
    assert!(!inta_t1.pins.inte);
    assert!(!cpu.interrupts_enabled());
    assert_eq!(cpu.registers().pc, 0x0004);
}

#[test]
fn ei_di_cancels_the_delayed_enable_even_with_int_asserted() {
    let mut memory = [0u8; 65_536];
    memory[0x0000] = 0xfb; // EI
    memory[0x0001] = 0xf3; // DI is the one-instruction delay slot
    memory[0x0002] = 0x00; // NOP

    let mut cpu = Cpu8080Cycle::new();
    cpu.set_registers(Registers {
        f: 0x02,
        pc: 0,
        ..Registers::default()
    });

    for _ in 0..8 {
        let trace = tick_memory(&mut cpu, &memory, true);
        assert_ne!(trace.machine_cycle, MachineCycle::InterruptAck);
    }
    assert_eq!(cpu.registers().pc, 0x0002);
    assert!(!cpu.interrupts_enabled());

    let next = tick_memory(&mut cpu, &memory, true);
    assert_eq!(next.machine_cycle, MachineCycle::InstructionFetch);
    assert_eq!(next.t_state, TState::T1);
    assert!(!next.pins.inte);
}
