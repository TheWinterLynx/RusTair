mod cpu_board;
mod front_panel;
mod io_devices;
mod memory;
mod panel_bus;
mod serial;

use std::time::Duration;

use rand::RngCore;

use crate::config::{RamBoardProfile, RamInit, RamSize};
use crate::cpu8080::{Bus, Cpu8080};
use cpu_board::{Fast8080S100Adapter, S100Cycle};
use front_panel::FrontPanelController;
use io_devices::IoDevices;
use memory::Memory;
use panel_bus::S100BusState;

pub(crate) use cpu_board::{Cycle8080S100Adapter, S100CpuControlLines, S100CpuSample};
pub(crate) use memory::MemoryReadyPhase;
pub use memory::{MAX_MEM_SIZE, MEM_SIZE, MEMORY_BOARD_COUNT, MEMORY_BOARD_SIZE};
pub use panel_bus::PanelLampSnapshot;

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

pub struct AltairBus {
    memory: Memory,
    io: IoDevices,
    panel: FrontPanelController,
    s100: S100BusState,
    fast_wait_t_states: u32,
    diagnostic_meter: Option<CpuDiagnosticMeter>,
    diagnostic_result: Option<CpuDiagnosticResult>,
}

impl Default for AltairBus {
    fn default() -> Self {
        let mut s = Self {
            memory: Memory::default(),
            io: IoDevices::default(),
            panel: FrontPanelController::default(),
            s100: S100BusState::default(),
            fast_wait_t_states: 0,
            diagnostic_meter: None,
            diagnostic_result: None,
        };
        s.initialize_memory();
        s
    }
}

