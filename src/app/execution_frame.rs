use std::time::{Duration, Instant};

use crate::backend::BackendHost;

const SERVICE_SLICE_T_STATES: u32 = 4_096;
pub(super) const CPU_FRAME_TIME: Duration = Duration::from_millis(8);

/// Yield between exact engine budgets. This changes host scheduling only: all
/// serviced T-states remain on the authoritative CPU and serial-card timeline.
/// The deadline can overrun by one slice, never by the entire Unlimited budget.
pub(super) fn run_cpu_frame(machine: &mut BackendHost, budget: u32, limit: Duration) -> u64 {
    let started = Instant::now();
    let before = machine.intel8080_state().total_t_states.expect("8080 T-state clock");
    let mut executed = 0;
    while executed < u64::from(budget) && machine.running() {
        let slice = (u64::from(budget) - executed).min(u64::from(SERVICE_SLICE_T_STATES)) as u32;
        machine.run_cycles(slice);
        let total = machine.intel8080_state().total_t_states.expect("8080 T-state clock") - before;
        if total == executed {
            break;
        }
        executed = total;
        if started.elapsed() >= limit {
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
        machine.configure_s100_hardware(S100HardwareConfig::historical_8800b_18_slot_starter(), RamInit::Zeroed);
        machine.power(true);
        machine.set_running(false);
        machine.reset();
        // Power-on registers are undefined. Initialize through guest code so
        // equality below compares identical, physically constructed states.
        machine.load_bytes(0, &[
            0x31, 0, 0x0f, 0x01, 0, 0, 0x11, 0, 0, 0x21, 0, 0,
            0xaf, 0, 0xc3, 13, 0,
        ]);
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
    fn stopped_machine_does_not_consume_frame_budget() {
        let mut machine = machine();
        machine.set_running(false);
        assert_eq!(run_cpu_frame(&mut machine, 1_000_000, CPU_FRAME_TIME), 0);
    }
}
