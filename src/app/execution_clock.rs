use std::time::{Duration, Instant};

use crate::config::EmulationSpeed;

/// One emulated T-state is represented as one billion fixed-point units so
/// nanosecond host intervals can be accumulated without losing fractional
/// cycles. Positive balance means the guest is behind wall clock; a small
/// negative balance is intentional when the Fast backend finishes an
/// instruction a few T-states past the requested budget.
const T_STATE_UNITS: i128 = 1_000_000_000;
const AUTHENTIC_CHUNK_T_STATES: u32 = 40_000;
const UNLIMITED_CHUNK_T_STATES: u32 = 1_000_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct ExecutionClock {
    last_accounted: Instant,
    balance_units: i128,
    rate: Option<(u32, EmulationSpeed)>,
}

impl ExecutionClock {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            last_accounted: now,
            balance_units: 0,
            rate: None,
        }
    }

    pub(super) fn reset_at(&mut self, now: Instant) {
        self.last_accounted = now;
        self.balance_units = 0;
        self.rate = None;
    }

    fn wall_clock_multiplier(speed: EmulationSpeed) -> Option<u32> {
        match speed {
            EmulationSpeed::Authentic => Some(1),
            EmulationSpeed::X2 => Some(2),
            EmulationSpeed::X5 => Some(5),
            EmulationSpeed::X10 => Some(10),
            EmulationSpeed::Unlimited => None,
        }
    }

    fn accrue(&mut self, elapsed: Duration, clock_hz: u32, multiplier: u32) {
        let added = elapsed
            .as_nanos()
            .saturating_mul(u128::from(clock_hz))
            .saturating_mul(u128::from(multiplier));
        let added = added.min(i128::MAX as u128) as i128;
        self.balance_units = self.balance_units.saturating_add(added);
    }

    /// Return the bounded T-state budget for this UI update.
    ///
    /// Stopped time is discarded rather than becoming catch-up debt. Throttled
    /// speeds preserve all elapsed time and fractional T-states. A clock/speed
    /// change starts a fresh debt epoch at `now`: the preceding host interval
    /// cannot be attributed safely to either side of an observed rate change,
    /// so it is deliberately not replayed at the new rate. Unlimited is
    /// intentionally detached from wall clock and simply supplies one bounded
    /// chunk per repaint.
    pub(super) fn budget(
        &mut self,
        now: Instant,
        running: bool,
        clock_hz: u32,
        speed: EmulationSpeed,
    ) -> u32 {
        let elapsed = now.saturating_duration_since(self.last_accounted);
        self.last_accounted = now;

        let rate = (clock_hz, speed);
        if self.rate != Some(rate) {
            self.balance_units = 0;
            self.rate = Some(rate);
            if !running {
                return 0;
            }
            return match Self::wall_clock_multiplier(speed) {
                Some(_) => 0,
                None => UNLIMITED_CHUNK_T_STATES,
            };
        }

        if !running {
            self.balance_units = 0;
            return 0;
        }

        let Some(multiplier) = Self::wall_clock_multiplier(speed) else {
            self.balance_units = 0;
            return UNLIMITED_CHUNK_T_STATES;
        };

        self.accrue(elapsed, clock_hz, multiplier);
        let available = if self.balance_units <= 0 {
            0
        } else {
            (self.balance_units / T_STATE_UNITS).min(i128::from(u32::MAX)) as u32
        };
        let max_chunk = AUTHENTIC_CHUNK_T_STATES.saturating_mul(multiplier);
        available.min(max_chunk)
    }

    /// Subtract the T-states the selected backend actually executed, not merely
    /// the requested budget. This preserves Fast-backend whole-instruction
    /// overshoot as a small negative balance that subsequent wall time repays.
    pub(super) fn record_executed(&mut self, executed_t_states: u64) {
        let consumed = i128::from(executed_t_states).saturating_mul(T_STATE_UNITS);
        self.balance_units = self.balance_units.saturating_sub(consumed);
    }

    /// A RUN latch can remain set while RESET/HOLD/debugger state prevents any
    /// CPU clocks from being serviced. Such blocked host time must not become a
    /// burst of fictitious catch-up execution after the condition is released.
    pub(super) fn discard_pending_debt(&mut self) {
        self.balance_units = 0;
    }

    #[cfg(test)]
    fn whole_t_state_balance(&self) -> i128 {
        self.balance_units / T_STATE_UNITS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_MHZ: u32 = 2_000_000;

    #[test]
    fn hundred_ms_stall_preserves_all_two_mhz_debt_across_bounded_updates() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);

        // Establish a RUN baseline with no elapsed host time.
        assert_eq!(
            clock.budget(t0, true, TWO_MHZ, EmulationSpeed::Authentic),
            0
        );

        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(
            clock.budget(t1, true, TWO_MHZ, EmulationSpeed::Authentic),
            40_000
        );
        assert_eq!(clock.whole_t_state_balance(), 200_000);

        // Catch-up is deliberately bounded to 20 ms of authentic CPU time per
        // UI update, but none of the remaining 160,000 T-states disappears.
        clock.record_executed(40_000);
        assert_eq!(clock.whole_t_state_balance(), 160_000);
        for expected_remaining in [120_000, 80_000, 40_000, 0] {
            assert_eq!(
                clock.budget(t1, true, TWO_MHZ, EmulationSpeed::Authentic),
                40_000
            );
            clock.record_executed(40_000);
            assert_eq!(clock.whole_t_state_balance(), expected_remaining);
        }
    }

    #[test]
    fn fractional_t_states_survive_between_updates() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);
        assert_eq!(clock.budget(t0, true, 3, EmulationSpeed::Authentic), 0);

        // 3 Hz * 500 ms = 1.5 T-states. Execute one and retain the half.
        let t1 = t0 + Duration::from_millis(500);
        assert_eq!(clock.budget(t1, true, 3, EmulationSpeed::Authentic), 1);
        clock.record_executed(1);
        assert_eq!(clock.whole_t_state_balance(), 0);

        // Another 500 ms contributes another 1.5, so two whole T-states are now due.
        let t2 = t1 + Duration::from_millis(500);
        assert_eq!(clock.budget(t2, true, 3, EmulationSpeed::Authentic), 2);
    }

    #[test]
    fn fast_backend_overshoot_is_repaid_instead_of_drifting_fast() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);
        assert_eq!(clock.budget(t0, true, TWO_MHZ, EmulationSpeed::Authentic), 0);
        let t1 = t0 + Duration::from_micros(2); // four T-states at 2 MHz
        assert_eq!(clock.budget(t1, true, TWO_MHZ, EmulationSpeed::Authentic), 4);

        clock.record_executed(7); // e.g. a whole seven-T-state instruction
        assert_eq!(clock.whole_t_state_balance(), -3);

        let t2 = t1 + Duration::from_micros(1); // +2, still one T-state ahead
        assert_eq!(clock.budget(t2, true, TWO_MHZ, EmulationSpeed::Authentic), 0);
        let t3 = t2 + Duration::from_micros(1); // +2, now one is due
        assert_eq!(clock.budget(t3, true, TWO_MHZ, EmulationSpeed::Authentic), 1);
    }

    #[test]
    fn stopped_time_never_becomes_execution_debt() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);
        let stopped = t0 + Duration::from_secs(10);
        assert_eq!(
            clock.budget(stopped, false, TWO_MHZ, EmulationSpeed::Authentic),
            0
        );
        assert_eq!(clock.whole_t_state_balance(), 0);

        let resumed = stopped + Duration::from_millis(16);
        assert_eq!(
            clock.budget(resumed, true, TWO_MHZ, EmulationSpeed::Authentic),
            32_000
        );
    }

    #[test]
    fn accelerated_modes_scale_clock_and_chunk_without_losing_debt() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);
        assert_eq!(clock.budget(t0, true, TWO_MHZ, EmulationSpeed::X10), 0);
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(clock.budget(t1, true, TWO_MHZ, EmulationSpeed::X10), 400_000);
        assert_eq!(clock.whole_t_state_balance(), 2_000_000);
    }

    #[test]
    fn changing_effective_speed_starts_new_epoch_without_retiming_prior_interval() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);
        assert_eq!(clock.budget(t0, true, TWO_MHZ, EmulationSpeed::Authentic), 0);
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(clock.budget(t1, true, TWO_MHZ, EmulationSpeed::Authentic), 40_000);
        assert_eq!(clock.whole_t_state_balance(), 200_000);

        // If a diagnostic changes to 10x before this observation, the 10 ms
        // since the last observation cannot rigorously be assigned to 1x or 10x.
        // Start the new rate at t2 instead of retroactively multiplying it.
        let t2 = t1 + Duration::from_millis(10);
        assert_eq!(clock.budget(t2, true, TWO_MHZ, EmulationSpeed::X10), 0);
        assert_eq!(clock.whole_t_state_balance(), 0);

        let t3 = t2 + Duration::from_millis(10);
        assert_eq!(clock.budget(t3, true, TWO_MHZ, EmulationSpeed::X10), 200_000);
        assert_eq!(clock.whole_t_state_balance(), 200_000);
    }

    #[test]
    fn unlimited_is_not_wall_clock_throttled() {
        let t0 = Instant::now();
        let mut clock = ExecutionClock::new(t0);
        assert_eq!(
            clock.budget(t0, true, TWO_MHZ, EmulationSpeed::Unlimited),
            UNLIMITED_CHUNK_T_STATES
        );
        let much_later = t0 + Duration::from_secs(5);
        assert_eq!(
            clock.budget(much_later, true, TWO_MHZ, EmulationSpeed::Unlimited),
            UNLIMITED_CHUNK_T_STATES
        );
        assert_eq!(clock.whole_t_state_balance(), 0);
    }
}
