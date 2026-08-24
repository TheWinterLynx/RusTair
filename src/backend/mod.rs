//! Emulator-engine abstraction used by the application/front panel.
//!
//! The first implementation is [`NativeMachineBackend`], which wraps the
//! existing Rust Altair implementation without changing its behaviour.  A SIMH
//! implementation can satisfy the same contract later without making the UI
//! depend on SIMH-specific FFI types.

mod native;

use std::time::Duration;

use crate::machine::PanelLampSnapshot;

pub use native::NativeMachineBackend;

/// Emulator engine selected behind the common backend contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    /// RusTair's built-in Intel 8080/S-100 implementation.
    Native,
    /// Reserved for the Open SIMH-backed implementation.
    Simh,
}

/// Backend-neutral Intel 8080 register snapshot.
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
    pub cycles: u64,
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

/// Common contract between the RusTair-native machine and alternate emulator
/// engines such as Open SIMH.
///
/// It deliberately models front-panel operations rather than exposing concrete
/// CPU/bus structs.  Transitional implementations may still offer an escape
/// hatch to their concrete machine type while the rest of the application is
/// migrated incrementally.
pub trait MachineBackend {
    fn kind(&self) -> BackendKind;
    fn name(&self) -> &'static str;

    fn cpu_state(&self) -> CpuState;
    fn front_panel_state(&self) -> FrontPanelState;

    fn power(&mut self, on: bool);
    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool);

    fn run(&mut self);
    fn halt(&mut self);
    fn step(&mut self);
    fn run_cycles(&mut self, cycles: u32);
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
    /// loaders; a SIMH backend can implement this through its front-panel API.
    fn load_bytes(&mut self, address: u16, bytes: &[u8]);
}