impl AltairBus {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.cancel_cpu_diagnostic_meter();
        self.memory.configure(size, init_mode);
        self.fast_wait_t_states = 0;
        self.refresh_protect_line();
    }

    pub fn installed_ram_bytes(&self) -> usize { self.memory.installed_size() }
    pub fn initialize_memory(&mut self) { self.memory.initialize(); self.refresh_protect_line(); }
    pub fn randomize(&mut self) { self.memory.randomize(); }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.memory.arm_basic32_full_memory_probe_guard() }
    pub fn clear_transient_memory_guards(&mut self) { self.memory.clear_transient_guards(); }
    pub fn load(&mut self, address: u16, bytes: &[u8]) { self.memory.load(address, bytes); }
    pub fn clear_protection(&mut self) { self.memory.clear_protection(); self.refresh_protect_line(); }
    pub fn board_index(address: u16) -> Option<usize> { Memory::board_index(address) }
    pub fn is_protected(&self, address: u16) -> bool { self.memory.is_protected(address) }
    pub fn set_protected(&mut self, address: u16, protected: bool) { self.memory.set_protected(address, protected); self.refresh_protect_line(); }
    pub fn serial_receive(&mut self, byte: u8) {
        self.io.serial_receive(byte);
        self.refresh_interrupt_request_line();
    }
    pub fn serial_rx_empty(&self) -> bool { self.io.serial_rx_empty() }
    pub fn serial_rx_len(&self) -> usize { self.io.serial_rx_len() }
    pub fn serial_tx_front(&self) -> Option<u8> { self.io.serial_tx_front() }
    pub fn serial_tx_complete(&mut self) -> Option<u8> {
        let completed = self.io.serial_tx_complete();
        self.refresh_interrupt_request_line();
        completed
    }
    pub fn tx_busy(&self) -> bool { self.io.serial_tx_busy() }
    pub fn clear_serial(&mut self) {
        self.io.clear_serial();
        self.refresh_interrupt_request_line();
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

    fn panel_switches(&self) -> u16 { self.panel.switches() }
    fn toggle_panel_switch(&mut self, bit: usize) { self.panel.toggle_switch(bit); }
    fn panel_lamps(&self) -> PanelLampSnapshot { self.s100.snapshot() }
    fn panel_address(&self) -> u16 { self.s100.signals().address }
    fn panel_data(&self) -> u8 { self.s100.signals().panel_data }

    fn sync_cpu_inte(&mut self, enabled: bool) {
        self.s100.set_inte(enabled);
    }

    fn set_run(&mut self, run: bool) { self.s100.set_run(run); }
    fn set_ready(&mut self, ready: bool) { self.s100.set_ready(ready); }
    fn set_hold(&mut self, hold: bool) { self.s100.set_hold(hold); }
    fn hold_requested(&self) -> bool { self.s100.signals().hold }
    fn set_hlda(&mut self, hlda: bool) { self.s100.set_hlda(hlda); }
    fn hlda(&self) -> bool { self.s100.signals().hlda }
    fn reset_asserted(&self) -> bool { self.s100.signals().reset }
    fn ext_clear_asserted(&self) -> bool { self.s100.signals().ext_clear }
    fn freeze_panel_bus(&mut self) { self.s100.freeze(); }
    fn commit_panel_activity(&mut self, dt: Duration, dynamic: bool) { self.s100.commit(dt, dynamic); }

    pub(crate) fn refresh_interrupt_request_line(&mut self) {
        let asserted = self.serial_interrupt_request();
        self.s100.set_interrupt_request(asserted);
    }

    pub(crate) fn direct_interrupt_opcode(&self) -> u8 {
        self.serial_interrupt_opcode()
    }

    pub(crate) fn cpu_control_lines(&self) -> S100CpuControlLines {
        let signals = self.s100.signals();
        S100CpuControlLines {
            ready: signals.ready,
            interrupt: signals.interrupt,
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
    }

    fn refresh_protect_line(&mut self) {
        let address = self.s100.signals().address;
        self.s100.refresh_protect(self.memory.is_protected(address));
    }

    fn drive_cpu_cycle(&mut self, address: u16, data: u8, cycle: S100Cycle) {
        let signals = self.s100.signals();
        let inte = signals.inte;
        Fast8080S100Adapter::for_each_sample(
            address,
            data,
            cycle,
            inte,
            signals.ready,
            signals.wait,
            |sample| self.drive_cpu_board_sample(sample),
        );
    }

    fn drive_power_on_state(&mut self, address: u16, run: bool) {
        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .drive_power_on_state(address, data, protected, inte, run);
    }

    fn assert_front_panel_reset_bus(&mut self, run: bool) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
        self.s100.assert_front_panel_reset(run);
    }

    fn release_front_panel_reset_bus(&mut self, address: u16, run: bool) {
        let data = self.memory.peek(address).unwrap_or(0);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100
            .release_front_panel_reset(address, data, protected, inte, run);
    }

    fn set_ext_clear(&mut self, asserted: bool) {
        let was_asserted = self.s100.signals().ext_clear;
        self.s100.set_ext_clear(asserted);
        if asserted && !was_asserted {
            self.io.clear_serial();
            self.refresh_interrupt_request_line();
        }
    }

    fn front_panel_deposit(&mut self, address: u16, value: u8) {
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100.drive_front_panel_deposit(address, value, protected, inte);
        self.memory.write(address, value);
        self.refresh_protect_line();
    }

    fn power_off_s100(&mut self) {
        self.memory.reset_timing();
        self.s100.power_off();
    }

    #[inline]
    fn io_bus_address(port: u8) -> u16 { u16::from(port) * 0x0101 }
}

impl Bus for AltairBus {
    fn read(&mut self, address: u16) -> u8 {
        self.fast_account_memory_read_wait(address);
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::MemoryRead);
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.drive_cpu_cycle(address, value, S100Cycle::MemoryWrite);
        self.memory.write(address, value);
    }

    fn input(&mut self, port: u8) -> u8 {
        let value = match port { 0xff => self.panel.input(), _ => self.io.input(port) };
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::InputRead);
        if port != 0xff {
            self.refresh_interrupt_request_line();
        }
        value
    }

    fn output(&mut self, port: u8, value: u8) {
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::OutputWrite);
        if port != 0xff {
            self.io.output(port, value);
            self.refresh_interrupt_request_line();
        }
    }

    fn set_inte(&mut self, enabled: bool) { self.sync_cpu_inte(enabled); }

    fn opcode_fetch(&mut self, address: u16) -> u8 {
        self.fast_account_memory_read_wait(address);
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::InstructionFetch);
        value
    }

    fn stack_read(&mut self, address: u16) -> u8 {
        self.fast_account_memory_read_wait(address);
        let value = self.memory.read(address);
        self.drive_cpu_cycle(address, value, S100Cycle::StackRead);
        value
    }

    fn stack_write(&mut self, address: u16, value: u8) {
        self.drive_cpu_cycle(address, value, S100Cycle::StackWrite);
        self.memory.write(address, value);
    }

    fn halt_ack(&mut self, address: u16, opcode: u8) {
        self.drive_cpu_cycle(address, opcode, S100Cycle::HaltAcknowledge);
    }

    fn take_wait_states(&mut self) -> u32 {
        self.take_fast_memory_wait_t_states()
    }

    fn interrupt_ack(&mut self, address: u16, opcode: u8, while_halted: bool) {
        let cycle = if while_halted {
            S100Cycle::InterruptAcknowledgeWhileHalted
        } else {
            S100Cycle::InterruptAcknowledge
        };
        self.drive_cpu_cycle(address, opcode, cycle);
    }

    fn instruction_complete(&mut self, address: u16, _opcode: u8, t_states: u32) {
        self.record_cpu_diagnostic_instruction(address, t_states);
    }
}

