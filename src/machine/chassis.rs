use super::AltairBus;

/// CPU-independent physical state of the Altair 8800 chassis.
///
/// This type deliberately owns no processor implementation and no processor
/// registers. It is intended to become the chassis owned by the cycle-accurate
/// backend while the existing `AltairMachine` remains untouched for Fast during
/// the migration. Keeping the first stage data-only avoids coupling the chassis
/// extraction to CPU ownership, execution, RESET or front-panel sequencing.
pub(super) struct AltairChassis {
    pub(super) bus: AltairBus,
    pub(super) powered: bool,
    /// Physical Display/Control RUN/STOP R-S latch.
    pub(super) running: bool,
    pub(super) stop_switch_asserted: bool,
    pub(super) run_switch_asserted: bool,
}

impl Default for AltairChassis {
    fn default() -> Self {
        Self {
            bus: AltairBus::default(),
            powered: false,
            running: false,
            stop_switch_asserted: false,
            run_switch_asserted: false,
        }
    }
}
