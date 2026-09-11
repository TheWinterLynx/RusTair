//! Cycle-accurate Intel 8080 backend dispatcher.
//!
//! `partial_impl.rs` is the existing edge-by-edge electrical oracle. `full.rs`
//! adds a MAME-style whole-instruction executor only for chassis/instructions
//! whose intervening electrical states are proven not to affect installed
//! hardware. Both operate on the same CPU state, Altair chassis and S-100 cards.

mod full;

include!("cycle/partial_impl.rs");

impl CycleAccurateMachineBackend {
    /// Mount one validated physical S-100 inventory directly on this Cycle
    /// backend. This is the concrete-backend counterpart of the host wrapper's
    /// configuration boundary and is primarily useful for exact/adaptive oracle
    /// comparisons that must share the same live chassis without introducing a
    /// second machine implementation.
    pub fn configure_s100_hardware(
        &mut self,
        hardware: crate::config::S100HardwareConfig,
        init: crate::config::RamInit,
    ) -> super::BackendResult<()> {
        if self.machine.powered {
            return Err(super::BackendError::Operation {
                operation: "configure S-100 hardware",
                detail: "POWER OFF is required to move physical S-100 cards".into(),
            });
        }
        self.machine
            .bus
            .configure_s100_hardware_memory(hardware, init)
            .map_err(|error| super::BackendError::Operation {
                operation: "configure S-100 hardware",
                detail: format!("{error:?}"),
            })?;
        self.last_teaching_tick = None;
        self.stop_wait_park_pending = false;
        self.cpu_fault = None;
        Ok(())
    }
}
