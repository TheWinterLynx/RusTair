//! Open-SIMH backend support.
//!
//! RusTair ships a validated Windows x64 Open-SIMH runtime bundle. The three
//! runtime binaries are embedded into the Rust executable and materialized only
//! when a SIMH backend is started. `simh_frontpanel.dll` is loaded dynamically,
//! so no Open-SIMH installation or import library is required to compile.

use std::path::{Path, PathBuf};

use super::EmulationEngine;

#[cfg(feature = "simh-ffi")]
mod altair;
#[cfg(feature = "simh-ffi")]
mod altairz80;
#[cfg(feature = "simh-ffi")]
mod ffi;
#[cfg(feature = "simh-ffi")]
mod integration;
#[cfg(feature = "simh-ffi")]
mod machine;
#[cfg(feature = "simh-ffi")]
mod machine_altairz80;
mod profile;
#[cfg(feature = "simh-ffi")]
mod runtime;
#[cfg(feature = "simh-ffi")]
mod serial_bridge;
#[cfg(feature = "simh-ffi")]
mod session;
#[cfg(feature = "simh-ffi")]
mod threaded;

#[cfg(feature = "simh-ffi")]
pub use altair::{ClassicAltairRegisters, set_switch_register};
#[cfg(feature = "simh-ffi")]
pub use altairz80::{AltairZ80CpuMode, AltairZ80Registers, set_altairz80_switch_register_low};
#[cfg(feature = "simh-ffi")]
pub use integration::{create_embedded_backend, embedded_backend_available};
#[cfg(feature = "simh-ffi")]
pub use machine::SimhAltairBackend;
#[cfg(feature = "simh-ffi")]
pub use machine_altairz80::SimhAltairZ80Backend;
pub use profile::ClassicAltairProfile;
#[cfg(feature = "simh-ffi")]
pub use runtime::{
    OPEN_SIMH_UPSTREAM_COMMIT, RUSTAIR_SIMH_BUNDLE_REVISION, SimhRuntimeError,
    SimhRuntimePaths, embedded_altair_launch_config, embedded_altairz80_launch_config,
    prepare_embedded_runtime,
};
#[cfg(feature = "simh-ffi")]
pub use session::{
    SimhLivePanelSample, SimhOperationalState, SimhSession, SimhSessionError,
};
#[cfg(feature = "simh-ffi")]
pub use threaded::SimhThreadedBackend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimhTarget {
    Altair,
    AltairZ80,
}

impl SimhTarget {
    pub const ALL: [Self; 2] = [Self::Altair, Self::AltairZ80];

    pub const fn engine(self) -> EmulationEngine {
        match self {
            Self::Altair => EmulationEngine::SimhAltair,
            Self::AltairZ80 => EmulationEngine::SimhAltairZ80,
        }
    }

    pub const fn executable_stem(self) -> &'static str {
        match self {
            Self::Altair => "altair",
            Self::AltairZ80 => "altairz80",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Altair => "Open SIMH Altair",
            Self::AltairZ80 => "Open SIMH AltairZ80",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimhLaunchConfig {
    pub target: SimhTarget,
    pub executable: PathBuf,
    /// Configure devices/media only. Do not put RUN/GO/BOOT in this file;
    /// execution is owned by the FrontPanel API.
    pub simulator_config: PathBuf,
    pub device_panel_count: usize,
}

impl SimhLaunchConfig {
    pub fn new(
        target: SimhTarget,
        executable: impl Into<PathBuf>,
        simulator_config: impl Into<PathBuf>,
    ) -> Self {
        Self {
            target,
            executable: executable.into(),
            simulator_config: simulator_config.into(),
            device_panel_count: 0,
        }
    }

    pub fn with_device_panels(mut self, count: usize) -> Self {
        self.device_panel_count = count;
        self
    }

    pub fn executable(&self) -> &Path { &self.executable }
    pub fn simulator_config(&self) -> &Path { &self.simulator_config }
}

/// Exact names exported by classic `ALTAIR`'s `cpu_reg[]` table.
pub mod altair_registers {
    pub const PC: &str = "PC";
    pub const A: &str = "A";
    pub const BC: &str = "BC";
    pub const DE: &str = "DE";
    pub const HL: &str = "HL";
    pub const SP: &str = "SP";
    pub const CARRY: &str = "C";
    pub const ZERO: &str = "Z";
    pub const AUX_CARRY: &str = "AC";
    pub const SIGN: &str = "S";
    pub const PARITY: &str = "P";
    pub const INTE: &str = "INTE";
    pub const SWITCH_REGISTER: &str = "SR";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simh_targets_map_to_product_engines() {
        assert_eq!(SimhTarget::Altair.engine(), EmulationEngine::SimhAltair);
        assert_eq!(SimhTarget::AltairZ80.engine(), EmulationEngine::SimhAltairZ80);
    }

    #[test]
    fn launch_config_never_implies_execution() {
        let config = SimhLaunchConfig::new(SimhTarget::Altair, "altair.exe", "rustair-altair.ini");
        assert_eq!(config.device_panel_count, 0);
        assert_eq!(config.target.executable_stem(), "altair");
    }

    #[test]
    fn classic_altair_register_contract_includes_switch_register() {
        assert_eq!(altair_registers::PC, "PC");
        assert_eq!(altair_registers::SWITCH_REGISTER, "SR");
    }
}
