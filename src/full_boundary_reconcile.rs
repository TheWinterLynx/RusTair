use crate::cpu8080_cycle::Cpu8080Pins;
use crate::s100_runtime::S100RuntimeFabric;

/// Host-side reconciliation used only when Cycle Full rejoins the live S-100
/// fabric. Full has already simulated the elapsed 8080 T-states and the CPU
/// board's 8212 status-latch state, but deliberately did not replay every
/// connector delta through the runtime fabric. Importing that retained board
/// state here consumes zero emulated time.
///
/// The temporary package states below are never resolved onto S-100: they are
/// used only to drive the existing CPU-board latch implementation to the exact
/// state Full already reached. The final cached connector drive is the real
/// dead-time boundary. If UART time is settled before the next Partial edge,
/// other cards therefore see only that legitimate final boundary state, never
/// the host-side reconciliation sequence.
pub(crate) trait FullCpuBoundaryReconcile {
    fn reconcile_full_cpu_boundary(&mut self, boundary_pins: Cpu8080Pins, latched_status_word: u8);
}

impl FullCpuBoundaryReconcile for S100RuntimeFabric {
    fn reconcile_full_cpu_boundary(&mut self, boundary_pins: Cpu8080Pins, latched_status_word: u8) {
        debug_assert!(!boundary_pins.phi1 && !boundary_pins.phi2);
        debug_assert!(!boundary_pins.sync && !boundary_pins.dbin);

        if self.cpu_latched_status_word() != latched_status_word {
            // Exercise only the CPU-board handle. set_cpu_package_pins updates
            // its cached connector drive but does not resolve the backplane, so
            // this is state import rather than an observable clock edge.
            let mut latch = boundary_pins;
            latch.phi1 = true;
            latch.phi2 = false;
            latch.sync = true;
            latch.data_out = Some(latched_status_word);
            self.set_cpu_package_pins(latch);
        }

        // Every completed 8080 T-state ends after PHI2 falling; CLOC is therefore
        // low at a Full instruction boundary. Drive that internal retained phase
        // before publishing the final dead-time package pins. Again, no resolver
        // runs between these two host-side state-import steps.
        let mut phi2 = boundary_pins;
        phi2.phi1 = false;
        phi2.phi2 = true;
        phi2.sync = false;
        self.set_cpu_package_pins(phi2);
        self.set_cpu_package_pins(boundary_pins);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RamInit, S100HardwareConfig};
    use crate::s100::S100Signal;

    #[test]
    fn full_boundary_reconcile_imports_8212_state_before_next_real_settle() {
        let mut fabric = S100RuntimeFabric::new(S100HardwareConfig::default(), RamInit::Zeroed)
            .expect("default S-100 chassis");
        let boundary = Cpu8080Pins {
            address: Some(0x0010),
            wr_n: true,
            ..Cpu8080Pins::default()
        };

        fabric.reconcile_full_cpu_boundary(boundary, 0xa2);
        assert_eq!(fabric.cpu_latched_status_word(), 0xa2);

        // Reconciliation changes the CPU board's cached connector drive only.
        // The ordinary resolver is still the sole path by which another card
        // observes the imported status.
        fabric
            .settle(
                crate::s100_runtime::DisplayControlLines {
                    ready: true,
                    run: true,
                    ..Default::default()
                },
                &[],
            )
            .unwrap();
        assert_eq!(
            fabric.sample().signal_level(S100Signal::MemoryRead),
            Some(true)
        );
        assert_eq!(fabric.sample().signal_level(S100Signal::M1), Some(true));
    }
}
