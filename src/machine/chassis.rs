use std::time::Duration;

use super::AltairBus;

/// CPU-independent physical state of the Altair 8800 chassis.
///
/// This type deliberately owns no processor implementation and no processor
/// registers. During the migration the Fast `AltairMachine` remains untouched;
/// Cycle will eventually own this chassis beside its exact processor core.
/// Ownership stays explicit: no deref-based compatibility layer is used.
pub(crate) struct AltairChassis {
    pub(crate) bus: AltairBus,
    pub(crate) powered: bool,
    /// Physical Display/Control RUN/STOP R-S latch.
    pub(crate) running: bool,
    stop_switch_asserted: bool,
    run_switch_asserted: bool,
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

impl AltairChassis {
    /// Power only physical chassis/S-100 state. Processor power-on state belongs
    /// to the backend-owned CPU core and is supplied here only as bus-visible
    /// address/INTE inputs.
    pub(crate) fn cycle_power_chassis(
        &mut self,
        on: bool,
        run: bool,
        cpu_address: u16,
        cpu_inte: bool,
    ) {
        self.bus.cancel_cpu_diagnostic_meter();
        self.powered = on;
        self.stop_switch_asserted = false;
        self.run_switch_asserted = false;

        if on {
            self.bus.clear_protection();
            self.bus.clear_transient_memory_guards();
            self.bus.clear_serial();
            self.running = run;
            self.bus.set_run(run);
            self.bus.sync_cpu_inte(cpu_inte);
            self.bus.set_hlda(false);
            self.bus.panel.set_address_latch(cpu_address);
            self.bus.drive_power_on_state(cpu_address, run);
        } else {
            self.running = false;
            self.bus.clear_serial();
            self.bus.initialize_memory();
            self.bus.power_off_s100();
        }
    }

    /// Assert physical RESET. Processor registers are deliberately outside this
    /// type and must be reset by the backend-owned CPU core.
    pub(crate) fn cycle_assert_front_panel_reset_from_cpu(&mut self) {
        if !self.powered {
            return;
        }
        self.bus.cancel_cpu_diagnostic_meter();
        self.bus.clear_transient_memory_guards();
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(false);
        self.bus.set_hlda(false);
        self.bus.assert_front_panel_reset_bus(self.running);
    }

    /// Release physical RESET using the processor guarantees visible after
    /// reset (address 0000h and INTE low), without consulting another CPU core.
    pub(crate) fn cycle_release_front_panel_reset_from_cpu(&mut self) {
        if !self.powered {
            return;
        }
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(false);
        self.bus.set_hlda(false);
        self.bus.release_front_panel_reset_bus(address, self.running);
    }

    /// Integrate optical lamp persistence from chassis state plus the exact
    /// core's externally supplied HALT truth.
    pub(crate) fn cycle_commit_panel_activity(&mut self, dt: Duration, cpu_halted: bool) {
        let dynamic = self.powered
            && self.running
            && !cpu_halted
            && !self.bus.hlda()
            && !self.bus.reset_asserted();
        self.bus.commit_panel_activity(dt, dynamic);
    }

    /// PROTECT/UNPROTECT is a front-panel/memory-board operation. Exact HALT and
    /// HOLD truth are supplied by the backend rather than inferred from another
    /// processor implementation.
    pub(crate) fn cycle_front_panel_set_memory_protection(
        &mut self,
        protected: bool,
        cpu_halted: bool,
        cpu_holding: bool,
    ) {
        if !self.powered
            || self.running
            || self.bus.reset_asserted()
            || self.bus.hold_requested()
            || cpu_halted
            || cpu_holding
        {
            return;
        }

        let address = self.bus.panel_address();
        self.bus.set_protected(address, protected);
        self.bus.freeze_panel_bus();
    }

    /// Cycle-accurate RUN latch mutation. READY follows the Display/Control
    /// board; WAIT remains an output of the exact processor sample.
    pub(crate) fn cycle_set_running(&mut self, run: bool) {
        if !self.powered || self.bus.reset_asserted() {
            return;
        }
        self.running = run;
        self.bus.set_run(run);
        self.bus.cycle_set_ready_input(run);
        if !run {
            let address = self.bus.panel_address();
            self.bus.panel.set_address_latch(address);
        }
    }

    fn cycle_set_run_latch_during_reset(&mut self) {
        debug_assert!(self.powered && self.bus.reset_asserted());
        self.running = true;
        self.bus.set_run(true);
        self.bus.cycle_set_ready_input(true);
    }

    /// Cycle-accurate RUN/STOP entry point. STOP records the physical switch
    /// level but does not clear RUN while HLT/HLDA suppresses PSYNC.
    pub(crate) fn cycle_assert_run_stop(
        &mut self,
        run: bool,
        cpu_halted: bool,
        cpu_holding: bool,
    ) {
        if !self.powered {
            return;
        }
        self.run_switch_asserted = run;
        self.stop_switch_asserted = !run;

        if run {
            if self.bus.reset_asserted() {
                self.cycle_set_run_latch_during_reset();
            } else {
                self.cycle_set_running(true);
            }
        } else if !self.bus.reset_asserted() && !cpu_halted && !cpu_holding {
            self.cycle_set_running(false);
        }
    }

    /// A STOP held while HLT/HLDA suppressed PSYNC becomes effective at the
    /// first real synchronization opportunity after the processor can drive it.
    pub(crate) fn cycle_capture_pending_stop_at_psync(&mut self) -> bool {
        if self.powered
            && self.running
            && self.stop_switch_asserted
            && !self.bus.reset_asserted()
        {
            self.cycle_set_running(false);
            true
        } else {
            false
        }
    }
}
