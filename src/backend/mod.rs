//! Emulator-engine abstraction used by the application/front panel.
//!
//! The product-level selector distinguishes four engines:
//! - RusTair fast Intel 8080
//! - RusTair cycle-accurate Intel 8080
//! - Open SIMH Altair
//! - Open SIMH AltairZ80
//!
//! Only the fast RusTair engine is wired on this branch. The cycle-accurate
//! implementation is intentionally developed on its own branch and can be
//! wrapped here after both branches are merged. SIMH engines will likewise sit
//! behind this contract without leaking FFI types into the UI.

mod native;

use std::fmt;
use std::time::Duration;

use crate::config::SerialBoard;
use crate::machine::PanelLampSnapshot;

pub use native::NativeMachineBackend;
/// Product-facing name for the existing instruction-level native backend.
/// `NativeMachineBackend` remains exported during the migration so existing
/// branch work does not need a noisy rename-only conflict.
pub type FastMachineBackend = NativeMachineBackend;

/// Broad implementation family. Useful for diagnostics and configuration, but
/// deliberately not used as the product-level engine selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendFamily {
    Rustair,
    Simh,
}

/// Concrete emulator engine visible to the product/UI.
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

    /// Whether this branch currently has a concrete backend constructor for the
    /// engine. Keeping availability explicit lets the UI expose the final
    /// product shape without pretending unfinished engines are selectable.
    pub const fn is_available(self) -> bool {
        matches!(self, Self::RustFast8080)
    }
}

impl Default for EmulationEngine {
    fn default() -> Self { Self::RustFast8080 }
}

/// Logical serial channel exposed by the installed MITS serial board. Port 1 is
/// meaningful for 88-2SIO and intentionally remains addressable in the backend
/// contract so external endpoints do not need to know implementation details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendSerialPort {
    Port0,
    Port1,
}

impl BackendSerialPort {
    pub const ALL: [Self; 2] = [Self::Port0, Self::Port1];
}

/// Feature set exposed by one engine. The UI must query capabilities instead of
/// assuming that every backend can reproduce every physical Altair operation.
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

/// Backend-neutral Intel 8080 programmer-visible state.
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
    pub halted: bool,
    /// Total Intel 8080 T-states executed. The existing fast core historically
    /// calls this counter `cycles`; the abstraction uses the precise term so it
    /// matches the cycle-accurate core and future comparison tooling.
    pub total_t_states: u64,
}

impl CpuState {
    #[inline]
    pub const fn bc(self) -> u16 { ((self.b as u16) << 8) | self.c as u16 }

    #[inline]
    pub const fn de(self) -> u16 { ((self.d as u16) << 8) | self.e as u16 }

    #[inline]
    pub const fn hl(self) -> u16 { ((self.h as u16) << 8) | self.l as u16 }
}

/// State required to render and operate the Altair front panel without knowing
/// which emulator engine produced it.
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

/// Common machine-level contract between both RusTair CPU implementations and
/// the two Open SIMH targets.
///
/// Fast-vs-cycle-accurate remains an implementation detail inside the native
/// Altair machine layer: the fast core advances whole instructions, while the
/// cycle core advances physical T-states/pins. Both must nevertheless expose
/// the same machine/front-panel semantics here.
pub trait MachineBackend {
    fn engine(&self) -> EmulationEngine;
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;

    fn family(&self) -> BackendFamily { self.engine().family() }

    fn cpu_state(&self) -> CpuState;
    fn front_panel_state(&self) -> FrontPanelState;

