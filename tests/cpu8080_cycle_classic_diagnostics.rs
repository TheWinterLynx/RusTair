use std::time::Instant;

use rustair::cpu8080_cycle::{
    Cpu8080Cycle, Cpu8080Inputs, MachineCycle, TState, TickTrace,
};

const CPM_COM_LOAD_ADDRESS: usize = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const BDOS_BASE: u16 = 0xff00;
const BDOS_LEN: usize = 0x37;
const SHORT_MAX_TICKS: u64 = 5_000_000;
const CPUTEST_MAX_TICKS: u64 = 350_000_000;
const EXM_MAX_TICKS: u64 = 27_000_000_000;

#[derive(Debug)]
struct DiagnosticBus {
    memory: Vec<u8>,
    output: Vec<u8>,
}

impl DiagnosticBus {
    fn with_image(image: &[u8]) -> Self {
        let mut memory = vec![0u8; 65536];
        let mut page_zero = [0u8; 0x100];

        // CP/M warm boot. The bootstrap below changes byte 0000h to HLT before
        // entering the transient program, matching RusTair's production
        // diagnostic loader.
        page_zero[0x0000..0x0003].copy_from_slice(&[0xc3, 0x80, 0x00]);

        let [bdos_lo, bdos_hi] = BDOS_BASE.to_le_bytes();
        page_zero[0x0005..0x0008].copy_from_slice(&[0xc3, bdos_lo, bdos_hi]);

        let boot = [
            0x31, bdos_lo, bdos_hi, // LXI SP,BDOS_BASE
            0x3e, 0x76,             // MVI A,HLT
            0x32, 0x00, 0x00,       // STA 0000h
            0xc3, 0x00, 0x01,       // JMP 0100h
        ];
        page_zero[BOOT_ADDRESS..BOOT_ADDRESS + boot.len()].copy_from_slice(&boot);
        memory[..page_zero.len()].copy_from_slice(&page_zero);

        let image_end = CPM_COM_LOAD_ADDRESS + image.len();
        assert!(image_end < BDOS_BASE as usize, "diagnostic image overlaps BDOS");
        memory[CPM_COM_LOAD_ADDRESS..image_end].copy_from_slice(image);

        let bdos = build_bdos();
        assert_eq!(bdos.len(), BDOS_LEN);
        let bdos_start = BDOS_BASE as usize;
        memory[bdos_start..bdos_start + bdos.len()].copy_from_slice(&bdos);

        Self {
            memory,
            output: Vec::new(),
        }
    }

    fn data_in(&self, cpu: &Cpu8080Cycle) -> u8 {
        if cpu.t_state() != TState::T3 {
            return 0;
        }

        match cpu.machine_cycle() {
            MachineCycle::InstructionFetch | MachineCycle::MemoryRead | MachineCycle::StackRead => {
                let address = cpu
                    .pins()
                    .address
                    .expect("read cycle must expose an address before T3");
                self.memory[address as usize]
            }
            MachineCycle::InputRead => {
                // MITS 88-SIO Port 0 status. Zero means neither TX-busy bit
                // (C0h) is asserted, so the mini-BDOS transmitter is ready.
                0x00
            }
            MachineCycle::InterruptAck | MachineCycle::InterruptAckWhileHalt => {
                panic!("classic diagnostic harness does not inject interrupts")
            }
            _ => 0,
        }
    }

    fn apply_write(&mut self, trace: &TickTrace) {
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
                self.memory[address as usize] = value;
            }
            MachineCycle::OutputWrite => {
                let address = trace
                    .pins
                    .address
                    .expect("output cycle must expose the duplicated port address");
                let value = trace
                    .pins
                    .data_out
                    .expect("output cycle must expose data at T3");

                // The mini-BDOS uses MITS 88-SIO data port 01h. Keep only
                // console bytes; other OUT instructions are still executed.
                if address as u8 == 0x01 {
                    self.output.push(value);
                }
            }
            _ => {}
        }
    }
}

fn append_abs(code: &mut Vec<u8>, opcode: u8, address: u16) {
    let [lo, hi] = address.to_le_bytes();
    code.extend_from_slice(&[opcode, lo, hi]);
}

