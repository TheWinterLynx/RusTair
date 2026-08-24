use std::ops::{Deref, DerefMut};
use std::time::Duration;

use crate::config::SerialBoard;
use crate::machine::AltairMachine;

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, CpuState,
    EmulationEngine, FrontPanelState, Intel8080State, MachineBackend,
};

pub struct NativeMachineBackend {
    machine: AltairMachine,
}

impl Default for NativeMachineBackend {
    fn default() -> Self { Self { machine: AltairMachine::default() } }
}

impl NativeMachineBackend {
    pub fn new(machine: AltairMachine) -> Self { Self { machine } }
    pub fn machine(&self) -> &AltairMachine { &self.machine }
    pub fn machine_mut(&mut self) -> &mut AltairMachine { &mut self.machine }
    pub fn into_machine(self) -> AltairMachine { self.machine }

    fn snapshot_cpu(&self) -> CpuState {
        let cpu = &self.machine.cpu;
        CpuState::Intel8080(Intel8080State {
            a: cpu.a,
            b: cpu.b,
            c: cpu.c,
            d: cpu.d,
            e: cpu.e,
            h: cpu.h,
            l: cpu.l,
            flags: cpu.f,
            pc: cpu.pc,
            sp: cpu.sp,
            inte: cpu.inte,
            halted: Some(cpu.halted),
            total_t_states: Some(cpu.cycles),
        })
    }

    fn snapshot_panel(&self) -> FrontPanelState {
        FrontPanelState {
            powered: self.machine.powered,
            running: self.machine.running,
            switches: self.machine.panel_switches(),
            address: self.machine.address_leds(),
            data: self.machine.data_leds(),
            lamps: self.machine.panel_lamps(),
            current_board_protected: self.machine.current_board_protected(),
            ext_clear_asserted: self.machine.ext_clear_asserted(),
        }
    }
}

impl Deref for NativeMachineBackend {
    type Target = AltairMachine;
    fn deref(&self) -> &Self::Target { &self.machine }
}

impl DerefMut for NativeMachineBackend {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.machine }
}

impl MachineBackend for NativeMachineBackend {
    fn engine(&self) -> EmulationEngine { EmulationEngine::RustFast8080 }
    fn name(&self) -> &'static str { "RusTair fast 8080" }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            front_panel: true,
            exact_bus_activity: false,
            exact_t_state_timing: false,
            memory_protection: true,
            hold_hlda: true,
            direct_memory_access: true,
            serial_routing: true,
            disk_mount: false,
        }
    }

    fn execution_model(&self) -> BackendExecutionModel { BackendExecutionModel::HostDriven }
    fn cpu_state(&mut self) -> BackendResult<CpuState> { Ok(self.snapshot_cpu()) }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { Ok(self.snapshot_panel()) }

    fn power(&mut self, on: bool) -> BackendResult<()> { self.machine.power(on); Ok(()) }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.machine.power_with_historical_run_latch(on, historical); Ok(())
    }
    fn run(&mut self) -> BackendResult<()> { self.machine.set_running(true); Ok(()) }
    fn halt(&mut self) -> BackendResult<()> { self.machine.set_running(false); Ok(()) }
    fn step(&mut self) -> BackendResult<()> { self.machine.step(); Ok(()) }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if self.machine.running { self.machine.run_cycles(t_state_budget); }
        Ok(())
    }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> {
        self.machine.commit_panel_activity(dt); Ok(())
    }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { self.machine.assert_run_stop(run); Ok(()) }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.machine.release_run_stop(run); Ok(()) }
    fn assert_reset(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_reset(); Ok(()) }
    fn release_reset(&mut self) -> BackendResult<()> { self.machine.release_front_panel_reset(); Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_clear(); Ok(()) }
    fn release_clear(&mut self) -> BackendResult<()> { self.machine.release_front_panel_clear(); Ok(()) }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.machine.request_hold(hold); Ok(()) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> { self.machine.examine(next); Ok(()) }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> { self.machine.deposit(next); Ok(()) }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> {
        self.machine.protect_current_board(protected); Ok(())
    }
    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.machine.panel_switches()) }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        let changed = self.machine.panel_switches() ^ value;
        for bit in 0..16 {
            if changed & (1u16 << bit) != 0 { self.machine.toggle_sense_switch(bit); }
        }
        Ok(())
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        self.machine.configure_serial_board(board); Ok(())
    }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(self.machine.serial_board()) }
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_receive(byte),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_receive(byte),
        }
        Ok(())
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_rx_empty(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_empty(),
        })
    }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_rx_len(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_len(),
        })
    }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.tx_busy(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_busy(),
        })
    }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_front(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_front(),
        })
    }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> {
        Ok(match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_complete(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_complete(),
        })
    }
    fn clear_serial(&mut self) -> BackendResult<()> { self.machine.bus.clear_serial(); Ok(()) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> {
        Ok(self.machine.bus.peek_memory(address))
    }
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> BackendResult<bool> {
        Ok(self.machine.bus.debugger_write_memory(address, value, respect_protection))
    }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.machine.bus.load(address, bytes); Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_snapshot_is_explicitly_8080() {
        let mut backend = NativeMachineBackend::default();
        backend.machine_mut().cpu.a = 0x12;
        backend.machine_mut().cpu.b = 0x34;
        backend.machine_mut().cpu.c = 0x56;
        backend.machine_mut().cpu.pc = 0x789a;
        backend.machine_mut().cpu.cycles = 1234;
        let CpuState::Intel8080(state) = backend.cpu_state().unwrap() else { panic!("expected 8080") };
        assert_eq!(state.a, 0x12);
        assert_eq!(state.bc(), 0x3456);
        assert_eq!(state.pc, 0x789a);
        assert_eq!(state.total_t_states, Some(1234));
    }

    #[test]
    fn fast_backend_is_host_driven() {
        let backend = NativeMachineBackend::default();
        assert_eq!(backend.execution_model(), BackendExecutionModel::HostDriven);
        assert!(!backend.capabilities().exact_t_state_timing);
    }

    #[test]
    fn debugger_memory_access_round_trips() {
        let mut backend = NativeMachineBackend::default();
        assert!(backend.write_memory(0x0010, 0x5a, false).unwrap());
        assert_eq!(backend.peek_memory(0x0010).unwrap(), Some(0x5a));
    }
}
