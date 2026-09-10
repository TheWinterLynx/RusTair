use std::time::Instant;

use rustair::cpu8080::{Bus, Cpu8080};

const LOAD_ADDRESS: usize = 0x0100;
const EXPECTED_EI: u64 = 949_495;

#[derive(Clone)]
struct ProfileBus {
    memory: Vec<u8>,
}

impl ProfileBus {
    fn new(image: &[u8]) -> Self {
        let mut memory = vec![0u8; 65_536];
        let end = LOAD_ADDRESS + image.len();
        memory[LOAD_ADDRESS..end].copy_from_slice(image);
        memory[0] = 0x76;
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

fn cpm_bdos_return(cpu: &mut Cpu8080, bus: &ProfileBus) {
    let sp = cpu.sp as usize;
    let lo = bus.memory[sp] as u16;
    let hi = bus.memory[sp.wrapping_add(1) & 0xffff] as u16;
    cpu.sp = cpu.sp.wrapping_add(2);
    cpu.pc = lo | (hi << 8);
}

fn mnemonic(opcode: u8) -> &'static str {
    match opcode {
        0x00 => "NOP",
        0xc0 => "RNZ",
        0xc8 => "RZ",
        0xc9 | 0xd9 => "RET",
        0xc3 | 0xcb => "JMP",
        0xcd | 0xdd | 0xed | 0xfd => "CALL",
        0xd3 => "OUT",
        0xdb => "IN",
        0xe3 => "XTHL",
        0xf3 => "DI",
        0xfb => "EI",
        _ => "other",
    }
}

#[test]
#[ignore = "semantic EI-successor profiler; run manually in --release"]
fn profile_8080exm_dynamic_successors_after_ei() {
    let image = include_bytes!("../assets/cpu-tests/8080EXM.COM");
    let mut bus = ProfileBus::new(image);
    let mut cpu = Cpu8080::new();
    cpu.pc = LOAD_ADDRESS as u16;
    cpu.sp = 0xf000;
    bus.memory[cpu.sp as usize] = 0;
    bus.memory[cpu.sp.wrapping_add(1) as usize] = 0;

    let started = Instant::now();
    let mut successors = [0u64; 256];
    let mut ei_count = 0u64;
    let mut previous_was_ei = false;

    while cpu.pc != 0 {
        if cpu.pc == 5 {
            cpm_bdos_return(&mut cpu, &bus);
            previous_was_ei = false;
            continue;
        }

        let opcode = bus.memory[cpu.pc as usize];
        if previous_was_ei {
            successors[opcode as usize] += 1;
        }
        previous_was_ei = opcode == 0xfb;
        if previous_was_ei {
            ei_count += 1;
        }
        cpu.step(&mut bus);
    }

    let successor_total: u64 = successors.iter().sum();
    eprintln!(
        "8080EXM EI successor profile: {ei_count} EI, {successor_total} observed successors, {:.3?}",
        started.elapsed()
    );
    let mut rows: Vec<(u8, u64)> = successors
        .iter()
        .enumerate()
        .filter_map(|(opcode, &count)| (count != 0).then_some((opcode as u8, count)))
        .collect();
    rows.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    eprintln!("opcode  mnemonic        count       % after EI");
    for (opcode, count) in rows {
        eprintln!(
            "  {opcode:02X}    {:<8} {:>12} {:>12.3}%",
            mnemonic(opcode),
            count,
            count as f64 * 100.0 / successor_total.max(1) as f64,
        );
    }

    assert_eq!(ei_count, EXPECTED_EI, "dynamic EI reference count changed");
    assert_eq!(successor_total, ei_count, "every EI must have one dynamic successor");
}
