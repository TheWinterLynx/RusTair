use std::time::Instant;

use rustair::cpu8080::{Bus, Cpu8080};

const LOAD_ADDRESS: usize = 0x0100;
const REFERENCE_INSTRUCTIONS: u64 = 2_919_050_698;
const REFERENCE_T_STATES: u64 = 23_803_381_171;
const MAX_INSTRUCTIONS: u64 = REFERENCE_INSTRUCTIONS + 1_000_000;
const PROGRESS_INTERVAL: u64 = 250_000_000;

#[derive(Clone)]
struct ProfileBus {
    memory: Vec<u8>,
}

impl ProfileBus {
    fn new(image: &[u8]) -> Self {
        let mut memory = vec![0u8; 65_536];
        let end = LOAD_ADDRESS + image.len();
        memory[LOAD_ADDRESS..end].copy_from_slice(image);
        // CP/M warm boot / return target. The profiler stops before executing it.
        memory[0] = 0x76; // HLT
        Self { memory }
    }
}

impl Bus for ProfileBus {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        self.memory[address as usize]
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        self.memory[address as usize] = value;
    }
}

#[derive(Clone, Copy, Default)]
struct OpcodeCost {
    instructions: u64,
    t_states: u64,
}

fn cpm_bdos_return(cpu: &mut Cpu8080, bus: &ProfileBus) {
    // A CALL 0005h has already pushed the return address. We deliberately trap
    // BDOS in the host instead of executing a console shim: this profiler is only
    // measuring the diagnostic's dynamic opcode mix, not machine performance.
    let sp = cpu.sp as usize;
    let lo = bus.memory[sp] as u16;
    let hi = bus.memory[sp.wrapping_add(1) & 0xffff] as u16;
    cpu.sp = cpu.sp.wrapping_add(2);
    cpu.pc = lo | (hi << 8);
}

fn current_full_barrier_name(opcode: u8) -> Option<&'static str> {
    Some(match opcode {
        0x09 => "DAD B",
        0x19 => "DAD D",
        0x29 => "DAD H",
        0x39 => "DAD SP",
        0x76 => "HLT",
        0xc0 => "RNZ",
        0xc4 => "CNZ",
        0xc5 => "PUSH B",
        0xc7 => "RST 0",
        0xc8 => "RZ",
        0xcc => "CZ",
        0xcd => "CALL",
        0xcf => "RST 1",
        0xd0 => "RNC",
        0xd3 => "OUT",
        0xd4 => "CNC",
        0xd5 => "PUSH D",
        0xd7 => "RST 2",
        0xd8 => "RC",
        0xdb => "IN",
        0xdc => "CC",
        0xdf => "RST 3",
        0xe0 => "RPO",
        0xe3 => "XTHL",
        0xe4 => "CPO",
        0xe5 => "PUSH H",
        0xe7 => "RST 4",
        0xe8 => "RPE",
        0xec => "CPE",
        0xef => "RST 5",
        0xf0 => "RP",
        0xf3 => "DI",
        0xf4 => "CP",
        0xf5 => "PUSH PSW",
        0xf7 => "RST 6",
        0xf8 => "RM",
        0xfb => "EI",
        0xfc => "CM",
        0xff => "RST 7",
        _ => return None,
    })
}

fn print_barrier_table(costs: &[OpcodeCost; 256], total_instructions: u64, total_t_states: u64) {
    let mut rows: Vec<(u8, OpcodeCost, &'static str)> = (0u16..=255)
        .filter_map(|raw| {
            let opcode = raw as u8;
            let name = current_full_barrier_name(opcode)?;
            let cost = costs[opcode as usize];
            (cost.instructions != 0).then_some((opcode, cost, name))
        })
        .collect();
    rows.sort_unstable_by(|a, b| b.1.t_states.cmp(&a.1.t_states));

    let barrier_instructions: u64 = rows.iter().map(|row| row.1.instructions).sum();
    let barrier_t_states: u64 = rows.iter().map(|row| row.1.t_states).sum();
    eprintln!(
        "8080EXM current-Full barriers: {barrier_instructions} instructions ({:.2}% of diagnostic), {barrier_t_states} T-states ({:.2}% of diagnostic)",
        barrier_instructions as f64 * 100.0 / total_instructions as f64,
        barrier_t_states as f64 * 100.0 / total_t_states as f64,
    );
    eprintln!("opcode  mnemonic   instructions       T-states   % all T   % barrier T");
    for (opcode, cost, name) in rows {
        eprintln!(
            "  {opcode:02X}    {name:<9} {:>12} {:>14} {:>8.3}% {:>11.3}%",
            cost.instructions,
            cost.t_states,
            cost.t_states as f64 * 100.0 / total_t_states as f64,
            cost.t_states as f64 * 100.0 / barrier_t_states.max(1) as f64,
        );
    }
}

#[test]
#[ignore = "semantic workload profiler; run manually in --release"]
fn profile_8080exm_dynamic_pressure_from_current_full_barrier_opcodes() {
    let image = include_bytes!("../assets/cpu-tests/8080EXM.COM");
    let mut bus = ProfileBus::new(image);
    let mut cpu = Cpu8080::new();
    cpu.pc = LOAD_ADDRESS as u16;
    cpu.sp = 0xf000;
    // CP/M launches transient programs with a zero return target available on the
    // initial stack. This lets a plain RET terminate at 0000h.
    bus.memory[cpu.sp as usize] = 0;
    bus.memory[cpu.sp.wrapping_add(1) as usize] = 0;

    let mut costs = [OpcodeCost::default(); 256];
    let mut instructions = 0u64;
    let mut t_states = 0u64;
    let started = Instant::now();
    let mut next_progress = PROGRESS_INTERVAL;

    loop {
        if cpu.pc == 0 {
            break;
        }
        if cpu.pc == 5 {
            cpm_bdos_return(&mut cpu, &bus);
            continue;
        }

        assert!(!cpu.halted, "8080EXM halted before returning to CP/M");
        assert!(instructions < MAX_INSTRUCTIONS, "8080EXM semantic profiler exceeded instruction guard");

        let opcode = bus.memory[cpu.pc as usize];
        let elapsed = u64::from(cpu.step(&mut bus));
        instructions += 1;
        t_states += elapsed;
        let cost = &mut costs[opcode as usize];
        cost.instructions += 1;
        cost.t_states += elapsed;

        if instructions >= next_progress {
            eprintln!(
                "8080EXM semantic profile: {instructions}/{REFERENCE_INSTRUCTIONS} instructions ({:.1}%), {t_states} T-states, {:.1?}",
                instructions as f64 * 100.0 / REFERENCE_INSTRUCTIONS as f64,
                started.elapsed(),
            );
            next_progress = next_progress.saturating_add(PROGRESS_INTERVAL);
        }
    }

    eprintln!(
        "8080EXM semantic profile complete: {instructions} instructions, {t_states} T-states, {:.3?}",
        started.elapsed(),
    );
    assert_eq!(instructions, REFERENCE_INSTRUCTIONS, "8080EXM instruction reference total");
    assert_eq!(t_states, REFERENCE_T_STATES, "8080EXM T-state reference total");
    print_barrier_table(&costs, instructions, t_states);
}
