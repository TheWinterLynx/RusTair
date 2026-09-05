use std::time::Instant;

use rustair::adaptive_metrics;
use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig, TwoSioInterruptWiring, TwoSioStraps};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

const CPM_COM_LOAD_ADDRESS: u16 = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const BDOS_BASE: u16 = 0xff00;
const BDOS_LEN: usize = 0x37;
const SERVICE_CHUNK_T_STATES: u32 = 100_000;
const SHORT_MAX_T_STATES: u64 = 5_000_000;
const CPUTEST_MAX_T_STATES: u64 = 400_000_000;
const EXM_MAX_T_STATES: u64 = 30_000_000_000;

#[derive(Clone, Copy, Debug)]
struct Reference {
    instructions: u64,
    t_states: u64,
}

fn full_64k_two_sio_hardware() -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(18)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    for (slot, base) in [(2, 0x0000), (3, 0x4000), (4, 0x8000), (5, 0xc000)] {
        hardware
            .set_slot(
                slot,
                Some(S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(
                    S100RamBoardModel::Mits16KStatic88_16Mcs,
                    base,
                ))),
            )
            .unwrap();
    }
    hardware
        .set_slot(
            6,
            Some(S100InstalledCardConfig::Mits88TwoSio {
                straps: TwoSioStraps::default(),
                interrupt_wiring: TwoSioInterruptWiring::default(),
            }),
        )
        .unwrap();
    hardware.validate().unwrap()
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
    bdos.push(0x7b); // MOV A,E
    append_abs(&mut bdos, 0xcd, putc_addr);
    append_abs(&mut bdos, 0xc3, done_addr);

    assert_eq!(bdos.len(), STRING_OFFSET as usize);
    bdos.push(0x1a); // LDAX D
    bdos.extend_from_slice(&[0xfe, 0x24]); // CPI '$'
    append_abs(&mut bdos, 0xca, done_addr);
    append_abs(&mut bdos, 0xcd, putc_addr);
    bdos.push(0x13); // INX D
    append_abs(&mut bdos, 0xc3, string_addr);

    assert_eq!(bdos.len(), DONE_OFFSET as usize);
    bdos.extend_from_slice(&[0xe1, 0xd1, 0xc1, 0xf1, 0xc9]); // POP H/D/B/PSW; RET

    assert_eq!(bdos.len(), PUTC_OFFSET as usize);
    bdos.push(0x47); // MOV B,A
    bdos.extend_from_slice(&[0xdb, 0x10]); // IN 88-2SIO Port 0 status
    bdos.extend_from_slice(&[0xe6, 0x02]); // ANI TDRE
    append_abs(&mut bdos, 0xca, poll_addr); // JZ while not ready
    bdos.extend_from_slice(&[0x78, 0xd3, 0x11, 0xc9]); // MOV A,B; OUT data; RET

    assert_eq!(bdos.len(), BDOS_LEN);
    bdos
}

fn prepare_machine(image: &[u8], reference: Reference, name: &str) -> BackendHost {
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(full_64k_two_sio_hardware(), RamInit::Zeroed);
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    machine.clear_memory_protection();
    machine.clear_transient_memory_guards();

    let mut page_zero = [0u8; 0x100];
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

    let image_end = CPM_COM_LOAD_ADDRESS as usize + image.len();
    assert!(image_end < BDOS_BASE as usize, "diagnostic image overlaps BDOS");
    machine.load_bytes(0, &page_zero);
    machine.load_bytes(CPM_COM_LOAD_ADDRESS, image);
    machine.load_bytes(BDOS_BASE, &build_bdos());
    machine.begin_cpu_diagnostic_meter(
        name.to_owned(),
        BDOS_BASE,
        BDOS_LEN,
        Some(reference.instructions),
        Some(reference.t_states),
    );
    machine.set_running(true);
    machine
}

fn drain_console(machine: &mut BackendHost, output: &mut Vec<u8>) {
    while let Some(byte) = machine.serial_tx_complete(BackendSerialPort::Port0) {
        output.push(byte);
    }
}

