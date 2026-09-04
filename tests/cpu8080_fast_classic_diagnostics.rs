use std::time::Instant;

use rustair::cpu8080::{Bus, Cpu8080};

const CPM_COM_LOAD_ADDRESS: usize = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const BDOS_BASE: u16 = 0xff00;
const BDOS_LEN: usize = 0x37;
const SHORT_MAX_INSTRUCTIONS: u64 = 1_000_000;
const CPUTEST_MAX_INSTRUCTIONS: u64 = 40_000_000;
const EXM_MAX_INSTRUCTIONS: u64 = 3_500_000_000;

#[derive(Debug)]
struct ReferenceMeter {
    started: bool,
    instructions: u64,
    t_states: u64,
    done: bool,
}

impl ReferenceMeter {
    fn new() -> Self {
        Self {
            started: false,
            instructions: 0,
            t_states: 0,
            done: false,
        }
    }

    fn record(&mut self, address: u16, instruction_t_states: u32) {
        if !self.started {
            if address == 0x0100 {
                self.started = true;
                self.instructions = 1;
                self.t_states = u64::from(instruction_t_states);
            }
            return;
        }

        // Match the established RusTair/reference accounting exactly. CALL 0005h
        // is normalized to the conventional OUT 1 + RET pair while the richer
        // mini-BDOS still executes normally in the guest.
        if address == 0x0005 {
            self.instructions += 2;
            self.t_states += 20;
            return;
        }

        // The loader replaces warm boot with HLT but reports the conventional
        // reference OUT 0 cost for the final instruction, matching Cycle's
        // classic-diagnostic harness.
        if address == 0x0000 {
            self.instructions += 1;
            self.t_states += 10;
            self.done = true;
            return;
        }

        let bdos_end = BDOS_BASE.wrapping_add(BDOS_LEN as u16);
        if address >= BDOS_BASE && address < bdos_end {
            return;
        }

        self.instructions += 1;
        self.t_states += u64::from(instruction_t_states);
    }
}

#[derive(Debug)]
struct FastDiagnosticBus {
    memory: Vec<u8>,
    output: Vec<u8>,
    meter: ReferenceMeter,
}

impl FastDiagnosticBus {
    fn with_image(image: &[u8]) -> Self {
        let mut memory = vec![0u8; 65536];
        let mut page_zero = [0u8; 0x100];

        // CP/M warm boot. The bootstrap changes byte 0000h to HLT before
        // entering the transient program, matching the Cycle diagnostic harness.
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
            meter: ReferenceMeter::new(),
        }
    }
}

impl Bus for FastDiagnosticBus {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        self.memory[address as usize]
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        self.memory[address as usize] = value;
    }

    #[inline]
    fn input(&mut self, port: u8) -> u8 {
        // MITS 88-SIO status port used by the mini-BDOS. Zero means the
        // transmitter-ready polling mask (C0h) is clear.
        if port == 0x00 { 0x00 } else { 0xff }
    }

    #[inline]
    fn output(&mut self, port: u8, value: u8) {
        if port == 0x01 {
            self.output.push(value);
        }
    }

    #[inline]
    fn instruction_complete(&mut self, address: u16, _opcode: u8, t_states: u32) {
        self.meter.record(address, t_states);
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
    bdos.extend_from_slice(&[0xf5, 0xc5, 0xd5, 0xe5]);
    bdos.push(0x79);
    bdos.extend_from_slice(&[0xfe, 0x02]);
    append_abs(&mut bdos, 0xca, char_addr);
    bdos.extend_from_slice(&[0xfe, 0x09]);
    append_abs(&mut bdos, 0xca, string_addr);
    append_abs(&mut bdos, 0xc3, done_addr);

    assert_eq!(bdos.len(), CHAR_OFFSET as usize);
    bdos.push(0x7b);
    append_abs(&mut bdos, 0xcd, putc_addr);
    append_abs(&mut bdos, 0xc3, done_addr);

    assert_eq!(bdos.len(), STRING_OFFSET as usize);
    bdos.push(0x1a);
    bdos.extend_from_slice(&[0xfe, 0x24]);
    append_abs(&mut bdos, 0xca, done_addr);
    append_abs(&mut bdos, 0xcd, putc_addr);
    bdos.push(0x13);
    append_abs(&mut bdos, 0xc3, string_addr);

    assert_eq!(bdos.len(), DONE_OFFSET as usize);
    bdos.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xc9]);

    assert_eq!(bdos.len(), PUTC_OFFSET as usize);
    bdos.push(0x47);
    bdos.extend_from_slice(&[0xdb, 0x00]);
    bdos.extend_from_slice(&[0xe6, 0xc0]);
    append_abs(&mut bdos, 0xc2, poll_addr);
    bdos.extend_from_slice(&[0x78, 0xd3, 0x01, 0xc9]);

    bdos
}

