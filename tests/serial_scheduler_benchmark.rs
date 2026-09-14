use std::time::{Duration, Instant};

use rustair::adaptive_metrics;
use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{RamInit, S100HardwareConfig};

const TWO_MHZ: u32 = 2_000_000;
const SERVICE_SLICE_T_STATES: u32 = 4_096;
const PHYSICAL_MEASURE_TIME: Duration = Duration::from_secs(1);
const PHYSICAL_WARMUP_TIME: Duration = Duration::from_millis(10);
const ROUNDS: usize = 3;
const MODES: [(&str, u32); 4] = [("Authentic", 1), ("X2", 2), ("X5", 5), ("X10", 10)];

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

fn active_break_machine(port: BackendSerialPort) -> BackendHost {
    let mut machine = machine();
    let control_port = match port {
        BackendSerialPort::Port0 => 0x10,
        BackendSerialPort::Port1 => 0x12,
    };
    machine.debugger_output_port(control_port, 0x15);
    assert!(machine.serial_set_receive_break(port, true));
    assert!(machine.serial_clock_deadline_t_states().is_some());
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

fn guest_t_states_for_deadline(deadline: u64, multiplier: u32) -> u32 {
    let numerator = u128::from(deadline)
        .saturating_mul(u128::from(TWO_MHZ))
        .saturating_mul(u128::from(multiplier));
    ((numerator / u128::from(TWO_MHZ))
        .max(1)
        .min(u128::from(u32::MAX))) as u32
}

fn physical_time_for_t_states(t_states: u64, multiplier: u32) -> Duration {
    let effective_cpu_hz = u128::from(TWO_MHZ).saturating_mul(u128::from(multiplier));
    let nanos = u128::from(t_states).saturating_mul(1_000_000_000) / effective_cpu_hz.max(1);
    Duration::from_nanos(nanos.min(u128::from(u64::MAX)) as u64)
}

fn run_deadline_interval(machine: &mut BackendHost, budget: u64, multiplier: u32) {
    let before = machine.intel8080_state().total_t_states.unwrap();
    let mut executed = 0u64;
    while executed < budget && machine.running() {
        let service_slice = machine
            .serial_clock_deadline_t_states()
            .map(|deadline| {
                guest_t_states_for_deadline(deadline, multiplier).min(SERVICE_SLICE_T_STATES)
            })
            .unwrap_or(SERVICE_SLICE_T_STATES);
        let slice = (budget - executed).min(u64::from(service_slice)) as u32;
        let before_slice = executed;
        machine.run_cycles(slice);
        let total = machine.intel8080_state().total_t_states.unwrap() - before;
        assert!(
            total > before_slice,
            "scheduler deadline made no CPU progress"
        );
        machine.advance_serial_physical_time(physical_time_for_t_states(
            total - before_slice,
            multiplier,
        ));
        executed = total;
    }
    assert_eq!(executed, budget);
}

fn warm_up_deadline(machine: &mut BackendHost, multiplier: u32) {
    run_deadline_interval(
        machine,
        budget_for(multiplier, PHYSICAL_WARMUP_TIME),
        multiplier,
    );
}

fn warm_up_coarse(machine: &mut BackendHost, multiplier: u32) {
    let budget = budget_for(multiplier, PHYSICAL_WARMUP_TIME);
    machine.run_cycles(budget as u32);
    machine.advance_serial_physical_time(PHYSICAL_WARMUP_TIME);
}

fn measure_deadline(multiplier: u32) -> Sample {
    let mut machine = machine();
    warm_up_deadline(&mut machine, multiplier);
    let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);

    adaptive_metrics::begin_measurement();
    let started = Instant::now();
    run_deadline_interval(&mut machine, budget, multiplier);
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

fn measure_active_deadline(multiplier: u32, port: BackendSerialPort) -> Sample {
    let mut machine = active_break_machine(port);
    warm_up_deadline(&mut machine, multiplier);
    let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);

    adaptive_metrics::begin_measurement();
    let started = Instant::now();
    run_deadline_interval(&mut machine, budget, multiplier);
    let elapsed = started.elapsed();
    let stats = adaptive_metrics::end_measurement();
    assert_eq!(stats.total_t_states(), budget);

    Sample {
        elapsed,
        full_percent: stats.full_percent(),
        partial_percent: stats.partial_percent(),
    }
}

fn measure_active_coarse(multiplier: u32, port: BackendSerialPort) -> Sample {
    let mut machine = active_break_machine(port);
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
    let min = samples.iter().map(|sample| sample.elapsed).min().unwrap();
    let max = samples.iter().map(|sample| sample.elapsed).max().unwrap();
    (min, max)
}

