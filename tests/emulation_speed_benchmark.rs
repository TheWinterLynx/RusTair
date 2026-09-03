use std::time::{Duration, Instant};

use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

const ALTAIR_CLOCK_HZ: f64 = 2_000_000.0;
const WARMUP_T_STATES: u64 = 250_000;
const MEASURE_T_STATES: u64 = 5_000_000;
const SERVICE_CHUNK_T_STATES: u32 = 100_000;

// NOP ; JMP 0000h
//
// This deliberately keeps the CPU fetching real opcodes and operands from the
// installed S-100 RAM instead of benchmarking a host-side empty loop.
const BENCH_PROGRAM: [u8; 4] = [0x00, 0xc3, 0x00, 0x00];

#[derive(Clone, Copy)]
struct ResultRow {
    engine: EmulationEngine,
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

fn benchmark_one(
    engine: EmulationEngine,
    scenario: &'static str,
    hardware: S100HardwareConfig,
) -> ResultRow {
    let mut machine = BackendHost::from_engine(engine).expect("built-in Rust backend");
    machine.configure_s100_hardware(hardware, RamInit::Zeroed);
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    machine.clear_memory_protection();
    machine.load_bytes(0x0000, &BENCH_PROGRAM);
    machine.set_running(true);

    // Warm caches and backend bookkeeping, but exclude this work from the result.
    let _ = run_t_states(&mut machine, WARMUP_T_STATES);

    let before = machine.intel8080_state().total_t_states.unwrap_or(0);
    let wall_start = Instant::now();
    let _ = run_t_states(&mut machine, MEASURE_T_STATES);
    let elapsed = wall_start.elapsed();
    let after = machine.intel8080_state().total_t_states.unwrap_or(before);
    let t_states = after.saturating_sub(before);

    let seconds = elapsed.as_secs_f64();
    let hz = t_states as f64 / seconds;
    ResultRow {
        engine,
        scenario,
        t_states,
        elapsed,
        mhz: hz / 1_000_000.0,
        realtime_multiple: hz / ALTAIR_CLOCK_HZ,
    }
}

fn print_row(row: ResultRow) {
    println!(
        "{:<28} | {:<30} | {:>10} T | {:>8.3} s | {:>8.3} MHz | {:>7.2}x Altair 2 MHz",
        row.engine.label(),
        row.scenario,
        row.t_states,
        row.elapsed.as_secs_f64(),
        row.mhz,
        row.realtime_multiple,
    );
}

/// Explicit performance meter, intentionally excluded from ordinary `cargo test`.
///
/// Run with:
/// `cargo test --release --test emulation_speed_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "manual performance benchmark"]
fn measure_fast_and_cycle_effective_mhz() {
    println!();
    println!("RusTair effective 8080 throughput");
    println!("Measurement: {MEASURE_T_STATES} emulated T-states after {WARMUP_T_STATES}T warm-up");
    println!("Reference: MITS Altair 8800 nominal CPU clock = 2.000 MHz");
    println!();

    for (scenario, hardware) in [
        ("CPU + 88-4MCS 4K Static", minimal_historical_hardware()),
        ("8800b + 16K Static + 88-2SIO", historical_starter_hardware()),
    ] {
        for engine in [
            EmulationEngine::RustFast8080,
            EmulationEngine::RustCycleAccurate8080,
        ] {
            print_row(benchmark_one(engine, scenario, hardware));
        }
    }
}
