use std::time::{Duration, Instant};

use rustair::adaptive_metrics::{self, AdaptiveCycleStats};
use rustair::backend::BackendHost;
use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

const ALTAIR_CLOCK_HZ: f64 = 2_000_000.0;
const WARMUP_T_STATES: u64 = 5_000_000;
const MEASURE_T_STATES: u64 = 250_000_000;
const METRICS_T_STATES: u64 = 5_000_000;
const SERVICE_CHUNK_T_STATES: u32 = 1_000_000;
const BENCH_ROUNDS: usize = 5;
const DCDD_WARMUP_T_STATES: u64 = 20_000_000;
const DCDD_MEASURE_T_STATES: u64 = 1_000_000_000;
const DCDD_BENCH_ROUNDS: usize = 7;

// NOP ; JMP 0000h
//
// Deliberately a ceiling microbenchmark: it keeps the adaptive backend in a
// tiny, side-effect-free RAM loop so Full execution can show its best possible
// dispatch/presentation throughput while each historical RAM model still pays
// for its own digital timing contract. The 88-4MCD therefore includes exact
// refresh-collision/TW accounting inside Full; the 88-S4K advances synchronous
// refresh without PRDY; the 88-16MCD remains processor-transparent at the
// digital boundary. Classic diagnostics are measured separately through the
// same BackendHost as representative workloads.
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

fn minimal_four_k_hardware(model: S100RamBoardModel) -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(model, 0x0000),
            )),
        )
        .unwrap();
    hardware.validate().unwrap()
}

fn four_k_static_hardware() -> S100HardwareConfig {
    minimal_four_k_hardware(S100RamBoardModel::Mits4KStatic88_4Mcs)
}

fn four_k_dynamic_hardware() -> S100HardwareConfig {
    minimal_four_k_hardware(S100RamBoardModel::Mits4KDynamic88_4Mcd)
}

fn four_k_synchronous_hardware() -> S100HardwareConfig {
    minimal_four_k_hardware(S100RamBoardModel::Mits4KSynchronous88S4K)
}

fn historical_starter_hardware() -> S100HardwareConfig {
    S100HardwareConfig::historical_8800b_18_slot_starter()
        .validate()
        .unwrap()
}

fn historical_starter_with_ram(model: S100RamBoardModel) -> S100HardwareConfig {
    let mut hardware = historical_starter_hardware();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(model, 0x0000),
            )),
        )
        .unwrap();
    hardware.set_slot(3, None).unwrap();
    hardware.validate().unwrap()
}

fn historical_starter_without_two_sio() -> S100HardwareConfig {
    historical_starter_with_ram(S100RamBoardModel::Mits16KStatic88_16Mcs)
}

fn historical_starter_with_16mcd() -> S100HardwareConfig {
    historical_starter_with_ram(S100RamBoardModel::Mits16KDynamic88_16Mcd)
}

fn historical_starter_with_dcdd() -> S100HardwareConfig {
    let mut hardware = historical_starter_without_two_sio();
    hardware
        .set_slot(3, Some(S100InstalledCardConfig::Mits88DcddBoard1))
        .unwrap();
    hardware
        .set_slot(4, Some(S100InstalledCardConfig::Mits88DcddBoard2))
        .unwrap();
    hardware.validate().unwrap()
}

