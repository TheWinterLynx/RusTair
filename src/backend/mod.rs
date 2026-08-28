//! Machine-level abstraction for RusTair's selectable emulator engines.

mod bus_teaching;
mod cycle;
mod cycle_host;
mod native;
pub mod simh;

use std::fmt;
use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::{CpuDiagnosticResult, PanelLampSnapshot};

use cycle_host::CycleHostBackend;
pub use bus_teaching::{
    BusCpuPins, BusMachineCycle, BusStatusLines, BusTeachingAccuracy, BusTeachingSnapshot, BusTState,
};
pub use crate::debugger_control::{DebugStopReason, MemoryWatchAccess};
pub use crate::trace8080::{InstructionTraceEntry, InstructionTraceMetadata};
pub use cycle::CycleAccurateMachineBackend;
pub use native::NativeMachineBackend;
pub type FastMachineBackend = NativeMachineBackend;
pub type InstructionTraceSnapshot = Vec<InstructionTraceEntry>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendFamily { Rustair, Simh }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmulationEngine {
    RustFast8080,
    RustCycleAccurate8080,
    SimhAltair,
    SimhAltairZ80,
}

impl EmulationEngine {
    pub const ALL: [Self; 4] = [
        Self::RustFast8080,
        Self::RustCycleAccurate8080,
        Self::SimhAltair,
        Self::SimhAltairZ80,
    ];
    pub const fn family(self) -> BackendFamily {
        match self {
            Self::RustFast8080 | Self::RustCycleAccurate8080 => BackendFamily::Rustair,
            Self::SimhAltair | Self::SimhAltairZ80 => BackendFamily::Simh,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::RustFast8080 => "RusTair — Fast 8080",
            Self::RustCycleAccurate8080 => "RusTair — Cycle Accurate 8080",
            Self::SimhAltair => "Open SIMH — Altair",
            Self::SimhAltairZ80 => "Open SIMH — AltairZ80",
        }
    }
    pub const fn is_available(self) -> bool {
        matches!(self, Self::RustFast8080 | Self::RustCycleAccurate8080)
    }
}

