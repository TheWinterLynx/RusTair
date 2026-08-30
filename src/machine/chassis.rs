use std::time::Duration;

use crate::config::{RamBoardProfile, SerialBoard};

use super::{AltairBus, CpuDiagnosticResult, PanelLampSnapshot};

/// CPU-independent physical state of the Altair 8800 chassis.
///
/// This type deliberately owns no processor implementation and no processor
/// registers. Fast keeps the existing `AltairMachine`; Cycle owns this chassis
/// beside its exact processor core. Ownership stays explicit and field-based.
pub struct AltairChassis {
    pub bus: AltairBus,
    pub powered: bool,
    /// Physical Display/Control RUN/STOP R-S latch.
    pub running: bool,
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
    pub fn installed_ram_bytes(&self) -> usize {
        self.bus.installed_ram_bytes()
    }

    pub fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.bus.configure_memory_board_profile(profile);
    }

    pub fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        self.bus.memory_board_profile(address)
    }

    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.bus.arm_basic32_full_memory_probe_guard()
    }

    pub fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) {
        self.bus.begin_cpu_diagnostic_meter(
            name,
            bdos_start,
            bdos_len,
            expected_instructions,
            expected_t_states,
        );
    }

    pub fn take_cpu_diagnostic_result(&mut self) -> Option<CpuDiagnosticResult> {
        self.bus.take_cpu_diagnostic_result()
    }

    /// Select only the physical serial board. Any processor reset caused by a
    /// board change belongs to the backend that owns that processor core.
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        if self.bus.serial_board() == board {
            return;
        }
        self.bus.configure_serial_board(board);
        self.bus.clear_transient_memory_guards();
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.bus.serial_board()
    }

    pub fn release_run_stop(&mut self, run: bool) {
        if run {
            self.run_switch_asserted = false;
        } else {
            self.stop_switch_asserted = false;
        }
    }

    pub fn assert_front_panel_clear(&mut self) {
        if !self.powered {
            return;
        }
        self.bus.set_ext_clear(true);
    }

    pub fn release_front_panel_clear(&mut self) {
        if !self.powered {
            return;
        }
        self.bus.set_ext_clear(false);
    }

    pub fn current_board_protected(&self) -> bool {
        self.powered && self.bus.s100.signals().prot
    }

    pub fn panel_switches(&self) -> u16 {
        self.bus.panel_switches()
    }

    pub fn toggle_sense_switch(&mut self, bit: usize) {
        self.bus.toggle_panel_switch(bit);
    }

    pub fn address_leds(&self) -> u16 {
        self.bus.panel_address()
    }

    pub fn data_leds(&self) -> u8 {
        self.bus.panel_data()
    }

    pub fn panel_lamps(&self) -> PanelLampSnapshot {
        self.bus.panel_lamps()
    }

    pub fn wait_led(&self) -> bool {
        self.powered && self.bus.s100.signals().wait
    }

    pub fn ext_clear_asserted(&self) -> bool {
        self.powered && self.bus.ext_clear_asserted()
    }

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
