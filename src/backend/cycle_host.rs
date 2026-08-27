use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::cpu8080_cycle::{MachineCycle, TState};
use crate::machine::CpuDiagnosticResult;
use crate::trace8080::{
    collect_post_instruction_effects, collect_pre_instruction_effects, CpuSnapshot8080,
    InstructionEffect8080, InstructionTraceBuffer,
};

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, CycleAccurateMachineBackend, EmulationEngine, FrontPanelState,
    InstructionTraceSnapshot, IoPortActivity, IoTraceSnapshot, MachineBackend,
};

#[derive(Clone, Debug)]
struct PendingInstructionTrace {
    address: u16,
    bytes: [u8; 3],
    before: CpuSnapshot8080,
    start_t_states: u64,
    effects: Vec<InstructionEffect8080>,
}

/// Application-host wrapper around the validated cycle-accurate backend.
///
/// The shared Altair/S-100 chassis remains an implementation detail of this
/// backend. Application/UI code talks only to `MachineBackend`/`BackendHost`.
pub(super) struct CycleHostBackend {
    inner: CycleAccurateMachineBackend,
    instruction_trace: InstructionTraceBuffer,
    pending_instruction_trace: Option<PendingInstructionTrace>,
}

impl Default for CycleHostBackend {
    fn default() -> Self {
        Self {
            inner: CycleAccurateMachineBackend::default(),
            instruction_trace: InstructionTraceBuffer::default(),
            pending_instruction_trace: None,
        }
    }
}

impl CycleHostBackend {
    fn trace_cpu_snapshot(&self) -> CpuSnapshot8080 {
        let r = self.inner.cpu().registers();
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
            inte: self.inner.cpu().interrupts_enabled(),
            halted: self.inner.cpu().is_halted(),
        }
    }

    fn trace_bytes(&self, address: u16) -> [u8; 3] {
        [
            self.inner.machine().bus.peek_memory(address).unwrap_or(0),
            self.inner.machine().bus.peek_memory(address.wrapping_add(1)).unwrap_or(0),
            self.inner.machine().bus.peek_memory(address.wrapping_add(2)).unwrap_or(0),
        ]
    }

    fn begin_instruction_trace_if_needed(&mut self) {
        if !self.instruction_trace.enabled()
            || self.pending_instruction_trace.is_some()
            || self.inner.cpu().is_halted()
            || self.inner.cpu().machine_cycle() != MachineCycle::InstructionFetch
            || self.inner.cpu().t_state() != TState::T1
        {
            return;
        }

        let before = self.trace_cpu_snapshot();
        let bytes = self.trace_bytes(before.pc);
        let effects = collect_pre_instruction_effects(bytes, before, |address| {
            self.inner.machine().bus.peek_memory(address)
        });
        self.pending_instruction_trace = Some(PendingInstructionTrace {
            address: before.pc,
            bytes,
            before,
            start_t_states: self.inner.cpu().total_t_states(),
            effects,
        });
    }

    fn finish_instruction_trace_if_complete(&mut self) {
        let Some(pending) = self.pending_instruction_trace.as_ref() else { return; };
        let at_next_fetch = self.inner.cpu().machine_cycle() == MachineCycle::InstructionFetch
            && self.inner.cpu().t_state() == TState::T1
            && self.inner.cpu().total_t_states() > pending.start_t_states;
        if !at_next_fetch && !self.inner.cpu().is_halted() {
            return;
        }

        let mut pending = self.pending_instruction_trace.take().expect("pending trace exists");
        let after = self.trace_cpu_snapshot();
        pending.effects.extend(collect_post_instruction_effects(
            pending.bytes,
            pending.before,
            after,
            |address| self.inner.machine().bus.peek_memory(address),
        ));
        let delta = self.inner.cpu().total_t_states().saturating_sub(pending.start_t_states) as u32;
        if delta != 0 {
            self.instruction_trace.push_with_effects(
                pending.address,
                pending.bytes,
                pending.before,
                after,
                delta,
                pending.effects,
            );
        }
    }

    fn clear_pending_instruction_trace(&mut self) {
        self.pending_instruction_trace = None;
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
        self.clear_pending_instruction_trace();
        self.instruction_trace.clear();
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> {
        self.power_with_historical_run_latch(on, false)
    }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.inner.power_with_historical_run_latch(on, historical)?;
        self.clear_pending_instruction_trace();
        self.instruction_trace.clear();
        Ok(())
    }
    fn run(&mut self) -> BackendResult<()> { self.inner.run() }
    fn halt(&mut self) -> BackendResult<()> { self.inner.halt() }
    fn step(&mut self) -> BackendResult<()> {
        self.begin_instruction_trace_if_needed();
        self.inner.step()?;
        self.finish_instruction_trace_if_complete();
        Ok(())
    }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if !self.instruction_trace.enabled() {
            return self.inner.service_execution(t_state_budget);
        }

        // The cycle core already advances one real T-state per unit of budget.
        // Keeping this wrapper around each tick lets us snapshot exact guest
        // instruction boundaries while preserving the inner pin-level model.
        for _ in 0..t_state_budget {
            if !self.inner.machine().running {
                break;
            }
            self.begin_instruction_trace_if_needed();
            self.inner.service_execution(1)?;
            self.finish_instruction_trace_if_complete();
        }
        Ok(())
    }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.inner.commit_panel_activity(dt)
    }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { self.inner.assert_run_stop(run) }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.inner.release_run_stop(run) }
    fn assert_reset(&mut self) -> BackendResult<()> {
        self.clear_pending_instruction_trace();
        self.inner.assert_reset()
    }
    fn release_reset(&mut self) -> BackendResult<()> { self.inner.release_reset() }
    fn assert_clear(&mut self) -> BackendResult<()> { self.inner.assert_clear() }
    fn release_clear(&mut self) -> BackendResult<()> { self.inner.release_clear() }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.inner.request_hold(hold) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.clear_pending_instruction_trace();
        self.inner.panel_examine(next)
    }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> { self.inner.panel_deposit(next) }
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
        self.inner.machine_mut().configure_serial_board(board);
        if powered {
            self.clear_pending_instruction_trace();
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
        self.inner.write_memory(address, value, respect_protection)
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.inner.load_bytes(address, bytes)
    }
    fn memory_is_protected(&mut self, address: u16) -> BackendResult<bool> {
        Ok(self.inner.machine().bus.is_protected(address))
    }
    fn clear_memory_protection(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.clear_protection();
        Ok(())
    }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> {
        self.inner.machine_mut().bus.clear_transient_memory_guards();
        Ok(())
    }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> {
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
    fn set_instruction_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> {
        self.instruction_trace.set_enabled(enabled);
        if !enabled {
            self.clear_pending_instruction_trace();
        }
        Ok(())
    }
    fn clear_instruction_trace(&mut self) -> BackendResult<()> {
        self.instruction_trace.clear();
        self.clear_pending_instruction_trace();
        Ok(())
    }
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
        backend.load_bytes(0, &[0x3e, 0x42, 0x3c, 0x76]).unwrap(); // MVI A,42 / INR A / HLT

        // RESET guarantees the execution entry point, but the 8080 general
        // registers are not a contractual zero-filled debugger state. Capture
        // the actual pre-execution CPU state and require the trace to preserve it.
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
}
