mod chassis;
mod cpu_board;
mod front_panel;
mod io_devices;
mod memory;
mod panel_bus;
mod serial;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use rand::RngCore;

use crate::config::{
    RamBoardProfile, RamInit, RamSize, SerialBoard, SioHardwareConfig, SioInterruptTarget,
    SioInterruptWiring, SioRevision, TwoSioInterruptWiring, TwoSioStraps,
};
use crate::cpu8080::{Bus, Cpu8080};
use crate::s100::S100Signal;
use crate::s100_backplane::S100BusSample;
use crate::s100_io_card::{S100IoDeviceLines, S100IoRegisterDevice};
use cpu_board::{Fast8080S100Adapter, S100Cycle};
use front_panel::FrontPanelController;
use io_devices::IoDevices;
use memory::Memory;
use panel_bus::S100BusState;

pub use chassis::AltairChassis;
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

/// Shared ownership boundary for one physical serial card instance.
///
/// `IoDevices` still contains the already-audited COM2502/MC6850 models, but a
/// runtime S-100 slot and its host-side endpoint handle point at the same
/// instance instead of constructing a second UART. Explicit Cycle chassis route
/// endpoints and debugger operations through these per-slot handles; the legacy
/// singleton remains only for aggregate-configuration compatibility.
#[derive(Clone)]
pub(crate) struct RuntimeSerialCardHandle {
    state: Rc<RefCell<IoDevices>>,
    connector_dirty: Rc<Cell<bool>>,
    board: SerialBoard,
    base: u8,
}

/// Register-facing half of `RuntimeSerialCardHandle`. `S100IoCardAdapter` owns
/// this value inside the physical backplane while endpoint/debugger code may keep
/// a clone of the handle above. Both mutate the same finite UART state.
pub(crate) struct RuntimeSerialCardDevice {
    handle: RuntimeSerialCardHandle,
    previous_clear: bool,
    input_wait_active: bool,
    input_cycle_waited: bool,
}

impl RuntimeSerialCardHandle {
    pub(crate) fn new_sio(
        config: SioHardwareConfig,
    ) -> (RuntimeSerialCardDevice, RuntimeSerialCardHandle) {
        let mut state = IoDevices::default();
        state.configure_serial_board(SerialBoard::Sio88);
        state.configure_sio_hardware(config);
        let handle = Self {
            state: Rc::new(RefCell::new(state)),
            connector_dirty: Rc::new(Cell::new(true)),
            board: SerialBoard::Sio88,
            base: config.address.status(),
        };
        (
            RuntimeSerialCardDevice {
                handle: handle.clone(),
                previous_clear: false,
                input_wait_active: false,
                input_cycle_waited: false,
            },
            handle,
        )
    }

    pub(crate) fn new_two_sio(
        straps: TwoSioStraps,
        interrupt_wiring: TwoSioInterruptWiring,
    ) -> (RuntimeSerialCardDevice, RuntimeSerialCardHandle) {
        let mut state = IoDevices::default();
        state.configure_serial_board(SerialBoard::TwoSio88);
        state.configure_two_sio_straps(straps);
        state.configure_two_sio_interrupt_wiring(interrupt_wiring);
        let handle = Self {
            state: Rc::new(RefCell::new(state)),
            connector_dirty: Rc::new(Cell::new(true)),
            board: SerialBoard::TwoSio88,
            base: straps.address.base(),
        };
        (
            RuntimeSerialCardDevice {
                handle: handle.clone(),
                previous_clear: false,
                input_wait_active: false,
                input_cycle_waited: false,
            },
            handle,
        )
    }

    pub(crate) const fn board(&self) -> SerialBoard {
        self.board
    }

    #[cfg(test)]
    pub(crate) const fn base(&self) -> u8 { self.base }

    pub(crate) fn receive(&self, port_index: usize, byte: u8) -> bool {
        let mut state = self.state.borrow_mut();
        let received = match (self.board, port_index) {
            (_, 0) => {
                state.serial_receive(byte);
                true
            }
            (SerialBoard::TwoSio88, 1) => {
                state.port1_receive(byte);
                true
            }
            _ => false,
        };
        if received {
            self.connector_dirty.set(true);
        }
        received
    }

