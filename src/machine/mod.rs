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

use crate::config::{
    RamInit, RamSize, SerialBoard, SioHardwareConfig, SioInterruptTarget,
    SioInterruptWiring, SioRevision, TwoSioInterruptWiring, TwoSioStraps,
};
use crate::s100::S100Signal;
use crate::s100_backplane::S100BusSample;
use crate::s100_io_card::{S100IoDeviceLines, S100IoRegisterDevice};
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
/// instance instead of constructing a second UART. Explicit Adaptive Cycle
/// chassis routes endpoints and debugger operations through these per-slot
/// handles; the singleton remains only for aggregate-configuration compatibility.
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

    pub(crate) const fn board(&self) -> SerialBoard { self.board }

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
        if received { self.connector_dirty.set(true); }
        received
    }

    pub(crate) fn advance_t_states(&self, t_states: u64) {
        if t_states == 0 { return; }
        let mut state = self.state.borrow_mut();
        let before_pint = state.interrupt_request();
        let before_vi = state.vector_interrupt_requests();
        state.advance_t_states(t_states);
        let connector_changed = before_pint != state.interrupt_request()
            || before_vi != state.vector_interrupt_requests();
        drop(state);
        if connector_changed { self.connector_dirty.set(true); }
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
        Some((
            lines.rsi_high,
            lines.input_device_ready,
            lines.output_device_ready,
            lines.tso_high,
            lines.bin_high,
            lines.bot_high,
        ))
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
    /// D0=input-enable and D1=output-enable flip-flops written through the
    /// 88-SIO control channel. Physical routing lives in SioHardwareConfig so
    /// the whole dormant/installed card is one persisted hardware state.
    sio_interrupt_control: u8,
    /// True only for the chassis owned by Adaptive Cycle. Every call to
    /// `drive_cpu_board_sample` is then one real 8080 clock T-state and may
    /// advance independent card oscillators exactly once.
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
    pub fn set_protected(&mut self, address: u16, protected: bool) {
        self.memory.set_protected(address, protected);
        self.refresh_protect_line();
    }

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
    fn hold_requested(&self) -> bool { self.s100.signals().hold }
    fn set_hlda(&mut self, hlda: bool) { self.s100.set_hlda(hlda); }
    fn hlda(&self) -> bool { self.s100.signals().hlda }
    fn reset_asserted(&self) -> bool { self.s100.signals().reset }
    fn ext_clear_asserted(&self) -> bool { self.s100.signals().ext_clear }
    fn freeze_panel_bus(&mut self) { self.s100.freeze(); }
    fn commit_panel_activity(&mut self, dt: Duration, dynamic: bool) { self.s100.commit(dt, dynamic); }

    /// Active 88-SIO interrupt sources after software D0/D1 enables but before
    /// the physical IN/OUT/BH routing pads. D0/D7 are already resolved by the
    /// selected board revision: Rev0 exposes the external RIN/ROT device-ready
    /// flip-flops there, while Rev1 exposes the internal COM2502 RDA/TBMT ready
    /// conditions. Routing therefore stays revision-agnostic at this boundary.
    fn sio_internal_interrupt_sources(&self) -> (bool, bool) {
        if self.io.serial_board() != SerialBoard::Sio88 { return (false, false); }
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

    /// Active raw 88-SIO requests presented to an optional 88-VI board. Bit n is
    /// VIn. These raw lines never fabricate a processor restart opcode by
    /// themselves; only a separate 88-VI implementation may arbitrate them.
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
            // Advance the independent serial-card oscillators by exactly this real
            // CPU-clock quantum, but leave PINT projection to Cycle's existing
            // post-sample refresh so the Teacher snapshot keeps the interrupt
            // level the processor actually saw on this tick.
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
