mod chassis;
mod cpu_board;
mod dcdd;
mod front_panel;
mod memory;
mod panel_bus;
mod serial;
mod serial_bus;
mod serial_card;
mod serial_devices;

use std::time::Duration;

// `serial.rs` exposes these historical 88-SIO types through the parent module.
use crate::config::{RamInit, RamSize, SioInterruptTarget, SioRevision};
use front_panel::FrontPanelController;
use memory::Memory;
use panel_bus::S100BusState;

pub use chassis::AltairChassis;
pub(crate) use cpu_board::{Cycle8080S100Adapter, S100CpuControlLines, S100CpuSample};
pub(crate) use dcdd::Mits88DcddHarness;
pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};
pub use panel_bus::PanelLampSnapshot;
pub(crate) use panel_bus::FullPanelDuty;
pub(crate) use serial_card::RuntimeSerialCardHandle;

pub const CLOCK_HZ: u32 = 2_000_000;

#[derive(Clone, Debug)]
pub struct CpuDiagnosticResult {
    pub name: String,
    pub instructions: u64,
    pub t_states: u64,
    pub expected_instructions: Option<u64>,
    pub expected_t_states: Option<u64>,
}

#[derive(Clone, Debug)]
struct CpuDiagnosticMeter {
    name: String,
    bdos_start: u16,
    bdos_end: u16,
    expected_instructions: Option<u64>,
    expected_t_states: Option<u64>,
    started: bool,
    instructions: u64,
    t_states: u64,
}

impl CpuDiagnosticMeter {
    fn new(
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) -> Self {
        Self {
            name,
            bdos_start,
            bdos_end: bdos_start.saturating_add(bdos_len as u16),
            expected_instructions,
            expected_t_states,
            started: false,
            instructions: 0,
            t_states: 0,
        }
    }

    fn complete(&self) -> CpuDiagnosticResult {
        CpuDiagnosticResult {
            name: self.name.clone(),
            instructions: self.instructions,
            t_states: self.t_states,
            expected_instructions: self.expected_instructions,
            expected_t_states: self.expected_t_states,
        }
    }
}

/// Bus-facing state owned by the Altair chassis.
///
/// RAM and serial-card silicon live only in the slot-native `S100RuntimeFabric`
/// inside `Memory`. `AltairBus` owns no hidden UART or alternative I/O machine.
pub struct AltairBus {
    memory: Memory,
    panel: FrontPanelController,
    s100: S100BusState,
    diagnostic_meter: Option<CpuDiagnosticMeter>,
    diagnostic_result: Option<CpuDiagnosticResult>,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut bus = Self {
            memory: Memory::default(),
            panel: FrontPanelController::default(),
            s100: S100BusState::default(),
            diagnostic_meter: None,
            diagnostic_result: None,
        };
        bus.initialize_memory();
        bus
    }
}