impl Default for EmulationEngine { fn default() -> Self { Self::RustFast8080 } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendExecutionModel { HostDriven, ExternalProcess }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendSerialPort { Port0, Port1 }
impl BackendSerialPort { pub const ALL: [Self; 2] = [Self::Port0, Self::Port1]; }

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendCapabilities {
    pub front_panel: bool,
    pub exact_bus_activity: bool,
    pub exact_t_state_timing: bool,
    pub memory_protection: bool,
    pub hold_hlda: bool,
    pub direct_memory_access: bool,
    pub serial_routing: bool,
    pub disk_mount: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Intel8080State {
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub flags: u8,
    pub pc: u16,
    pub sp: u16,
    pub inte: bool,
    pub halted: Option<bool>,
    pub total_t_states: Option<u64>,
}
impl Intel8080State {
    pub const fn af(self) -> u16 { ((self.a as u16) << 8) | self.flags as u16 }
    pub const fn bc(self) -> u16 { ((self.b as u16) << 8) | self.c as u16 }
    pub const fn de(self) -> u16 { ((self.d as u16) << 8) | self.e as u16 }
    pub const fn hl(self) -> u16 { ((self.h as u16) << 8) | self.l as u16 }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Z80State {
    pub a: u8,
    pub flags: u8,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub pc: u16,
    pub sp: u16,
    pub ix: u16,
    pub iy: u16,
    pub af_alt: u16,
    pub bc_alt: u16,
    pub de_alt: u16,
    pub hl_alt: u16,
    pub iff: u8,
    pub interrupt_mode: u8,
    pub ir: u16,
    pub halted: Option<bool>,
    pub total_t_states: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuState { Intel8080(Intel8080State), Z80(Z80State) }
impl Default for CpuState { fn default() -> Self { Self::Intel8080(Intel8080State::default()) } }

#[derive(Clone, Copy, Debug)]
pub struct FrontPanelState {
    pub powered: bool,
    pub running: bool,
    pub switches: u16,
    pub address: u16,
    pub data: u8,
    pub lamps: PanelLampSnapshot,
    pub current_board_protected: bool,
    pub ext_clear_asserted: bool,
}
impl Default for FrontPanelState {
    fn default() -> Self {
        Self {
            powered: false,
            running: false,
            switches: 0,
            address: 0,
            data: 0,
            lamps: PanelLampSnapshot::default(),
            current_board_protected: false,
            ext_clear_asserted: false,
        }
    }
}

pub type IoPortActivity = (Option<u8>, Option<u8>, u64, u64);
pub type IoTraceSnapshot = Vec<(u64, u8, u8, u8, u32)>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendError {
    Unsupported { operation: &'static str, engine: EmulationEngine },
    Operation { operation: &'static str, detail: String },
}
impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { operation, engine } => write!(f, "{} does not support {operation}", engine.label()),
            Self::Operation { operation, detail } => write!(f, "{operation} failed: {detail}"),
        }
    }
}
impl std::error::Error for BackendError {}
pub type BackendResult<T> = Result<T, BackendError>;

pub trait MachineBackend {
    fn engine(&self) -> EmulationEngine;
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn execution_model(&self) -> BackendExecutionModel;
    fn family(&self) -> BackendFamily { self.engine().family() }

    fn cpu_state(&mut self) -> BackendResult<CpuState>;
    fn front_panel_state(&mut self) -> BackendResult<FrontPanelState>;
    fn configure_memory(&mut self, _size: RamSize, _init: RamInit) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "configure memory", engine: self.engine() })
    }
    fn power(&mut self, on: bool) -> BackendResult<()>;
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) -> BackendResult<()>;
    fn run(&mut self) -> BackendResult<()>;
    fn halt(&mut self) -> BackendResult<()>;
    fn step(&mut self) -> BackendResult<()>;
    fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()>;
    fn commit_panel_activity(&mut self, dt: Duration) -> BackendResult<()>;
    fn assert_run_stop(&mut self, run: bool) -> BackendResult<()>;
    fn release_run_stop(&mut self, run: bool) -> BackendResult<()>;
    fn assert_reset(&mut self) -> BackendResult<()>;
    fn release_reset(&mut self) -> BackendResult<()>;
    fn assert_clear(&mut self) -> BackendResult<()>;
    fn release_clear(&mut self) -> BackendResult<()>;
    fn request_hold(&mut self, hold: bool) -> BackendResult<()>;
    fn panel_examine(&mut self, next: bool) -> BackendResult<()>;
    fn panel_deposit(&mut self, next: bool) -> BackendResult<()>;
    fn protect_current_board(&mut self, protected: bool) -> BackendResult<()>;
    fn switch_register(&mut self) -> BackendResult<u16>;
    fn set_switch_register(&mut self, value: u16) -> BackendResult<()>;
    fn configure_serial_board(&mut self, board: SerialBoard) -> BackendResult<()>;
    fn serial_board(&mut self) -> BackendResult<SerialBoard>;
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) -> BackendResult<()>;
    fn serial_rx_empty(&mut self, port: BackendSerialPort) -> BackendResult<bool>;
    fn serial_rx_len(&mut self, port: BackendSerialPort) -> BackendResult<usize>;
    fn serial_tx_busy(&mut self, port: BackendSerialPort) -> BackendResult<bool>;
    fn serial_tx_front(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>>;
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> BackendResult<Option<u8>>;
    fn clear_serial(&mut self) -> BackendResult<()>;
    fn installed_ram_bytes(&mut self) -> BackendResult<usize> {
        Err(BackendError::Unsupported { operation: "query installed RAM", engine: self.engine() })
    }
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>>;
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> BackendResult<bool>;
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()>;
    fn memory_is_protected(&mut self, _address: u16) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "query memory protection", engine: self.engine() })
    }
    fn clear_memory_protection(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "clear memory protection", engine: self.engine() })
    }
    fn clear_transient_memory_guards(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "clear transient memory guards", engine: self.engine() })
    }
    fn arm_basic32_full_memory_probe_guard(&mut self) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "arm BASIC 3.2 memory guard", engine: self.engine() })
    }
    fn begin_cpu_diagnostic_meter(
        &mut self,
        _name: String,
        _bdos_start: u16,
        _bdos_len: usize,
        _expected_instructions: Option<u64>,
        _expected_t_states: Option<u64>,
    ) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "begin CPU diagnostic meter", engine: self.engine() })
    }
    fn cancel_cpu_diagnostic_meter(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "cancel CPU diagnostic meter", engine: self.engine() })
    }
    fn take_cpu_diagnostic_result(&mut self) -> BackendResult<Option<CpuDiagnosticResult>> {
        Err(BackendError::Unsupported { operation: "take CPU diagnostic result", engine: self.engine() })
    }
    fn peek_io_port(&mut self, _port: u8) -> BackendResult<u8> {
        Err(BackendError::Unsupported { operation: "peek I/O port", engine: self.engine() })
    }
    fn io_port_activity(&mut self, _port: u8) -> BackendResult<IoPortActivity> {
        Err(BackendError::Unsupported { operation: "query I/O activity", engine: self.engine() })
    }
    fn io_trace_snapshot(&mut self) -> BackendResult<IoTraceSnapshot> {
        Err(BackendError::Unsupported { operation: "read I/O trace", engine: self.engine() })
    }
    fn io_trace_enabled(&mut self) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "query I/O trace", engine: self.engine() })
    }
    fn set_io_trace_enabled(&mut self, _enabled: bool) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "configure I/O trace", engine: self.engine() })
    }
    fn clear_io_trace(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "clear I/O trace", engine: self.engine() })
    }
    fn instruction_trace_snapshot(&mut self) -> BackendResult<InstructionTraceSnapshot> {
        Err(BackendError::Unsupported { operation: "read instruction trace", engine: self.engine() })
    }
    fn instruction_trace_enabled(&mut self) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "query instruction trace", engine: self.engine() })
    }
    fn instruction_trace_metadata(&mut self) -> BackendResult<InstructionTraceMetadata> {
        Err(BackendError::Unsupported { operation: "query instruction trace metadata", engine: self.engine() })
    }
    fn set_instruction_trace_enabled(&mut self, _enabled: bool) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "configure instruction trace", engine: self.engine() })
    }
    fn clear_instruction_trace(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "clear instruction trace", engine: self.engine() })
    }
    fn bus_teaching_snapshot(&mut self) -> BackendResult<Option<BusTeachingSnapshot>> {
        let engine = self.engine();
        let panel = self.front_panel_state()?;
        let cpu = self.cpu_state()?;
        Ok(Some(BusTeachingSnapshot::reconstructed(engine, panel, cpu)))
    }
    fn debugger_step_t_state(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "debugger T-state step", engine: self.engine() })
    }
    fn debugger_step_machine_cycle(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "debugger machine-cycle step", engine: self.engine() })
    }
    fn debugger_step_instruction(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "debugger step instruction", engine: self.engine() })
    }
    fn debugger_breakpoints(&mut self) -> BackendResult<Vec<u16>> {
        Err(BackendError::Unsupported { operation: "read debugger breakpoints", engine: self.engine() })
    }
    fn debugger_set_breakpoint(&mut self, _address: u16, _enabled: bool) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "set debugger breakpoint", engine: self.engine() })
    }
    fn debugger_clear_breakpoints(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "clear debugger breakpoints", engine: self.engine() })
    }
    fn debugger_watchpoints(&mut self) -> BackendResult<Vec<(u16, MemoryWatchAccess)>> {
        Err(BackendError::Unsupported { operation: "read debugger watchpoints", engine: self.engine() })
    }
    fn debugger_set_watchpoint(
        &mut self,
        _address: u16,
        _access: Option<MemoryWatchAccess>,
    ) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "set debugger watchpoint", engine: self.engine() })
    }
    fn debugger_clear_watchpoints(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "clear debugger watchpoints", engine: self.engine() })
    }
    fn debugger_run_to(&mut self, _address: u16) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "debugger run to", engine: self.engine() })
    }
    fn debugger_run_to_with_sp(&mut self, address: u16, _required_sp: u16) -> BackendResult<()> {
        self.debugger_run_to(address)
    }
    fn debugger_cancel_run_to(&mut self) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "cancel debugger run to", engine: self.engine() })
    }
    fn debugger_run_to_target(&mut self) -> BackendResult<Option<u16>> {
        Err(BackendError::Unsupported { operation: "query debugger run to", engine: self.engine() })
    }
    fn debugger_stop_reason(&mut self) -> BackendResult<Option<DebugStopReason>> {
        Err(BackendError::Unsupported { operation: "query debugger stop reason", engine: self.engine() })
    }
    fn debugger_input_port(&mut self, _port: u8) -> BackendResult<u8> {
        Err(BackendError::Unsupported { operation: "debugger IN", engine: self.engine() })
    }
    fn debugger_output_port(&mut self, _port: u8, _value: u8) -> BackendResult<()> {
        Err(BackendError::Unsupported { operation: "debugger OUT", engine: self.engine() })
    }
    fn debugger_inject_serial_rx(&mut self, _port: u8, _byte: u8) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "inject serial RX", engine: self.engine() })
    }
    fn debugger_clear_serial_rx(&mut self, _port: u8) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "clear serial RX", engine: self.engine() })
    }
    fn debugger_clear_serial_tx(&mut self, _port: u8) -> BackendResult<bool> {
        Err(BackendError::Unsupported { operation: "clear serial TX", engine: self.engine() })
    }
    fn debugger_complete_serial_tx(&mut self, _port: u8) -> BackendResult<Option<u8>> {
        Err(BackendError::Unsupported { operation: "complete serial TX", engine: self.engine() })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendCreateError { Unavailable(EmulationEngine) }
impl fmt::Display for BackendCreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self { Self::Unavailable(engine) => write!(f, "{} backend is not available in this build", engine.label()) }
    }
}
impl std::error::Error for BackendCreateError {}

