use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugStopReason {
    ExecuteBreakpoint(u16),
    RunTo(u16),
}

impl DebugStopReason {
    pub fn label(self) -> String {
        match self {
            Self::ExecuteBreakpoint(address) => format!("execute breakpoint at ${address:04X}"),
            Self::RunTo(address) => format!("run-to target reached at ${address:04X}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DebugExecutionControl {
    breakpoints: BTreeSet<u16>,
    run_to: Option<u16>,
    resume_skip_once: Option<u16>,
    stop_reason: Option<DebugStopReason>,
}

impl DebugExecutionControl {
    pub fn active(&self) -> bool {
        !self.breakpoints.is_empty() || self.run_to.is_some()
    }

    pub fn breakpoints(&self) -> Vec<u16> {
        self.breakpoints.iter().copied().collect()
    }

    pub fn set_breakpoint(&mut self, address: u16, enabled: bool) {
        if enabled {
            self.breakpoints.insert(address);
        } else {
            self.breakpoints.remove(&address);
        }
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    pub fn set_run_to(&mut self, address: u16) {
        self.run_to = Some(address);
        self.stop_reason = None;
    }

    pub fn cancel_run_to(&mut self) {
        self.run_to = None;
    }

    pub fn run_to(&self) -> Option<u16> {
        self.run_to
    }

    pub fn stop_reason(&self) -> Option<DebugStopReason> {
        self.stop_reason
    }

    pub fn clear_transient(&mut self) {
        self.run_to = None;
        self.resume_skip_once = None;
        self.stop_reason = None;
    }

    /// Resuming while stopped exactly on a persistent breakpoint must execute
    /// that instruction once rather than immediately re-triggering forever.
    pub fn prepare_resume(&mut self, pc: u16) {
        self.resume_skip_once = self.breakpoints.contains(&pc).then_some(pc);
        self.stop_reason = None;
    }

    /// Called only at a true instruction boundary, before fetching the next
    /// guest opcode. Returns the reason when execution must stop at this PC.
    pub fn stop_before(&mut self, pc: u16) -> Option<DebugStopReason> {
        if self.resume_skip_once == Some(pc) {
            self.resume_skip_once = None;
            return None;
        }

        if self.run_to == Some(pc) {
            self.run_to = None;
            let reason = DebugStopReason::RunTo(pc);
            self.stop_reason = Some(reason);
            return Some(reason);
        }

        if self.breakpoints.contains(&pc) {
            let reason = DebugStopReason::ExecuteBreakpoint(pc);
            self.stop_reason = Some(reason);
            return Some(reason);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_breakpoint_skips_once_when_resuming_from_it() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x1234, true);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
        control.prepare_resume(0x1234);
        assert_eq!(control.stop_before(0x1234), None);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
    }

    #[test]
    fn run_to_is_one_shot_and_precedes_persistent_breakpoint() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x2000, true);
        control.set_run_to(0x2000);
        assert_eq!(control.stop_before(0x2000), Some(DebugStopReason::RunTo(0x2000)));
        assert_eq!(control.run_to(), None);
        control.prepare_resume(0x2000);
        assert_eq!(control.stop_before(0x2000), None);
    }
}
