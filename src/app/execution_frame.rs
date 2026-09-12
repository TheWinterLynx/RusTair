use std::time::{Duration, Instant};

use crate::backend::BackendHost;

use super::execution_clock::UNLIMITED_CHUNK_T_STATES;

const SERVICE_SLICE_T_STATES: u32 = 4_096;
const UNLIMITED_SERVICE_SLICE_T_STATES: u32 = 65_536;
pub(super) const CPU_FRAME_TIME: Duration = Duration::from_millis(8);
/// Unlimited remains host-throughput driven, but it must yield often enough for
/// the egui event/render loop to stay interactive. The CPU executes on this same
/// thread, so a 100 ms burst hard-limited the GUI to roughly 10 FPS. A 12 ms
/// deadline preserves the large service slices while returning control within a
/// normal interactive frame budget.
const UNLIMITED_CPU_FRAME_TIME: Duration = Duration::from_millis(12);

#[inline]
fn host_profile(budget: u32, requested_limit: Duration) -> (u32, Duration) {
    if budget == UNLIMITED_CHUNK_T_STATES {
        (UNLIMITED_SERVICE_SLICE_T_STATES, UNLIMITED_CPU_FRAME_TIME)
    } else {
        (SERVICE_SLICE_T_STATES, requested_limit)
    }
}

/// Yield between exact engine budgets. This changes host scheduling only: all
/// serviced T-states remain on the authoritative CPU and serial-card timeline.
/// Throttled modes use short slices and the caller's normal 8 ms UI deadline.
/// Unlimited receives the sentinel budget from `ExecutionClock`, keeps larger
/// service slices for throughput, and yields on a short host deadline so the
/// renderer is not starved. The deadline can overrun by one slice, never by the
/// full budget.
pub(super) fn run_cpu_frame(machine: &mut BackendHost, budget: u32, limit: Duration) -> u64 {
    let (service_slice_t_states, effective_limit) = host_profile(budget, limit);
    let started = Instant::now();
    let before = machine
        .intel8080_state()
        .total_t_states
        .expect("8080 T-state clock");
    let mut executed = 0;
    while executed < u64::from(budget) && machine.running() {
        let slice = (u64::from(budget) - executed).min(u64::from(service_slice_t_states)) as u32;
        machine.run_cycles(slice);
        let total = machine
            .intel8080_state()
            .total_t_states
            .expect("8080 T-state clock")
            - before;
        if total == executed {
            break;
        }
        executed = total;
        if started.elapsed() >= effective_limit {
            break;
        }
    }
    executed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RamInit, S100HardwareConfig};

    fn machine() -> BackendHost {
        let mut machine = BackendHost::default();
        machine.configure_s100_hardware(
            S100HardwareConfig::historical_8800b_18_slot_starter(),
            RamInit::Zeroed,
        );
        machine.power(true);
        machine.set_running(false);
        machine.reset();
        // Power-on registers are undefined. Initialize through guest code so
        // equality below compares identical, physically constructed states.
        machine.load_bytes(
            0,
            &[
                0x31, 0, 0x0f, 0x01, 0, 0, 0x11, 0, 0, 0x21, 0, 0, 0xaf, 0, 0xc3, 13, 0,
            ],
        );
        machine.set_running(true);
        machine
    }

    #[test]
    fn expired_frame_yields_then_resumes_the_same_exact_timeline() {
        let mut sliced = machine();
        let mut uninterrupted = machine();
        let first = run_cpu_frame(&mut sliced, 40_000, Duration::ZERO);
        assert_eq!(first, u64::from(SERVICE_SLICE_T_STATES));
        let rest = run_cpu_frame(&mut sliced, 40_000 - first as u32, Duration::from_secs(1));
        assert_eq!(first + rest, 40_000);
        uninterrupted.run_cycles(40_000);
        assert_eq!(sliced.intel8080_state(), uninterrupted.intel8080_state());
    }

    #[test]
    fn unlimited_budget_selects_high_throughput_host_profile() {
        assert_eq!(
            host_profile(UNLIMITED_CHUNK_T_STATES, CPU_FRAME_TIME),
            (UNLIMITED_SERVICE_SLICE_T_STATES, UNLIMITED_CPU_FRAME_TIME)
        );
        assert!(
            UNLIMITED_CPU_FRAME_TIME <= Duration::from_millis(16),
            "Unlimited must yield within an interactive GUI frame budget"
        );
        assert_eq!(
            host_profile(400_000, CPU_FRAME_TIME),
            (SERVICE_SLICE_T_STATES, CPU_FRAME_TIME)
        );
    }

    #[test]
    fn large_unlimited_slice_is_only_host_scheduling_and_preserves_exact_timeline() {
        let mut sliced = machine();
        let mut uninterrupted = machine();
        // Exercise exactly one large host service slice without depending on
        // wall-clock performance of the test runner.
        let before = sliced.intel8080_state().total_t_states.unwrap();
        sliced.run_cycles(UNLIMITED_SERVICE_SLICE_T_STATES);
        let after = sliced.intel8080_state().total_t_states.unwrap();
        uninterrupted.run_cycles(UNLIMITED_SERVICE_SLICE_T_STATES);
        assert_eq!(after - before, u64::from(UNLIMITED_SERVICE_SLICE_T_STATES));
        assert_eq!(sliced.intel8080_state(), uninterrupted.intel8080_state());
    }

    #[test]
    fn stopped_machine_does_not_consume_frame_budget() {
        let mut machine = machine();
        machine.set_running(false);
        assert_eq!(
            run_cpu_frame(&mut machine, UNLIMITED_CHUNK_T_STATES, CPU_FRAME_TIME),
            0
        );
    }
}
