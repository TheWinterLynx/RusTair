use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdaptiveCycleFallbackStats {
    pub chassis_unsupported: u64,
    pub serial_active: u64,
    pub ready_low: u64,
    pub hold: u64,
    pub interrupt_pending: u64,
    pub budget_tail: u64,
    pub not_instruction_boundary: u64,
    pub stop_wait_pending: u64,
    pub cpu_fault: u64,
    pub reset: u64,
    pub opcode_barrier: u64,
    pub full_window_unavailable: u64,
}

impl AdaptiveCycleFallbackStats {
    pub const fn total(self) -> u64 {
        self.chassis_unsupported
            + self.serial_active
            + self.ready_low
            + self.hold
            + self.interrupt_pending
            + self.budget_tail
            + self.not_instruction_boundary
            + self.stop_wait_pending
            + self.cpu_fault
            + self.reset
            + self.opcode_barrier
            + self.full_window_unavailable
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AdaptiveCycleStats {
    pub full_instructions: u64,
    pub full_t_states: u64,
    pub partial_t_states: u64,
    pub full_windows: u64,
    pub partial_entries: u64,
    pub full_to_partial: u64,
    pub partial_to_full: u64,
    pub fallbacks: AdaptiveCycleFallbackStats,
}

impl AdaptiveCycleStats {
    pub const fn total_t_states(self) -> u64 {
        self.full_t_states + self.partial_t_states
    }

    pub fn full_percent(self) -> f64 {
        let total = self.total_t_states();
        if total == 0 { 0.0 } else { self.full_t_states as f64 * 100.0 / total as f64 }
    }

    pub fn partial_percent(self) -> f64 {
        let total = self.total_t_states();
        if total == 0 { 0.0 } else { self.partial_t_states as f64 * 100.0 / total as f64 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionPath {
    Full,
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdaptiveFallbackReason {
    ChassisUnsupported,
    SerialActive,
    ReadyLow,
    Hold,
    InterruptPending,
    BudgetTail,
    NotInstructionBoundary,
    StopWaitPending,
    CpuFault,
    Reset,
    OpcodeBarrier,
    FullWindowUnavailable,
}

#[derive(Default)]
struct MetricsState {
    enabled: bool,
    stats: AdaptiveCycleStats,
    last_path: Option<ExecutionPath>,
}

thread_local! {
    static METRICS: RefCell<MetricsState> = RefCell::new(MetricsState::default());
}

/// Start a fresh measurement on the current execution thread. Metrics are
/// deliberately opt-in so production emulation pays no per-T-state observer cost.
pub fn begin_measurement() {
    METRICS.with(|metrics| {
        *metrics.borrow_mut() = MetricsState {
            enabled: true,
            ..MetricsState::default()
        };
    });
}

pub fn snapshot() -> AdaptiveCycleStats {
    METRICS.with(|metrics| metrics.borrow().stats)
}

pub fn end_measurement() -> AdaptiveCycleStats {
    METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        let stats = metrics.stats;
        metrics.enabled = false;
        metrics.last_path = None;
        stats
    })
}

pub(crate) fn record_full_window(instructions: u64, t_states: u64) {
    if instructions == 0 || t_states == 0 { return; }
    METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        if !metrics.enabled { return; }
        metrics.stats.full_windows = metrics.stats.full_windows.saturating_add(1);
        metrics.stats.full_instructions = metrics.stats.full_instructions.saturating_add(instructions);
        metrics.stats.full_t_states = metrics.stats.full_t_states.saturating_add(t_states);
        if metrics.last_path == Some(ExecutionPath::Partial) {
            metrics.stats.partial_to_full = metrics.stats.partial_to_full.saturating_add(1);
        }
        metrics.last_path = Some(ExecutionPath::Full);
    });
}

pub(crate) fn record_partial_span(t_states: u64, reason: AdaptiveFallbackReason) {
    if t_states == 0 { return; }
    METRICS.with(|metrics| {
        let mut metrics = metrics.borrow_mut();
        if !metrics.enabled { return; }
        metrics.stats.partial_t_states = metrics.stats.partial_t_states.saturating_add(t_states);
        if metrics.last_path == Some(ExecutionPath::Partial) { return; }

        metrics.stats.partial_entries = metrics.stats.partial_entries.saturating_add(1);
        if metrics.last_path == Some(ExecutionPath::Full) {
            metrics.stats.full_to_partial = metrics.stats.full_to_partial.saturating_add(1);
        }
        match reason {
            AdaptiveFallbackReason::ChassisUnsupported => {
                metrics.stats.fallbacks.chassis_unsupported = metrics.stats.fallbacks.chassis_unsupported.saturating_add(1)
            }
            AdaptiveFallbackReason::SerialActive => {
                metrics.stats.fallbacks.serial_active = metrics.stats.fallbacks.serial_active.saturating_add(1)
            }
            AdaptiveFallbackReason::ReadyLow => {
                metrics.stats.fallbacks.ready_low = metrics.stats.fallbacks.ready_low.saturating_add(1)
            }
            AdaptiveFallbackReason::Hold => {
                metrics.stats.fallbacks.hold = metrics.stats.fallbacks.hold.saturating_add(1)
            }
            AdaptiveFallbackReason::InterruptPending => {
                metrics.stats.fallbacks.interrupt_pending = metrics.stats.fallbacks.interrupt_pending.saturating_add(1)
            }
            AdaptiveFallbackReason::BudgetTail => {
                metrics.stats.fallbacks.budget_tail = metrics.stats.fallbacks.budget_tail.saturating_add(1)
            }
            AdaptiveFallbackReason::NotInstructionBoundary => {
                metrics.stats.fallbacks.not_instruction_boundary = metrics.stats.fallbacks.not_instruction_boundary.saturating_add(1)
            }
            AdaptiveFallbackReason::StopWaitPending => {
                metrics.stats.fallbacks.stop_wait_pending = metrics.stats.fallbacks.stop_wait_pending.saturating_add(1)
            }
            AdaptiveFallbackReason::CpuFault => {
                metrics.stats.fallbacks.cpu_fault = metrics.stats.fallbacks.cpu_fault.saturating_add(1)
            }
            AdaptiveFallbackReason::Reset => {
                metrics.stats.fallbacks.reset = metrics.stats.fallbacks.reset.saturating_add(1)
            }
            AdaptiveFallbackReason::OpcodeBarrier => {
                metrics.stats.fallbacks.opcode_barrier = metrics.stats.fallbacks.opcode_barrier.saturating_add(1)
            }
            AdaptiveFallbackReason::FullWindowUnavailable => {
                metrics.stats.fallbacks.full_window_unavailable = metrics.stats.fallbacks.full_window_unavailable.saturating_add(1)
            }
        }
        metrics.last_path = Some(ExecutionPath::Partial);
    });
}