pub fn create_backend(engine: EmulationEngine) -> Result<Box<dyn MachineBackend>, BackendCreateError> {
    match engine {
        EmulationEngine::RustFast8080 => Ok(Box::new(NativeMachineBackend::default())),
        EmulationEngine::RustCycleAccurate8080 => Ok(Box::new(CycleHostBackend::default())),
        EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80 =>
            Err(BackendCreateError::Unavailable(engine)),
    }
}

pub struct BackendHost { backend: Box<dyn MachineBackend> }
impl Default for BackendHost { fn default() -> Self { Self::rust_fast() } }
impl BackendHost {
    pub fn new(backend: Box<dyn MachineBackend>) -> Self { Self { backend } }
    pub fn from_engine(engine: EmulationEngine) -> Result<Self, BackendCreateError> { create_backend(engine).map(Self::new) }
    pub fn rust_fast() -> Self { Self::from_engine(EmulationEngine::RustFast8080).expect("built-in fast backend") }
    pub fn native() -> Self { Self::rust_fast() }
    pub fn replace_engine(&mut self, engine: EmulationEngine) -> Result<(), BackendCreateError> {
        self.backend = create_backend(engine)?;
        Ok(())
    }
    pub fn engine(&self) -> EmulationEngine { self.backend.engine() }
    pub fn family(&self) -> BackendFamily { self.backend.family() }
    pub fn capabilities(&self) -> BackendCapabilities { self.backend.capabilities() }
    pub fn execution_model(&self) -> BackendExecutionModel { self.backend.execution_model() }

