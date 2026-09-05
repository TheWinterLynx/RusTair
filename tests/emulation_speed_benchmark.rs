use std::time::{Duration, Instant};

use rustair::backend::BackendHost;
use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

const ALTAIR_CLOCK_HZ: f64 = 2_000_000.0;
const WARMUP_T_STATES: u64 = 250_000;
const MEASURE_T_STATES: u64 = 5_000_000;
const SERVICE_CHUNK_T_STATES: u32 = 100_000;
const BENCH_ROUNDS: usize = 3;

// NOP ; JMP 0000h
//
// Deliberately a ceiling microbenchmark: it keeps the adaptive backend in a
// tiny, side-effect-free static-RAM loop so Full execution can show its best
// possible dispatch/presentation throughput. Classic diagnostics are measured
// separately through the same BackendHost as representative workloads.
const BENCH_PROGRAM: [u8; 4] = [0x00, 0xc3, 0x00, 0x00];

type HardwareFactory = fn() -> S100HardwareConfig;

#[derive(Clone, Copy)]
struct ResultRow {
    scenario: &'static str,
    t_states: u64,
    elapsed: Duration,
    mhz: f64,
    realtime_multiple: f64,
}

fn minimal_historical_hardware() -> S100HardwareConfig {
    let mut hardware =
        S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(
                    S100RamBoardModel::Mits4KStatic88_4Mcs,
                    0x0000,
                ),
            )),
        )
        .unwrap();
    hardware.validate().unwrap()
}

fn historical_starter_hardware() -> S100HardwareConfig {
    S100HardwareConfig::historical_8800b_18_slot_starter()
        .validate()
        .unwrap()
}

fn historical_starter_without_two_sio() -> S100HardwareConfig {
    let mut hardware = historical_starter_hardware();
    hardware.set_slot(3, None).unwrap();
    hardware.validate().unwrap()
}

fn run_t_states(machine: &mut BackendHost, target: u64) -> u64 {
    let start = machine.intel8080_state().total_t_states.unwrap_or(0);
    loop {
        let now = machine.intel8080_state().total_t_states.unwrap_or(start);
        let executed = now.saturating_sub(start);
        if executed >= target {
            return executed;
        }
        let remaining = target - executed;
        machine.run_cycles(remaining.min(u64::from(SERVICE_CHUNK_T_STATES)) as u32);
        assert!(machine.running(), "benchmark loop unexpectedly stopped");
    }
}

fn benchmark_one(scenario: &'static str, hardware: S100HardwareConfig) -> ResultRow {
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(hardware, RamInit::Zeroed);
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    machine.clear_memory_protection();
    machine.load_bytes(0x0000, &BENCH_PROGRAM);
    machine.set_running(true);

    let _ = run_t_states(&mut machine, WARMUP_T_STATES);

    let before = machine.intel8080_state().total_t_states.unwrap_or(0);
    let wall_start = Instant::now();
    let _ = run_t_states(&mut machine, MEASURE_T_STATES);
    let elapsed = wall_start.elapsed();
    let after = machine.intel8080_state().total_t_states.unwrap_or(before);
    let t_states = after.saturating_sub(before);

    let hz = t_states as f64 / elapsed.as_secs_f64();
    ResultRow {
        scenario,
        t_states,
        elapsed,
        mhz: hz / 1_000_000.0,
        realtime_multiple: hz / ALTAIR_CLOCK_HZ,
    }
}

fn median_row(rows: &[ResultRow]) -> ResultRow {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|a, b| a.mhz.total_cmp(&b.mhz));
    ordered[ordered.len() / 2]
}

fn print_rows(rows: &[ResultRow]) {
    let row = median_row(rows);
    let min_mhz = rows.iter().map(|sample| sample.mhz).fold(f64::INFINITY, f64::min);
    let max_mhz = rows
        .iter()
        .map(|sample| sample.mhz)
        .fold(f64::NEG_INFINITY, f64::max);
    let spread_pct = if row.mhz == 0.0 {
        0.0
    } else {
        (max_mhz - min_mhz) / row.mhz * 100.0
    };
    println!(
        "{:<28} | {:<30} | {:>10} T | {:>8.3} s | {:>8.3} MHz | {:>7.2}x Altair 2 MHz | range {:>6.3}-{:>6.3} ({:>4.1}%)",
        "RusTair — Adaptive Cycle",
        row.scenario,
        row.t_states,
        row.elapsed.as_secs_f64(),
        row.mhz,
        row.realtime_multiple,
        min_mhz,
        max_mhz,
        spread_pct,
    );
}

#[test]
#[ignore = "manual Adaptive Cycle ceiling benchmark"]
fn measure_adaptive_cycle_effective_mhz() {
    println!();
    println!("RusTair Adaptive Cycle ceiling throughput");
    println!(
        "Measurement: median of {BENCH_ROUNDS} rounds × {MEASURE_T_STATES} emulated T-states after {WARMUP_T_STATES}T warm-up"
    );
    println!("This NOP/JMP loop is a ceiling microbenchmark, not a representative workload.");
    println!("Reference: MITS Altair 8800 nominal CPU clock = 2.000 MHz");
    println!();

    let cases: [(&'static str, HardwareFactory); 3] = [
        ("CPU + 88-4MCS 4K Static", minimal_historical_hardware),
        ("8800b + 16K Static", historical_starter_without_two_sio),
        ("8800b + 16K Static + 88-2SIO", historical_starter_hardware),
    ];
    let mut samples = vec![Vec::<ResultRow>::new(); cases.len()];

    for round in 0..BENCH_ROUNDS {
        let reverse = round & 1 != 0;
        for scenario_step in 0..cases.len() {
            let scenario_index = if reverse {
                cases.len() - 1 - scenario_step
            } else {
                scenario_step
            };
            let (scenario, factory) = cases[scenario_index];
            samples[scenario_index].push(benchmark_one(scenario, factory()));
        }
    }

    for rows in &samples {
        print_rows(rows);
    }
}