fn benchmark_cases() -> [(&'static str, HardwareFactory); 6] {
    [
        ("CPU + 88-4MCS 4K Static", four_k_static_hardware),
        ("CPU + 88-4MCD 4K Dynamic", four_k_dynamic_hardware),
        ("CPU + 88-S4K 4K Sync", four_k_synchronous_hardware),
        ("8800b + 16K Static", historical_starter_without_two_sio),
        ("8800b + 88-16MCD Dynamic", historical_starter_with_16mcd),
        ("8800b + 16K Static + 88-2SIO", historical_starter_hardware),
    ]
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

fn prepare_benchmark_machine(hardware: S100HardwareConfig) -> BackendHost {
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(hardware, RamInit::Zeroed);
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    machine.clear_memory_protection();
    machine.load_bytes(0x0000, &BENCH_PROGRAM);
    machine.set_running(true);
    machine
}

fn benchmark_one_with_budget(
    scenario: &'static str,
    hardware: S100HardwareConfig,
    warmup_t_states: u64,
    measure_t_states: u64,
) -> ResultRow {
    let mut machine = prepare_benchmark_machine(hardware);
    let _ = run_t_states(&mut machine, warmup_t_states);

    let before = machine.intel8080_state().total_t_states.unwrap_or(0);
    let wall_start = Instant::now();
    let _ = run_t_states(&mut machine, measure_t_states);
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

fn benchmark_one(scenario: &'static str, hardware: S100HardwareConfig) -> ResultRow {
    benchmark_one_with_budget(scenario, hardware, WARMUP_T_STATES, MEASURE_T_STATES)
}

fn adaptive_path_mix(hardware: S100HardwareConfig) -> AdaptiveCycleStats {
    let mut machine = prepare_benchmark_machine(hardware);
    adaptive_metrics::begin_measurement();
    let before = machine.intel8080_state().total_t_states.unwrap_or(0);
    let _ = run_t_states(&mut machine, METRICS_T_STATES);
    let stats = adaptive_metrics::end_measurement();
    let after = machine.intel8080_state().total_t_states.unwrap_or(before);
    assert_eq!(stats.total_t_states(), after.saturating_sub(before));
    stats
}

fn median_f64(values: &[f64]) -> f64 {
    let mut ordered = values.to_vec();
    ordered.sort_by(|a, b| a.total_cmp(b));
    ordered[ordered.len() / 2]
}

fn median_row(rows: &[ResultRow]) -> ResultRow {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|a, b| a.mhz.total_cmp(&b.mhz));
    ordered[ordered.len() / 2]
}

fn print_rows(rows: &[ResultRow]) {
    let row = median_row(rows);
    let min_mhz = rows
        .iter()
        .map(|sample| sample.mhz)
        .fold(f64::INFINITY, f64::min);
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

fn paired_ratios(baseline_rows: &[ResultRow], candidate_rows: &[ResultRow]) -> Vec<f64> {
    assert_eq!(baseline_rows.len(), candidate_rows.len());
    baseline_rows
        .iter()
        .zip(candidate_rows)
        .map(|(baseline, candidate)| candidate.mhz / baseline.mhz)
        .collect()
}

fn print_relative_cost(label: &str, baseline_rows: &[ResultRow], candidate_rows: &[ResultRow]) {
    let baseline_mhz = median_row(baseline_rows).mhz;
    let candidate_mhz = median_row(candidate_rows).mhz;
    let paired_ratios = paired_ratios(baseline_rows, candidate_rows);
    let paired_ratio = median_f64(&paired_ratios);
    let min_ratio = paired_ratios.iter().copied().fold(f64::INFINITY, f64::min);
    let max_ratio = paired_ratios
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let throughput_delta_pct = (paired_ratio - 1.0) * 100.0;
    let slowdown = 1.0 / paired_ratio;
    println!(
        "{label:<31} | medians {baseline_mhz:>8.3}->{candidate_mhz:>8.3} MHz | paired {throughput_delta_pct:>+7.2}% | {slowdown:>6.2}x | paired ratio {min_ratio:>5.3}-{max_ratio:>5.3}"
    );
}

#[test]
fn adaptive_cycle_benchmark_hardware_matrix_builds() {
    for (_, factory) in benchmark_cases() {
        let _ = factory();
    }
    let _ = historical_starter_with_dcdd();
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
    println!(
        "Supported historical dynamic RAM participates in Adaptive Full; its digital refresh/WAIT timing remains accounted for rather than bypassed."
    );
    println!(
        "Relative RAM costs use the median of same-round candidate/baseline ratios to suppress host clock and scheduler drift."
    );
    println!("Reference: MITS Altair 8800 nominal CPU clock = 2.000 MHz");
    println!();

    let cases = benchmark_cases();
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

    println!();
    println!("Historical RAM timing throughput cost inside Adaptive Full");
    print_relative_cost("88-4MCD vs 88-4MCS", &samples[0], &samples[1]);
    print_relative_cost("88-S4K vs 88-4MCS", &samples[0], &samples[2]);
    print_relative_cost("88-16MCD vs 88-16MCS", &samples[3], &samples[4]);
}

#[test]
#[ignore = "manual release benchmark for installed-idle DCDD overhead"]
fn measure_dcdd_installed_idle_overhead() {
    let baseline_mix = adaptive_path_mix(historical_starter_without_two_sio());
    let dcdd_mix = adaptive_path_mix(historical_starter_with_dcdd());
    assert_eq!(baseline_mix.fallbacks.chassis_unsupported, 0);
    assert_eq!(dcdd_mix.fallbacks.chassis_unsupported, 0);

    println!();
    println!("RusTair installed-idle DCDD performance");
    println!(
        "Path mix baseline: Full {:>7.3}% / Partial {:>7.3}%",
        baseline_mix.full_percent(),
        baseline_mix.partial_percent()
    );
    println!(
        "Path mix + DCDD : Full {:>7.3}% / Partial {:>7.3}%",
        dcdd_mix.full_percent(),
        dcdd_mix.partial_percent()
    );
    println!(
        "Timed measurement: median of {DCDD_BENCH_ROUNDS} paired rounds × {DCDD_MEASURE_T_STATES} T after {DCDD_WARMUP_T_STATES} T warm-up"
    );

    let mut baseline = Vec::with_capacity(DCDD_BENCH_ROUNDS);
    let mut dcdd = Vec::with_capacity(DCDD_BENCH_ROUNDS);
    for round in 0..DCDD_BENCH_ROUNDS {
        if round & 1 == 0 {
            baseline.push(benchmark_one_with_budget(
                "8800b + 16K Static",
                historical_starter_without_two_sio(),
                DCDD_WARMUP_T_STATES,
                DCDD_MEASURE_T_STATES,
            ));
            dcdd.push(benchmark_one_with_budget(
                "8800b + 16K Static + DCDD",
                historical_starter_with_dcdd(),
                DCDD_WARMUP_T_STATES,
                DCDD_MEASURE_T_STATES,
            ));
        } else {
            dcdd.push(benchmark_one_with_budget(
                "8800b + 16K Static + DCDD",
                historical_starter_with_dcdd(),
                DCDD_WARMUP_T_STATES,
                DCDD_MEASURE_T_STATES,
            ));
            baseline.push(benchmark_one_with_budget(
                "8800b + 16K Static",
                historical_starter_without_two_sio(),
                DCDD_WARMUP_T_STATES,
                DCDD_MEASURE_T_STATES,
            ));
        }
    }

    print_rows(&baseline);
    print_rows(&dcdd);
    for (index, ratio) in paired_ratios(&baseline, &dcdd).iter().copied().enumerate() {
        println!(
            "DCDD pair {:>2}: ratio {ratio:.5} ({:+.2}%)",
            index + 1,
            (ratio - 1.0) * 100.0
        );
    }
    print_relative_cost("DCDD installed-idle", &baseline, &dcdd);
    println!("Phase 11 target: installed-idle DCDD regression < 2%.");
}