    pub(crate) fn advance_t_states(&self, t_states: u64) {
        if t_states == 0 {
            return;
        }
        let mut state = self.state.borrow_mut();
        let before_pint = state.interrupt_request();
        let before_vi = state.vector_interrupt_requests();
        state.advance_t_states(t_states);
        let connector_changed = before_pint != state.interrupt_request()
            || before_vi != state.vector_interrupt_requests();
        drop(state);
        if connector_changed {
            self.connector_dirty.set(true);
        }
    }

    pub(crate) fn rx_empty(&self, port_index: usize) -> bool {
        let state = self.state.borrow();
        match (self.board, port_index) {
            (_, 0) => state.serial_rx_empty(),
            (SerialBoard::TwoSio88, 1) => state.port1_rx_empty(),
            _ => true,
        }
    }

    pub(crate) fn tx_front(&self, port_index: usize) -> Option<u8> {
        let state = self.state.borrow();
        match (self.board, port_index) {
            (_, 0) => state.serial_tx_front(),
            (SerialBoard::TwoSio88, 1) => state.port1_tx_front(),
            _ => None,
        }
    }

    pub(crate) fn supports_port(&self, port_index: usize) -> bool {
        port_index == 0 || (self.board == SerialBoard::TwoSio88 && port_index == 1)
    }

    pub(crate) fn data_port_matches(&self, port: u8) -> bool {
        let state = self.state.borrow();
        match self.board {
            SerialBoard::Sio88 => port == state.sio_hardware().address.data(),
            SerialBoard::TwoSio88 => matches!(state.two_sio_straps().address.offset(port), Some(1) | Some(3)),
        }
    }

    pub(crate) fn decodes_port(&self, port: u8) -> bool {
        let state = self.state.borrow();
        match self.board {
            SerialBoard::Sio88 => {
                port == state.sio_hardware().address.status()
                    || port == state.sio_hardware().address.data()
            }
            SerialBoard::TwoSio88 => state.two_sio_straps().address.offset(port).is_some(),
        }
    }

    pub(crate) fn peek_input(&self, port: u8) -> u8 { self.state.borrow().peek_input(port) }
    pub(crate) fn debugger_input(&self, port: u8) -> u8 {
        let value = self.state.borrow_mut().input(port);
        self.connector_dirty.set(true);
        value
    }
    pub(crate) fn debugger_output(&self, port: u8, value: u8) {
        self.state.borrow_mut().output(port, value);
        self.connector_dirty.set(true);
    }

    pub(crate) fn rx_len(&self, port_index: usize) -> usize {
        let state = self.state.borrow();
        match (self.board, port_index) {
            (_, 0) => state.serial_rx_len(),
            (SerialBoard::TwoSio88, 1) => state.port1_rx_len(),
            _ => 0,
        }
    }

    pub(crate) fn rx_line_idle(&self, port_index: usize) -> bool {
        let state = self.state.borrow();
        match (self.board, port_index) {
            (_, 0) => state.serial_rx_line_idle(),
            (SerialBoard::TwoSio88, 1) => state.port1_rx_line_idle(),
            _ => true,
        }
    }

    pub(crate) fn tx_busy(&self, port_index: usize) -> bool {
        let state = self.state.borrow();
        match (self.board, port_index) {
            (_, 0) => state.serial_tx_busy(),
            (SerialBoard::TwoSio88, 1) => state.port1_tx_busy(),
            _ => false,
        }
    }

    pub(crate) fn tx_complete(&self, port_index: usize) -> Option<u8> {
        let mut state = self.state.borrow_mut();
        let completed = match (self.board, port_index) {
            (_, 0) => state.serial_tx_complete(),
            (SerialBoard::TwoSio88, 1) => state.port1_tx_complete(),
            _ => None,
        };
        self.connector_dirty.set(true);
        completed
    }

    pub(crate) fn clear(&self) {
        self.state.borrow_mut().clear_serial();
        self.connector_dirty.set(true);
    }

    pub(crate) fn modem_lines(&self, port_index: usize) -> Option<(bool, bool, bool, bool)> {
        self.state.borrow().modem_lines(port_index)
    }