fn build_bdos() -> Vec<u8> {
    const CHAR_OFFSET: u16 = 0x0012;
    const STRING_OFFSET: u16 = 0x0019;
    const DONE_OFFSET: u16 = 0x0026;
    const PUTC_OFFSET: u16 = 0x002b;
    const POLL_OFFSET: u16 = 0x002c;

    let char_addr = BDOS_BASE.wrapping_add(CHAR_OFFSET);
    let string_addr = BDOS_BASE.wrapping_add(STRING_OFFSET);
    let done_addr = BDOS_BASE.wrapping_add(DONE_OFFSET);
    let putc_addr = BDOS_BASE.wrapping_add(PUTC_OFFSET);
    let poll_addr = BDOS_BASE.wrapping_add(POLL_OFFSET);

    let mut bdos = Vec::with_capacity(BDOS_LEN);
    bdos.extend_from_slice(&[0xf5, 0xc5, 0xd5, 0xe5]); // PUSH PSW/B/D/H
    bdos.push(0x79); // MOV A,C
    bdos.extend_from_slice(&[0xfe, 0x02]); // CPI 2
    append_abs(&mut bdos, 0xca, char_addr); // JZ char
    bdos.extend_from_slice(&[0xfe, 0x09]); // CPI 9
    append_abs(&mut bdos, 0xca, string_addr); // JZ string
    append_abs(&mut bdos, 0xc3, done_addr); // JMP done

    assert_eq!(bdos.len(), CHAR_OFFSET as usize);
    bdos.push(0x7b); // char: MOV A,E
    append_abs(&mut bdos, 0xcd, putc_addr); // CALL putc
    append_abs(&mut bdos, 0xc3, done_addr); // JMP done

    assert_eq!(bdos.len(), STRING_OFFSET as usize);
    bdos.push(0x1a); // string: LDAX D
    bdos.extend_from_slice(&[0xfe, 0x24]); // CPI '$'
    append_abs(&mut bdos, 0xca, done_addr); // JZ done
    append_abs(&mut bdos, 0xcd, putc_addr); // CALL putc
    bdos.push(0x13); // INX D
    append_abs(&mut bdos, 0xc3, string_addr); // JMP string

    assert_eq!(bdos.len(), DONE_OFFSET as usize);
    bdos.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xc9]); // POP H/D/B/PSW; RET

    assert_eq!(bdos.len(), PUTC_OFFSET as usize);
    bdos.push(0x47); // MOV B,A
    bdos.extend_from_slice(&[0xdb, 0x00]); // IN 88-SIO status
    bdos.extend_from_slice(&[0xe6, 0xc0]); // ANI TX-busy mask
    append_abs(&mut bdos, 0xc2, poll_addr); // JNZ poll while busy
    bdos.extend_from_slice(&[0x78, 0xd3, 0x01, 0xc9]); // MOV A,B; OUT data; RET

    bdos
}

#[derive(Debug)]
struct ReferenceMeter {
    started: bool,
    instructions: u64,
    t_states: u64,
}

impl ReferenceMeter {
    fn new() -> Self {
        Self {
            started: false,
            instructions: 0,
            t_states: 0,
        }
    }

    fn record(&mut self, address: u16, instruction_t_states: u32) -> bool {
        if !self.started {
            if address == 0x0100 {
                self.started = true;
                self.instructions = 1;
                self.t_states = u64::from(instruction_t_states);
            }
            return false;
        }

        // Match the established RusTair/reference accounting exactly. The
        // richer guest mini-BDOS still executes, but CALL 0005h is normalized
        // to the conventional OUT 1 + RET pair.
        if address == 0x0005 {
            self.instructions += 2;
            self.t_states += 20;
            return false;
        }

        // The production loader replaces warm boot with HLT but reports the
        // conventional reference OUT 0 cost for the final instruction.
        if address == 0x0000 {
            self.instructions += 1;
            self.t_states += 10;
            return true;
        }

        let bdos_end = BDOS_BASE.wrapping_add(BDOS_LEN as u16);
        if address >= BDOS_BASE && address < bdos_end {
            return false;
        }

        self.instructions += 1;
        self.t_states += u64::from(instruction_t_states);
        false
    }
}

