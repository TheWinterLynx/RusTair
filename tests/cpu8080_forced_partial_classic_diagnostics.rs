use std::time::{Duration, Instant};

use rustair::adaptive_metrics;
use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig, TwoSioInterruptWiring,
    TwoSioStraps,
};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

const CPM_COM_LOAD_ADDRESS: u16 = 0x0100;
const BOOT_ADDRESS: usize = 0x0080;
const BDOS_BASE: u16 = 0xff00;
const BDOS_LEN: usize = 0x37;
const FORCED_PARTIAL_CALL_T_STATES: u32 = 17;
const OUTER_SERVICE_T_STATES: u64 = 100_000;
const CPUTEST_MAX_T_STATES: u64 = 400_000_000;
const EXM_SAMPLE_T_STATES: u64 = 50_000_000;

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
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits16KStatic88_16Mcs,
                        base,
                    ),
                )),
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
    bdos.extend_from_slice(&[0xdb, 0x12]);
    bdos.extend_from_slice(&[0xe6, 0x02]);
    append_abs(&mut bdos, 0xca, poll_addr);
    bdos.extend_from_slice(&[0x78, 0xd3, 0x13, 0xc9]);

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
        0x31, bdos_lo, bdos_hi,
        0x3e, 0x15,
        0xd3, 0x12,
        0x3e, 0x76,
        0x32, 0x00, 0x00,
        0xc3, 0x00, 0x01,
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
    while let Some(byte) = machine.serial_tx_complete(BackendSerialPort::Port1) {
        output.push(byte);
    }
}

fn run_forced_partial_budget(machine: &mut BackendHost, mut budget: u64) {
    while budget != 0 {
        let chunk = budget.min(u64::from(FORCED_PARTIAL_CALL_T_STATES)) as u32;
        // Full reserves 18 T-states before entering a semantic window. A host
        // service budget of at most 17 therefore guarantees that this call can
        // execute only the exact Partial path without adding a production mode
        // switch or changing the physical machine configuration.
        machine.run_cycles(chunk);
        budget -= u64::from(chunk);
    }
}

#[test]
#[ignore = "long-running full-system exact-Partial diagnostic/performance measurement"]
fn full_system_forced_partial_runs_cputest_with_reference_totals() {
    let reference = Reference {
        instructions: 33_971_311,
        t_states: 255_653_383,
    };
    let mut machine = prepare_machine(
        include_bytes!("../assets/cpu-tests/CPUTEST.COM"),
        reference,
        "CPUTEST.COM",
    );
    let start_t = machine.intel8080_state().total_t_states.unwrap_or(0);
    let started = Instant::now();
    let mut last_progress_at = started;
    let mut last_progress_t = start_t;
    let mut output = Vec::new();
    adaptive_metrics::begin_measurement();

    loop {
        let now_t = machine.intel8080_state().total_t_states.unwrap_or(start_t);
        let executed = now_t.saturating_sub(start_t);
        assert!(executed <= CPUTEST_MAX_T_STATES);

        run_forced_partial_budget(&mut machine, OUTER_SERVICE_T_STATES);
        drain_console(&mut machine, &mut output);

        let progress_at = Instant::now();
        if progress_at.duration_since(last_progress_at) >= Duration::from_secs(2) {
            let progress_t = machine.intel8080_state().total_t_states.unwrap_or(now_t);
            let actual_t = progress_t.saturating_sub(start_t);
            let interval_t = progress_t.saturating_sub(last_progress_t);
            let interval_s = progress_at.duration_since(last_progress_at).as_secs_f64();
            let elapsed_s = progress_at.duration_since(started).as_secs_f64();
            eprintln!(
                "[FULL SYSTEM FORCED PARTIAL] CPUTEST.COM progress: {actual_t} machine T, {:.1?}, {:.2} MHz avg / {:.2} MHz recent",
                progress_at.duration_since(started),
                actual_t as f64 / elapsed_s / 1_000_000.0,
                interval_t as f64 / interval_s / 1_000_000.0,
            );
            last_progress_at = progress_at;
            last_progress_t = progress_t;
        }

        if let Some(result) = machine.take_cpu_diagnostic_result() {
            drain_console(&mut machine, &mut output);
            assert_eq!(result.instructions, reference.instructions);
            assert_eq!(result.t_states, reference.t_states);
            let final_t = machine.intel8080_state().total_t_states.unwrap_or(now_t);
            let actual_t = final_t.saturating_sub(start_t);
            let stats = adaptive_metrics::end_measurement();
            assert_eq!(stats.full_t_states, 0, "17T host chunks must make Full impossible");
            assert_eq!(stats.partial_t_states, actual_t);
            assert!(!output.is_empty());
            let elapsed = started.elapsed();
            let mhz = actual_t as f64 / elapsed.as_secs_f64() / 1_000_000.0;
            eprintln!(
                "[FULL SYSTEM FORCED PARTIAL] CPUTEST.COM: {} reference instructions, {} reference T-states, {} actual machine T-states, {:.3?}, {mhz:.2} MHz [Cpu8080Cycle exact + MITS CPU board + S-100 + 64K static RAM + physical 88-2SIO + front panel; Full=0%]",
                result.instructions,
                result.t_states,
                actual_t,
                elapsed,
            );
            break;
        }

        assert!(machine.running());
    }
}

#[test]
#[ignore = "50M-T-state sample of the exact full-system Partial path"]
fn full_system_forced_partial_samples_8080exm_50m_t_states() {
    let reference = Reference {
        instructions: 2_919_050_698,
        t_states: 23_803_381_171,
    };
    let mut machine = prepare_machine(
        include_bytes!("../assets/cpu-tests/8080EXM.COM"),
        reference,
        "8080EXM.COM",
    );
    let start_t = machine.intel8080_state().total_t_states.unwrap_or(0);
    let started = Instant::now();
    let mut output = Vec::new();
    adaptive_metrics::begin_measurement();

    let mut remaining = EXM_SAMPLE_T_STATES;
    while remaining != 0 {
        let outer = remaining.min(OUTER_SERVICE_T_STATES);
        run_forced_partial_budget(&mut machine, outer);
        remaining -= outer;
        drain_console(&mut machine, &mut output);
    }

    let actual_t = machine
        .intel8080_state()
        .total_t_states
        .unwrap_or(start_t)
        .saturating_sub(start_t);
    let stats = adaptive_metrics::end_measurement();
    assert_eq!(actual_t, EXM_SAMPLE_T_STATES);
    assert_eq!(stats.full_t_states, 0, "17T host chunks must make Full impossible");
    assert_eq!(stats.partial_t_states, actual_t);
    assert!(machine.running());

    let elapsed = started.elapsed();
    let mhz = actual_t as f64 / elapsed.as_secs_f64() / 1_000_000.0;
    eprintln!(
        "[FULL SYSTEM FORCED PARTIAL] 8080EXM.COM 50M-T sample: {actual_t} actual machine T-states, {:.3?}, {mhz:.2} MHz [Cpu8080Cycle exact + MITS CPU board + S-100 + 64K static RAM + physical 88-2SIO + front panel; Full=0%]",
        elapsed,
    );
}
