use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::cpu8080_cycle::{MachineCycle, TState};
use crate::debugger_control::DebugExecutionControl;
use crate::machine::CpuDiagnosticResult;
use crate::trace8080::{
    collect_post_instruction_effects, collect_pre_instruction_effects, CpuSnapshot8080,
    InstructionEffect8080, InstructionTraceBuffer, InstructionTraceMetadata,
};

use super::cycle::CycleExecutionEvent;
use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, CycleAccurateMachineBackend, DebugStopReason, EmulationEngine, FrontPanelState,
    InstructionTraceSnapshot, IoPortActivity, IoTraceSnapshot, MachineBackend, MemoryWatchAccess,
};

#[derive(Clone, Debug)]
struct PendingInstructionTrace {
    address: u16,
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    start_t_states: u64,
    effects: Vec<InstructionEffect8080>,
}

pub(super) struct CycleHostBackend {
    inner: CycleAccurateMachineBackend,
    instruction_trace: InstructionTraceBuffer,
    pending_instruction_trace: Option<PendingInstructionTrace>,
    debug_control: DebugExecutionControl,
}

impl Default for CycleHostBackend {
    fn default() -> Self {
        Self {
            inner: CycleAccurateMachineBackend::default(),
            instruction_trace: InstructionTraceBuffer::default(),
            pending_instruction_trace: None,
            debug_control: DebugExecutionControl::default(),
        }
    }
}

impl CycleHostBackend {
    fn trace_cpu_snapshot_from(inner: &CycleAccurateMachineBackend) -> CpuSnapshot8080 {
        let r = inner.cpu().registers();
        CpuSnapshot8080 {
            a: r.a,
            b: r.b,
            c: r.c,
            d: r.d,
            e: r.e,
            h: r.h,
            l: r.l,
            flags: r.f,
            pc: r.pc,
            sp: r.sp,
            inte: inner.cpu().interrupts_enabled(),
            halted: inner.cpu().is_halted(),
        }
    }

    fn trace_bytes_from(inner: &CycleAccurateMachineBackend, address: u16) -> [u8; 3] {
        [
            inner.machine().bus.preview_guest_memory(address),
            inner
                .machine()
                .bus
                .preview_guest_memory(address.wrapping_add(1)),
            inner
                .machine()
                .bus
                .preview_guest_memory(address.wrapping_add(2)),
        ]
    }

    fn at_instruction_boundary(&self) -> bool {
        !self.inner.cpu().is_halted()
            && !self.inner.cpu().is_holding()
            && self.inner.cpu().machine_cycle() == MachineCycle::InstructionFetch
            && self.inner.cpu().t_state() == TState::T1
    }

    fn observing_instruction_effects(&self) -> bool {
        self.instruction_trace.enabled() || self.debug_control.has_watchpoints()
    }

    fn begin_pending_trace(
        inner: &CycleAccurateMachineBackend,
        pending: &mut Option<PendingInstructionTrace>,
    ) {
        if pending.is_some() {
            return;
        }
        let before = Self::trace_cpu_snapshot_from(inner);
        let bytes = Self::trace_bytes_from(inner, before.pc);
        let effects = collect_pre_instruction_effects(bytes, before, |address| {
            inner.machine().bus.preview_guest_memory(address)
        });
        *pending = Some(PendingInstructionTrace {
            address: before.pc,
            bytes,
            before,
            start_t_states: inner.cpu().total_t_states(),
            effects,
        });
    }

    fn finalize_pending_trace(
        inner: &CycleAccurateMachineBackend,
        pending: &mut Option<PendingInstructionTrace>,
        instruction_trace: &mut InstructionTraceBuffer,
        debug_control: &mut DebugExecutionControl,
    ) -> bool {
        let Some(mut pending_trace) = pending.take() else {
            return false;
        };
        let after = Self::trace_cpu_snapshot_from(inner);
        let post = collect_post_instruction_effects(
            pending_trace.bytes,
            pending_trace.before,
            after,
            &pending_trace.effects,
        );
        pending_trace.effects.extend(post);
        let delta = inner
            .cpu()
            .total_t_states()
            .saturating_sub(pending_trace.start_t_states) as u32;
        if delta == 0 {
            return false;
        }

        let watch_stop = debug_control
            .stop_after_effects(pending_trace.address, &pending_trace.effects)
            .is_some();
        if instruction_trace.enabled() {
            instruction_trace.push_with_effects(
                pending_trace.address,
                pending_trace.bytes,
                pending_trace.before,
                after,
                delta,
                pending_trace.effects,
            );
        }
        watch_stop
    }

