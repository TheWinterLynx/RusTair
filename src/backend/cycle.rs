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

    /// Cross exactly one electrical T-state while a managed serial OUT barrier
    /// is in progress. Cycle owns the PHI1/PHI2 loop and all resulting S-100
    /// effects; the host only chooses this exact primitive so it can settle the
    /// independent physical serial clock after the activation edge.
    pub(crate) fn service_managed_serial_barrier_t_state(&mut self) -> super::BackendResult<()> {
        let lines = self.machine.bus.cpu_control_lines();
        if !self.machine.powered || !self.machine.running() || lines.reset {
            return self.fail_if_cpu_fault("managed serial barrier");
        }

        let before = self.cpu.total_t_states();
        let ready = self.machine.bus.cycle_front_panel_ready_input();
        let trace = self.tick_once(ready);
        if trace.fault.is_some() {
            return self.fail_if_cpu_fault("managed serial barrier");
        }
        if self.stop_wait_park_pending {
            self.park_physical_stop_at_first_tw();
        }
        let elapsed = self.cpu.total_t_states().saturating_sub(before);
        crate::adaptive_metrics::record_partial_span(
            elapsed,
            crate::adaptive_metrics::AdaptiveFallbackReason::OpcodeBarrier,
        );
        self.fail_if_cpu_fault("managed serial barrier")
    }
}
