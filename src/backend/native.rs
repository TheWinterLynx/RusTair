use std::time::Duration;

use crate::machine::AltairMachine;

use super::{BackendKind, CpuState, FrontPanelState, MachineBackend};

/// Adapter exposing the existing RusTair machine through [`MachineBackend`].
///
/// `machine()`/`machine_mut()` are intentionally retained as a migration escape
/// hatch. Existing serial, diagnostics and loader code can keep working while
/// UI-facing CPU/front-panel access is moved behind the backend contract in
/// small, reviewable steps.
pub struct NativeMachineBackend {
    machine: AltairMachine,
}

impl Default for NativeMachineBackend {
    fn default() -> Self {
        Self {
            machine: AltairMachine::default(),
        }
    }
}

impl NativeMachineBackend {
    pub fn new(machine: AltairMachine) -> Self { Self { machine } }

    pub fn machine(&self) -> &AltairMachine { &self.machine }

    pub fn machine_mut(&mut self) -> &mut AltairMachine { &mut self.machine }

    pub fn into_machine(self) -> AltairMachine { self.machine }

    fn snapshot_cpu(&self) -> CpuState {
        let cpu = &self.machine.cpu;
        CpuState {
            a: cpu.a,
            b: cpu.b,
            c: cpu.c,
            d: cpu.d,
            e: cpu.e,
            h: cpu.h,
            l: cpu.l,
            flags: cpu.f,
            pc: cpu.pc,
            sp: cpu.sp,
            inte: cpu.inte,
            halted: cpu.halted,
            cycles: cpu.cycles,
        }
    }

    fn snapshot_panel(&self) -> FrontPanelState {
        FrontPanelState {
            powered: self.machine.powered,
            running: self.machine.running,
            switches: self.machine.panel_switches(),
            address: self.machine.address_leds(),
            data: self.machine.data_leds(),
            lamps: self.machine.panel_lamps(),
            current_board_protected: self.machine.current_board_protected(),
            ext_clear_asserted: self.machine.ext_clear_asserted(),
        }
    }
}

impl MachineBackend for NativeMachineBackend {
    fn kind(&self) -> BackendKind { BackendKind::Native }

    fn name(&self) -> &'static str { "RusTair native 8080" }

    fn cpu_state(&self) -> CpuState { self.snapshot_cpu() }

    fn front_panel_state(&self) -> FrontPanelState { self.snapshot_panel() }

    fn power(&mut self, on: bool) { self.machine.power(on); }

    fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) {
        self.machine.power_with_historical_run_latch(on, historical);
    }

    fn run(&mut self) { self.machine.set_running(true); }

    fn halt(&mut self) { self.machine.set_running(false); }

    fn step(&mut self) { self.machine.step(); }

    fn run_cycles(&mut self, cycles: u32) { self.machine.run_cycles(cycles); }

    fn commit_panel_activity(&mut self, dt: Duration) {
        self.machine.commit_panel_activity(dt);
    }

    fn assert_run_stop(&mut self, run: bool) { self.machine.assert_run_stop(run); }

    fn release_run_stop(&mut self, run: bool) { self.machine.release_run_stop(run); }

    fn assert_reset(&mut self) { self.machine.assert_front_panel_reset(); }

    fn release_reset(&mut self) { self.machine.release_front_panel_reset(); }

    fn assert_clear(&mut self) { self.machine.assert_front_panel_clear(); }

    fn release_clear(&mut self) { self.machine.release_front_panel_clear(); }

    fn request_hold(&mut self, hold: bool) { self.machine.request_hold(hold); }

    fn panel_examine(&mut self, next: bool) { self.machine.examine(next); }

    fn panel_deposit(&mut self, next: bool) { self.machine.deposit(next); }

    fn protect_current_board(&mut self, protected: bool) {
        self.machine.protect_current_board(protected);
    }

    fn switch_register(&self) -> u16 { self.machine.panel_switches() }

    fn set_switch_register(&mut self, value: u16) {
        let changed = self.machine.panel_switches() ^ value;
        for bit in 0..16 {
            if changed & (1u16 << bit) != 0 {
                self.machine.toggle_sense_switch(bit);
            }
        }
    }

    fn peek_memory(&self, address: u16) -> Option<u8> {
        self.machine.bus.peek_memory(address)
    }

    fn write_memory(&mut self, address: u16, value: u8, respect_protection: bool) -> bool {
        self.machine
            .bus
            .debugger_write_memory(address, value, respect_protection)
    }

    fn load_bytes(&mut self, address: u16, bytes: &[u8]) {
        self.machine.bus.load(address, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_snapshot_is_backend_neutral() {
        let mut backend = NativeMachineBackend::default();
        backend.machine_mut().cpu.a = 0x12;
        backend.machine_mut().cpu.b = 0x34;
        backend.machine_mut().cpu.c = 0x56;
        backend.machine_mut().cpu.pc = 0x789a;
        backend.machine_mut().cpu.sp = 0xbcde;

        let state = backend.cpu_state();
        assert_eq!(state.a, 0x12);
        assert_eq!(state.bc(), 0x3456);
        assert_eq!(state.pc, 0x789a);
        assert_eq!(state.sp, 0xbcde);
    }

    #[test]
    fn switch_register_adapter_sets_exact_value() {
        let mut backend = NativeMachineBackend::default();
        backend.set_switch_register(0xa55a);
        assert_eq!(backend.switch_register(), 0xa55a);

        backend.set_switch_register(0x0f0f);
        assert_eq!(backend.switch_register(), 0x0f0f);
    }

    #[test]
    fn debugger_memory_access_round_trips_through_backend() {
        let mut backend = NativeMachineBackend::default();
        assert!(backend.write_memory(0x0010, 0x5a, false));
        assert_eq!(backend.peek_memory(0x0010), Some(0x5a));
    }
}
