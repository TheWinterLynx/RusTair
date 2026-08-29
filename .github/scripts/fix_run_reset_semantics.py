from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected RUN/RESET anchor not found in {path}: {old[:180]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# ---------------------------------------------------------------------------
# Original 8800 D/C board: RESET does not clear the RUN/STOP R-S latch.
# RUN sets the latch asynchronously; STOP resets it only at the qualified
# PSYNC/D05/phi2 boundary. PRDY is RUN + SS + EXM + EXM NXT, so RESET itself
# does not force the front-panel READY contribution low when RUN is already set.
# ---------------------------------------------------------------------------
replace_once(
    "src/machine/panel_bus.rs",
    '''    pub(super) fn assert_front_panel_reset(&mut self) {
        self.signals.reset = true;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = 0xffff;
        self.signals.data_in = Some(0xff);
        self.signals.data_out = None;
        self.signals.cpu_data = None;
        self.signals.panel_data = 0xff;
        self.signals.inte = false;
        self.signals.prot = false;
        self.signals.clear_status();
        self.signals.front_panel_ready = false;
        self.signals.memory_ready = true;
        self.signals.ready = false;
        self.signals.wait = false;
        self.signals.hlda = false;
        self.lamps.freeze(&self.signals);
    }''',
    '''    pub(super) fn assert_front_panel_reset(&mut self, run: bool) {
        self.signals.reset = true;
        self.signals.owner = BusOwner::FrontPanel;
        self.signals.address = 0xffff;
        self.signals.data_in = Some(0xff);
        self.signals.data_out = None;
        self.signals.cpu_data = None;
        self.signals.panel_data = 0xff;
        self.signals.inte = false;
        self.signals.prot = false;
        self.signals.clear_status();
        // PRESET/RESET belongs to the processor input path and does not clear
        // the original Display/Control RUN/STOP R-S latch. PRDY therefore still
        // follows RUN while RESET is physically held.
        self.signals.run = run;
        self.signals.front_panel_ready = run;
        self.signals.memory_ready = true;
        self.signals.ready = run;
        // WAIT is an 8080 output and is inactive while RESET is asserted.
        self.signals.wait = false;
        self.signals.hlda = false;
        self.lamps.freeze(&self.signals);
    }''',
)

replace_once(
    "src/machine/mod.rs",
    '''    fn assert_front_panel_reset_bus(&mut self) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
        self.s100.assert_front_panel_reset();
    }''',
    '''    fn assert_front_panel_reset_bus(&mut self, run: bool) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
        self.s100.assert_front_panel_reset(run);
    }''',
)

replace_once(
    "src/machine/mod.rs",
    '''    pub fn assert_run_stop(&mut self, run: bool) {
        if !self.powered { return; }
        self.run_switch_asserted = run;
        self.stop_switch_asserted = !run;

        if run {
            if !self.bus.reset_asserted() {
                self.set_running(true);
            }
        } else if self.bus.reset_asserted() || !self.cpu.halted {
            self.set_running(false);
        }
    }''',
    '''    pub fn assert_run_stop(&mut self, run: bool) {
        if !self.powered { return; }
        self.run_switch_asserted = run;
        self.stop_switch_asserted = !run;

        if run {
            if self.bus.reset_asserted() {
                // On the original D/C board RUN is the asynchronous SET input
                // of the R-S latch. RESET does not gate it; the processor simply
                // remains held in RESET until PRESET is released.
                self.running = true;
                self.bus.set_run(true);
                self.bus.set_ready(true);
            } else {
                self.set_running(true);
            }
        } else if !self.bus.reset_asserted() && !self.cpu.halted {
            // STOP needs a processor synchronization opportunity. While RESET
            // is held there is no qualifying post-reset fetch yet, so retain the
            // RUN latch and capture STOP when RESET is released.
            self.set_running(false);
        }
    }''',
)

replace_once(
    "src/machine/mod.rs",
    '''    pub fn assert_front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.bus.cancel_cpu_diagnostic_meter();
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        if self.stop_switch_asserted {
            self.running = false;
            self.bus.set_run(false);
        }
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.assert_front_panel_reset_bus();
    }

    pub fn release_front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.cpu.reset();
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.release_front_panel_reset_bus(address, self.running);
    }''',
    '''    pub fn assert_front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.bus.cancel_cpu_diagnostic_meter();
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        // RESET clears processor state but deliberately preserves the physical
        // RUN/STOP latch. A pending STOP is captured only after RESET release.
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.assert_front_panel_reset_bus(self.running);
    }

    fn release_front_panel_reset_common(&mut self, fast_capture_pending_stop: bool) {
        if !self.powered { return; }
        self.cpu.reset();
        if fast_capture_pending_stop && self.stop_switch_asserted {
            // The instruction-level backend cannot expose the first post-reset
            // PSYNC. Approximate that exact boundary here, not while RESET is held.
            self.running = false;
            self.bus.set_run(false);
        }
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.release_front_panel_reset_bus(address, self.running);
    }

    pub fn release_front_panel_reset(&mut self) {
        self.release_front_panel_reset_common(true);
    }

    pub(crate) fn cycle_release_front_panel_reset(&mut self) {
        self.release_front_panel_reset_common(false);
    }''',
)

# Cycle Accurate needs a separate latch mutation under RESET: update RUN and
# the D/C-board PRDY contribution but do not invent any CPU WAIT transition.
replace_once(
    "src/machine/cpu_board.rs",
    '''    pub(crate) fn cycle_set_running(&mut self, run: bool) {
        if !self.powered || self.bus.reset_asserted() { return; }
        self.running = run;
        self.bus.set_run(run);
        self.bus.cycle_set_ready_input(run);
        if !run {
            let address = self.bus.panel_address();
            self.bus.panel.set_address_latch(address);
        }
    }''',
    '''    pub(crate) fn cycle_set_running(&mut self, run: bool) {
        if !self.powered || self.bus.reset_asserted() { return; }
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
        // PRDY follows RUN even during PRESET. WAIT remains owned by the 8080
        // and stays low while the processor is reset.
        self.bus.cycle_set_ready_input(true);
    }''',
)

replace_once(
    "src/machine/cpu_board.rs",
    '''        if run {
            if !self.bus.reset_asserted() {
                self.cycle_set_running(true);
            }
        } else if self.bus.reset_asserted() || (!cpu_halted && !cpu_holding) {
            self.cycle_set_running(false);
        }''',
    '''        if run {
            if self.bus.reset_asserted() {
                self.cycle_set_run_latch_during_reset();
            } else {
                self.cycle_set_running(true);
            }
        } else if !self.bus.reset_asserted() && !cpu_halted && !cpu_holding {
            self.cycle_set_running(false);
        }''',
)

replace_once(
    "src/backend/cycle.rs",
    '''    fn release_reset(&mut self) -> BackendResult<()> {
        self.machine.release_front_panel_reset();
        self.stop_wait_park_pending = false;
        self.sync_machine_cpu();
        Ok(())
    }''',
    '''    fn release_reset(&mut self) -> BackendResult<()> {
        // Preserve a pending STOP until the first real post-reset PSYNC. The
        // exact T1 sample will clear RUN through cycle_capture_pending_stop_at_psync().
        self.machine.cycle_release_front_panel_reset();
        self.stop_wait_park_pending = false;
        self.sync_machine_cpu();
        Ok(())
    }''',
)

# Regression coverage lives at the backend contract so Fast and Cycle are both
# forced to reproduce the same physical latch semantics at their accuracy level.
Path("tests/run_reset_timing.rs").write_text(r'''use rustair::backend::{BackendHost, BusTState, EmulationEngine};
use rustair::config::{RamInit, RamSize};

fn prepared(engine: EmulationEngine, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(engine).expect("built-in backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

#[test]
fn run_sets_the_physical_latch_while_reset_is_held() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut host = prepared(engine, &[0x00, 0x00]);
        assert!(!host.running());

        host.assert_front_panel_reset();
        assert!(!host.running());
        host.assert_run_stop(true);
        assert!(host.running(), "{engine:?}: RUN must asynchronously set the D/C R-S latch during RESET");
        host.release_run_stop(true);

        let held = host.bus_teaching_snapshot().expect("RESET teaching state");
        assert_eq!(held.reset, Some(true));
        assert_eq!(held.ready, Some(true), "{engine:?}: PRDY follows the RUN latch even while PRESET is asserted");

        host.release_front_panel_reset();
        assert!(host.running(), "{engine:?}: releasing RESET must preserve RUN");
        host.run_cycles(4);
        assert_eq!(host.intel8080_state().pc, 1, "{engine:?}: execution must begin at reset vector zero");
    }
}

#[test]
fn stop_held_during_reset_waits_for_the_first_post_reset_fetch() {
    // HLT reproduces the classic original-8800 lock-up: STOP cannot reset the
    // RUN latch because a halted 8080 produces no qualifying PSYNC.
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut host = prepared(engine, &[0x76, 0x00]);
        host.assert_run_stop(true);
        host.release_run_stop(true);
        host.run_cycles(16);
        assert!(host.running(), "{engine:?}: HLT leaves the physical RUN latch set");
        assert!(host.intel8080_state().halted.unwrap_or(false));

        host.assert_run_stop(false);
        assert!(host.running(), "{engine:?}: STOP cannot clear RUN while the CPU is halted");
        host.assert_front_panel_reset();
        assert!(host.running(), "{engine:?}: RESET itself must not clear the RUN/STOP latch");

        host.release_front_panel_reset();
        match engine {
            EmulationEngine::RustFast8080 => {
                // Fast has no exact PSYNC/T-state boundary and captures the held
                // STOP at the reconstructed first fetch after RESET release.
                assert!(!host.running());
                assert_eq!(host.intel8080_state().pc, 0);
            }
            EmulationEngine::RustCycleAccurate8080 => {
                // Cycle retains RUN until the real first T1/PSYNC is clocked.
                assert!(host.running());
                host.run_cycles(1);
                assert!(!host.running());
                let sample = host.bus_teaching_snapshot().expect("post-reset STOP sample");
                assert_eq!(sample.t_state, BusTState::Tw);
                assert_eq!(sample.pins.wait, Some(true));
                assert_eq!(sample.ready, Some(false));
            }
            _ => unreachable!(),
        }

        host.release_run_stop(false);
    }
}
''', encoding="utf-8")
