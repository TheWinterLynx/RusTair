use std::hint::black_box;
use std::mem::size_of;
use std::time::Instant;

use rustair::cpu8080_cycle::{Cpu8080Cycle, Cpu8080Inputs, Cpu8080Pins, TickTrace};

const T_STATES_PER_CASE: u64 = 50_000_000;

fn stable_inputs(opcode: u8) -> Cpu8080Inputs {
    Cpu8080Inputs {
        data_in: opcode,
        ready: true,
        interrupt: false,
        hold: false,
        reset: false,
    }
}

fn profile_opcode(name: &str, opcode: u8) {
    let mut cpu = Cpu8080Cycle::new();
    let inputs = stable_inputs(opcode);

    let started = Instant::now();
    let mut completed = 0u64;
    for _ in 0..T_STATES_PER_CASE {
        let trace = cpu.tick(inputs);
        completed += trace.instruction_complete as u64;
        black_box(trace);
    }
    let elapsed = started.elapsed();
    let mticks = T_STATES_PER_CASE as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    eprintln!(
        "[CPU CYCLE HOTPATH] {name:<12} opcode={opcode:02X}  {T_STATES_PER_CASE} T  {completed} instructions  {:.3?}  {mticks:.2} M T-state/s",
        elapsed,
    );
}

fn profile_nop_trace_materialization() {
    let inputs = stable_inputs(0x00);

    let mut full_trace_cpu = Cpu8080Cycle::new();
    let mut completed = 0u64;
    let started = Instant::now();
    for _ in 0..T_STATES_PER_CASE {
        let trace = full_trace_cpu.tick(inputs);
        completed += trace.instruction_complete as u64;
        black_box(trace);
    }
    let elapsed = started.elapsed();
    eprintln!(
        "[CPU CYCLE TRACE COST] full TickTrace black_box : {:.3?}  {:.2} M T-state/s  completed={completed}",
        elapsed,
        T_STATES_PER_CASE as f64 / elapsed.as_secs_f64() / 1_000_000.0,
    );

    // Consume every externally visible field, but scalarize them into a checksum
    // instead of forcing the 40-byte TickTrace aggregate through black_box. This
    // distinguishes the cost of computing/using all observations from the cost
    // of materializing/copying the aggregate itself.
    let mut scalar_cpu = Cpu8080Cycle::new();
    let mut checksum = 0u64;
    let started = Instant::now();
    for _ in 0..T_STATES_PER_CASE {
        let trace = scalar_cpu.tick(inputs);
        checksum = checksum
            .wrapping_add(trace.total_t_states)
            .wrapping_add(u64::from(trace.instruction_t_states))
            .wrapping_add(trace.machine_cycle as u64)
            .wrapping_add(u64::from(trace.machine_cycle_index))
            .wrapping_add(trace.t_state as u64)
            .wrapping_add(u64::from(trace.opcode.unwrap_or(0)))
            .wrapping_add(trace.instruction_complete as u64)
            .wrapping_add(trace.reset as u64)
            .wrapping_add(trace.fault.is_some() as u64)
            .wrapping_add(trace.pins.phi1 as u64)
            .wrapping_add(trace.pins.phi2 as u64)
            .wrapping_add(u64::from(trace.pins.address.unwrap_or(0)))
            .wrapping_add(u64::from(trace.pins.data_out.unwrap_or(0)))
            .wrapping_add(trace.pins.sync as u64)
            .wrapping_add(trace.pins.dbin as u64)
            .wrapping_add(trace.pins.wr_n as u64)
            .wrapping_add(trace.pins.inte as u64)
            .wrapping_add(trace.pins.wait as u64)
            .wrapping_add(trace.pins.hlda as u64);
    }
    black_box(checksum);
    let elapsed = started.elapsed();
    eprintln!(
        "[CPU CYCLE TRACE COST] all fields scalarized     : {:.3?}  {:.2} M T-state/s  checksum={checksum}",
        elapsed,
        T_STATES_PER_CASE as f64 / elapsed.as_secs_f64() / 1_000_000.0,
    );

    let mut completion_cpu = Cpu8080Cycle::new();
    let mut completed = 0u64;
    let started = Instant::now();
    for _ in 0..T_STATES_PER_CASE {
        let trace = completion_cpu.tick(inputs);
        completed += trace.instruction_complete as u64;
    }
    black_box((
        completion_cpu.total_t_states(),
        completion_cpu.registers(),
        completion_cpu.pins(),
        completed,
    ));
    let elapsed = started.elapsed();
    eprintln!(
        "[CPU CYCLE TRACE COST] completion field only    : {:.3?}  {:.2} M T-state/s  completed={completed}",
        elapsed,
        T_STATES_PER_CASE as f64 / elapsed.as_secs_f64() / 1_000_000.0,
    );

    let mut state_only_cpu = Cpu8080Cycle::new();
    let started = Instant::now();
    for _ in 0..T_STATES_PER_CASE {
        let _ = state_only_cpu.tick(inputs);
    }
    black_box((
        state_only_cpu.total_t_states(),
        state_only_cpu.registers(),
        state_only_cpu.pins(),
    ));
    let elapsed = started.elapsed();
    eprintln!(
        "[CPU CYCLE TRACE COST] return ignored/state end : {:.3?}  {:.2} M T-state/s",
        elapsed,
        T_STATES_PER_CASE as f64 / elapsed.as_secs_f64() / 1_000_000.0,
    );
}

#[test]
#[ignore = "manual release-mode Cpu8080Cycle hot-path profiler"]
fn profile_cpu8080_cycle_hot_instruction_families() {
    eprintln!(
        "[CPU CYCLE LAYOUT] Cpu8080Cycle={} B, TickTrace={} B, Cpu8080Pins={} B, Cpu8080Inputs={} B",
        size_of::<Cpu8080Cycle>(),
        size_of::<TickTrace>(),
        size_of::<Cpu8080Pins>(),
        size_of::<Cpu8080Inputs>(),
    );

    profile_nop_trace_materialization();

    // Constant data_in is sufficient here because the core samples it only on
    // read T3. For multi-byte control flow the immediate/stack bytes therefore
    // become the same constant value as the opcode, which still produces a
    // stable repeating instruction family without a bus or harness in the loop.
    profile_opcode("NOP", 0x00);      // 4T
    profile_opcode("MOV B,B", 0x40);  // 5T
    profile_opcode("INR B", 0x04);    // 5T + SZP/parity
    profile_opcode("ADD B", 0x80);    // 4T + SZP/parity
    profile_opcode("ADI imm", 0xC6);  // 7T + operand read + SZP/parity
    profile_opcode("DAD B", 0x09);    // 10T, internal cycles
    profile_opcode("PUSH B", 0xC5);   // 11T, two stack writes
    profile_opcode("JMP", 0xC3);      // 10T, two operand reads
    profile_opcode("RET", 0xC9);      // 10T, two stack reads
    profile_opcode("CALL", 0xCD);     // 17T, operand reads + stack writes
}
