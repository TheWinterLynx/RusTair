use rustair::cpu8080::{Bus, Cpu8080};
use rustair::cpu8080_cycle::{Cpu8080Cycle, Cpu8080Inputs, MachineCycle, Registers, TState};

#[derive(Clone, Debug)]
struct DifferentialBus {
    memory: Vec<u8>,
    outputs: Vec<(u8, u8)>,
}

impl DifferentialBus {
    fn seeded(opcode: u8) -> Self {
        let mut memory = vec![0u8; 65536];
        for (address, byte) in memory.iter_mut().enumerate() {
            let a = address as u16;
            *byte = (a as u8)
                .wrapping_mul(37)
                .wrapping_add((a >> 8) as u8)
                .rotate_left(1)
                ^ 0x5a;
        }

        // Instruction and immediate operands.
        memory[0x2000] = opcode;
        memory[0x2001] = 0x34;
        memory[0x2002] = 0x12;

        // Deterministic data for HL/DE/BC addressed operations.
        memory[0x3000] = 0xa6;
        memory[0x3100] = 0x5c;
        memory[0x3200] = 0xc3;

        // Stack data used by POP/RET/XTHL.
        memory[0x4000] = 0x78;
        memory[0x4001] = 0x56;

        // Direct-address data used by LDA/LHLD and overwritten by stores.
        memory[0x1234] = 0x9b;
        memory[0x1235] = 0xcd;

        Self {
            memory,
            outputs: Vec::new(),
        }
    }

    fn input_value(port: u8) -> u8 {
        port.rotate_left(1) ^ 0xa5
    }
}

impl Bus for DifferentialBus {
    fn read(&mut self, address: u16) -> u8 {
        self.memory[address as usize]
    }

    fn write(&mut self, address: u16, value: u8) {
        self.memory[address as usize] = value;
    }

    fn input(&mut self, port: u8) -> u8 {
        Self::input_value(port)
    }

    fn output(&mut self, port: u8, value: u8) {
        self.outputs.push((port, value));
    }
}

fn seed_fast(a: u8, flags: u8) -> Cpu8080 {
    let mut cpu = Cpu8080::new();
    cpu.a = a;
    cpu.b = 0x32;
    cpu.c = 0x00;
    cpu.d = 0x31;
    cpu.e = 0x00;
    cpu.h = 0x30;
    cpu.l = 0x00;
    cpu.f = flags;
    cpu.pc = 0x2000;
    cpu.sp = 0x4000;
    cpu.inte = false;
    cpu.halted = false;
    cpu
}

fn seed_cycle(a: u8, flags: u8) -> Cpu8080Cycle {
    let mut cpu = Cpu8080Cycle::new();
    cpu.set_registers(Registers {
        a,
        b: 0x32,
        c: 0x00,
        d: 0x31,
        e: 0x00,
        h: 0x30,
        l: 0x00,
        f: flags,
        sp: 0x4000,
        pc: 0x2000,
    });
    cpu
}

fn cycle_data_in(cpu: &Cpu8080Cycle, bus: &DifferentialBus) -> u8 {
    if cpu.t_state() != TState::T3 {
        return 0;
    }

    match cpu.machine_cycle() {
        MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead => {
            let address = cpu
                .pins()
                .address
                .expect("read cycle must expose an address before T3");
            bus.memory[address as usize]
        }
        MachineCycle::InputRead => {
            let address = cpu
                .pins()
                .address
                .expect("input cycle must expose the duplicated port address");
            DifferentialBus::input_value(address as u8)
        }
        MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => {
            panic!("single-opcode differential does not inject interrupts")
        }
        _ => 0,
    }
}

fn apply_cycle_write(trace: &rustair::cpu8080_cycle::TickTrace, bus: &mut DifferentialBus) {
    if trace.t_state != TState::T3 {
        return;
    }

    match trace.machine_cycle {
        MachineCycle::MemoryWrite | MachineCycle::StackWrite => {
            let address = trace
                .pins
                .address
                .expect("write cycle must expose an address at T3");
            let value = trace
                .pins
                .data_out
                .expect("write cycle must expose data at T3");
            bus.memory[address as usize] = value;
        }
        MachineCycle::OutputWrite => {
            let address = trace
                .pins
                .address
                .expect("output cycle must expose the duplicated port address");
            let value = trace
                .pins
                .data_out
                .expect("output cycle must expose accumulator data at T3");
            bus.outputs.push((address as u8, value));
        }
        _ => {}
    }
}