impl AltairBus {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.cancel_cpu_diagnostic_meter();
        self.memory.configure(size, init_mode);
        self.refresh_protect_line();
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.memory.installed_size()
    }

    pub fn initialize_memory(&mut self) {
        self.memory.initialize();
        self.refresh_protect_line();
    }

    pub fn randomize(&mut self) {
        self.memory.randomize();
    }

    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool {
        self.memory.arm_basic32_full_memory_probe_guard()
    }

    pub fn clear_transient_memory_guards(&mut self) {
        self.memory.clear_transient_guards();
    }

    pub fn load(&mut self, address: u16, bytes: &[u8]) {
        self.memory.load(address, bytes);
    }

    pub fn clear_protection(&mut self) {
        self.memory.clear_protection();
        self.refresh_protect_line();
    }

    pub fn board_index(address: u16) -> Option<usize> {
        Memory::board_index(address)
    }

    pub fn is_protected(&self, address: u16) -> bool {
        self.memory.is_protected(address)
    }

    pub fn set_protected(&mut self, address: u16, protected: bool) {
        self.memory.set_protected(address, protected);
        self.refresh_protect_line();
    }

    pub fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) {
        self.diagnostic_result = None;
        self.diagnostic_meter = Some(CpuDiagnosticMeter::new(
            name,
            bdos_start,
            bdos_len,
            expected_instructions,
            expected_t_states,
        ));
    }

    pub fn cancel_cpu_diagnostic_meter(&mut self) {
        self.diagnostic_meter = None;
        self.diagnostic_result = None;
    }

    pub fn take_cpu_diagnostic_result(&mut self) -> Option<CpuDiagnosticResult> {
        self.diagnostic_result.take()
    }

    fn record_cpu_diagnostic_instruction(&mut self, address: u16, t_states: u32) {
        let mut completed = None;
        if let Some(meter) = self.diagnostic_meter.as_mut() {
            if !meter.started {
                if address == 0x0100 {
                    meter.started = true;
                    meter.instructions = 1;
                    meter.t_states = u64::from(t_states);
                }
                return;
            }

            if address == 0x0005 {
                meter.instructions = meter.instructions.saturating_add(2);
                meter.t_states = meter.t_states.saturating_add(20);
                return;
            }

            if address == 0x0000 {
                meter.instructions = meter.instructions.saturating_add(1);
                meter.t_states = meter.t_states.saturating_add(10);
                completed = Some(meter.complete());
            } else if address >= meter.bdos_start && address < meter.bdos_end {
                return;
            } else {
                meter.instructions = meter.instructions.saturating_add(1);
                meter.t_states = meter.t_states.saturating_add(u64::from(t_states));
            }
        }

        if let Some(result) = completed {
            self.diagnostic_meter = None;
            self.diagnostic_result = Some(result);
        }
    }

    fn panel_switches(&self) -> u16 {
        self.panel.switches()
    }

    fn toggle_panel_switch(&mut self, bit: usize) {
        self.panel.toggle_switch(bit);
    }

    fn panel_lamps(&self) -> PanelLampSnapshot {
        self.s100.snapshot()
    }

    fn panel_address(&self) -> u16 {
        self.s100.signals().address
    }

    fn panel_data(&self) -> u8 {
        self.s100.signals().panel_data
    }

    fn sync_cpu_inte(&mut self, enabled: bool) {
        self.s100.set_inte(enabled);
    }

    fn set_run(&mut self, run: bool) {
        self.s100.set_run(run);
    }

    fn run_latched(&self) -> bool {
        self.s100.signals().run
    }

    fn hold_requested(&self) -> bool {
        self.s100.signals().hold
    }

    fn set_hlda(&mut self, hlda: bool) {
        self.s100.set_hlda(hlda);
    }

    fn hlda(&self) -> bool {
        self.s100.signals().hlda
    }

    fn reset_asserted(&self) -> bool {
        self.s100.signals().reset
    }

    fn ext_clear_asserted(&self) -> bool {
        self.s100.signals().ext_clear
    }

    fn freeze_panel_bus(&mut self) {
        self.s100.freeze();
    }

    fn commit_panel_activity(&mut self, dt: Duration, dynamic: bool) {
        self.s100.commit(dt, dynamic);
    }

    pub(crate) fn cpu_control_lines(&self) -> S100CpuControlLines {
        let signals = self.s100.signals();
        let physical = self.memory.cycle_live_inputs();
        S100CpuControlLines {
            ready: signals.ready,
            interrupt: physical.interrupt,
            hold: signals.hold,
            reset: signals.reset,
        }
    }

    pub(crate) fn drive_cpu_board_sample(&mut self, sample: S100CpuSample) {
        self.cycle_drive_s100_t_state(
            sample.address,
            sample.cpu_data,
            sample.data_in,
            sample.data_out,
            sample.status_word,
            sample.inte,
            sample.ready,
            sample.wait,
            sample.hlda,
        );
        // Every caller of this boundary is one exact Cpu8080Cycle T-state.
        // Independent card oscillators therefore advance exactly once here;
        // Full advances equivalent elapsed card time at its sync boundary. If
        // that elapsed T-state changes PINT/VI/PRDY, settle the real connector
        // before the next CPU T-state samples its initial package inputs.
        self.memory.advance_serial_time(1);
        self.settle_host_serial_change();
    }

    fn refresh_protect_line(&mut self) {
        let address = self.s100.signals().address;
        self.s100
            .refresh_protect(self.memory.is_protected(address));
    }

    fn drive_power_on_state(&mut self, address: u16, run: bool) {
        let data = self.memory.preview_read(address);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .drive_power_on_state(address, data, protected, inte, run);
    }

    fn assert_front_panel_reset_bus(&mut self) {
        self.s100.set_memory_ready_input(true);
        self.s100.assert_front_panel_reset();
    }

    fn release_front_panel_reset_bus(&mut self, address: u16) {
        let data = self.memory.preview_read(address);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .release_front_panel_reset(address, data, protected, inte);
    }

    fn set_ext_clear(&mut self, asserted: bool) {
        let was_asserted = self.s100.signals().ext_clear;
        self.s100.set_ext_clear(asserted);
        if asserted && !was_asserted {
            // The front-panel CLEAR line resets the actual installed UART card
            // and the resulting PINT/VI/PRDY change is resolved on S-100 now.
            self.clear_serial();
        }
    }

    fn front_panel_deposit(&mut self, address: u16, value: u8) {
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .drive_front_panel_deposit(address, value, protected, inte);
        self.memory.write(address, value);
        self.refresh_protect_line();
    }

    fn power_off_s100(&mut self) {
        self.s100.power_off();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protection_is_scoped_to_the_historical_ram_card() {
        let mut bus = AltairBus::default();
        bus.set_protected(0x0410, true);
        assert!(bus.is_protected(0x0000));
        assert!(bus.is_protected(0x0fff));
        assert!(!bus.is_protected(0x1000));
        assert!(!bus.is_protected(0x1fff));
    }

    #[test]
    fn diagnostic_meter_normalizes_real_bdos_to_reference_stub() {
        let mut bus = AltairBus::default();
        bus.begin_cpu_diagnostic_meter("TEST.COM".into(), 0xff00, 0x37, Some(7), Some(65));
        bus.record_cpu_diagnostic_instruction(0x0000, 10);
        bus.record_cpu_diagnostic_instruction(0x0080, 10);
        bus.record_cpu_diagnostic_instruction(0x0100, 4);
        bus.record_cpu_diagnostic_instruction(0x0101, 17);
        bus.record_cpu_diagnostic_instruction(0x0005, 10);
        bus.record_cpu_diagnostic_instruction(0xff00, 11);
        bus.record_cpu_diagnostic_instruction(0xff01, 11);
        bus.record_cpu_diagnostic_instruction(0x0104, 4);
        bus.record_cpu_diagnostic_instruction(0x0105, 10);
        bus.record_cpu_diagnostic_instruction(0x0000, 7);
        let result = bus.take_cpu_diagnostic_result().unwrap();
        assert_eq!(result.instructions, 7);
        assert_eq!(result.t_states, 65);
        assert_eq!(result.expected_instructions, Some(7));
        assert_eq!(result.expected_t_states, Some(65));
    }
}
