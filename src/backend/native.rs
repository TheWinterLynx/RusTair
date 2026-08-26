use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::{AltairMachine, CpuDiagnosticResult};

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, CpuState,
    EmulationEngine, FrontPanelState, Intel8080State, IoPortActivity, IoTraceSnapshot,
    MachineBackend,
};

pub struct NativeMachineBackend { machine: AltairMachine }
impl Default for NativeMachineBackend { fn default() -> Self { Self { machine: AltairMachine::default() } } }
impl NativeMachineBackend {
    pub fn new(machine: AltairMachine) -> Self { Self { machine } }
    pub fn machine(&self) -> &AltairMachine { &self.machine }
    pub fn machine_mut(&mut self) -> &mut AltairMachine { &mut self.machine }
    pub fn into_machine(self) -> AltairMachine { self.machine }
    fn snapshot_cpu(&self) -> CpuState {
        let cpu = &self.machine.cpu;
        CpuState::Intel8080(Intel8080State {
            a: cpu.a, b: cpu.b, c: cpu.c, d: cpu.d, e: cpu.e, h: cpu.h, l: cpu.l,
            flags: cpu.f, pc: cpu.pc, sp: cpu.sp, inte: cpu.inte,
            halted: Some(cpu.halted), total_t_states: Some(cpu.cycles),
        })
    }
    fn snapshot_panel(&self) -> FrontPanelState {
        FrontPanelState {
            powered: self.machine.powered, running: self.machine.running,
            switches: self.machine.panel_switches(), address: self.machine.address_leds(),
            data: self.machine.data_leds(), lamps: self.machine.panel_lamps(),
            current_board_protected: self.machine.current_board_protected(),
            ext_clear_asserted: self.machine.ext_clear_asserted(),
        }
    }
}