    pub(crate) fn set_modem_inputs(&self, port_index: usize, cts: bool, dcd: bool) -> bool {
        let accepted = self.state.borrow_mut().set_modem_inputs(port_index, cts, dcd);
        self.connector_dirty.set(true);
        accepted
    }

    pub(crate) fn set_receive_break(&self, port_index: usize, active: bool) -> bool {
        let accepted = self.state.borrow_mut().set_receive_break(port_index, active);
        self.connector_dirty.set(true);
        accepted
    }

    pub(crate) fn sio_handshake_lines(&self) -> Option<(bool, bool, bool, bool, bool, bool)> {
        let state = self.state.borrow();
        let lines = state.sio_handshake_lines()?;
        Some((lines.rsi_high, lines.input_device_ready, lines.output_device_ready,
              lines.tso_high, lines.bin_high, lines.bot_high))
    }

    pub(crate) fn pulse_sio_input_device_ready(&self) -> bool {
        let pulsed = self.state.borrow_mut().pulse_sio_input_device_ready();
        self.connector_dirty.set(true);
        pulsed
    }

    pub(crate) fn pulse_sio_output_device_ready(&self) -> bool {
        let pulsed = self.state.borrow_mut().pulse_sio_output_device_ready();
        self.connector_dirty.set(true);
        pulsed
    }

    pub(crate) fn debugger_inject_rx(&self, port: u8, byte: u8) -> bool {
        let injected = self.state.borrow_mut().debugger_inject_rx(port, byte);
        self.connector_dirty.set(true);
        injected
    }

    pub(crate) fn debugger_clear_rx(&self, port: u8) -> bool {
        let cleared = self.state.borrow_mut().debugger_clear_rx(port);
        self.connector_dirty.set(true);
        cleared
    }

    pub(crate) fn debugger_clear_tx(&self, port: u8) -> bool {
        let cleared = self.state.borrow_mut().debugger_clear_tx(port);
        self.connector_dirty.set(true);
        cleared
    }

    pub(crate) fn debugger_complete_tx(&self, port: u8) -> Option<u8> {
        let completed = self.state.borrow_mut().debugger_complete_tx(port);
        self.connector_dirty.set(true);
        completed
    }

    pub(crate) fn vector_interrupt_requests(&self) -> u8 {
        self.state.borrow().vector_interrupt_requests()
    }

    pub(crate) fn sio_hardware(&self) -> Option<SioHardwareConfig> {
        (self.board == SerialBoard::Sio88).then(|| self.state.borrow().sio_hardware())
    }

    pub(crate) fn two_sio_straps(&self) -> Option<TwoSioStraps> {
        (self.board == SerialBoard::TwoSio88).then(|| self.state.borrow().two_sio_straps())
    }

    pub(crate) fn two_sio_interrupt_wiring(&self) -> Option<TwoSioInterruptWiring> {
        (self.board == SerialBoard::TwoSio88)
            .then(|| self.state.borrow().two_sio_interrupt_wiring())
    }

    pub(crate) fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        self.state.borrow().trace_port_activity(port)
    }
    pub(crate) fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.state.borrow().trace_snapshot()
    }
    pub(crate) fn io_trace_enabled(&self) -> bool { self.state.borrow().trace_enabled() }
    pub(crate) fn set_io_trace_enabled(&self, enabled: bool) {
        self.state.borrow_mut().set_trace_enabled(enabled);
    }
    pub(crate) fn clear_io_trace(&self) { self.state.borrow_mut().clear_trace(); }
}

impl S100IoRegisterDevice for RuntimeSerialCardDevice {
    fn read_register(&mut self, offset: u8) -> u8 {
        let port = self.handle.base.wrapping_add(offset);
        self.handle.state.borrow_mut().input(port)
    }

    fn write_register(&mut self, offset: u8, value: u8) {
        let port = self.handle.base.wrapping_add(offset);
        self.handle.state.borrow_mut().output(port, value);
    }

