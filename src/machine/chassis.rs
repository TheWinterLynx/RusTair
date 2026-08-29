use std::time::Duration;

use crate::config::{RamBoardProfile, SerialBoard};

use super::{AltairBus, CpuDiagnosticResult, PanelLampSnapshot};

/// CPU-independent physical Altair 8800 chassis state.
///
/// This object owns the S-100 bus, RAM and I/O cards, front-panel state and the
/// physical RUN/STOP switch/latch state. It deliberately contains no processor
/// implementation or processor register state. Fast and Cycle backends attach
/// their own CPU authorities to the same chassis model.
pub struct AltairChassis {
    pub bus: AltairBus,
    pub powered: bool,
    /// Physical Display/Control RUN/STOP R-S latch, not merely host execution.
    pub running: bool,
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

    pub fn clear_io(&mut self) {
        if !self.powered {
            return;
        }
        self.assert_front_panel_clear();
        self.release_front_panel_clear();
    }

    /// Chassis-only serial-card replacement. CPU reset semantics belong to the
    /// selected backend and are deliberately not performed here.
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        if self.bus.serial_board() == board {
            return;
        }
        self.running = false;
        self.bus.set_run(false);
        self.bus.configure_serial_board(board);
        self.bus.clear_transient_memory_guards();
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.bus.serial_board()
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

    /// Power the physical chassis from a CPU-owned power-on sample. No CPU
    /// object is created or touched here.
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

    /// RESET assertion for a backend whose processor state lives elsewhere.
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

    /// RESET release uses only guaranteed 8080 reset outputs (PC=0000h,
    /// INTE=0). Processor registers are not present in the chassis.
    pub(crate) fn cycle_release_front_panel_reset_from_cpu(&mut self) {
        if !self.powered {
            return;
        }
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(false);
        self.bus.set_hlda(false);
        self.bus.release_front_panel_reset_bus(address, self.running);
    }

    pub(crate) fn cycle_commit_panel_activity(&mut self, dt: Duration, cpu_halted: bool) {
        let dynamic = self.powered
            && self.running
            && !cpu_halted
            && !self.bus.hlda()
            && !self.bus.reset_asserted();
        self.bus.commit_panel_activity(dt, dynamic);
    }

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

    /// Cycle-accurate RUN-latch mutation. READY is a chassis input to the CPU;
    /// WAIT remains a CPU output and is changed only by an exact CPU sample.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chassis_contains_no_cpu_authority_and_preserves_cycle_control_state() {
        let mut chassis = AltairChassis::default();
        chassis.cycle_power_chassis(true, false, 0x1234, false);
        assert!(chassis.powered);
        assert!(!chassis.running);
        assert_eq!(chassis.address_leds(), 0x1234);

        chassis.cycle_assert_front_panel_reset_from_cpu();
        assert!(chassis.bus.cpu_control_lines().reset);
        chassis.cycle_release_front_panel_reset_from_cpu();
        assert!(!chassis.bus.cpu_control_lines().reset);
        assert_eq!(chassis.address_leds(), 0x0000);
    }
}
