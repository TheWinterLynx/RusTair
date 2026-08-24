use std::ops::{Deref, DerefMut};
use std::time::Duration;

use crate::config::SerialBoard;
use crate::machine::AltairMachine;

use super::{
    BackendCapabilities, BackendSerialPort, CpuState, EmulationEngine, FrontPanelState,
    MachineBackend,
};

/// Adapter exposing the existing fast RusTair machine through [`MachineBackend`].
///
/// `machine()`/`machine_mut()` and the temporary `Deref` implementation are a
/// migration escape hatch. Existing serial, diagnostics and loader code can
/// keep working while UI-facing CPU/front-panel access is moved behind the
/// backend contract in small, reviewable steps.
pub struct NativeMachineBackend {
    machine: AltairMachine,
}

impl Default for NativeMachineBackend {
    fn default() -> Self {
        Self {
            machine: AltairMachine::default(),
        }
    }
}

impl NativeMachineBackend {
    pub fn new(machine: AltairMachine) -> Self { Self { machine } }

    pub fn machine(&self) -> &AltairMachine { &self.machine }

    pub fn machine_mut(&mut self) -> &mut AltairMachine { &mut self.machine }

    pub fn into_machine(self) -> AltairMachine { self.machine }

    fn snapshot_cpu(&self) -> CpuState {
        let cpu = &self.machine.cpu;
        CpuState {
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
            halted: cpu.halted,
            total_t_states: cpu.cycles,
        }
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

/// Transitional compatibility layer: `RusTairApp` can own a backend wrapper now
/// while legacy code still using `machine.cpu` / `machine.bus` keeps compiling.
/// This is intentionally removable once those call sites use `MachineBackend`.
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
            // The current core reports instruction-level S-100 activity and
            // synthesizes presentation persistence, rather than exposing each
            // physical Intel 8080 T-state/pin transition.
            exact_bus_activity: false,
            exact_t_state_timing: false,
            memory_protection: true,
            hold_hlda: true,
            direct_memory_access: true,
            serial_routing: true,
            disk_mount: false,
        }
    }

    fn cpu_state(&self) -> CpuState { self.snapshot_cpu() }

    fn front_panel_state(&self) -> FrontPanelState { self.snapshot_panel() }

    fn power(&mut self, on: bool) { self.machine.power(on); }

    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) {
        self.machine.power_with_historical_run_latch(on, historical);
    }

    fn run(&mut self) { self.machine.set_running(true); }

    fn halt(&mut self) { self.machine.set_running(false); }

    fn step(&mut self) { self.machine.step(); }

    fn run_t_states(&mut self, budget: u32) { self.machine.run_cycles(budget); }

    fn commit_panel_activity(&mut self, dt: Duration) {
        self.machine.commit_panel_activity(dt);
    }

    fn assert_run_stop(&mut self, run: bool) { self.machine.assert_run_stop(run); }

    fn release_run_stop(&mut self, run: bool) { self.machine.release_run_stop(run); }

    fn assert_reset(&mut self) { self.machine.assert_front_panel_reset(); }

    fn release_reset(&mut self) { self.machine.release_front_panel_reset(); }

    fn assert_clear(&mut self) { self.machine.assert_front_panel_clear(); }

    fn release_clear(&mut self) { self.machine.release_front_panel_clear(); }

    fn request_hold(&mut self, hold: bool) { self.machine.request_hold(hold); }

    fn panel_examine(&mut self, next: bool) { self.machine.examine(next); }

    fn panel_deposit(&mut self, next: bool) { self.machine.deposit(next); }

    fn protect_current_board(&mut self, protected: bool) {
        self.machine.protect_current_board(protected);
    }

    fn switch_register(&self) -> u16 { self.machine.panel_switches() }

    fn set_switch_register(&mut self, value: u16) {
        let changed = self.machine.panel_switches() ^ value;
        for bit in 0..16 {
            if changed & (1u16 << bit) != 0 {
                self.machine.toggle_sense_switch(bit);
            }
        }
    }

    fn configure_serial_board(&mut self, board: SerialBoard) {
        self.machine.configure_serial_board(board);
    }

    fn serial_board(&self) -> SerialBoard { self.machine.serial_board() }

    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_receive(byte),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_receive(byte),
        }
    }

    fn serial_rx_empty(&self, port: BackendSerialPort) -> bool {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_rx_empty(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_empty(),
        }
    }

    fn serial_rx_len(&self, port: BackendSerialPort) -> usize {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_rx_len(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_len(),
        }
    }

    fn serial_tx_busy(&self, port: BackendSerialPort) -> bool {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.tx_busy(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_busy(),
        }
    }

    fn serial_tx_front(&self, port: BackendSerialPort) -> Option<u8> {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_front(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_front(),
        }
    }

    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> Option<u8> {
        match port {
            BackendSerialPort::Port0 => self.machine.bus.serial_tx_complete(),
            BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_complete(),
        }
    }

    fn clear_serial(&mut self) { self.machine.bus.clear_serial(); }

    fn peek_memory(&self, address: u16) -> Option<u8> {
        self.machine.bus.peek_memory(address)
    }

    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> bool {
        self.machine
            .bus
            .debugger_write_memory(address, value, respect_protection)
    }

    fn load_bytes(&mut self, address: u16, bytes: &[u8]) {
        self.machine.bus.load(address, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_snapshot_is_backend_neutral() {
        let mut backend = NativeMachineBackend::default();
        backend.machine_mut().cpu.a = 0x12;
        backend.machine_mut().cpu.b = 0x34;
        backend.machine_mut().cpu.c = 0x56;
        backend.machine_mut().cpu.pc = 0x789a;
        backend.machine_mut().cpu.sp = 0xbcde;
        backend.machine_mut().cpu.cycles = 1234;

        let state = backend.cpu_state();
        assert_eq!(state.a, 0x12);
        assert_eq!(state.bc(), 0x3456);
        assert_eq!(state.pc, 0x789a);
        assert_eq!(state.sp, 0xbcde);
        assert_eq!(state.total_t_states, 1234);
    }

    #[test]
    fn fast_backend_identifies_itself_and_its_timing_limits() {
        let backend = NativeMachineBackend::default();
        assert_eq!(backend.engine(), EmulationEngine::RustFast8080);
        assert!(!backend.capabilities().exact_bus_activity);
        assert!(!backend.capabilities().exact_t_state_timing);
        assert!(backend.capabilities().front_panel);
    }

    #[test]
    fn switch_register_adapter_sets_exact_value() {
        let mut backend = NativeMachineBackend::default();
        backend.set_switch_register(0xa55a);
        assert_eq!(backend.switch_register(), 0xa55a);

        backend.set_switch_register(0x0f0f);
        assert_eq!(backend.switch_register(), 0x0f0f);
    }

    #[test]
    fn serial_contract_routes_both_88_2sio_channels() {
        let mut backend = NativeMachineBackend::default();
        backend.configure_serial_board(SerialBoard::TwoSio88);
        backend.serial_receive(BackendSerialPort::Port0, b'A');
        backend.serial_receive(BackendSerialPort::Port1, b'B');
        assert_eq!(backend.serial_rx_len(BackendSerialPort::Port0), 1);
        assert_eq!(backend.serial_rx_len(BackendSerialPort::Port1), 1);
        assert!(!backend.serial_rx_empty(BackendSerialPort::Port0));
        assert!(!backend.serial_rx_empty(BackendSerialPort::Port1));
    }

    #[test]
    fn debugger_memory_access_round_trips_through_backend() {
        let mut backend = NativeMachineBackend::default();
        assert!(backend.write_memory(0x0010, 0x5a, false));
        assert_eq!(backend.peek_memory(0x0010), Some(0x5a));
    }
}