    fn begin_instruction_trace_if_needed(&mut self) {
        if !self.observing_instruction_effects() || !self.at_instruction_boundary() {
            return;
        }
        Self::begin_pending_trace(&self.inner, &mut self.pending_instruction_trace);
    }

    fn finish_instruction_trace_if_complete(&mut self) -> bool {
        let Some(pending) = self.pending_instruction_trace.as_ref() else {
            return false;
        };
        let at_next_fetch = self.at_instruction_boundary()
            && self.inner.cpu().total_t_states() > pending.start_t_states;
        if !at_next_fetch && !self.inner.cpu().is_halted() {
            return false;
        }

        Self::finalize_pending_trace(
            &self.inner,
            &mut self.pending_instruction_trace,
            &mut self.instruction_trace,
            &mut self.debug_control,
        )
    }

    fn clear_pending_instruction_trace(&mut self) {
        self.pending_instruction_trace = None;
    }

    /// A trace generation represents one continuous execution context. RESET,
    /// front-panel PC injection, serial-board reset and bulk program replacement
    /// break that continuity. Clearing here prevents Call Stack / Loop Inspector
    /// from treating pre-discontinuity observations as live state afterwards.
    fn reset_debugger_epoch(&mut self) {
        self.clear_pending_instruction_trace();
        self.instruction_trace.clear();
        self.debug_control.clear_transient();
    }

    /// Cycle Accurate can be stopped between machine cycles. If debugger RAM is
    /// patched after an instruction has already been snapshotted, its operands
    /// and predicted data reads may no longer describe what the CPU will really
    /// consume. Drop that partial observation rather than publish a false trace.
    fn invalidate_partial_trace_for_external_memory_change(&mut self) {
        if self.pending_instruction_trace.is_some() {
            self.reset_debugger_epoch();
        }
    }

    fn debugger_step_one_instruction(&mut self) -> BackendResult<()> {
        let lines = self.inner.machine().bus.cpu_control_lines();
        if !self.inner.machine().powered
            || self.inner.machine().running
            || lines.reset
            || lines.hold
            || self.inner.cpu().is_halted()
            || self.inner.cpu().is_holding()
        {
            return Ok(());
        }

        self.debug_control.prepare_manual_step();
        let start_t_states = self.inner.cpu().total_t_states();
        for _ in 0..16 {
            self.begin_instruction_trace_if_needed();
            self.inner.step()?;
            let watch_stop = self.finish_instruction_trace_if_complete();

            if watch_stop || self.inner.cpu().is_halted() || self.inner.cpu().is_holding() {
                break;
            }
            if self.inner.cpu().total_t_states() > start_t_states && self.at_instruction_boundary() {
                break;
            }
        }
        Ok(())
    }
}

impl MachineBackend for CycleHostBackend {
    fn engine(&self) -> EmulationEngine { self.inner.engine() }
    fn name(&self) -> &'static str { self.inner.name() }
    fn capabilities(&self) -> BackendCapabilities { self.inner.capabilities() }
    fn execution_model(&self) -> BackendExecutionModel { self.inner.execution_model() }

