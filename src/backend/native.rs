use std::time::Duration;

use crate::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};
use crate::debugger_control::DebugExecutionControl;
use crate::machine::{CpuDiagnosticResult, FastAltairMachine};
use crate::trace8080::{
    collect_post_instruction_effects, collect_pre_instruction_effects, CpuSnapshot8080,
    InstructionTraceBuffer, InstructionTraceMetadata,
};

use super::{
    BackendCapabilities, BackendExecutionModel, BackendResult, BackendSerialPort, CpuState,
    DebugStopReason, EmulationEngine, FrontPanelState, InstructionTraceSnapshot, Intel8080State,
    IoPortActivity, IoTraceSnapshot, MachineBackend, MemoryWatchAccess,
};

pub struct NativeMachineBackend {
    machine: FastAltairMachine,
    instruction_trace: InstructionTraceBuffer,
    debug_control: DebugExecutionControl,
}

impl Default for NativeMachineBackend {
    fn default() -> Self {
        Self {
            machine: FastAltairMachine::default(),
            instruction_trace: InstructionTraceBuffer::default(),
            debug_control: DebugExecutionControl::default(),
        }
    }
}

impl NativeMachineBackend {
    pub fn new(machine: FastAltairMachine) -> Self {
        Self {
            machine,
            instruction_trace: InstructionTraceBuffer::default(),
            debug_control: DebugExecutionControl::default(),
        }
    }
    pub fn machine(&self) -> &FastAltairMachine { &self.machine }
    pub fn machine_mut(&mut self) -> &mut FastAltairMachine { &mut self.machine }
    pub fn into_machine(self) -> FastAltairMachine { self.machine }

    fn snapshot_cpu(&self) -> CpuState {
        let cpu = &self.machine.cpu;
        CpuState::Intel8080(Intel8080State {
            a: cpu.a, b: cpu.b, c: cpu.c, d: cpu.d, e: cpu.e, h: cpu.h, l: cpu.l,
            flags: cpu.f, pc: cpu.pc, sp: cpu.sp, inte: cpu.inte,
            halted: Some(cpu.halted), total_t_states: Some(cpu.cycles),
        })
    }

    fn trace_cpu_snapshot(&self) -> CpuSnapshot8080 {
        let cpu = &self.machine.cpu;
        CpuSnapshot8080 {
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
        }
    }

    fn trace_bytes(&self, address: u16) -> [u8; 3] {
        [
            self.machine.bus.preview_guest_memory(address),
            self.machine.bus.preview_guest_memory(address.wrapping_add(1)),
            self.machine.bus.preview_guest_memory(address.wrapping_add(2)),
        ]
    }

    fn reset_debugger_epoch(&mut self) {
        self.instruction_trace.clear();
        self.debug_control.clear_transient();
    }

    fn record_one_stopped_instruction(&mut self) {
        if self.machine.cpu.halted {
            return;
        }

        let before = self.trace_cpu_snapshot();
        let bytes = self.trace_bytes(before.pc);
        let mut effects = collect_pre_instruction_effects(bytes, before, |address| {
            self.machine.bus.preview_guest_memory(address)
        });
        let start_cycles = self.machine.cpu.cycles;
        self.machine.step();
        let after = self.trace_cpu_snapshot();
        let post = collect_post_instruction_effects(bytes, before, after, &effects);
        effects.extend(post);
        let delta = self.machine.cpu.cycles.saturating_sub(start_cycles) as u32;
        if delta != 0 {
            self.debug_control.stop_after_effects(before.pc, &effects);
            if self.instruction_trace.enabled() {
                self.instruction_trace
                    .push_with_effects(before.pc, bytes, before, after, delta, effects);
            }
        }
    }

