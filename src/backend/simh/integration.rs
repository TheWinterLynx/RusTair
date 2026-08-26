use crate::backend::{BackendError, BackendHost, EmulationEngine, MachineBackend};

use super::{
    AltairZ80CpuMode, SimhAltairBackend, SimhAltairZ80Backend,
    embedded_altair_launch_config, embedded_altairz80_launch_config,
};

pub fn embedded_backend_available(engine: EmulationEngine) -> bool {
    cfg!(windows)
        && matches!(engine, EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80)
}

pub fn create_embedded_backend(
    engine: EmulationEngine,
) -> Result<Box<dyn MachineBackend>, BackendError> {
    match engine {
        EmulationEngine::SimhAltair => {
            let launch = embedded_altair_launch_config().map_err(|error| BackendError::Operation {
                operation: "prepare embedded SIMH classic Altair",
                detail: error.to_string(),
            })?;
            Ok(Box::new(SimhAltairBackend::new_unpowered(launch)?))
        }
        EmulationEngine::SimhAltairZ80 => {
            let mode = AltairZ80CpuMode::Intel8080;
            let launch = embedded_altairz80_launch_config(mode).map_err(|error| {
                BackendError::Operation {
                    operation: "prepare embedded SIMH AltairZ80",
                    detail: error.to_string(),
                }
            })?;
            Ok(Box::new(SimhAltairZ80Backend::new_unpowered_with_serial(
                launch, mode,
            )?))
        }
        _ => Err(BackendError::Unsupported {
            operation: "embedded Open-SIMH backend creation",
            engine,
        }),
    }
}

impl BackendHost {
    /// Replace the selected backend with RusTair's embedded Open-SIMH runtime.
    ///
    /// This is intentionally separate from the built-in Rust backend factory:
    /// Open-SIMH is Windows-only and may fail at runtime (for example if Windows
    /// refuses to materialize or load the bundled DLL). The new backend remains
    /// POWER OFF until the user operates the POWER switch.
    pub fn replace_embedded_simh(&mut self, engine: EmulationEngine) -> Result<(), BackendError> {
        let backend = create_embedded_backend(engine)?;
        self.backend = backend;
        Ok(())
    }
}