#[derive(Debug)]
struct DiagnosticResult {
    instructions: u64,
    t_states: u64,
    output: Vec<u8>,
    halted: bool,
}

fn run_diagnostic(image: &[u8], max_instructions: u64) -> DiagnosticResult {
    let mut cpu = Cpu8080::new();
    let mut bus = FastDiagnosticBus::with_image(image);

    for _ in 0..max_instructions {
        cpu.step(&mut bus);
        if bus.meter.done {
            return DiagnosticResult {
                instructions: bus.meter.instructions,
                t_states: bus.meter.t_states,
                output: bus.output,
                halted: cpu.halted,
            };
        }
    }

    panic!(
        "diagnostic did not reach CP/M warm boot within {max_instructions} Fast instructions; PC={:04x}",
        cpu.pc
    );
}

fn assert_reference(
    name: &str,
    image: &[u8],
    expected_instructions: u64,
    expected_t_states: u64,
    max_instructions: u64,
) -> DiagnosticResult {
    let result = run_diagnostic(image, max_instructions);
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

#[test]
fn fast_core_runs_8080pre_with_reference_totals() {
    assert_reference(
        "8080PRE.COM",
        include_bytes!("../assets/cpu-tests/8080PRE.COM"),
        1_061,
        7_817,
        SHORT_MAX_INSTRUCTIONS,
    );
}

#[test]
fn fast_core_runs_tst8080_with_reference_totals() {
    assert_reference(
        "TST8080.COM",
        include_bytes!("../assets/cpu-tests/TST8080.COM"),
        651,
        4_924,
        SHORT_MAX_INSTRUCTIONS,
    );
}

#[test]
#[ignore = "long-running Fast diagnostic; run explicitly in --release mode"]
fn fast_core_runs_cputest_with_reference_totals() {
    let started = Instant::now();
    let result = assert_reference(
        "CPUTEST.COM",
        include_bytes!("../assets/cpu-tests/CPUTEST.COM"),
        33_971_311,
        255_653_383,
        CPUTEST_MAX_INSTRUCTIONS,
    );
    let elapsed = started.elapsed();
    let emulated_mhz = result.t_states as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    eprintln!(
        "CPUTEST Fast core: {} instructions, {} T-states in {:.3?} ({emulated_mhz:.2} MHz host throughput)",
        result.instructions, result.t_states, elapsed
    );
}

#[test]
#[ignore = "very long Fast exerciser; run explicitly in --release mode"]
fn fast_core_runs_8080exm_with_reference_totals() {
    let started = Instant::now();
    let result = assert_reference(
        "8080EXM.COM",
        include_bytes!("../assets/cpu-tests/8080EXM.COM"),
        2_919_050_698,
        23_803_381_171,
        EXM_MAX_INSTRUCTIONS,
    );
    let elapsed = started.elapsed();
    let emulated_mhz = result.t_states as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    eprintln!(
        "8080EXM Fast core: {} instructions, {} T-states in {:.3?} ({emulated_mhz:.2} MHz host throughput)",
        result.instructions, result.t_states, elapsed
    );
}