    fn execute_one_running_instruction(&mut self) -> u32 {
        let tracing = self.instruction_trace.enabled();
        let observing_effects = tracing || self.debug_control.has_watchpoints();
        let before = observing_effects.then(|| self.trace_cpu_snapshot());
        let bytes = before.map(|snapshot| self.trace_bytes(snapshot.pc));
        let mut effects = match (before, bytes) {
            (Some(snapshot), Some(opcodes)) => collect_pre_instruction_effects(
                opcodes,
                snapshot,
                |address| self.machine.bus.preview_guest_memory(address),
            ),
            _ => Vec::new(),
        };
        let start_cycles = self.machine.cpu.cycles;

        self.machine.run_cycles(1);

        let delta = self.machine.cpu.cycles.saturating_sub(start_cycles) as u32;
        if let (Some(before), Some(bytes)) = (before, bytes) {
            let after = self.trace_cpu_snapshot();
            let post = collect_post_instruction_effects(bytes, before, after, &effects);
            effects.extend(post);
            if delta != 0 {
                let watch_stop = self.debug_control.stop_after_effects(before.pc, &effects).is_some();
                if tracing {
                    self.instruction_trace
                        .push_with_effects(before.pc, bytes, before, after, delta, effects);
                }
                if watch_stop {
                    self.machine.set_running(false);
                }
            }
        }
        delta
    }

