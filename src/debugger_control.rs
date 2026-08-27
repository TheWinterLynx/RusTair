use std::collections::{BTreeMap, BTreeSet};

use crate::trace8080::InstructionEffect8080;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryWatchAccess {
    Read,
    Write,
    ReadWrite,
}

impl MemoryWatchAccess {
    pub const fn reads(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub const fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Read => "READ",
            Self::Write => "WRITE",
            Self::ReadWrite => "READ/WRITE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugStopReason {
    ExecuteBreakpoint(u16),
    RunTo(u16),
    MemoryReadWatchpoint {
        instruction_pc: u16,
        address: u16,
        value: u8,
    },
    MemoryWriteWatchpoint {
        instruction_pc: u16,
        address: u16,
        value: u8,
    },
}

impl DebugStopReason {
    pub fn label(self) -> String {
        match self {
            Self::ExecuteBreakpoint(address) => format!("execute breakpoint at ${address:04X}"),
            Self::RunTo(address) => format!("run-to target reached at ${address:04X}"),
            Self::MemoryReadWatchpoint { instruction_pc, address, value } => format!(
                "memory READ watchpoint at ${address:04X}: ${value:02X} read by instruction at ${instruction_pc:04X}"
            ),
            Self::MemoryWriteWatchpoint { instruction_pc, address, value } => format!(
                "memory WRITE watchpoint at ${address:04X}: ${value:02X} written by instruction at ${instruction_pc:04X}"
            ),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DebugExecutionControl {
    breakpoints: BTreeSet<u16>,
    watchpoints: BTreeMap<u16, MemoryWatchAccess>,
    run_to: Option<u16>,
    resume_skip_once: Option<u16>,
    stop_reason: Option<DebugStopReason>,
}

impl DebugExecutionControl {
    pub fn active(&self) -> bool {
        !self.breakpoints.is_empty() || !self.watchpoints.is_empty() || self.run_to.is_some()
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

    pub fn watchpoints(&self) -> Vec<(u16, MemoryWatchAccess)> {
        self.watchpoints
            .iter()
            .map(|(&address, &access)| (address, access))
            .collect()
    }

    pub fn set_watchpoint(&mut self, address: u16, access: Option<MemoryWatchAccess>) {
        match access {
            Some(access) => {
                self.watchpoints.insert(address, access);
            }
            None => {
                self.watchpoints.remove(&address);
            }
        }
    }

    pub fn clear_watchpoints(&mut self) {
        self.watchpoints.clear();
    }

    pub fn has_watchpoints(&self) -> bool {
        !self.watchpoints.is_empty()
    }

    pub fn set_run_to(&mut self, address: u16) {
        // Keep the previous stop reason until prepare_resume() consumes it.
        // This is required when Run to / Step over starts while stopped on an
        // execute breakpoint at the current PC: that opcode must be allowed to
        // execute once rather than immediately re-triggering the breakpoint.
        self.run_to = Some(address);
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

    /// A manual debugger single-step is not a RUN resume. Clear stale stop/run
    /// state without arming a future breakpoint skip at the old PC.
    pub fn prepare_manual_step(&mut self) {
        self.run_to = None;
        self.resume_skip_once = None;
        self.stop_reason = None;
    }

    /// Resuming after an execute breakpoint must execute that instruction once
    /// instead of immediately re-triggering. Merely having a breakpoint at the
    /// current PC is not enough: a fresh RUN must still stop before that opcode.
    pub fn prepare_resume(&mut self, pc: u16) {
        self.resume_skip_once = matches!(
            self.stop_reason,
            Some(DebugStopReason::ExecuteBreakpoint(address)) if address == pc
        )
        .then_some(pc);
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

    /// Called after one guest instruction has completed. Memory watchpoints are
    /// deliberately post-instruction stops: the trace tells us the exact access
    /// caused by that instruction, while the resulting CPU state remains intact
    /// for inspection in the debugger.
    pub fn stop_after_effects(
        &mut self,
        instruction_pc: u16,
        effects: &[InstructionEffect8080],
    ) -> Option<DebugStopReason> {
        for effect in effects {
            let reason = match *effect {
                InstructionEffect8080::MemoryRead { address, value }
                | InstructionEffect8080::StackRead { address, value }
                    if self.watchpoints.get(&address).is_some_and(|access| access.reads()) =>
                {
                    Some(DebugStopReason::MemoryReadWatchpoint {
                        instruction_pc,
                        address,
                        value,
                    })
                }
                InstructionEffect8080::MemoryWrite { address, value }
                | InstructionEffect8080::StackWrite { address, value }
                    if self.watchpoints.get(&address).is_some_and(|access| access.writes()) =>
                {
                    Some(DebugStopReason::MemoryWriteWatchpoint {
                        instruction_pc,
                        address,
                        value,
                    })
                }
                _ => None,
            };

            if let Some(reason) = reason {
                self.stop_reason = Some(reason);
                self.run_to = None;
                return Some(reason);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_run_still_stops_on_breakpoint_at_current_pc() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x1234, true);
        control.prepare_resume(0x1234);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
    }

    #[test]
    fn persistent_breakpoint_skips_once_when_resuming_from_triggered_stop() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x1234, true);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
        control.prepare_resume(0x1234);
        assert_eq!(control.stop_before(0x1234), None);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
    }

    #[test]
    fn manual_step_does_not_leave_future_breakpoint_skip_armed() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x1234, true);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
        control.prepare_manual_step();
        assert_eq!(control.stop_reason(), None);
        assert_eq!(control.stop_before(0x1234), Some(DebugStopReason::ExecuteBreakpoint(0x1234)));
    }

    #[test]
    fn run_to_from_triggered_breakpoint_can_resume_past_current_pc() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x1000, true);
        assert_eq!(control.stop_before(0x1000), Some(DebugStopReason::ExecuteBreakpoint(0x1000)));
        control.set_run_to(0x1002);
        control.prepare_resume(0x1000);
        assert_eq!(control.stop_before(0x1000), None);
        assert_eq!(control.stop_before(0x1002), Some(DebugStopReason::RunTo(0x1002)));
    }

    #[test]
    fn run_to_is_one_shot_and_precedes_persistent_breakpoint() {
        let mut control = DebugExecutionControl::default();
        control.set_breakpoint(0x2000, true);
        control.set_run_to(0x2000);
        assert_eq!(control.stop_before(0x2000), Some(DebugStopReason::RunTo(0x2000)));
        assert_eq!(control.run_to(), None);
        control.prepare_resume(0x2000);
        assert_eq!(control.stop_before(0x2000), Some(DebugStopReason::ExecuteBreakpoint(0x2000)));
    }

    #[test]
    fn read_write_watchpoints_match_data_and_stack_effects() {
        let mut control = DebugExecutionControl::default();
        control.set_watchpoint(0x3456, Some(MemoryWatchAccess::ReadWrite));
        assert!(control.active());
        assert_eq!(control.watchpoints(), vec![(0x3456, MemoryWatchAccess::ReadWrite)]);

        let read = [InstructionEffect8080::MemoryRead { address: 0x3456, value: 0xaa }];
        assert_eq!(
            control.stop_after_effects(0x0100, &read),
            Some(DebugStopReason::MemoryReadWatchpoint {
                instruction_pc: 0x0100,
                address: 0x3456,
                value: 0xaa,
            })
        );

        control.prepare_resume(0x0101);
        let write = [InstructionEffect8080::StackWrite { address: 0x3456, value: 0x55 }];
        assert_eq!(
            control.stop_after_effects(0x0200, &write),
            Some(DebugStopReason::MemoryWriteWatchpoint {
                instruction_pc: 0x0200,
                address: 0x3456,
                value: 0x55,
            })
        );
    }

    #[test]
    fn access_direction_is_respected() {
        let mut control = DebugExecutionControl::default();
        control.set_watchpoint(0x2222, Some(MemoryWatchAccess::Write));
        let read = [InstructionEffect8080::MemoryRead { address: 0x2222, value: 1 }];
        assert_eq!(control.stop_after_effects(0x1000, &read), None);
        let write = [InstructionEffect8080::MemoryWrite { address: 0x2222, value: 2 }];
        assert!(matches!(
            control.stop_after_effects(0x1001, &write),
            Some(DebugStopReason::MemoryWriteWatchpoint { .. })
        ));
    }
}