    fn cpu_state(&mut self) -> BackendResult<CpuState> { self.inner.cpu_state() }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { self.inner.front_panel_state() }

    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {
        let powered = self.inner.machine().powered;
        let serial_board = self.inner.machine().serial_board();

        if powered {
            self.inner.machine_mut().configure_memory(size, init);
            self.inner.assert_reset()?;
            self.inner.release_reset()?;
        } else {
            let mut replacement = CycleAccurateMachineBackend::default();
            replacement.machine_mut().configure_memory(size, init);
            replacement.machine_mut().configure_serial_board(serial_board);
            self.inner = replacement;
        }
        self.reset_debugger_epoch();
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        self.power_with_historical_run_latch(on, false)
    }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.inner.power_with_historical_run_latch(on, historical)?;
        self.reset_debugger_epoch();
        Ok(())
    }
    fn run(&mut self) -> BackendResult<()> {
        let r = self.inner.cpu().registers();
        self.debug_control.prepare_resume_with_sp(r.pc, r.sp);
        self.inner.run()
    }
    fn halt(&mut self) -> BackendResult<()> {
        self.debug_control.cancel_run_to();
        self.inner.halt()
    }
    fn step(&mut self) -> BackendResult<()> {
        self.debug_control.prepare_manual_step();
        self.begin_instruction_trace_if_needed();
        self.inner.step()?;
        self.finish_instruction_trace_if_complete();
        Ok(())
    }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if !self.instruction_trace.enabled() && !self.debug_control.active() {
            return self.inner.service_execution(t_state_budget);
        }

        let observing_effects = self.observing_instruction_effects();
        let pending = &mut self.pending_instruction_trace;
        let instruction_trace = &mut self.instruction_trace;
        let debug_control = &mut self.debug_control;

        self.inner
            .service_execution_with_observer(t_state_budget, |inner, event| match event {
                CycleExecutionEvent::BeforeInstruction => {
                    let r = inner.cpu().registers();
                    if debug_control.stop_before_with_sp(r.pc, r.sp).is_some() {
                        return true;
                    }
                    if observing_effects {
                        Self::begin_pending_trace(inner, pending);
                    }
                    false
                }
                CycleExecutionEvent::InstructionComplete => Self::finalize_pending_trace(
                    inner,
                    pending,
                    instruction_trace,
                    debug_control,
                ),
            })
    }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.inner.commit_panel_activity(dt)
    }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> {
        if run {
            let r = self.inner.cpu().registers();
            self.debug_control.prepare_resume_with_sp(r.pc, r.sp);
        } else {
            self.debug_control.cancel_run_to();
        }
        self.inner.assert_run_stop(run)
    }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.inner.release_run_stop(run) }
    fn assert_reset(&mut self) -> BackendResult<()> {
        self.reset_debugger_epoch();
        self.inner.assert_reset()
    }
    fn release_reset(&mut self) -> BackendResult<()> { self.inner.release_reset() }
    fn assert_clear(&mut self) -> BackendResult<()> { self.inner.assert_clear() }
    fn release_clear(&mut self) -> BackendResult<()> { self.inner.release_clear() }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.inner.request_hold(hold) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.reset_debugger_epoch();
        self.inner.panel_examine(next)
    }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        if next {
            self.reset_debugger_epoch();
        } else {
            self.invalidate_partial_trace_for_external_memory_change();
        }
        self.inner.panel_deposit(next)
    }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> {
        self.inner.protect_current_board(protected)
    }
    fn switch_register(&mut self) -> BackendResult<u16> { self.inner.switch_register() }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        self.inner.set_switch_register(value)
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        if self.inner.machine().serial_board() == board {
            return Ok(());
        }
        let powered = self.inner.machine().powered;
        self.reset_debugger_epoch();
        self.inner.machine_mut().configure_serial_board(board);
        if powered {
            self.inner.assert_reset()?;
            self.inner.release_reset()?;
        }
        Ok(())
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { self.inner.serial_board() }
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        self.inner.serial_receive(port, byte)
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        self.inner.serial_rx_empty(port)
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        self.inner.serial_rx_len(port)
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        self.inner.serial_tx_busy(port)
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        self.inner.serial_tx_front(port)
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        self.inner.serial_tx_complete(port)
    }
    fn clear_serial(&mut self) -> BackendResult<()> { self.inner.clear_serial() }
    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(self.inner.machine().installed_ram_bytes()) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        self.inner.peek_memory(address)
    }
    fn write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> BackendResult<bool> {
        self.invalidate_partial_trace_for_external_memory_change();
        self.inner.write_memory(address, value, respect_protection)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.inner.load_bytes(address, bytes)?;
        self.reset_debugger_epoch();
        Ok(())
    }
    fn memory_is_protected(&mut self, address: u16) -> BackendResult<bool> {
        Ok(self.inner.machine().bus.is_protected(address))
    }
    fn clear_memory_protection(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.clear_protection();
        Ok(())
    }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> {
        self.invalidate_partial_trace_for_external_memory_change();
        self.inner.machine_mut().bus.clear_transient_memory_guards();
        Ok(())
    }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> {
        self.invalidate_partial_trace_for_external_memory_change();
        Ok(self.inner.machine_mut().arm_basic32_full_memory_probe_guard())
    }
    fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) -> BackendResult<()> {
        self.inner.machine_mut().begin_cpu_diagnostic_meter(
            name,
            bdos_start,
            bdos_len,
            expected_instructions,
            expected_t_states,
        );
        Ok(())
    }
    fn cancel_cpu_diagnostic_meter(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.cancel_cpu_diagnostic_meter();
        Ok(())
    }
    fn take_cpu_diagnostic_result(&mut self) -> BackendResult<Option<CpuDiagnosticResult>> {
        Ok(self.inner.machine_mut().take_cpu_diagnostic_result())
    }
    fn peek_io_port(&mut self, port: u8) -> BackendResult<u8> {
        Ok(self.inner.machine().bus.peek_io_port(port))
    }
    fn io_port_activity(&mut self, port: u8) -> BackendResult<IoPortActivity> {
        Ok(self.inner.machine().bus.io_port_activity(port))
    }
    fn io_trace_snapshot(&mut self) -> BackendResult<IoTraceSnapshot> {
        Ok(self.inner.machine().bus.io_trace_snapshot())
    }
    fn io_trace_enabled(&mut self) -> BackendResult<bool> {
        Ok(self.inner.machine().bus.io_trace_enabled())
    }
    fn set_io_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.inner.machine_mut().bus.set_io_trace_enabled(enabled);
        Ok(())
    }
    fn clear_io_trace(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.clear_io_trace();
        Ok(())
    }
    fn instruction_trace_snapshot(&mut self) -> BackendResult<InstructionTraceSnapshot> {
        Ok(self.instruction_trace.snapshot())
    }
    fn instruction_trace_enabled(&mut self) -> BackendResult<bool> {
        Ok(self.instruction_trace.enabled())
    }
    fn instruction_trace_metadata(&mut self) -> BackendResult<InstructionTraceMetadata> {
        Ok(self.instruction_trace.metadata())
    }
    fn set_instruction_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.instruction_trace.set_enabled(enabled);
        if !enabled && !self.debug_control.has_watchpoints() {
            self.clear_pending_instruction_trace();
        }
        Ok(())
    }
    fn clear_instruction_trace(&mut self) -> BackendResult<()> {
        self.instruction_trace.clear();
        if !self.debug_control.has_watchpoints() {
            self.clear_pending_instruction_trace();
        }
        Ok(())
    }

    fn debugger_step_instruction(&mut self) -> BackendResult<()> {
        self.debugger_step_one_instruction()
    }
    fn debugger_breakpoints(&mut self) -> BackendResult<Vec<u16>> { Ok(self.debug_control.breakpoints()) }
    fn debugger_set_breakpoint(&mut self, address: u16, enabled: bool) -> BackendResult<()> {
        self.debug_control.set_breakpoint(address, enabled);
        Ok(())
    }
    fn debugger_clear_breakpoints(&mut self) -> BackendResult<()> {
        self.debug_control.clear_breakpoints();
        Ok(())
    }
    fn debugger_watchpoints(&mut self) -> BackendResult<Vec<(u16, MemoryWatchAccess)>> {
        Ok(self.debug_control.watchpoints())
    }
    fn debugger_set_watchpoint(
        &mut self,
        address: u16,
        access: Option<MemoryWatchAccess>,
    ) -> BackendResult<()> {
        self.debug_control.set_watchpoint(address, access);
        if access.is_none() && !self.observing_instruction_effects() {
            self.clear_pending_instruction_trace();
        }
        Ok(())
    }
    fn debugger_clear_watchpoints(&mut self) -> BackendResult<()> {
        self.debug_control.clear_watchpoints();
        if !self.instruction_trace.enabled() {
            self.clear_pending_instruction_trace();
        }
        Ok(())
    }
    fn debugger_run_to(&mut self, address: u16) -> BackendResult<()> {
        self.debug_control.set_run_to(address);
        let r = self.inner.cpu().registers();
        self.debug_control.prepare_resume_with_sp(r.pc, r.sp);
        self.inner.run()
    }
    fn debugger_run_to_with_sp(&mut self, address: u16, required_sp: u16) -> BackendResult<()> {
        self.debug_control.set_run_to_with_sp(address, required_sp);
        let r = self.inner.cpu().registers();
        self.debug_control.prepare_resume_with_sp(r.pc, r.sp);
        self.inner.run()
    }
    fn debugger_cancel_run_to(&mut self) -> BackendResult<()> {
        self.debug_control.cancel_run_to();
        Ok(())
    }
    fn debugger_run_to_target(&mut self) -> BackendResult<Option<u16>> { Ok(self.debug_control.run_to()) }
    fn debugger_stop_reason(&mut self) -> BackendResult<Option<DebugStopReason>> { Ok(self.debug_control.stop_reason()) }

    fn debugger_input_port(&mut self, port: u8) -> BackendResult<u8> {
        Ok(self.inner.machine_mut().bus.debugger_input_port(port))
    }
    fn debugger_output_port(&mut self, port: u8, value: u8) -> BackendResult<()> {
        self.inner.machine_mut().bus.debugger_output_port(port, value);
        Ok(())
    }
    fn debugger_inject_serial_rx(&mut self, port: u8, byte: u8) -> BackendResult<bool> {
        Ok(self.inner.machine_mut().bus.debugger_inject_serial_rx(port, byte))
    }
    fn debugger_clear_serial_rx(&mut self, port: u8) -> BackendResult<bool> {
        Ok(self.inner.machine_mut().bus.debugger_clear_serial_rx(port))
    }
    fn debugger_clear_serial_tx(&mut self, port: u8) -> BackendResult<bool> {
        Ok(self.inner.machine_mut().bus.debugger_clear_serial_tx(port))
    }
    fn debugger_complete_serial_tx(&mut self, port: u8) -> BackendResult<Option<u8>> {
        Ok(self.inner.machine_mut().bus.debugger_complete_serial_tx(port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_dispatches_cycle_step_without_exposing_chassis_to_app() {
        let mut backend = CycleHostBackend::default();
        backend.configure_memory(RamSize::K1, RamInit::Zeroed).unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();
        backend.step().unwrap();

        let CpuState::Intel8080(cpu) = backend.cpu_state().unwrap() else { unreachable!() };
        let panel = backend.front_panel_state().unwrap();
        assert_eq!(cpu.pc, 1);
        assert_eq!(cpu.total_t_states, Some(4));
        assert!(panel.lamps.wait > 0.0);
    }

    #[test]
    fn powered_serial_board_change_resets_real_cycle_core() {
        let mut backend = CycleHostBackend::default();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();
        backend.run().unwrap();
        backend.service_execution(4).unwrap();
        let CpuState::Intel8080(before) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(before.pc, 1);

        backend.configure_serial_board(SerialBoard::TwoSio88).unwrap();
        let CpuState::Intel8080(after) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(after.pc, 0);
        assert!(!backend.front_panel_state().unwrap().running);
        assert_eq!(backend.serial_board().unwrap(), SerialBoard::TwoSio88);
    }

    #[test]
    fn cycle_history_records_complete_guest_instruction_boundaries() {
        let mut backend = CycleHostBackend::default();
        backend.configure_memory(RamSize::K1, RamInit::Zeroed).unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x42, 0x3c, 0x76]).unwrap();

        let CpuState::Intel8080(initial) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(initial.pc, 0x0000);

        backend.set_instruction_trace_enabled(true).unwrap();
        backend.run().unwrap();
        backend.service_execution(128).unwrap();

        let history = backend.instruction_trace_snapshot().unwrap();
        assert!(history.len() >= 3, "expected MVI, INR and HLT in history: {history:?}");
        assert_eq!(history[0].address, 0x0000);
        assert_eq!(history[0].bytes[0], 0x3e);
        assert_eq!(history[0].before.a, initial.a);
        assert_eq!(history[0].before.pc, initial.pc);
        assert_eq!(history[0].after.a, 0x42);
        assert_eq!(history[1].address, 0x0002);
        assert_eq!(history[1].after.a, 0x43);
        assert_eq!(history[2].bytes[0], 0x76);
        assert!(history[2].after.halted);
    }

    #[test]
    fn debugger_memory_patch_between_machine_cycles_drops_stale_partial_trace() {
        let mut backend = CycleHostBackend::default();
        backend.configure_memory(RamSize::K1, RamInit::Zeroed).unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x11, 0x00]).unwrap(); // MVI A,11h / NOP
        backend.set_instruction_trace_enabled(true).unwrap();

        backend.step().unwrap(); // physical SINGLE STEP: fetch M1 only
        let generation = backend.instruction_trace_metadata().unwrap().generation;
        assert!(backend.instruction_trace_snapshot().unwrap().is_empty());

        assert!(backend.write_memory(0x0001, 0x22, true).unwrap());
        assert_ne!(backend.instruction_trace_metadata().unwrap().generation, generation);
        backend.step().unwrap(); // operand M2 completes MVI, but stale partial trace is gone
        assert!(backend.instruction_trace_snapshot().unwrap().is_empty());

        backend.step().unwrap(); // next NOP is captured normally
        let history = backend.instruction_trace_snapshot().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].address, 0x0002);
        let CpuState::Intel8080(cpu) = backend.cpu_state().unwrap() else { unreachable!() };
        assert_eq!(cpu.a, 0x22);
    }

    #[test]
    fn front_panel_deposit_between_machine_cycles_drops_stale_partial_trace() {
        let mut backend = CycleHostBackend::default();
        backend.configure_memory(RamSize::K1, RamInit::Zeroed).unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x3e, 0x11, 0x00]).unwrap();
        backend.set_instruction_trace_enabled(true).unwrap();

        backend.step().unwrap(); // MVI fetch complete, operand cycle pending
        let generation = backend.instruction_trace_metadata().unwrap().generation;
        assert!(backend.pending_instruction_trace.is_some());

        backend.panel_deposit(false).unwrap();
        assert_ne!(backend.instruction_trace_metadata().unwrap().generation, generation);
        assert!(backend.pending_instruction_trace.is_none());
    }
}