    fn bus_lines(&self) -> S100IoDeviceLines {
        let state = self.handle.state.borrow();
        let lines = S100IoDeviceLines {
            pint: state.interrupt_request(),
            vi_asserted: state.vector_interrupt_requests(),
            ready_low: self.input_wait_active,
        };
        self.handle.connector_dirty.set(false);
        lines
    }

    fn observe_bus(&mut self, sample: &S100BusSample, selected: bool) -> bool {
        let mut drive_dirty = false;
        let clear = sample.signal_level(S100Signal::PowerOnClear) == Some(true);
        if clear && !self.previous_clear {
            self.handle.clear();
            self.input_wait_active = false;
            self.input_cycle_waited = false;
            drive_dirty = true;
        }
        self.previous_clear = clear;

        if self.handle.board != SerialBoard::TwoSio88 {
            return drive_dirty;
        }

        let input_cycle = selected && sample.signal_level(S100Signal::Inp) == Some(true);
        if !input_cycle {
            drive_dirty |= self.input_wait_active;
            self.input_wait_active = false;
            self.input_cycle_waited = false;
            return drive_dirty;
        }
        let waiting = sample.signal_level(S100Signal::Wait) == Some(true);
        if waiting && self.input_wait_active {
            self.input_wait_active = false;
            self.input_cycle_waited = true;
            drive_dirty = true;
        } else if !waiting && !self.input_cycle_waited && !self.input_wait_active {
            self.input_wait_active = true;
            drive_dirty = true;
        }
        drive_dirty
    }

    fn external_drive_dirty(&self) -> bool { self.handle.connector_dirty.get() }

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
    /// D0=input-enable and D1=output-enable flip-flops written through the
    /// 88-SIO control channel. Physical routing lives in SioHardwareConfig so
    /// the whole dormant/installed card is one persisted hardware state.
    sio_interrupt_control: u8,
    /// True only for the chassis owned by the exact Cycle backend. Every call to
    /// `drive_cpu_board_sample` is then one real 8080 clock T-state and may advance
    /// independent card oscillators without making the semantic adapter double-count
    /// its reconstructed samples.
    exact_t_state_clock_owner: bool,
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
            sio_interrupt_control: 0,
            exact_t_state_clock_owner: false,
            diagnostic_meter: None,
            diagnostic_result: None,
        };
        s.initialize_memory();
        s
    }
}

impl AltairBus {
    pub(crate) fn cycle_uses_physical_serial(&self) -> bool {
        self.exact_t_state_clock_owner && self.memory.uses_explicit_hardware()
    }

