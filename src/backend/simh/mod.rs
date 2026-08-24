//! Open SIMH backend support.
//!
//! The default RusTair build keeps this module dependency-free. Enable the
//! `simh-ffi` Cargo feature only when the matching Open-SIMH FrontPanel C
//! objects are linked from the same source revision as the simulator binaries.

use std::path::{Path, PathBuf};

use super::EmulationEngine;

#[cfg(feature = "simh-ffi")]
mod altair;
#[cfg(feature = "simh-ffi")]
mod ffi;
#[cfg(feature = "simh-ffi")]
mod session;

#[cfg(feature = "simh-ffi")]
pub use altair::{ClassicAltairRegisters, set_switch_register};
#[cfg(feature = "simh-ffi")]
pub use session::{SimhOperationalState, SimhSession, SimhSessionError};

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
    /// FrontPanel startup configuration. It may configure devices and attach
    /// media but must not issue RUN/GO/BOOT; execution is controlled via API.
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

/// Register names exported by the classic Open SIMH `ALTAIR` CPU. These are
/// sourced from its `cpu_reg[]` table rather than inferred from monitor output.
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
    fn simh_targets_map_to_the_product_engines() {
        assert_eq!(SimhTarget::Altair.engine(), EmulationEngine::SimhAltair);
        assert_eq!(
            SimhTarget::AltairZ80.engine(),
            EmulationEngine::SimhAltairZ80
        );
    }

    #[test]
    fn launch_config_never_implies_execution() {
        let config = SimhLaunchConfig::new(
            SimhTarget::Altair,
            "altair.exe",
            "rustair-altair.ini",
        );
        assert_eq!(config.device_panel_count, 0);
        assert_eq!(config.target.executable_stem(), "altair");
    }

    #[test]
    fn classic_altair_register_contract_includes_switch_register() {
        assert_eq!(altair_registers::PC, "PC");
        assert_eq!(altair_registers::SWITCH_REGISTER, "SR");
    }
}