impl MachineBackend for NativeMachineBackend {
    fn engine(&self) -> EmulationEngine { EmulationEngine::RustFast8080 }
    fn name(&self) -> &'static str { "RusTair fast 8080" }
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities { front_panel: true, exact_bus_activity: false, exact_t_state_timing: false,
            memory_protection: true, hold_hlda: true, direct_memory_access: true,
            serial_routing: true, disk_mount: false }
    }
    fn execution_model(&self) -> BackendExecutionModel { BackendExecutionModel::HostDriven }
    fn cpu_state(&mut self) -> BackendResult<CpuState> { Ok(self.snapshot_cpu()) }
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState> { Ok(self.snapshot_panel()) }
    fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {
        self.machine.configure_memory(size, init);
        Ok(())
    }
    fn power(&mut self, on: bool) -> BackendResult<()> { self.machine.power(on); Ok(()) }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> { self.machine.power_with_historical_run_latch(on, historical); Ok(()) }
    fn run(&mut self) -> BackendResult<()> { self.machine.set_running(true); Ok(()) }
    fn halt(&mut self) -> BackendResult<()> { self.machine.set_running(false); Ok(()) }
    fn step(&mut self) -> BackendResult<()> { self.machine.step(); Ok(()) }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> { if self.machine.running { self.machine.run_cycles(t_state_budget); } Ok(()) }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> { self.machine.commit_panel_activity(dt); Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> { self.machine.assert_run_stop(run); Ok(()) }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.machine.release_run_stop(run); Ok(()) }
    fn assert_reset(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_reset(); Ok(()) }
    fn release_reset(&mut self) -> BackendResult<()> { self.machine.release_front_panel_reset(); Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_clear(); Ok(()) }
    fn release_clear(&mut self) -> BackendResult<()> { self.machine.release_front_panel_clear(); Ok(()) }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.machine.request_hold(hold); Ok(()) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> { self.machine.fast_front_panel_examine_via_cpu_board(next); Ok(()) }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> { self.machine.fast_front_panel_deposit_via_cpu_board(next); Ok(()) }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> { self.machine.front_panel_set_memory_protection_via_s100(protected); Ok(()) }
    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.machine.panel_switches()) }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        let changed = self.machine.panel_switches() ^ value;
        for bit in 0..16 { if changed & (1u16 << bit) != 0 { self.machine.toggle_sense_switch(bit); } }
        Ok(())
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> { self.machine.configure_serial_board(board); Ok(()) }
    fn serial_board(&mut self) -> BackendResult<SerialBoard> { Ok(self.machine.serial_board()) }
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()> {
        match port { BackendSerialPort::Port0 => self.machine.bus.serial_receive(byte), BackendSerialPort::Port1 => self.machine.bus.serial_port1_receive(byte) }; Ok(())
    }
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool> { Ok(match port { BackendSerialPort::Port0 => self.machine.bus.serial_rx_empty(), BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_empty() }) }
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize> { Ok(match port { BackendSerialPort::Port0 => self.machine.bus.serial_rx_len(), BackendSerialPort::Port1 => self.machine.bus.serial_port1_rx_len() }) }
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool> { Ok(match port { BackendSerialPort::Port0 => self.machine.bus.tx_busy(), BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_busy() }) }
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> { Ok(match port { BackendSerialPort::Port0 => self.machine.bus.serial_tx_front(), BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_front() }) }
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>> { Ok(match port { BackendSerialPort::Port0 => self.machine.bus.serial_tx_complete(), BackendSerialPort::Port1 => self.machine.bus.serial_port1_tx_complete() }) }
    fn clear_serial(&mut self) -> BackendResult<()> { self.machine.bus.clear_serial(); Ok(()) }
    fn installed_ram_bytes(&mut self) -> BackendResult<usize> { Ok(self.machine.installed_ram_bytes()) }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>> { Ok(self.machine.bus.peek_memory(address)) }
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> BackendResult<bool> { Ok(self.machine.bus.debugger_write_memory(address, value, respect_protection)) }
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> { self.machine.bus.load(address, bytes); Ok(()) }
    fn memory_is_protected(&mut self, address: u16) -> BackendResult<bool> { Ok(self.machine.bus.is_protected(address)) }
    fn clear_memory_protection(&mut self) -> BackendResult<()> { self.machine.bus.clear_protection(); Ok(()) }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> { self.machine.bus.clear_transient_memory_guards(); Ok(()) }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> { Ok(self.machine.arm_basic32_full_memory_probe_guard()) }
    fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) -> BackendResult<()> {
        self.machine.begin_cpu_diagnostic_meter(name, bdos_start, bdos_len, expected_instructions, expected_t_states);
        Ok(())
    }
    fn cancel_cpu_diagnostic_meter(&mut self) -> BackendResult<()> { self.machine.bus.cancel_cpu_diagnostic_meter(); Ok(()) }
    fn take_cpu_diagnostic_result(&mut self) -> BackendResult<Option<CpuDiagnosticResult>> { Ok(self.machine.take_cpu_diagnostic_result()) }
    fn peek_io_port(&mut self, port: u8) -> BackendResult<u8> { Ok(self.machine.bus.peek_io_port(port)) }
    fn io_port_activity(&mut self, port: u8) -> BackendResult<IoPortActivity> { Ok(self.machine.bus.io_port_activity(port)) }
    fn io_trace_snapshot(&mut self) -> BackendResult<IoTraceSnapshot> { Ok(self.machine.bus.io_trace_snapshot()) }
    fn io_trace_enabled(&mut self) -> BackendResult<bool> { Ok(self.machine.bus.io_trace_enabled()) }
    fn set_io_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> { self.machine.bus.set_io_trace_enabled(enabled); Ok(()) }
    fn clear_io_trace(&mut self) -> BackendResult<()> { self.machine.bus.clear_io_trace(); Ok(()) }
    fn debugger_input_port(&mut self, port: u8) -> BackendResult<u8> { Ok(self.machine.bus.debugger_input_port(port)) }
    fn debugger_output_port(&mut self, port: u8, value: u8) -> BackendResult<()> { self.machine.bus.debugger_output_port(port, value); Ok(()) }
    fn debugger_inject_serial_rx(&mut self, port: u8, byte: u8) -> BackendResult<bool> { Ok(self.machine.bus.debugger_inject_serial_rx(port, byte)) }
    fn debugger_clear_serial_rx(&mut self, port: u8) -> BackendResult<bool> { Ok(self.machine.bus.debugger_clear_serial_rx(port)) }
    fn debugger_clear_serial_tx(&mut self, port: u8) -> BackendResult<bool> { Ok(self.machine.bus.debugger_clear_serial_tx(port)) }
    fn debugger_complete_serial_tx(&mut self, port: u8) -> BackendResult<Option<u8>> { Ok(self.machine.bus.debugger_complete_serial_tx(port)) }
}