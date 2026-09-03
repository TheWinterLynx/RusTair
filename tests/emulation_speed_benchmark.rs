use std::time::{Duration, Instant};

use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};

const ALTAIR_CLOCK_HZ: f64 = 2_000_000.0;
const WARMUP_T_STATES: u64 = 250_000;
const MEASURE_T_STATES: u64 = 5_000_000;
const SERVICE_CHUNK_T_STATES: u32 = 100_000;

// NOP ; JMP 0000h
// Keeps the CPU fetching real opcodes and operands through main's normal memory
// path instead of benchmarking a host-side empty loop.
const BENCH_PROGRAM: [u8; 4] = [0x00, 0xc3, 0x00, 0x00];

#[derive(Clone, Copy)]
struct Scenario {
    label: &'static str,
    ram: RamSize,
    serial: SerialBoard,
}

#[derive(Clone, Copy)]
struct ResultRow {
    engine: EmulationEngine,
    scenario: &'static str,
    t_states: u64,
    elapsed: Duration,
    mhz: f64,
    realtime_multiple: f64,
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

fn benchmark_one(engine: EmulationEngine, scenario: Scenario) -> ResultRow {
    let mut machine = BackendHost::from_engine(engine).expect("built-in Rust backend");
    machine.configure_memory(scenario.ram, RamInit::Zeroed);
    machine.configure_memory_board_profile(RamBoardProfile::FastNoWait);
    machine.configure_serial_board(scenario.serial);
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

    let seconds = elapsed.as_secs_f64();
    let hz = t_states as f64 / seconds;
    ResultRow {
        engine,
        scenario: scenario.label,
        t_states,
        elapsed,
        mhz: hz / 1_000_000.0,
        realtime_multiple: hz / ALTAIR_CLOCK_HZ,
    }
}

fn print_row(row: ResultRow) {
    println!(
        "{:<28} | {:<34} | {:>10} T | {:>8.3} s | {:>8.3} MHz | {:>7.2}x Altair 2 MHz",
        row.engine.label(),
        row.scenario,
        row.t_states,
        row.elapsed.as_secs_f64(),
        row.mhz,
        row.realtime_multiple,
    );
}

/// Mainline performance baseline. Explicitly ignored so normal `cargo test`
/// never pays for a performance measurement.
#[test]
#[ignore = "manual performance benchmark"]
fn measure_fast_and_cycle_effective_mhz() {
    println!();
    println!("RusTair main effective 8080 throughput");
    println!("Measurement: {MEASURE_T_STATES} emulated T-states after {WARMUP_T_STATES}T warm-up");
    println!("Reference: MITS Altair 8800 nominal CPU clock = 2.000 MHz");
    println!();

    // These are the closest mainline equivalents of the two physical S-100
    // scenarios used on the architecture branch. Main still models RAM/serial
    // through aggregate globals, so the labels say so explicitly rather than
    // pretending they are identical hardware assemblies.
    for scenario in [
        Scenario {
            label: "main legacy 4K no-wait + 88-SIO",
            ram: RamSize::K4,
            serial: SerialBoard::Sio88,
        },
        Scenario {
            label: "main legacy 16K no-wait + 88-2SIO",
            ram: RamSize::K16,
            serial: SerialBoard::TwoSio88,
        },
    ] {
        for engine in [
            EmulationEngine::RustFast8080,
            EmulationEngine::RustCycleAccurate8080,
        ] {
            print_row(benchmark_one(engine, scenario));
        }
    }
}
