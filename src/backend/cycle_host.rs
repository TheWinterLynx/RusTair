use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::AltairMachine;

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort,
    CpuState, CycleAccurateMachineBackend, EmulationEngine, FrontPanelState,
    MachineBackend,
};

/// Transitional application-host wrapper around the frozen cycle-accurate
/// backend. It does not alter CPU execution; it only exposes the shared Rust
/// Altair/S-100 chassis required while legacy UI readers are migrated behind
/// `MachineBackend`.
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
    fn rust_machine(&self) -> Option<&AltairMachine> { Some(self.inner.machine()) }
    fn rust_machine_mut(&mut self) -> Option<&mut AltairMachine> { Some(self.inner.machine_mut()) }

    fn cpu_state(&mut self) -> BackendResult<CpuState> { self.inner.cpu_state() }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { self.inner.front_panel_state() }

    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {
        let powered = self.inner.machine().powered;
        let serial_board = self.inner.machine().serial_board();

        if powered {
            // The shared chassis performs the same memory-board reconfiguration
            // as the fast backend and drops the RUN latch. Pulse the real cycle
            // core RESET afterwards so its internal state follows the chassis.
            self.inner.machine_mut().configure_memory(size, init);
            self.inner.assert_reset()?;
            self.inner.release_reset()?;
        } else {
            // With power off there is no physical RESET line to assert. Recreate
            // the cycle core in its reset state while preserving the installed
            // serial-board choice, then configure the new RAM board set.
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
            // `AltairMachine` resets its legacy mirror when a card is changed.
            // Reset the real T-state core through its backend path as well.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_exposes_cycle_chassis_without_bypassing_cycle_step() {
        let mut backend = CycleHostBackend::default();
        backend.configure_memory(RamSize::K1, RamInit::Zeroed).unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00]).unwrap();
        backend.step().unwrap();

        assert_eq!(backend.inner.machine().cpu.pc, 1);
        assert_eq!(backend.inner.machine().cpu.cycles, 4);
        assert!(backend.inner.machine().wait_led());
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
        assert!(!backend.inner.machine().running);
        assert_eq!(backend.inner.machine().serial_board(), SerialBoard::TwoSio88);
    }
}