use std::time::{Duration, Instant};

use rustair::adaptive_metrics;
use rustair::backend::BackendHost;
use rustair::config::{RamInit, S100HardwareConfig};

const TWO_MHZ: u32 = 2_000_000;
const SERIAL_SLICE: Duration = Duration::from_micros(4);
const PHYSICAL_MEASURE_TIME: Duration = Duration::from_secs(1);
const PHYSICAL_WARMUP_TIME: Duration = Duration::from_millis(10);
const ROUNDS: usize = 3;
const MODES: [(&str, u32, u32); 4] = [
    ("Authentic", 1, 8),
    ("X2", 2, 16),
    ("X5", 5, 40),
    ("X10", 10, 80),
];

#[derive(Clone, Copy)]
struct Sample {
    elapsed: Duration,
    full_percent: f64,
    partial_percent: f64,
}

fn machine() -> BackendHost {
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(
        S100HardwareConfig::historical_8800b_18_slot_starter()
            .validate()
            .unwrap(),
        RamInit::Zeroed,
    );
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    machine.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]);
    machine.set_serial_clock_managed(true);
    machine.set_running(true);
    machine
}

fn budget_for(multiplier: u32, duration: Duration) -> u64 {
    let nanos = duration.as_nanos();
    let t_states = u128::from(TWO_MHZ)
        .saturating_mul(u128::from(multiplier))
        .saturating_mul(nanos)
        / 1_000_000_000;
    t_states.min(u128::from(u64::MAX)) as u64
}

fn run_sliced_interval(machine: &mut BackendHost, budget: u64, slice_t_states: u32) {
    let before = machine.intel8080_state().total_t_states.unwrap();
    let mut executed = 0u64;
    while executed < budget && machine.running() {
        let slice = (budget - executed).min(u64::from(slice_t_states)) as u32;
        let before_slice = executed;
        machine.run_cycles(slice);
        let total = machine.intel8080_state().total_t_states.unwrap() - before;
        assert!(total > before_slice, "scheduler slice made no CPU progress");
        machine.advance_serial_physical_time(SERIAL_SLICE);
        executed = total;
    }
    assert_eq!(executed, budget);
}

fn warm_up_sliced(machine: &mut BackendHost, multiplier: u32, slice_t_states: u32) {
    run_sliced_interval(
        machine,
        budget_for(multiplier, PHYSICAL_WARMUP_TIME),
        slice_t_states,
    );
}

fn warm_up_coarse(machine: &mut BackendHost, multiplier: u32) {
    let budget = budget_for(multiplier, PHYSICAL_WARMUP_TIME);
    machine.run_cycles(budget as u32);
    machine.advance_serial_physical_time(PHYSICAL_WARMUP_TIME);
}

fn measure_sliced(multiplier: u32, slice_t_states: u32) -> Sample {
    let mut machine = machine();
    warm_up_sliced(&mut machine, multiplier, slice_t_states);
    let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);

    adaptive_metrics::begin_measurement();
    let started = Instant::now();
    run_sliced_interval(&mut machine, budget, slice_t_states);
    let elapsed = started.elapsed();
    let stats = adaptive_metrics::end_measurement();
    assert_eq!(stats.total_t_states(), budget);

    Sample {
        elapsed,
        full_percent: stats.full_percent(),
        partial_percent: stats.partial_percent(),
    }
}

fn measure_coarse(multiplier: u32) -> Sample {
    let mut machine = machine();
    warm_up_coarse(&mut machine, multiplier);
    let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);

    adaptive_metrics::begin_measurement();
    let started = Instant::now();
    machine.run_cycles(budget as u32);
    machine.advance_serial_physical_time(PHYSICAL_MEASURE_TIME);
    let elapsed = started.elapsed();
    let stats = adaptive_metrics::end_measurement();
    assert_eq!(stats.total_t_states(), budget);

    Sample {
        elapsed,
        full_percent: stats.full_percent(),
        partial_percent: stats.partial_percent(),
    }
}

fn median(samples: &[Sample]) -> Sample {
    let mut ordered = samples.to_vec();
    ordered.sort_by_key(|sample| sample.elapsed);
    ordered[ordered.len() / 2]
}

fn elapsed_range(samples: &[Sample]) -> (Duration, Duration) {
    let min = samples
        .iter()
        .map(|sample| sample.elapsed)
        .min()
        .unwrap();
    let max = samples
        .iter()
        .map(|sample| sample.elapsed)
        .max()
        .unwrap();
    (min, max)
}

#[test]
#[ignore = "manual release benchmark for the 4 us managed serial scheduler"]
fn measure_managed_serial_scheduler_cost() {
    println!();
    println!("RusTair managed physical serial scheduler cost");
    println!(
        "Hardware: historical 8800b starter, 16K static RAM + installed idle 88-2SIO"
    );
    println!(
        "Workload: NOP/JMP loop, exactly 1.000 s of guest CPU target time and serial physical time"
    );
    println!(
        "Sliced path reproduces production's 4 us interleave: 8/16/40/80 T at Authentic/X2/X5/X10"
    );
    println!(
        "Coarse path executes the same T-states and the same serial elapsed time without 250000 interleave boundaries"
    );
    println!("Median of {ROUNDS} paired rounds after a 10 ms warm-up");
    println!();

    for (label, multiplier, slice_t_states) in MODES {
        let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);
        let target_mhz = f64::from(TWO_MHZ) * f64::from(multiplier) / 1_000_000.0;
        let mut sliced_samples = Vec::with_capacity(ROUNDS);
        let mut coarse_samples = Vec::with_capacity(ROUNDS);

        for round in 0..ROUNDS {
            if round & 1 == 0 {
                coarse_samples.push(measure_coarse(multiplier));
                sliced_samples.push(measure_sliced(multiplier, slice_t_states));
            } else {
                sliced_samples.push(measure_sliced(multiplier, slice_t_states));
                coarse_samples.push(measure_coarse(multiplier));
            }
        }

        let sliced = median(&sliced_samples);
        let coarse = median(&coarse_samples);
        let sliced_mhz = budget as f64 / sliced.elapsed.as_secs_f64() / 1_000_000.0;
        let coarse_mhz = budget as f64 / coarse.elapsed.as_secs_f64() / 1_000_000.0;
        let headroom = PHYSICAL_MEASURE_TIME.as_secs_f64() / sliced.elapsed.as_secs_f64();
        let slowdown = sliced.elapsed.as_secs_f64() / coarse.elapsed.as_secs_f64();
        let overhead_percent = (slowdown - 1.0) * 100.0;
        let (sliced_min, sliced_max) = elapsed_range(&sliced_samples);

        println!(
            "{label:>9} target={target_mhz:>5.1} MHz slice={slice_t_states:>2}T | sliced {:>8.3} ms ({sliced_mhz:>8.2} MHz host, {headroom:>6.2}x realtime) | Full={:>6.2}% Partial={:>6.2}% | coarse {:>8.3} ms ({coarse_mhz:>8.2} MHz) | scheduler cost {slowdown:>6.2}x ({overhead_percent:>+8.1}%) | sliced range {:>8.3}-{:>8.3} ms",
            sliced.elapsed.as_secs_f64() * 1_000.0,
            sliced.full_percent,
            sliced.partial_percent,
            coarse.elapsed.as_secs_f64() * 1_000.0,
            sliced_min.as_secs_f64() * 1_000.0,
            sliced_max.as_secs_f64() * 1_000.0,
        );
    }
}