    fn service_execution_with_debug_boundaries(&mut self, t_state_budget: u32) {
        if !self.machine.running || t_state_budget == 0 {
            return;
        }

        let mut used = 0u32;
        while used < t_state_budget && self.machine.running {
            if self.machine.cpu.halted {
                self.machine.run_cycles(t_state_budget - used);
                break;
            }

            if self.machine.bus.cpu_control_lines().reset {
                self.machine.run_cycles(t_state_budget - used);
                break;
            }

            let pc = self.machine.cpu.pc;
            let sp = self.machine.cpu.sp;
            if self.debug_control.stop_before_with_sp(pc, sp).is_some() {
                self.machine.set_running(false);
                break;
            }

            let delta = self.execute_one_running_instruction();
            if delta == 0 {
                break;
            }
            used = used.saturating_add(delta);
        }
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
        self.reset_debugger_epoch();
        Ok(())
    }
    fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) -> BackendResult<()> {
        self.machine.configure_memory_board_profile(profile);
        self.reset_debugger_epoch();
        Ok(())
    }
    fn power(&mut self, on: bool) -> BackendResult<()> { self.power_with_historical_run_latch(on, false) }
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()> {
        self.machine.power_with_historical_run_latch(on, historical);
        self.reset_debugger_epoch();
        Ok(())
    }
    fn run(&mut self) -> BackendResult<()> {
        self.debug_control.prepare_resume_with_sp(self.machine.cpu.pc, self.machine.cpu.sp);
        self.machine.set_running(true);
        Ok(())
    }
    fn halt(&mut self) -> BackendResult<()> {
        self.debug_control.cancel_run_to();
        self.machine.set_running(false);
        Ok(())
    }
    fn step(&mut self) -> BackendResult<()> {
        self.debug_control.prepare_manual_step();
        if self.machine.cpu.halted {
            self.machine.step();
        } else if self.instruction_trace.enabled() || self.debug_control.has_watchpoints() {
            self.record_one_stopped_instruction();
        } else {
            self.machine.step();
        }
        Ok(())
    }
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        if self.instruction_trace.enabled() || self.debug_control.active() {
            self.service_execution_with_debug_boundaries(t_state_budget);
        } else if self.machine.running {
            self.machine.run_cycles(t_state_budget);
        }
        Ok(())
    }
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()> { self.machine.commit_panel_activity(dt); Ok(()) }
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()> {
        if run {
            self.debug_control.prepare_resume_with_sp(self.machine.cpu.pc, self.machine.cpu.sp);
        } else {
            self.debug_control.cancel_run_to();
        }
        self.machine.assert_run_stop(run);
        Ok(())
    }
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()> { self.machine.release_run_stop(run); Ok(()) }
    fn assert_reset(&mut self) -> BackendResult<()> {
        self.reset_debugger_epoch();
        self.machine.assert_front_panel_reset();
        Ok(())
    }
    fn release_reset(&mut self) -> BackendResult<()> { self.machine.release_front_panel_reset(); Ok(()) }
    fn assert_clear(&mut self) -> BackendResult<()> { self.machine.assert_front_panel_clear(); Ok(()) }
    fn release_clear(&mut self) -> BackendResult<()> { self.machine.release_front_panel_clear(); Ok(()) }
    fn request_hold(&mut self, hold: bool) -> BackendResult<()> { self.machine.request_hold(hold); Ok(()) }
    fn panel_examine(&mut self, next: bool) -> BackendResult<()> {
        self.reset_debugger_epoch();
        self.machine.fast_front_panel_examine_via_cpu_board(next);
        Ok(())
    }
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()> {
        if next {
            self.reset_debugger_epoch();
        }
        self.machine.fast_front_panel_deposit_via_cpu_board(next);
        Ok(())
    }
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()> { self.machine.front_panel_set_memory_protection_via_s100(protected); Ok(()) }
    fn switch_register(&mut self) -> BackendResult<u16> { Ok(self.machine.panel_switches()) }
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()> {
        let changed = self.machine.panel_switches() ^ value;
        for bit in 0..16 { if changed & (1u16 << bit) != 0 { self.machine.toggle_sense_switch(bit); } }
        Ok(())
    }
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()> {
        if self.machine.serial_board() != board {
            self.machine.configure_serial_board(board);
            self.reset_debugger_epoch();
        }
        Ok(())
    }
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
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()> {
        self.machine.bus.load(address, bytes);
        self.reset_debugger_epoch();
        Ok(())
    }
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
    fn instruction_trace_snapshot(&mut self) -> BackendResult<InstructionTraceSnapshot> { Ok(self.instruction_trace.snapshot()) }
    fn instruction_trace_enabled(&mut self) -> BackendResult<bool> { Ok(self.instruction_trace.enabled()) }
    fn instruction_trace_metadata(&mut self) -> BackendResult<InstructionTraceMetadata> { Ok(self.instruction_trace.metadata()) }
    fn set_instruction_trace_enabled(&mut self, enabled: bool) -> BackendResult<()> { self.instruction_trace.set_enabled(enabled); Ok(()) }
    fn clear_instruction_trace(&mut self) -> BackendResult<()> { self.instruction_trace.clear(); Ok(()) }

    fn debugger_step_instruction(&mut self) -> BackendResult<()> {
        if self.machine.powered && !self.machine.running && !self.machine.cpu.halted {
            self.debug_control.prepare_manual_step();
            if self.instruction_trace.enabled() || self.debug_control.has_watchpoints() {
                self.record_one_stopped_instruction();
            } else {
                self.machine.step();
            }
        }
        Ok(())
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
        Ok(())
    }
    fn debugger_clear_watchpoints(&mut self) -> BackendResult<()> {
        self.debug_control.clear_watchpoints();
        Ok(())
    }
    fn debugger_run_to(&mut self, address: u16) -> BackendResult<()> {
        self.debug_control.set_run_to(address);
        self.debug_control.prepare_resume_with_sp(self.machine.cpu.pc, self.machine.cpu.sp);
        self.machine.set_running(true);
        Ok(())
    }
    fn debugger_run_to_with_sp(&mut self, address: u16, required_sp: u16) -> BackendResult<()> {
        self.debug_control.set_run_to_with_sp(address, required_sp);
        self.debug_control.prepare_resume_with_sp(self.machine.cpu.pc, self.machine.cpu.sp);
        self.machine.set_running(true);
        Ok(())
    }
    fn debugger_cancel_run_to(&mut self) -> BackendResult<()> {
        self.debug_control.cancel_run_to();
        Ok(())
    }
    fn debugger_run_to_target(&mut self) -> BackendResult<Option<u16>> { Ok(self.debug_control.run_to()) }
    fn debugger_stop_reason(&mut self) -> BackendResult<Option<DebugStopReason>> { Ok(self.debug_control.stop_reason()) }

    fn debugger_input_port(&mut self, port: u8) -> BackendResult<u8> { Ok(self.machine.bus.debugger_input_port(port)) }
    fn debugger_output_port(&mut self, port: u8, value: u8) -> BackendResult<()> { self.machine.bus.debugger_output_port(port, value); Ok(()) }
    fn debugger_inject_serial_rx(&mut self, port: u8, byte: u8) -> BackendResult<bool> { Ok(self.machine.bus.debugger_inject_serial_rx(port, byte)) }
    fn debugger_clear_serial_rx(&mut self, port: u8) -> BackendResult<bool> { Ok(self.machine.bus.debugger_clear_serial_rx(port)) }
    fn debugger_clear_serial_tx(&mut self, port: u8) -> BackendResult<bool> { Ok(self.machine.bus.debugger_clear_serial_tx(port)) }
    fn debugger_complete_serial_tx(&mut self, port: u8) -> BackendResult<Option<u8>> { Ok(self.machine.bus.debugger_complete_serial_tx(port)) }
}
