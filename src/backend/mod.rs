//! Emulator-engine abstraction used by the application/front panel.
//!
//! Product engines:
//! - RusTair fast Intel 8080
//! - RusTair cycle-accurate Intel 8080
//! - Open SIMH Altair
//! - Open SIMH AltairZ80

mod native;
pub mod simh;

use std::fmt;
use std::time::Duration;

use crate::config::SerialBoard;
use crate::machine::PanelLampSnapshot;

pub use native::NativeMachineBackend;
pub type FastMachineBackend = NativeMachineBackend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendFamily {
    Rustair,
    Simh,
}

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
        matches!(self, Self::RustFast8080)
    }
}

impl Default for EmulationEngine {
    fn default() -> Self { Self::RustFast8080 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendExecutionModel {
    HostDriven,
    ExternalProcess,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendSerialPort {
    Port0,
    Port1,
}

impl BackendSerialPort {
    pub const ALL: [Self; 2] = [Self::Port0, Self::Port1];
}

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
pub struct CpuState {
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

impl CpuState {
    #[inline]
    pub const fn bc(self) -> u16 { ((self.b as u16) << 8) | self.c as u16 }
    #[inline]
    pub const fn de(self) -> u16 { ((self.d as u16) << 8) | self.e as u16 }
    #[inline]
    pub const fn hl(self) -> u16 { ((self.h as u16) << 8) | self.l as u16 }
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendError {
    Unsupported { operation: &'static str, engine: EmulationEngine },
    Operation { operation: &'static str, detail: String },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { operation, engine } => {
                write!(f, "{} does not support {operation}", engine.label())
            }
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
    fn write_memory(
        &mut self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> BackendResult<bool>;
    fn load_bytes(&mut self, address: u16, bytes: &[u8]) -> BackendResult<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendCreateError {
    Unavailable(EmulationEngine),
}

impl fmt::Display for BackendCreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(engine) => {
                write!(f, "{} backend is not available in this build", engine.label())
            }
        }
    }
}

impl std::error::Error for BackendCreateError {}

pub fn create_backend(
    engine: EmulationEngine,
) -> Result<Box<dyn MachineBackend>, BackendCreateError> {
    match engine {
        EmulationEngine::RustFast8080 => Ok(Box::new(NativeMachineBackend::default())),
        EmulationEngine::RustCycleAccurate8080
        | EmulationEngine::SimhAltair
        | EmulationEngine::SimhAltairZ80 => Err(BackendCreateError::Unavailable(engine)),
    }
}

pub struct BackendHost {
    backend: Box<dyn MachineBackend>,
}

impl Default for BackendHost {
    fn default() -> Self { Self::rust_fast() }
}

impl BackendHost {
    pub fn new(backend: Box<dyn MachineBackend>) -> Self { Self { backend } }
    pub fn from_engine(engine: EmulationEngine) -> Result<Self, BackendCreateError> {
        create_backend(engine).map(Self::new)
    }
    pub fn rust_fast() -> Self {
        Self::from_engine(EmulationEngine::RustFast8080)
            .expect("the built-in fast backend must always be available")
    }
    pub fn native() -> Self { Self::rust_fast() }
    pub fn backend(&self) -> &dyn MachineBackend { self.backend.as_ref() }
    pub fn backend_mut(&mut self) -> &mut dyn MachineBackend { self.backend.as_mut() }
    pub fn replace(&mut self, backend: Box<dyn MachineBackend>) { self.backend = backend; }
    pub fn replace_engine(&mut self, engine: EmulationEngine) -> Result<(), BackendCreateError> {
        self.backend = create_backend(engine)?;
        Ok(())
    }
    pub fn engine(&self) -> EmulationEngine { self.backend.engine() }
    pub fn family(&self) -> BackendFamily { self.backend.family() }
    pub fn capabilities(&self) -> BackendCapabilities { self.backend.capabilities() }
    pub fn execution_model(&self) -> BackendExecutionModel { self.backend.execution_model() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_product_engines_have_the_expected_family() {
        assert_eq!(EmulationEngine::RustFast8080.family(), BackendFamily::Rustair);
        assert_eq!(EmulationEngine::RustCycleAccurate8080.family(), BackendFamily::Rustair);
        assert_eq!(EmulationEngine::SimhAltair.family(), BackendFamily::Simh);
        assert_eq!(EmulationEngine::SimhAltairZ80.family(), BackendFamily::Simh);
    }

    #[test]
    fn engine_catalog_contains_four_choices_but_only_fast_is_wired_here() {
        assert_eq!(EmulationEngine::ALL.len(), 4);
        assert!(EmulationEngine::RustFast8080.is_available());
        assert!(!EmulationEngine::RustCycleAccurate8080.is_available());
        assert!(!EmulationEngine::SimhAltair.is_available());
        assert!(!EmulationEngine::SimhAltairZ80.is_available());
    }

    #[test]
    fn unavailable_engine_does_not_replace_active_backend() {
        let mut host = BackendHost::default();
        assert_eq!(
            host.replace_engine(EmulationEngine::RustCycleAccurate8080),
            Err(BackendCreateError::Unavailable(EmulationEngine::RustCycleAccurate8080))
        );
        assert_eq!(host.engine(), EmulationEngine::RustFast8080);
    }

    #[test]
    fn fast_backend_is_host_driven() {
        let host = BackendHost::default();
        assert_eq!(host.execution_model(), BackendExecutionModel::HostDriven);
    }
}