#[derive(Debug)]
struct DiagnosticResult {
    instructions: u64,
    t_states: u64,
    actual_t_states: u64,
    output: Vec<u8>,
    halted: bool,
}

fn run_diagnostic(image: &[u8], max_ticks: u64) -> DiagnosticResult {
    let mut cpu = Cpu8080Cycle::new();
    let mut bus = DiagnosticBus::with_image(image);
    let mut meter = ReferenceMeter::new();
    let mut instruction_address = 0u16;

    for _ in 0..max_ticks {
        if cpu.machine_cycle() == MachineCycle::InstructionFetch && cpu.t_state() == TState::T1 {
            instruction_address = cpu.registers().pc;
        }

        let data_in = bus.data_in(&cpu);
        let trace = cpu.tick(Cpu8080Inputs {
            data_in,
            ready: true,
            interrupt: false,
            hold: false,
            reset: false,
        });
        bus.apply_write(&trace);

        assert_eq!(
            trace.fault, None,
            "cycle core faulted at instruction address {instruction_address:04x} on opcode {:?}",
            trace.opcode
        );

        if trace.instruction_complete
            && meter.record(instruction_address, trace.instruction_t_states)
        {
            return DiagnosticResult {
                instructions: meter.instructions,
                t_states: meter.t_states,
                actual_t_states: cpu.total_t_states(),
                output: bus.output,
                halted: cpu.is_halted(),
            };
        }
    }

    panic!(
        "diagnostic did not reach CP/M warm boot within {max_ticks} T-state ticks; PC={:04x}",
        cpu.registers().pc
    );
}

fn assert_reference(
    name: &str,
    image: &[u8],
    expected_instructions: u64,
    expected_t_states: u64,
    max_ticks: u64,
) -> DiagnosticResult {
    let result = run_diagnostic(image, max_ticks);
    assert!(result.halted, "{name}: warm boot must end in the installed HLT");
    assert!(!result.output.is_empty(), "{name}: diagnostic produced no console output");
    assert_eq!(
        result.instructions, expected_instructions,
        "{name}: normalized instruction count"
    );
    assert_eq!(
        result.t_states, expected_t_states,
        "{name}: normalized T-state count"
    );
    result
}

fn benchmark_reference(
    name: &str,
    image: &[u8],
    expected_instructions: u64,
    expected_t_states: u64,
    max_ticks: u64,
) {
    let started = Instant::now();
    let result = assert_reference(
        name,
        image,
        expected_instructions,
        expected_t_states,
        max_ticks,
    );
    let elapsed = started.elapsed();
    let mhz = result.actual_t_states as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    eprintln!(
        "[CPU CORE ONLY] {name}: {} reference instructions, {} reference T-states, {} actual core T-states, {:.3?}, {mhz:.2} MHz [Cpu8080Cycle + minimal diagnostic bus; no chassis/S-100/front panel/UART]",
        result.instructions,
        result.t_states,
        result.actual_t_states,
        elapsed,
    );
}

#[test]
fn cpu_core_runs_8080pre_with_reference_totals() {
    benchmark_reference(
        "8080PRE.COM",
        include_bytes!("../assets/cpu-tests/8080PRE.COM"),
        1_061,
        7_817,
        SHORT_MAX_TICKS,
    );
}

#[test]
fn cpu_core_runs_tst8080_with_reference_totals() {
    benchmark_reference(
        "TST8080.COM",
        include_bytes!("../assets/cpu-tests/TST8080.COM"),
        651,
        4_924,
        SHORT_MAX_TICKS,
    );
}

#[test]
#[ignore = "long-running CPU-core diagnostic/performance measurement"]
fn cpu_core_runs_cputest_with_reference_totals() {
    benchmark_reference(
        "CPUTEST.COM",
        include_bytes!("../assets/cpu-tests/CPUTEST.COM"),
        33_971_311,
        255_653_383,
        CPUTEST_MAX_TICKS,
    );
}

#[test]
#[ignore = "very long CPU-core exerciser/performance measurement"]
fn cpu_core_runs_8080exm_with_reference_totals() {
    benchmark_reference(
        "8080EXM.COM",
        include_bytes!("../assets/cpu-tests/8080EXM.COM"),
        2_919_050_698,
        23_803_381_171,
        EXM_MAX_TICKS,
    );
}
