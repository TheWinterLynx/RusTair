//! Machine-level abstraction for RusTair's selectable emulator engines.

mod cycle;
mod native;
pub mod simh;

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use crate::config::{RamInit, RamSize, SerialBoard};
use crate::machine::{AltairMachine, PanelLampSnapshot};

pub use cycle::CycleAccurateMachineBackend;
pub use native::NativeMachineBackend;
pub type FastMachineBackend = NativeMachineBackend;

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
        Self { powered: false, running: false, switches: 0, address: 0, data: 0,
            lamps: PanelLampSnapshot::default(), current_board_protected: false,
            ext_clear_asserted: false }
    }
}

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

    /// Transitional access to RusTair's shared Altair/S-100 chassis. Both Rust
    /// engines own the same chassis model; external engines intentionally return
    /// `None`. The application migration uses this only for legacy read-only or
    /// board-level access while CPU-affecting operations already dispatch through
    /// `MachineBackend`.
    fn rust_machine(&self) -> Option<&AltairMachine> { None }
    fn rust_machine_mut(&mut self) -> Option<&mut AltairMachine> { None }

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
    fn peek_memory(&mut self, address: u16) -> BackendResult<Option<u8>>;
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> BackendResult<bool>;
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()>;
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
        EmulationEngine::RustCycleAccurate8080 => Ok(Box::new(CycleAccurateMachineBackend::default())),
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
    pub fn backend(&self) -> &dyn MachineBackend { self.backend.as_ref() }
    pub fn backend_mut(&mut self) -> &mut dyn MachineBackend { self.backend.as_mut() }
    pub fn replace(&mut self, backend: Box<dyn MachineBackend>) { self.backend = backend; }
    pub fn replace_engine(&mut self, engine: EmulationEngine) -> Result<(), BackendCreateError> {
        self.backend = create_backend(engine)?; Ok(())
    }
    pub fn engine(&self) -> EmulationEngine { self.backend.engine() }
    pub fn family(&self) -> BackendFamily { self.backend.family() }
    pub fn capabilities(&self) -> BackendCapabilities { self.backend.capabilities() }
    pub fn execution_model(&self) -> BackendExecutionModel { self.backend.execution_model() }

    fn rust_call<T>(result: BackendResult<T>) -> T {
        result.unwrap_or_else(|error| panic!("selected Rust backend operation failed: {error}"))
    }

    // Transitional AltairMachine-compatible facade. Inherent methods take
    // precedence over Deref methods, ensuring anything that can execute or
    // reconfigure the CPU is routed through the selected backend.
    pub fn configure_memory(&mut self, size: RamSize, init: RamInit) {
        Self::rust_call(self.backend.configure_memory(size, init));
    }
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        Self::rust_call(self.backend.configure_serial_board(board));
    }
    pub fn serial_board(&mut self) -> SerialBoard {
        Self::rust_call(self.backend.serial_board())
    }
    pub fn power(&mut self, on: bool) { Self::rust_call(self.backend.power(on)); }
    pub fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) {
        Self::rust_call(self.backend.power_with_historical_run_latch(on, historical));
    }
    pub fn set_running(&mut self, run: bool) {
        if run { Self::rust_call(self.backend.run()); } else { Self::rust_call(self.backend.halt()); }
    }
    pub fn run_cycles(&mut self, t_state_budget: u32) {
        Self::rust_call(self.backend.service_execution(t_state_budget));
    }
    pub fn step(&mut self) { Self::rust_call(self.backend.step()); }
    pub fn commit_panel_activity(&mut self, dt: Duration) {
        Self::rust_call(self.backend.commit_panel_activity(dt));
    }
    pub fn assert_run_stop(&mut self, run: bool) {
        Self::rust_call(self.backend.assert_run_stop(run));
    }
    pub fn release_run_stop(&mut self, run: bool) {
        Self::rust_call(self.backend.release_run_stop(run));
    }
    pub fn assert_front_panel_reset(&mut self) { Self::rust_call(self.backend.assert_reset()); }
    pub fn release_front_panel_reset(&mut self) { Self::rust_call(self.backend.release_reset()); }
    pub fn front_panel_reset(&mut self) {
        self.assert_front_panel_reset();
        self.release_front_panel_reset();
    }
    pub fn reset(&mut self) {
        self.front_panel_reset();
        Self::rust_call(self.backend.clear_serial());
    }
    pub fn assert_front_panel_clear(&mut self) { Self::rust_call(self.backend.assert_clear()); }
    pub fn release_front_panel_clear(&mut self) { Self::rust_call(self.backend.release_clear()); }
    pub fn clear_io(&mut self) {
        self.assert_front_panel_clear();
        self.release_front_panel_clear();
    }
    pub fn request_hold(&mut self, hold: bool) { Self::rust_call(self.backend.request_hold(hold)); }
    pub fn examine(&mut self, next: bool) { Self::rust_call(self.backend.panel_examine(next)); }
    pub fn deposit(&mut self, next: bool) { Self::rust_call(self.backend.panel_deposit(next)); }
    pub fn protect_current_board(&mut self, protected: bool) {
        Self::rust_call(self.backend.protect_current_board(protected));
    }
}

/// Temporary migration aid for the two in-process Rust engines. Direct chassis
/// reads (lamps/RAM/UART state and the synchronized CPU mirror) keep legacy UI
/// code compiling while execution is dispatched above. This must be removed
/// before a SIMH engine is exposed through the application UI.
impl Deref for BackendHost {
    type Target = AltairMachine;
    fn deref(&self) -> &Self::Target {
        self.backend
            .rust_machine()
            .expect("BackendHost AltairMachine compatibility view is available only for Rust engines")
    }
}
impl DerefMut for BackendHost {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.backend
            .rust_machine_mut()
            .expect("BackendHost AltairMachine compatibility view is available only for Rust engines")
    }
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
    fn rust_host_facade_dispatches_step_to_cycle_backend() {
        let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080).unwrap();
        host.configure_memory(RamSize::K1, RamInit::Zeroed);
        host.power(true);
        host.front_panel_reset();
        host.bus.load(0, &[0x00]);
        host.step();
        assert_eq!(host.cpu.pc, 1, "legacy CPU field is only the synchronized mirror");
        assert_eq!(host.cpu.cycles, 4);
        assert!(host.wait_led());
    }
}