    pub(crate) fn set_exact_t_state_clock_owner(&mut self, enabled: bool) {
        self.exact_t_state_clock_owner = enabled;
    }

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
        if self.cycle_uses_physical_serial() {
            let _ = self.memory.serial_receive(0, byte);
            return;
        }
        self.io.serial_receive(byte);
        self.refresh_interrupt_request_line();
    }
    pub fn serial_rx_empty(&self) -> bool {
        if self.cycle_uses_physical_serial() { self.memory.serial_rx_empty(0) } else { self.io.serial_rx_empty() }
    }
    pub fn serial_rx_len(&self) -> usize {
        if self.cycle_uses_physical_serial() { self.memory.serial_rx_len(0) } else { self.io.serial_rx_len() }
    }
    pub fn serial_tx_front(&self) -> Option<u8> {
        if self.cycle_uses_physical_serial() { self.memory.serial_tx_front(0) } else { self.io.serial_tx_front() }
    }
    pub fn serial_tx_complete(&mut self) -> Option<u8> {
        if self.cycle_uses_physical_serial() { return self.memory.serial_tx_complete(0); }
        let completed = self.io.serial_tx_complete();
        self.refresh_interrupt_request_line();
        completed
    }
    pub fn tx_busy(&self) -> bool {
        if self.cycle_uses_physical_serial() { self.memory.serial_tx_busy(0) } else { self.io.serial_tx_busy() }
    }
    pub fn clear_serial(&mut self) {
        if self.cycle_uses_physical_serial() {
            self.memory.clear_serial();
            return;
        }
        self.io.clear_serial();
        self.sio_interrupt_control = 0;
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

    fn sync_cpu_inte(&mut self, enabled: bool) { self.s100.set_inte(enabled); }
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

    fn sio_internal_interrupt_sources(&self) -> (bool, bool) {
        if self.io.serial_board() != SerialBoard::Sio88 {
            return (false, false);
        }
        let status = self.peek_io_port(self.io.sio_hardware().address.status());
        let input = self.sio_interrupt_control & 0x01 != 0 && status & 0x01 == 0;
        let output = self.sio_interrupt_control & 0x02 != 0 && status & 0x80 == 0;
        (input, output)
    }

    fn sio_pint_request(&self) -> bool {
        let (input, output) = self.sio_internal_interrupt_sources();
        let wiring = self.io.sio_hardware().interrupt_wiring;
        (input && wiring.input.drives_pint()) || (output && wiring.output.drives_pint())
    }

    pub fn configure_sio_interrupt_wiring(&mut self, wiring: SioInterruptWiring) {
        let mut config = self.io.sio_hardware();
        if config.interrupt_wiring == wiring { return; }
        config.interrupt_wiring = wiring;
        self.io.configure_sio_hardware(config);
        self.sio_interrupt_control = 0;
        self.refresh_interrupt_request_line();
    }

    pub fn sio_interrupt_wiring(&self) -> SioInterruptWiring {
        self.io.sio_hardware().interrupt_wiring
    }

    pub fn sio_vector_interrupt_requests(&self) -> u8 {
        if self.cycle_uses_physical_serial() { return self.memory.serial_vector_interrupt_requests(); }
        if self.io.serial_board() != SerialBoard::Sio88 { return 0; }
        let (input, output) = self.sio_internal_interrupt_sources();
        let wiring = self.io.sio_hardware().interrupt_wiring;
        let mut mask = 0u8;
        if input {
            if let Some(level) = wiring.input.vector_level() { mask |= 1u8 << level; }
        }
        if output {
            if let Some(level) = wiring.output.vector_level() { mask |= 1u8 << level; }
        }
        mask
    }

    pub(crate) fn refresh_interrupt_request_line(&mut self) {
        if self.cycle_uses_physical_serial() { return; }
        let asserted = match self.io.serial_board() {
            SerialBoard::Sio88 => self.sio_pint_request(),
            SerialBoard::TwoSio88 => self.serial_interrupt_request(),
        };
        self.s100.set_interrupt_request(asserted);
    }

    pub(crate) fn direct_interrupt_opcode(&self) -> u8 { self.serial_interrupt_opcode() }

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
        if self.exact_t_state_clock_owner {
            if self.memory.uses_explicit_hardware() {
                self.memory.advance_serial_time(1);
            } else {
                self.io.advance_t_states(1);
            }
        }
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
        let data = self.memory.preview_read(address);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100.drive_power_on_state(address, data, protected, inte, run);
    }

    fn assert_front_panel_reset_bus(&mut self, run: bool) {
        self.memory.reset_timing();
        self.s100.set_memory_ready_input(true);
        self.s100.assert_front_panel_reset(run);
    }

    fn release_front_panel_reset_bus(&mut self, address: u16, run: bool) {
        let data = self.memory.preview_read(address);
        let protected = self.memory.is_protected(address);
        let inte = self.s100.signals().inte;
        self.s100.release_front_panel_reset(address, data, protected, inte, run);
    }

    fn set_ext_clear(&mut self, asserted: bool) {
        let was_asserted = self.s100.signals().ext_clear;
        self.s100.set_ext_clear(asserted);
        if asserted && !was_asserted {
            self.io.clear_serial();
            self.sio_interrupt_control = 0;
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
        if port != 0xff { self.fast_account_io_input_wait(port); }
        let value = match port { 0xff => self.panel.input(), _ => self.io.input(port) };
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::InputRead);
        if port != 0xff { self.refresh_interrupt_request_line(); }
        value
    }

    fn output(&mut self, port: u8, value: u8) {
        self.drive_cpu_cycle(Self::io_bus_address(port), value, S100Cycle::OutputWrite);
        if port != 0xff {
            if self.io.serial_board() == SerialBoard::Sio88
                && port == self.io.sio_hardware().address.status()
            {
                self.sio_interrupt_control = value & 0x03;
            }
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

    fn take_wait_states(&mut self) -> u32 { self.take_fast_memory_wait_t_states() }

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
        if self.powered { self.front_panel_reset(); } else { self.bus.power_off_s100(); }
    }

    pub fn installed_ram_bytes(&self) -> usize { self.bus.installed_ram_bytes() }
    pub fn configure_memory_board_profile(&mut self, profile: RamBoardProfile) { self.bus.configure_memory_board_profile(profile); }
    pub fn memory_board_profile(&self, address: u16) -> Option<RamBoardProfile> { self.bus.memory_board_profile(address) }
    pub fn arm_basic32_full_memory_probe_guard(&mut self) -> bool { self.bus.arm_basic32_full_memory_probe_guard() }

    pub fn begin_cpu_diagnostic_meter(
        &mut self,
        name: String,
        bdos_start: u16,
        bdos_len: usize,
        expected_instructions: Option<u64>,
        expected_t_states: Option<u64>,
    ) {
        self.bus.begin_cpu_diagnostic_meter(name, bdos_start, bdos_len, expected_instructions, expected_t_states);
    }
    pub fn take_cpu_diagnostic_result(&mut self) -> Option<CpuDiagnosticResult> { self.bus.take_cpu_diagnostic_result() }

    pub fn power(&mut self, on: bool) { self.power_with_historical_run_latch(on, false); }

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
                self.running = true;
                self.bus.set_run(true);
                self.bus.set_ready(true);
            } else {
                self.set_running(true);
            }
        } else if !self.bus.reset_asserted() && !self.cpu.halted {
            self.set_running(false);
        }
    }

    pub fn release_run_stop(&mut self, run: bool) {
        if run { self.run_switch_asserted = false; } else { self.stop_switch_asserted = false; }
    }

    pub fn assert_front_panel_reset(&mut self) {
        if !self.powered { return; }
        self.bus.cancel_cpu_diagnostic_meter();
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.assert_front_panel_reset_bus(self.running);
    }

    fn release_front_panel_reset_common(&mut self, fast_capture_pending_stop: bool) {
        if !self.powered { return; }
        self.cpu.reset();
        if fast_capture_pending_stop && self.stop_switch_asserted {
            self.running = false;
            self.bus.set_run(false);
        }
        let address = self.bus.panel.reset_address();
        self.bus.sync_cpu_inte(self.cpu.inte);
        self.bus.set_hlda(false);
        self.bus.release_front_panel_reset(address, self.running);
    }

    pub fn release_front_panel_reset(&mut self) { self.release_front_panel_reset_common(true); }
    pub fn front_panel_reset(&mut self) { if self.powered { self.assert_front_panel_reset(); self.release_front_panel_reset(); } }
    pub fn reset(&mut self) { if self.powered { self.front_panel_reset(); self.bus.clear_serial(); } }
    pub fn assert_front_panel_clear(&mut self) { if self.powered { self.bus.set_ext_clear(true); } }
    pub fn release_front_panel_clear(&mut self) { if self.powered { self.bus.set_ext_clear(false); } }
    pub fn clear_io(&mut self) { if self.powered { self.assert_front_panel_clear(); self.release_front_panel_clear(); } }

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
        if !lines.interrupt || !self.cpu.inte { return 0; }

        let opcode = self.bus.direct_interrupt_opcode();
        self.bus.sync_cpu_inte(false);
        let before = self.cpu.cycles;
        let accepted = self.cpu.interrupt(&mut self.bus, opcode);
        debug_assert!(accepted);
        self.bus.sync_cpu_inte(self.cpu.inte);
        let elapsed = self.cpu.cycles.saturating_sub(before) as u32;
        self.bus.advance_serial_hardware_time(u64::from(elapsed));
        elapsed
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
            let elapsed = self.cpu.step(&mut self.bus);
            self.bus.advance_serial_hardware_time(u64::from(elapsed));
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
                let elapsed = self.cpu.step(&mut self.bus);
                self.bus.advance_serial_hardware_time(u64::from(elapsed));
                used = used.saturating_add(elapsed);
            }
            self.bus.sync_cpu_inte(self.cpu.inte);
        }
    }

    pub fn advance_idle_chassis_time(&mut self, t_states: u64) {
        if self.powered { self.bus.advance_serial_hardware_time(t_states); }
    }

    pub fn request_hold(&mut self, hold: bool) { self.bus.set_hold(hold); if !hold { self.bus.set_hlda(false); } }

    pub fn commit_panel_activity(&mut self, dt: Duration) {
        let dynamic = self.powered && self.running && !self.cpu.halted && !self.bus.hlda() && !self.bus.reset_asserted();
        self.bus.commit_panel_activity(dt, dynamic);
    }

    pub fn examine(&mut self, next: bool) { self.fast_front_panel_examine_via_cpu_board(next); }
    pub fn deposit(&mut self, next: bool) { self.fast_front_panel_deposit_via_cpu_board(next); }
    pub fn protect_current_board(&mut self, protected: bool) { self.front_panel_set_memory_protection_via_s100(protected); }
    pub fn current_board_protected(&self) -> bool { self.powered && self.bus.s100.signals().prot }
    pub fn panel_switches(&self) -> u16 { self.bus.panel_switches() }
    pub fn toggle_sense_switch(&mut self, bit: usize) { self.bus.toggle_panel_switch(bit); }
    pub fn address_leds(&self) -> u16 { self.bus.panel_address() }
    pub fn data_leds(&self) -> u8 { self.bus.panel_data() }
    pub fn panel_lamps(&self) -> PanelLampSnapshot { self.bus.panel_lamps() }
    pub fn wait_led(&self) -> bool { self.powered && self.bus.s100.signals().wait }
    pub fn ext_clear_asserted(&self) -> bool { self.powered && self.bus.ext_clear_asserted() }

    pub fn configure_sio_interrupt_wiring(&mut self, wiring: SioInterruptWiring) {
        if self.bus.sio_interrupt_wiring() == wiring { return; }
        self.running = false;
        self.bus.configure_sio_interrupt_wiring(wiring);
        self.bus.clear_transient_memory_guards();
        if self.powered && self.bus.serial_board() == SerialBoard::Sio88 {
            self.reset();
        } else {
            self.cpu.reset();
        }
    }

    pub fn sio_interrupt_wiring(&self) -> SioInterruptWiring { self.bus.sio_interrupt_wiring() }
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
        bus.begin_cpu_diagnostic_meter("TEST.COM".into(), 0xff00, 0x37, Some(7), Some(65));
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
        assert!(machine.running);
        machine.assert_front_panel_reset();
        assert!(machine.running);
        machine.release_front_panel_reset();
        assert!(!machine.running);
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
        assert!(!machine.bus.cpu_control_lines().interrupt, "88-SIO RDA/PINT must wait for the physical receive frame");
        machine.bus.advance_serial_hardware_time(200_000);
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
        assert!(!machine.bus.cpu_control_lines().interrupt, "HALTed CPU must not see 88-SIO PINT before RDA is physically ready");
        machine.bus.advance_serial_hardware_time(200_000);
        assert!(machine.bus.cpu_control_lines().interrupt);
        machine.run_cycles(11);
        assert!(!machine.cpu.halted);
        assert!(!machine.cpu.inte);
        assert_eq!(machine.cpu.pc, 0x0038);
        assert_eq!(machine.cpu.sp, 0x03fe);
    }

    #[test]
    fn sio_rev1_input_and_output_sources_route_independently() {
        let mut bus = AltairBus::default();
        bus.configure_sio_interrupt_wiring(SioInterruptWiring {
            input: SioInterruptTarget::Vi3,
            output: SioInterruptTarget::Disconnected,
        });
        bus.output(0x00, 0x01);
        assert!(bus.debugger_inject_serial_rx(0x01, b'I'));
        assert!(!bus.cpu_control_lines().interrupt, "VI3 must not masquerade as direct PINT");
        assert_eq!(bus.sio_vector_interrupt_requests(), 1 << 3);

        bus.configure_sio_interrupt_wiring(SioInterruptWiring {
            input: SioInterruptTarget::Disconnected,
            output: SioInterruptTarget::Pint,
        });
        bus.output(0x00, 0x02);
        assert!(bus.cpu_control_lines().interrupt, "enabled TBMT source wired to PINT must assert direct request");
        assert_eq!(bus.sio_vector_interrupt_requests(), 0);
    }

    #[test]
    fn sio_rev1_input_and_output_vi_lines_can_have_different_priorities() {
        let mut bus = AltairBus::default();
        bus.configure_sio_interrupt_wiring(SioInterruptWiring {
            input: SioInterruptTarget::Vi2,
            output: SioInterruptTarget::Vi5,
        });
        bus.output(0x00, 0x03);
        assert!(bus.debugger_inject_serial_rx(0x01, b'B'));
        assert!(!bus.cpu_control_lines().interrupt);
        assert_eq!(bus.sio_vector_interrupt_requests(), (1 << 2) | (1 << 5));
    }

    #[test]
    fn rev0_external_device_ready_interrupt_path_is_not_fabricated_from_com2502_ready() {
        let mut bus = AltairBus::default();
        let mut config = bus.sio_hardware();
        config.revision = SioRevision::Rev0;
        bus.configure_sio_hardware(config);
        bus.configure_sio_interrupt_wiring(SioInterruptWiring {
            input: SioInterruptTarget::Pint,
            output: SioInterruptTarget::Pint,
        });
        bus.output(0x00, 0x03);
        assert!(bus.debugger_inject_serial_rx(0x01, b'R'));
        assert!(!bus.cpu_control_lines().interrupt, "unmodified Rev0 requires external device-ready flip-flops; COM2502 RDA/TBMT must not be silently substituted");
        assert_eq!(bus.sio_vector_interrupt_requests(), 0);
    }

    #[test]
    fn rev0_external_device_ready_interrupts_follow_rin_rot_and_data_handshake() {
        let mut bus = AltairBus::default();
        let mut config = bus.sio_hardware();
        config.revision = SioRevision::Rev0;
        bus.configure_sio_hardware(config);

        bus.configure_sio_interrupt_wiring(SioInterruptWiring {
            input: SioInterruptTarget::Pint,
            output: SioInterruptTarget::Disconnected,
        });
        bus.output(0x00, 0x01);
        assert!(!bus.cpu_control_lines().interrupt);
        assert!(bus.pulse_sio_input_device_ready());
        assert!(bus.cpu_control_lines().interrupt, "RIN ready plus D0 enable must reach PINT");
        assert_eq!(bus.sio_handshake_lines(), Some((true, false, true, false)));
        let _ = bus.debugger_input_port(0x01);
        assert!(!bus.cpu_control_lines().interrupt, "DATA IN resets the Rev0 input-ready flip-flop");
        assert_eq!(bus.sio_handshake_lines(), Some((false, false, false, false)));

        bus.configure_sio_interrupt_wiring(SioInterruptWiring {
            input: SioInterruptTarget::Disconnected,
            output: SioInterruptTarget::Vi4,
        });
        bus.output(0x00, 0x02);
        assert!(bus.pulse_sio_output_device_ready());
        assert!(!bus.cpu_control_lines().interrupt, "VI4 must remain a raw vector request, not direct PINT");
        assert_eq!(bus.sio_vector_interrupt_requests(), 1 << 4);
        assert_eq!(bus.sio_handshake_lines(), Some((false, true, false, true)));
        bus.debugger_output_port(0x01, b'O');
        assert_eq!(bus.sio_vector_interrupt_requests(), 0, "DATA OUT resets the Rev0 output-ready flip-flop");
        assert_eq!(bus.sio_handshake_lines(), Some((false, false, false, false)));
    }

    #[test]
    fn idle_uart_time_does_not_dirty_an_unchanged_s100_connector() {
        let (device, handle) = RuntimeSerialCardHandle::new_two_sio(
            TwoSioStraps::default(),
            TwoSioInterruptWiring::default(),
        );
        let _ = device.bus_lines();
        assert!(!device.external_drive_dirty());
        handle.advance_t_states(1);
        assert!(!device.external_drive_dirty());
    }
}
