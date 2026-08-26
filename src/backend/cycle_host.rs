use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::CpuDiagnosticResult;

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, CycleAccurateMachineBackend, EmulationEngine, FrontPanelState,
    IoPortActivity, IoTraceSnapshot, MachineBackend,
};

/// Application-host wrapper around the validated cycle-accurate backend.
///
/// The shared Altair/S-100 chassis remains an implementation detail of this
/// backend. Application/UI code talks only to `MachineBackend`/`BackendHost`.
pub(super) struct CycleHostBackend {
    inner: CycleAccurateMachineBackend,
}

impl Default for CycleHostBackend {
    fn default() -> Self {
        Self { inner: CycleAccurateMachineBackend::default() }
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
        Ok(())
    }

    fn power(&mut self, on: bool) -> BackendResult<()> { self.inner.power(on) }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.inner.power_with_historical_run_latch(on, historical)
    }
    fn run(&mut self) -> BackendResult<()> { self.inner.run() }
    fn halt(&mut self) -> BackendResult<()> { self.inner.halt() }
    fn step(&mut self) -> BackendResult<()> { self.inner.step() }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        self.inner.service_execution(t_state_budget)
    }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.inner.commit_panel_activity(dt)
    }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { self.inner.assert_run_stop(run) }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.inner.release_run_stop(run) }
    fn assert_reset(&mut self) -> BackendResult<()> { self.inner.assert_reset() }
    fn release_reset(&mut self) -> BackendResult<()> { self.inner.release_reset() }
    fn assert_clear(&mut self) -> BackendResult<()> { self.inner.assert_clear() }
    fn release_clear(&mut self) -> BackendResult<()> { self.inner.release_clear() }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.inner.request_hold(hold) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> { self.inner.panel_examine(next) }
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
}