#[test]
#[ignore = "manual release benchmark for the managed card-driven serial scheduler"]
fn measure_managed_serial_scheduler_cost() {
    println!();
    println!("RusTair event-driven physical serial scheduler cost");
    println!("Hardware: historical 8800b starter, 16K static RAM + installed idle 88-2SIO");
    println!(
        "Workload: NOP/JMP loop, exactly 1.000 s of guest CPU target time and serial physical time"
    );
    println!(
        "Idle UARTs expose no deadline and retain the normal 4096T Adaptive service slice; active UARTs use card-owned deadlines"
    );
    println!(
        "Coarse path executes the same T-states and the same serial elapsed time without intermediate service boundaries"
    );
    println!("Median of {ROUNDS} paired rounds after a 10 ms warm-up");
    println!();

    for (label, multiplier) in MODES {
        let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);
        let target_mhz = f64::from(TWO_MHZ) * f64::from(multiplier) / 1_000_000.0;
        let initial_guest_slice = machine()
            .serial_clock_deadline_t_states()
            .map(|deadline| {
                guest_t_states_for_deadline(deadline, multiplier).min(SERVICE_SLICE_T_STATES)
            })
            .unwrap_or(SERVICE_SLICE_T_STATES);
        let mut deadline_samples = Vec::with_capacity(ROUNDS);
        let mut coarse_samples = Vec::with_capacity(ROUNDS);

        for round in 0..ROUNDS {
            if round & 1 == 0 {
                coarse_samples.push(measure_coarse(multiplier));
                deadline_samples.push(measure_deadline(multiplier));
            } else {
                deadline_samples.push(measure_deadline(multiplier));
                coarse_samples.push(measure_coarse(multiplier));
            }
        }

        let deadline = median(&deadline_samples);
        let coarse = median(&coarse_samples);
        let deadline_mhz = budget as f64 / deadline.elapsed.as_secs_f64() / 1_000_000.0;
        let coarse_mhz = budget as f64 / coarse.elapsed.as_secs_f64() / 1_000_000.0;
        let headroom = PHYSICAL_MEASURE_TIME.as_secs_f64() / deadline.elapsed.as_secs_f64();
        let slowdown = deadline.elapsed.as_secs_f64() / coarse.elapsed.as_secs_f64();
        let overhead_percent = (slowdown - 1.0) * 100.0;
        let (deadline_min, deadline_max) = elapsed_range(&deadline_samples);

        println!(
            "{label:>9} target={target_mhz:>5.1} MHz first={initial_guest_slice:>4}T | deadline {:>8.3} ms ({deadline_mhz:>8.2} MHz host, {headroom:>6.2}x realtime) | Full={:>6.2}% Partial={:>6.2}% | coarse {:>8.3} ms ({coarse_mhz:>8.2} MHz) | scheduler cost {slowdown:>6.2}x ({overhead_percent:>+8.1}%) | deadline range {:>8.3}-{:>8.3} ms",
            deadline.elapsed.as_secs_f64() * 1_000.0,
            deadline.full_percent,
            deadline.partial_percent,
            coarse.elapsed.as_secs_f64() * 1_000.0,
            deadline_min.as_secs_f64() * 1_000.0,
            deadline_max.as_secs_f64() * 1_000.0,
        );
    }
}

#[test]
#[ignore = "manual release benchmark for active card-driven serial deadlines"]
fn measure_active_serial_scheduler_cost() {
    println!();
    println!("RusTair active physical serial deadline cost");
    println!("Hardware: historical 8800b starter, 16K static RAM + 88-2SIO");
    println!("Workload: NOP/JMP loop with a continuous RX BREAK on one MC6850 port");
    println!("Each ACIA is configured for /16 before BREAK is asserted");
    println!("Median of {ROUNDS} paired rounds after a 10 ms warm-up");

    for (activity, port) in [
        ("Port0 110 baud", BackendSerialPort::Port0),
        ("Port1 9600 baud", BackendSerialPort::Port1),
    ] {
        println!();
        println!("{activity} continuous RX BREAK");

        for (label, multiplier) in MODES {
            let budget = budget_for(multiplier, PHYSICAL_MEASURE_TIME);
            let target_mhz = f64::from(TWO_MHZ) * f64::from(multiplier) / 1_000_000.0;
            let initial_guest_slice = active_break_machine(port)
                .serial_clock_deadline_t_states()
                .map(|deadline| {
                    guest_t_states_for_deadline(deadline, multiplier)
                        .min(SERVICE_SLICE_T_STATES)
                })
                .unwrap_or(SERVICE_SLICE_T_STATES);
            let mut deadline_samples = Vec::with_capacity(ROUNDS);
            let mut coarse_samples = Vec::with_capacity(ROUNDS);

            for round in 0..ROUNDS {
                if round & 1 == 0 {
                    coarse_samples.push(measure_active_coarse(multiplier, port));
                    deadline_samples.push(measure_active_deadline(multiplier, port));
                } else {
                    deadline_samples.push(measure_active_deadline(multiplier, port));
                    coarse_samples.push(measure_active_coarse(multiplier, port));
                }
            }

            let deadline = median(&deadline_samples);
            let coarse = median(&coarse_samples);
            let deadline_mhz = budget as f64 / deadline.elapsed.as_secs_f64() / 1_000_000.0;
            let coarse_mhz = budget as f64 / coarse.elapsed.as_secs_f64() / 1_000_000.0;
            let headroom = PHYSICAL_MEASURE_TIME.as_secs_f64() / deadline.elapsed.as_secs_f64();
            let slowdown = deadline.elapsed.as_secs_f64() / coarse.elapsed.as_secs_f64();
            let overhead_percent = (slowdown - 1.0) * 100.0;
            let (deadline_min, deadline_max) = elapsed_range(&deadline_samples);

            println!(
                "{label:>9} target={target_mhz:>5.1} MHz first={initial_guest_slice:>6}T | deadline {:>8.3} ms ({deadline_mhz:>8.2} MHz host, {headroom:>6.2}x realtime) | Full={:>6.2}% Partial={:>6.2}% | coarse {:>8.3} ms ({coarse_mhz:>8.2} MHz) | scheduler cost {slowdown:>6.2}x ({overhead_percent:>+8.1}%) | deadline range {:>8.3}-{:>8.3} ms",
                deadline.elapsed.as_secs_f64() * 1_000.0,
                deadline.full_percent,
                deadline.partial_percent,
                coarse.elapsed.as_secs_f64() * 1_000.0,
                deadline_min.as_secs_f64() * 1_000.0,
                deadline_max.as_secs_f64() * 1_000.0,
            );
        }
    }
}
