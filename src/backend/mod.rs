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

use std::time::Duration;

use crate::machine::PanelLampSnapshot;

pub use native::NativeMachineBackend;

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

    /// Non-invasive debugger-style memory access. These methods are separate
    /// from EXAMINE/DEPOSIT because the latter have visible front-panel/bus
    /// side-effects on a real Altair.
    fn peek_memory(&self, address: u16) -> Option<u8>;
    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> bool;

    /// Load a host buffer directly into guest RAM. Used by existing convenience
    /// loaders; SIMH backends can implement this through the front-panel API.
    fn load_bytes(&mut self, address: u16, bytes: &[u8]);
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

    pub fn rust_fast() -> Self {
        Self::new(Box::new(NativeMachineBackend::default()))
    }

    /// Transitional alias retained while existing code still calls the native
    /// fast engine simply `native`.
    pub fn native() -> Self { Self::rust_fast() }

    pub fn backend(&self) -> &dyn MachineBackend { self.backend.as_ref() }

    pub fn backend_mut(&mut self) -> &mut dyn MachineBackend { self.backend.as_mut() }

    pub fn replace(&mut self, backend: Box<dyn MachineBackend>) {
        self.backend = backend;
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
    fn backend_host_defaults_to_fast_rust_engine() {
        let host = BackendHost::default();
        assert_eq!(host.engine(), EmulationEngine::RustFast8080);
        assert_eq!(host.family(), BackendFamily::Rustair);
        assert_eq!(host.backend().name(), "RusTair fast 8080");
        assert!(host.capabilities().front_panel);
        assert!(!host.capabilities().exact_t_state_timing);
    }
}