fn run_cycle_instruction(cpu: &mut Cpu8080Cycle, bus: &mut DifferentialBus) -> u32 {
    for _ in 0..64 {
        let data_in = cycle_data_in(cpu, bus);
        let trace = cpu.tick(Cpu8080Inputs {
            data_in,
            ready: true,
            interrupt: false,
            hold: false,
            reset: false,
        });
        apply_cycle_write(&trace, bus);
        assert_eq!(trace.fault, None, "cycle core faulted on opcode {:?}", trace.opcode);
        if trace.instruction_complete {
            return trace.instruction_t_states;
        }
    }

    panic!("cycle core did not complete one instruction within 64 T-states");
}

fn assert_same_registers(opcode: u8, seed: usize, fast: &Cpu8080, cycle: &Cpu8080Cycle) {
    let r = cycle.registers();
    assert_eq!(r.a, fast.a, "opcode {opcode:02x} seed {seed}: A");
    assert_eq!(r.b, fast.b, "opcode {opcode:02x} seed {seed}: B");
    assert_eq!(r.c, fast.c, "opcode {opcode:02x} seed {seed}: C");
    assert_eq!(r.d, fast.d, "opcode {opcode:02x} seed {seed}: D");
    assert_eq!(r.e, fast.e, "opcode {opcode:02x} seed {seed}: E");
    assert_eq!(r.h, fast.h, "opcode {opcode:02x} seed {seed}: H");
    assert_eq!(r.l, fast.l, "opcode {opcode:02x} seed {seed}: L");
    assert_eq!(r.f, fast.f, "opcode {opcode:02x} seed {seed}: F");
    assert_eq!(r.pc, fast.pc, "opcode {opcode:02x} seed {seed}: PC");
    assert_eq!(r.sp, fast.sp, "opcode {opcode:02x} seed {seed}: SP");
    assert_eq!(cycle.interrupts_enabled(), fast.inte, "opcode {opcode:02x} seed {seed}: INTE");
    assert_eq!(cycle.is_halted(), fast.halted, "opcode {opcode:02x} seed {seed}: HALT");
}

fn assert_same_memory(opcode: u8, seed: usize, fast: &DifferentialBus, cycle: &DifferentialBus) {
    if let Some((address, (fast_value, cycle_value))) = fast
        .memory
        .iter()
        .zip(&cycle.memory)
        .enumerate()
        .find(|(_, (a, b))| a != b)
    {
        panic!(
            "opcode {opcode:02x} seed {seed}: memory mismatch at {address:04x}: fast={fast_value:02x} cycle={cycle_value:02x}"
        );
    }
}

#[test]
fn all_256_8080_opcodes_match_the_validated_fast_core() {
    // Two complementary flag/accumulator states exercise both polarities of
    // every conditional family and carry-sensitive arithmetic paths.
    let seeds = [(0x96, 0x02), (0x0f, 0xd7)];

    for opcode in 0u16..=0xff {
        let opcode = opcode as u8;
        for (seed_index, (a, flags)) in seeds.into_iter().enumerate() {
            let initial_bus = DifferentialBus::seeded(opcode);
            let mut fast_bus = initial_bus.clone();
            let mut cycle_bus = initial_bus;
            let mut fast = seed_fast(a, flags);
            let mut cycle = seed_cycle(a, flags);

            let fast_t_states = fast.step(&mut fast_bus);
            let cycle_t_states = run_cycle_instruction(&mut cycle, &mut cycle_bus);

            assert_eq!(
                cycle_t_states, fast_t_states,
                "opcode {opcode:02x} seed {seed_index}: T-state count"
            );
            assert_same_registers(opcode, seed_index, &fast, &cycle);
            assert_same_memory(opcode, seed_index, &fast_bus, &cycle_bus);
            assert_eq!(
                cycle_bus.outputs, fast_bus.outputs,
                "opcode {opcode:02x} seed {seed_index}: output events"
            );
        }
    }
}