pub struct AltairMachine {
    pub cpu: Cpu8080,
    pub bus: AltairBus,
    pub powered: bool,
    /// Mirrors the physical RUN/STOP R-S latch, not merely "CPU is executing".
    pub running: bool,
    stop_switch_asserted: bool,
    run_switch_asserted: bool,
}

impl Default for AltairMachine {
    fn default() -> Self {
        Self {
            cpu: Cpu8080::new(),
            bus: AltairBus::default(),
            powered: false,
            running: false,
            stop_switch_asserted: false,
            run_switch_asserted: false,
        }
    }
}

impl AltairMachine {
    pub fn configure_memory(&mut self, size: RamSize, init_mode: RamInit) {
        self.running = false;
        self.bus.set_run(false);
        self.bus.configure_memory(size, init_mode);
        self.cpu.reset();
        self.bus.clear_serial();
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        if self.powered {
            self.front_panel_reset();
        } else {
            self.bus.power_off_s100();
        }
    }

    pub fn installed_ram_bytes(&self) -> usize { self.bus.installed_ram_bytes() }
    pub fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) {
        self.bus.configure_memory_board_profile(profile);
    }
    pub fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> {
        self.bus.memory_board_profile(address)
    }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.bus.arm_basic32_full_memory_probe_guard() }

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

    pub fn power(&mut self, on: bool) {
        self.power_with_historical_run_latch(on, false);
    }

    pub fn power_with_historical_run_latch(&mut self, on: bool, historical: bool) {
        self.bus.cancel_cpu_diagnostic_meter();
        self.powered = on;
        self.stop_switch_asserted = false;
        self.run_switch_asserted = false;
        if on {
            self.bus.clear_protection();
            self.bus.clear_transient_memory_guards();
            self.bus.clear_serial();
            self.randomize_power_on_cpu();
            let run = historical && (rand::rng().next_u32() & 1 != 0);
            self.running = run;
            self.bus.set_run(run);
            self.bus.sync_cpu_inte(self.cpu.inte);
            self.bus.set_hlda(false);
            self.bus.panel.set_address_latch(self.cpu.pc);
            self.bus.drive_power_on_state(self.cpu.pc, run);
        } else {
            self.running = false;
            self.bus.clear_serial();
            self.bus.initialize_memory();
            self.bus.power_off_s100();
        }
    }

    fn randomize_power_on_cpu(&mut self) {
        self.cpu.reset();
        let mut rng = rand::rng();
        self.cpu.a = rng.next_u32() as u8;
        self.cpu.b = rng.next_u32() as u8;
        self.cpu.c = rng.next_u32() as u8;
        self.cpu.d = rng.next_u32() as u8;
        self.cpu.e = rng.next_u32() as u8;
        self.cpu.h = rng.next_u32() as u8;
        self.cpu.l = rng.next_u32() as u8;
        self.cpu.f = ((rng.next_u32() as u8) & 0xd5) | 0x02;
        self.cpu.pc = rng.next_u32() as u16;
        self.cpu.sp = rng.next_u32() as u16;
        self.cpu.inte = rng.next_u32() & 1 != 0;
        self.cpu.halted = false;
        self.cpu.cycles = 0;
    }

    pub fn assert_run_stop(&mut self, run: bool) {
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
    }

    pub fn release_run_stop(&mut self, run: bool) {
        if run {
            self.run_switch_asserted = false;
        } else {
            self.stop_switch_asserted = false;
        }
    }

    pub fn assert_front_panel_reset(&mut self) {
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

    pub fn front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.assert_front_panel_reset();
        self.release_front_panel_reset();
    }

    pub fn reset(&mut self) {
        if !self.powered { return; }
        self.front_panel_reset();
        self.bus.clear_serial();
    }

    pub fn assert_front_panel_clear(&mut self) {
        if !self.powered { return; }
        self.bus.set_ext_clear(true);
    }

    pub fn release_front_panel_clear(&mut self) {
        if !self.powered { return; }
        self.bus.set_ext_clear(false);
    }

    pub fn clear_io(&mut self) {
        if !self.powered { return; }
        self.assert_front_panel_clear();
        self.release_front_panel_clear();
    }

    pub fn set_running(&mut self, run: bool) {
        if !self.powered || self.bus.reset_asserted() { return; }
        self.running = run;
        self.bus.set_run(run);
        if run {
            self.bus.set_ready(true);
        } else {
            let address = self.bus.panel_address();
            self.bus.panel.set_address_latch(address);
            self.bus.set_ready(false);
            self.bus.set_hlda(false);
            self.bus.freeze_panel_bus();
        }
    }

    fn service_fast_interrupt_if_requested(&mut self) -> u32 {
        self.bus.refresh_interrupt_request_line();
        let lines = self.bus.cpu_control_lines();
        if !lines.interrupt || !self.cpu.inte {
            return 0;
        }

        let opcode = self.bus.direct_interrupt_opcode();
        self.bus.sync_cpu_inte(false);
        let before = self.cpu.cycles;
        let accepted = self.cpu.interrupt(&mut self.bus, opcode);
        debug_assert!(accepted);
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.cpu.cycles.saturating_sub(before) as u32
    }

    pub fn step(&mut self) {
        if !self.powered || self.running || self.bus.reset_asserted() { return; }
        if self.bus.hold_requested() {
            self.bus.set_hlda(true);
            self.bus.freeze_panel_bus();
            return;
        }
        self.bus.set_hlda(false);
        self.bus.set_ready(true);
        self.bus.sync_cpu_inte(self.cpu.inte);
        if self.service_fast_interrupt_if_requested() == 0 {
            self.cpu.step(&mut self.bus);
        }
        self.bus.sync_cpu_inte(self.cpu.inte);
        let address = self.bus.panel_address();
        self.bus.panel.set_address_latch(address);
        self.bus.set_ready(false);
        self.bus.freeze_panel_bus();
    }

    pub fn run_cycles(&mut self, cycles: u32) {
        if !self.powered || !self.running || self.bus.reset_asserted() { return; }
        self.bus.set_ready(true);
        if self.bus.hold_requested() {
            self.bus.set_hlda(true);
            return;
        }
        self.bus.set_hlda(false);
        self.bus.sync_cpu_inte(self.cpu.inte);

        let mut used = 0u32;
        while used < cycles {
            let interrupt_t_states = self.service_fast_interrupt_if_requested();
            if interrupt_t_states != 0 {
                used = used.saturating_add(interrupt_t_states);
            } else {
                used = used.saturating_add(self.cpu.step(&mut self.bus));
            }
            self.bus.sync_cpu_inte(self.cpu.inte);
        }
    }

    pub fn request_hold(&mut self, hold: bool) {
        self.bus.set_hold(hold);
        if !hold { self.bus.set_hlda(false); }
    }

    pub fn commit_panel_activity(&mut self, dt: Duration) {
        let dynamic = self.powered
            && self.running
            && !self.cpu.halted
            && !self.bus.hlda()
            && !self.bus.reset_asserted();
        self.bus.commit_panel_activity(dt, dynamic);
    }

    pub fn examine(&mut self, next: bool) {
        self.fast_front_panel_examine_via_cpu_board(next);
    }

    pub fn deposit(&mut self, next: bool) {
        self.fast_front_panel_deposit_via_cpu_board(next);
    }

    pub fn protect_current_board(&mut self, protected: bool) {
        self.front_panel_set_memory_protection_via_s100(protected);
    }

    pub fn current_board_protected(&self) -> bool { self.powered && self.bus.s100.signals().prot }
    pub fn panel_switches(&self) -> u16 { self.bus.panel_switches() }
    pub fn toggle_sense_switch(&mut self, bit: usize) { self.bus.toggle_panel_switch(bit); }
    pub fn address_leds(&self) -> u16 { self.bus.panel_address() }
    pub fn data_leds(&self) -> u8 { self.bus.panel_data() }
    pub fn panel_lamps(&self) -> PanelLampSnapshot { self.bus.panel_lamps() }
    pub fn wait_led(&self) -> bool { self.powered && self.bus.s100.signals().wait }
    pub fn ext_clear_asserted(&self) -> bool { self.powered && self.bus.ext_clear_asserted() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protection_is_per_1k_board() {
        let mut bus = AltairBus::default();
        bus.set_protected(0x0410, true);
        assert!(!bus.is_protected(0x03ff));
        assert!(bus.is_protected(0x0400));
        assert!(bus.is_protected(0x07ff));
        assert!(!bus.is_protected(0x0800));
    }

    #[test]
    fn diagnostic_meter_normalizes_real_bdos_to_reference_stub() {
        let mut bus = AltairBus::default();
        bus.begin_cpu_diagnostic_meter(
            "TEST.COM".into(),
            0xff00,
            0x37,
            Some(7),
            Some(65),
        );

        bus.instruction_complete(0x0000, 0xc3, 10);
        bus.instruction_complete(0x0080, 0x31, 10);
        bus.instruction_complete(0x0100, 0x00, 4);
        bus.instruction_complete(0x0101, 0xcd, 17);
        bus.instruction_complete(0x0005, 0xc3, 10);
        bus.instruction_complete(0xff00, 0xf5, 11);
        bus.instruction_complete(0xff01, 0xc5, 11);
        bus.instruction_complete(0x0104, 0x00, 4);
        bus.instruction_complete(0x0105, 0xc3, 10);
        bus.instruction_complete(0x0000, 0x76, 7);

        let result = bus.take_cpu_diagnostic_result().unwrap();
        assert_eq!(result.instructions, 7);
        assert_eq!(result.t_states, 65);
        assert_eq!(result.expected_instructions, Some(7));
        assert_eq!(result.expected_t_states, Some(65));
    }

    #[test]
    fn reset_held_and_released_match_mits_checkout_sequence_when_stopped() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.bus.load(0, &[0xa5]);

        machine.assert_front_panel_reset();
        assert_eq!(machine.address_leds(), 0xffff);
        assert_eq!(machine.data_leds(), 0xff);
        let held = machine.panel_lamps();
        assert_eq!(held.inte, 0.0);
        assert_eq!(held.memr, 0.0);
        assert_eq!(held.m1, 0.0);
        assert_eq!(held.wo, 0.0);
        assert_eq!(held.wait, 0.0);

        machine.release_front_panel_reset();
        assert_eq!(machine.cpu.pc, 0);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0xa5);
        let released = machine.panel_lamps();
        assert_eq!(released.inte, 0.0);
        assert_eq!(released.memr, 1.0);
        assert_eq!(released.m1, 1.0);
        assert_eq!(released.wo, 1.0);
        assert_eq!(released.wait, 1.0);
    }

    #[test]
    fn physical_reset_preserves_run_latch() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.set_running(true);
        machine.assert_front_panel_reset();
        assert!(machine.running);
        machine.release_front_panel_reset();
        assert!(machine.running);
        assert!(!machine.wait_led());
        assert_eq!(machine.cpu.pc, 0);
    }

    #[test]
    fn stop_while_halted_requires_stop_plus_reset_recovery() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.set_running(true);
        machine.cpu.halted = true;

        machine.assert_run_stop(false);
        assert!(machine.running, "STOP cannot latch without PSYNC while halted");
        machine.assert_front_panel_reset();
        assert!(machine.running, "RESET itself must preserve the physical RUN/STOP latch");
        machine.release_front_panel_reset();
        assert!(!machine.running, "Fast must capture held STOP at its reconstructed first post-reset fetch boundary");
        machine.release_run_stop(false);
        assert!(machine.wait_led());
    }

    #[test]
    fn front_panel_reset_preserves_serial_io_state() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.bus.serial_receive(b'R');
        machine.front_panel_reset();
        assert_eq!(machine.cpu.pc, 0);
        assert_eq!(machine.bus.serial_rx_len(), 1);
    }

    #[test]
    fn ext_clear_is_held_bus_signal_and_clears_io_without_touching_cpu() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.cpu.pc = 0x1234;
        machine.bus.serial_receive(b'X');

        machine.assert_front_panel_clear();
        assert!(machine.ext_clear_asserted());
        assert_eq!(machine.cpu.pc, 0x1234);
        assert_eq!(machine.bus.serial_rx_len(), 0);
        assert!(!machine.bus.cpu_control_lines().interrupt);

        machine.release_front_panel_clear();
        assert!(!machine.ext_clear_asserted());
        assert_eq!(machine.cpu.pc, 0x1234);
    }

    #[test]
    fn safe_power_on_defaults_run_latch_to_stop() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        assert!(!machine.running);
        assert!(!machine.bus.s100.signals().run);
    }

    #[test]
    fn hold_request_drives_hlda_through_bus_arbitration() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.set_running(true);
        machine.request_hold(true);
        machine.run_cycles(10);
        machine.commit_panel_activity(Duration::from_secs(1));
        assert_eq!(machine.panel_lamps().hlda, 1.0);
        machine.request_hold(false);
        assert!(!machine.bus.s100.signals().hlda);
    }

    #[test]
    fn examine_and_deposit_drive_front_panel_bus_with_physical_wo_polarity() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.bus.load(0, &[0x12]);
        machine.examine(false);
        assert_eq!(machine.address_leds(), 0);
        assert_eq!(machine.data_leds(), 0x12);
        assert_eq!(machine.panel_lamps().memr, 1.0);
        assert_eq!(machine.panel_lamps().wo, 1.0);

        for bit in [1, 2, 4, 6] { machine.toggle_sense_switch(bit); }
        machine.deposit(false);
        assert_eq!(machine.bus.peek_memory(0), Some(0x56));
        assert_eq!(machine.panel_lamps().wo, 0.0);
    }

    #[test]
    fn cpu_board_control_lines_are_read_from_the_shared_s100_state() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.request_hold(true);
        let lines = machine.bus.cpu_control_lines();
        assert!(!lines.ready);
        assert!(!lines.interrupt);
        assert!(lines.hold);
        assert!(!lines.reset);
    }

    #[test]
    fn serial_irq_projects_to_pint_and_fast_cpu_accepts_direct_rst7() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.cpu.sp = 0x0400;
        machine.bus.load(0, &[0xfb, 0x00, 0x00]);
        machine.bus.output(0x00, 0x01);
        machine.set_running(true);

        machine.run_cycles(8);
        assert_eq!(machine.cpu.pc, 0x0002);
        assert!(machine.cpu.inte);

        machine.bus.serial_receive(b'I');
        assert!(machine.bus.cpu_control_lines().interrupt);
        machine.run_cycles(11);

        assert_eq!(machine.cpu.pc, 0x0038);
        assert_eq!(machine.cpu.sp, 0x03fe);
        assert!(!machine.cpu.inte);
        assert_eq!(machine.bus.peek_memory(0x03fe), Some(0x02));
        assert_eq!(machine.bus.peek_memory(0x03ff), Some(0x00));
        assert!(machine.bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn fast_pint_wakes_halted_cpu_when_inte_is_enabled() {
        let mut machine = AltairMachine::default();
        machine.power(true);
        machine.front_panel_reset();
        machine.cpu.sp = 0x0400;
        machine.bus.load(0, &[0xfb, 0x76]);
        machine.bus.output(0x00, 0x01);
        machine.set_running(true);

        machine.run_cycles(11);
        assert!(machine.cpu.halted);
        assert!(machine.cpu.inte);
        assert_eq!(machine.cpu.pc, 0x0002);

        machine.bus.serial_receive(b'W');
        machine.run_cycles(11);
        assert!(!machine.cpu.halted);
        assert!(!machine.cpu.inte);
        assert_eq!(machine.cpu.pc, 0x0038);
        assert_eq!(machine.cpu.sp, 0x03fe);
    }
}
