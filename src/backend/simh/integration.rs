use crate::backend::{BackendError, BackendHost, EmulationEngine, MachineBackend};

use super::SimhThreadedBackend;

pub fn embedded_backend_available(engine: EmulationEngine) -> bool {
    cfg!(windows)
        && matches!(engine, EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80)
}

pub fn create_embedded_backend(
    engine: EmulationEngine,
) -> Result<Box<dyn MachineBackend>, BackendError> {
    match engine {
        EmulationEngine::SimhAltair | EmulationEngine::SimhAltairZ80 => {
            Ok(Box::new(SimhThreadedBackend::new(engine)?))
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
    /// Product SIMH runs behind a dedicated worker thread. The egui thread only
    /// reads cached snapshots and queues commands, so FrontPanel/TMXR latency can
    /// never stall repaint/input handling. The selected backend remains POWER
    /// OFF until the user operates the POWER switch.
    pub fn replace_embedded_simh(&mut self, engine: EmulationEngine) -> Result<(), BackendError> {
        let backend = create_embedded_backend(engine)?;
        self.backend = backend;
        Ok(())
    }
}