    fn call<T>(result: BackendResult<T>) -> T {
        result.unwrap_or_else(|error| panic!("selected backend operation failed: {error}"))
    }

    pub fn cpu_state(&mut self) -> CpuState { Self::call(self.backend.cpu_state()) }
    pub fn intel8080_state(&mut self) -> Intel8080State {
        match self.cpu_state() {
            CpuState::Intel8080(state) => state,
            CpuState::Z80(_) => panic!("selected backend exposes a Z80, not an Intel 8080"),
        }
    }
    pub fn front_panel_state(&mut self) -> FrontPanelState { Self::call(self.backend.front_panel_state()) }
    pub fn powered(&mut self) -> bool { self.front_panel_state().powered }
    pub fn running(&mut self) -> bool { self.front_panel_state().running }
    pub fn configure_memory(&mut self, size: RamSize, init: RamInit) { Self::call(self.backend.configure_memory(size, init)); }
    pub fn configure_serial_board(&mut self, board: SerialBoard) { Self::call(self.backend.configure_serial_board(board)); }
    pub fn serial_board(&mut self) -> SerialBoard { Self::call(self.backend.serial_board()) }
    pub fn power(&mut self, on: bool) { Self::call(self.backend.power(on)); }
    pub fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) { Self::call(self.backend.power_with_historical_run_latch(on, historical)); }
    pub fn set_running(&mut self, run: bool) { if run { Self::call(self.backend.run()); } else { Self::call(self.backend.halt()); } }
    pub fn run_cycles(&mut self, t_state_budget: u32) { Self::call(self.backend.service_execution(t_state_budget)); }
    pub fn step(&mut self) { Self::call(self.backend.step()); }
    pub fn commit_panel_activity(&mut self, dt: Duration) { Self::call(self.backend.commit_panel_activity(dt)); }
    pub fn assert_run_stop(&mut self, run: bool) { Self::call(self.backend.assert_run_stop(run)); }
    pub fn release_run_stop(&mut self, run: bool) { Self::call(self.backend.release_run_stop(run)); }
    pub fn assert_front_panel_reset(&mut self) { Self::call(self.backend.assert_reset()); }
    pub fn release_front_panel_reset(&mut self) { Self::call(self.backend.release_reset()); }
    pub fn front_panel_reset(&mut self) { self.assert_front_panel_reset(); self.release_front_panel_reset(); }
    pub fn reset(&mut self) { self.front_panel_reset(); Self::call(self.backend.clear_serial()); }
    pub fn assert_front_panel_clear(&mut self) { Self::call(self.backend.assert_clear()); }
    pub fn release_front_panel_clear(&mut self) { Self::call(self.backend.release_clear()); }
    pub fn clear_io(&mut self) { self.assert_front_panel_clear(); self.release_front_panel_clear(); }
    pub fn request_hold(&mut self, hold: bool) { Self::call(self.backend.request_hold(hold)); }
    pub fn examine(&mut self, next: bool) { Self::call(self.backend.panel_examine(next)); }
    pub fn deposit(&mut self, next: bool) { Self::call(self.backend.panel_deposit(next)); }
    pub fn protect_current_board(&mut self, protected: bool) { Self::call(self.backend.protect_current_board(protected)); }
    pub fn switch_register(&mut self) -> u16 { Self::call(self.backend.switch_register()) }
    pub fn set_switch_register(&mut self, value: u16) { Self::call(self.backend.set_switch_register(value)); }
    pub fn toggle_sense_switch(&mut self, bit: usize) {
        let next = self.switch_register() ^ (1u16 << bit);
        self.set_switch_register(next);
    }
    pub fn serial_receive(&mut self, port: BackendSerialPort, byte: u8) { Self::call(self.backend.serial_receive(port, byte)); }
    pub fn serial_rx_empty(&mut self, port: BackendSerialPort) -> bool { Self::call(self.backend.serial_rx_empty(port)) }
    pub fn serial_rx_len(&mut self, port: BackendSerialPort) -> usize { Self::call(self.backend.serial_rx_len(port)) }
    pub fn serial_tx_busy(&mut self, port: BackendSerialPort) -> bool { Self::call(self.backend.serial_tx_busy(port)) }
    pub fn serial_tx_front(&mut self, port: BackendSerialPort) -> Option<u8> { Self::call(self.backend.serial_tx_front(port)) }
    pub fn serial_tx_complete(&mut self, port: BackendSerialPort) -> Option<u8> { Self::call(self.backend.serial_tx_complete(port)) }
    pub fn clear_serial(&mut self) { Self::call(self.backend.clear_serial()); }
    pub fn installed_ram_bytes(&mut self) -> usize { Self::call(self.backend.installed_ram_bytes()) }
    pub fn peek_memory(&mut self, address: u16) -> Option<u8> { Self::call(self.backend.peek_memory(address)) }
    pub fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> bool { Self::call(self.backend.write_memory(address, value, respect_protection)) }
    pub fn load_bytes(&mut self, address: u16, bytes: &[u8]) { Self::call(self.backend.load_bytes(address, bytes)); }
    pub fn memory_is_protected(&mut self, address: u16) -> bool { Self::call(self.backend.memory_is_protected(address)) }
    pub fn clear_memory_protection(&mut self) { Self::call(self.backend.clear_memory_protection()); }
    pub fn clear_transient_memory_guards(&mut self) { Self::call(self.backend.clear_transient_memory_guards()); }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { Self::call(self.backend.arm_basic32_full_memory_probe_guard()) }
    pub fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) {
        Self::call(self.backend.begin_cpu_diagnostic_meter(name, bdos_start, bdos_len, expected_instructions, expected_t_states));
    }
    pub fn cancel_cpu_diagnostic_meter(&mut self) { Self::call(self.backend.cancel_cpu_diagnostic_meter()); }
    pub fn take_cpu_diagnostic_result(&mut self) -> Option<CpuDiagnosticResult> { Self::call(self.backend.take_cpu_diagnostic_result()) }
    pub fn peek_io_port(&mut self, port: u8) -> u8 { Self::call(self.backend.peek_io_port(port)) }
    pub fn io_port_activity(&mut self, port: u8) -> IoPortActivity { Self::call(self.backend.io_port_activity(port)) }
    pub fn io_trace_snapshot(&mut self) -> IoTraceSnapshot { Self::call(self.backend.io_trace_snapshot()) }
    pub fn io_trace_enabled(&mut self) -> bool { Self::call(self.backend.io_trace_enabled()) }
    pub fn set_io_trace_enabled(&mut self, enabled: bool) { Self::call(self.backend.set_io_trace_enabled(enabled)); }
    pub fn clear_io_trace(&mut self) { Self::call(self.backend.clear_io_trace()); }
    pub fn instruction_trace_snapshot(&mut self) -> InstructionTraceSnapshot { Self::call(self.backend.instruction_trace_snapshot()) }
    pub fn instruction_trace_enabled(&mut self) -> bool { Self::call(self.backend.instruction_trace_enabled()) }
    pub fn instruction_trace_metadata(&mut self) -> InstructionTraceMetadata { Self::call(self.backend.instruction_trace_metadata()) }
    pub fn set_instruction_trace_enabled(&mut self, enabled: bool) { Self::call(self.backend.set_instruction_trace_enabled(enabled)); }
    pub fn clear_instruction_trace(&mut self) { Self::call(self.backend.clear_instruction_trace()); }
    pub fn bus_teaching_snapshot(&mut self) -> Option<BusTeachingSnapshot> { Self::call(self.backend.bus_teaching_snapshot()) }
    pub fn debugger_step_t_state(&mut self) { Self::call(self.backend.debugger_step_t_state()); }
    pub fn debugger_step_machine_cycle(&mut self) { Self::call(self.backend.debugger_step_machine_cycle()); }
    pub fn debugger_step_instruction(&mut self) { Self::call(self.backend.debugger_step_instruction()); }
    pub fn debugger_breakpoints(&mut self) -> Vec<u16> { Self::call(self.backend.debugger_breakpoints()) }
    pub fn debugger_set_breakpoint(&mut self, address: u16, enabled: bool) { Self::call(self.backend.debugger_set_breakpoint(address, enabled)); }
    pub fn debugger_clear_breakpoints(&mut self) { Self::call(self.backend.debugger_clear_breakpoints()); }
    pub fn debugger_watchpoints(&mut self) -> Vec<(u16, MemoryWatchAccess)> { Self::call(self.backend.debugger_watchpoints()) }
    pub fn debugger_set_watchpoint(&mut self, address: u16, access: Option<MemoryWatchAccess>) { Self::call(self.backend.debugger_set_watchpoint(address, access)); }
    pub fn debugger_clear_watchpoints(&mut self) { Self::call(self.backend.debugger_clear_watchpoints()); }
    pub fn debugger_run_to(&mut self, address: u16) { Self::call(self.backend.debugger_run_to(address)); }
    pub fn debugger_run_to_with_sp(&mut self, address: u16, required_sp: u16) { Self::call(self.backend.debugger_run_to_with_sp(address, required_sp)); }
    pub fn debugger_cancel_run_to(&mut self) { Self::call(self.backend.debugger_cancel_run_to()); }
    pub fn debugger_run_to_target(&mut self) -> Option<u16> { Self::call(self.backend.debugger_run_to_target()) }
    pub fn debugger_stop_reason(&mut self) -> Option<DebugStopReason> { Self::call(self.backend.debugger_stop_reason()) }
    pub fn debugger_input_port(&mut self, port: u8) -> u8 { Self::call(self.backend.debugger_input_port(port)) }
    pub fn debugger_output_port(&mut self, port: u8, value: u8) { Self::call(self.backend.debugger_output_port(port, value)); }
    pub fn debugger_inject_serial_rx(&mut self, port: u8, byte: u8) -> bool { Self::call(self.backend.debugger_inject_serial_rx(port, byte)) }
    pub fn debugger_clear_serial_rx(&mut self, port: u8) -> bool { Self::call(self.backend.debugger_clear_serial_rx(port)) }
    pub fn debugger_clear_serial_tx(&mut self, port: u8) -> bool { Self::call(self.backend.debugger_clear_serial_tx(port)) }
    pub fn debugger_complete_serial_tx(&mut self, port: u8) -> Option<u8> { Self::call(self.backend.debugger_complete_serial_tx(port)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_builtin_rust_8080_engines_are_available() {
        assert!(EmulationEngine::RustFast8080.is_available());
        assert!(EmulationEngine::RustCycleAccurate8080.is_available());
        assert!(BackendHost::from_engine(EmulationEngine::RustFast8080).is_ok());
        assert!(BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080).is_ok());
    }

    #[test]
    fn engine_families_remain_separate_from_simh() {
        assert_eq!(EmulationEngine::RustFast8080.family(), BackendFamily::Rustair);
        assert_eq!(EmulationEngine::RustCycleAccurate8080.family(), BackendFamily::Rustair);
        assert_eq!(EmulationEngine::SimhAltair.family(), BackendFamily::Simh);
        assert_eq!(EmulationEngine::SimhAltairZ80.family(), BackendFamily::Simh);
    }

    #[test]
    fn host_dispatches_step_without_altair_machine_escape_hatch() {
        let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080).unwrap();
        host.configure_memory(RamSize::K1, RamInit::Zeroed);
        host.power(true);
        host.front_panel_reset();
        host.load_bytes(0, &[0x00]);
        host.step();
        let cpu = host.intel8080_state();
        let panel = host.front_panel_state();
        assert_eq!(cpu.pc, 1);
        assert_eq!(cpu.total_t_states, Some(4));
        assert!(panel.lamps.wait > 0.0);
    }
}