    fn power(&mut self, on: bool);
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool);

    fn run(&mut self);
    fn halt(&mut self);
    fn step(&mut self);
    /// Advance by an approximate T-state budget. A cycle-accurate native backend
    /// will consume this one physical T-state at a time; the fast backend may
    /// cross the budget at the final instruction boundary, as it does today.
    fn run_t_states(&mut self, budget: u32);
    fn commit_panel_activity(&mut self, dt: Duration);

    fn assert_run_stop(&mut self, run: bool);
    fn release_run_stop(&mut self, run: bool);
    fn assert_reset(&mut self);
    fn release_reset(&mut self);
    fn assert_clear(&mut self);
    fn release_clear(&mut self);
    fn request_hold(&mut self, hold: bool);

    fn panel_examine(&mut self, next: bool);
    fn panel_deposit(&mut self, next: bool);
    fn protect_current_board(&mut self, protected: bool);

    fn switch_register(&self) -> u16;
    fn set_switch_register(&mut self, value: u16);

    /// Serial-board operations used by the internal terminals and external TCP/
    /// COM endpoints. Keeping these at machine level is essential for SIMH: the
    /// app routes endpoints, while each backend owns its UART implementation.
    fn configure_serial_board(&mut self, board: SerialBoard);
    fn serial_board(&self) -> SerialBoard;
    fn serial_receive(&mut self, port: BackendSerialPort, byte: u8);
    fn serial_rx_empty(&self, port: BackendSerialPort) -> bool;
    fn serial_rx_len(&self, port: BackendSerialPort) -> usize;
    fn serial_tx_busy(&self, port: BackendSerialPort) -> bool;
    fn serial_tx_front(&self, port: BackendSerialPort) -> Option<u8>;
    fn serial_tx_complete(&mut self, port: BackendSerialPort) -> Option<u8>;
    fn clear_serial(&mut self);

    /// Non-invasive debugger-style memory access. These methods are separate
    /// from EXAMINE/DEPOSIT because the latter have visible front-panel/bus
    /// side-effects on a real Altair.
    fn peek_memory(&self, address: u16) -> Option<u8>;
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> bool;

    /// Load a host buffer directly into guest RAM. Used by existing convenience
    /// loaders; SIMH backends can implement this through the front-panel API.
    fn load_bytes(&mut self, address: u16, bytes: &[u8]);
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

/// Single construction point for product engines. Future merge work should wire
/// new backends here instead of teaching the UI how to instantiate them.
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

/// Runtime-owned indirection point. Engine replacement intentionally creates a
/// new machine; live-state migration is a separate concern and is not required
/// for the first four-engine selector.
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

    /// Transitional alias retained while existing code still calls the native
    /// fast engine simply `native`.
    pub fn native() -> Self { Self::rust_fast() }

    pub fn backend(&self) -> &dyn MachineBackend { self.backend.as_ref() }

    pub fn backend_mut(&mut self) -> &mut dyn MachineBackend { self.backend.as_mut() }

    pub fn replace(&mut self, backend: Box<dyn MachineBackend>) {
        self.backend = backend;
    }

    /// Replace the current machine with a freshly-created selected engine. This
    /// deliberately does not attempt live state transfer between engines.
    pub fn replace_engine(
        &mut self,
        engine: EmulationEngine,
    ) -> Result<(), BackendCreateError> {
        self.backend = create_backend(engine)?;
        Ok(())
    }

    pub fn engine(&self) -> EmulationEngine { self.backend.engine() }

    pub fn family(&self) -> BackendFamily { self.backend.family() }

    pub fn capabilities(&self) -> BackendCapabilities { self.backend.capabilities() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_product_engines_have_the_expected_family() {
        assert_eq!(EmulationEngine::RustFast8080.family(), BackendFamily::Rustair);
        assert_eq!(
            EmulationEngine::RustCycleAccurate8080.family(),
            BackendFamily::Rustair
        );
        assert_eq!(EmulationEngine::SimhAltair.family(), BackendFamily::Simh);
        assert_eq!(EmulationEngine::SimhAltairZ80.family(), BackendFamily::Simh);
    }

    #[test]
    fn engine_catalog_contains_the_four_product_choices() {
        assert_eq!(EmulationEngine::ALL.len(), 4);
        assert!(EmulationEngine::RustFast8080.is_available());
        assert!(!EmulationEngine::RustCycleAccurate8080.is_available());
        assert!(!EmulationEngine::SimhAltair.is_available());
        assert!(!EmulationEngine::SimhAltairZ80.is_available());
    }

    #[test]
    fn unavailable_engines_fail_without_replacing_the_active_backend() {
        let mut host = BackendHost::default();
        let error = host
            .replace_engine(EmulationEngine::RustCycleAccurate8080)
            .unwrap_err();
        assert_eq!(
            error,
            BackendCreateError::Unavailable(EmulationEngine::RustCycleAccurate8080)
        );
        assert_eq!(host.engine(), EmulationEngine::RustFast8080);
    }

    #[test]
    fn backend_host_defaults_to_fast_rust_engine() {
        let host = BackendHost::default();
        assert_eq!(host.engine(), EmulationEngine::RustFast8080);
        assert_eq!(host.family(), BackendFamily::Rustair);
        assert_eq!(host.backend().name(), "RusTair fast 8080");
        assert!(host.capabilities().front_panel);
        assert!(!host.capabilities().exact_t_state_timing);
    }
}