fn print_strategy_metrics(name: &str, stats: adaptive_metrics::AdaptiveCycleStats) {
    eprintln!(
        "{name} strategy: Full={} T ({:.2}%), Partial={} T ({:.2}%), Full instructions={}, Full windows={}, F->P={}, P->F={}, Partial entries={}",
        stats.full_t_states,
        stats.full_percent(),
        stats.partial_t_states,
        stats.partial_percent(),
        stats.full_instructions,
        stats.full_windows,
        stats.full_to_partial,
        stats.partial_to_full,
        stats.partial_entries,
    );
    let f = stats.fallbacks;
    eprintln!(
        "{name} Partial-entry reasons: chassis={} serial={} ready={} hold={} irq={} budget={} mid_instruction={} stop={} fault={} reset={} opcode_barrier={} full_window_unavailable={} total={}",
        f.chassis_unsupported,
        f.serial_active,
        f.ready_low,
        f.hold,
        f.interrupt_pending,
        f.budget_tail,
        f.not_instruction_boundary,
        f.stop_wait_pending,
        f.cpu_fault,
        f.reset,
        f.opcode_barrier,
        f.full_window_unavailable,
        f.total(),
    );
}

fn run_diagnostic(
    name: &str,
    image: &[u8],
    reference: Reference,
    max_t_states: u64,
) -> (f64, u64, Vec<u8>) {
    let mut machine = prepare_machine(image, reference, name);
    let start_t = machine.intel8080_state().total_t_states.unwrap_or(0);
    adaptive_metrics::begin_measurement();
    let started = Instant::now();
    let mut output = Vec::new();

    loop {
        let now_t = machine.intel8080_state().total_t_states.unwrap_or(start_t);
        let executed = now_t.saturating_sub(start_t);
        assert!(executed <= max_t_states, "{name}: exceeded {max_t_states} actual Adaptive Cycle T-states");

        machine.run_cycles(SERVICE_CHUNK_T_STATES);
        drain_console(&mut machine, &mut output);

        if let Some(result) = machine.take_cpu_diagnostic_result() {
            drain_console(&mut machine, &mut output);
            assert_eq!(result.instructions, reference.instructions, "{name}: normalized instruction count");
            assert_eq!(result.t_states, reference.t_states, "{name}: normalized reference T-state count");
            let final_t = machine.intel8080_state().total_t_states.unwrap_or(now_t);
            let actual_t = final_t.saturating_sub(start_t);
            let stats = adaptive_metrics::end_measurement();
            assert_eq!(
                stats.total_t_states(),
                actual_t,
                "{name}: Full + Partial metrics must account for every executed CPU T-state",
            );
            let elapsed = started.elapsed();
            let mhz = actual_t as f64 / elapsed.as_secs_f64() / 1_000_000.0;
            assert!(!output.is_empty(), "{name}: diagnostic produced no 88-2SIO console output");
            eprintln!(
                "{name} adaptive-cycle: {} reference instructions, {} reference T-states, {} actual machine T-states, {:.3?}, {mhz:.2} MHz",
                result.instructions,
                result.t_states,
                actual_t,
                elapsed,
            );
            print_strategy_metrics(name, stats);
            return (mhz, actual_t, output);
        }

        assert!(machine.running(), "{name}: machine stopped before diagnostic meter completed");
    }
}

#[test]
fn adaptive_cycle_runs_8080pre_with_reference_totals() {
    let _ = run_diagnostic(
        "8080PRE.COM",
        include_bytes!("../assets/cpu-tests/8080PRE.COM"),
        Reference { instructions: 1_061, t_states: 7_817 },
        SHORT_MAX_T_STATES,
    );
}

#[test]
fn adaptive_cycle_runs_tst8080_with_reference_totals() {
    let _ = run_diagnostic(
        "TST8080.COM",
        include_bytes!("../assets/cpu-tests/TST8080.COM"),
        Reference { instructions: 651, t_states: 4_924 },
        SHORT_MAX_T_STATES,
    );
}

#[test]
#[ignore = "long-running Adaptive Cycle diagnostic/performance measurement"]
fn adaptive_cycle_runs_cputest_with_reference_totals() {
    let _ = run_diagnostic(
        "CPUTEST.COM",
        include_bytes!("../assets/cpu-tests/CPUTEST.COM"),
        Reference { instructions: 33_971_311, t_states: 255_653_383 },
        CPUTEST_MAX_T_STATES,
    );
}

#[test]
#[ignore = "very long Adaptive Cycle exerciser/performance measurement"]
fn adaptive_cycle_runs_8080exm_with_reference_totals() {
    let _ = run_diagnostic(
        "8080EXM.COM",
        include_bytes!("../assets/cpu-tests/8080EXM.COM"),
        Reference { instructions: 2_919_050_698, t_states: 23_803_381_171 },
        EXM_MAX_T_STATES,
    );
}
