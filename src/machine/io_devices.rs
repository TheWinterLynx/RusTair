use std::collections::VecDeque;

use crate::config::{
    SerialBoard, SioHardwareConfig, TwoSioBaudTap as ConfigTwoSioBaudTap,
    TwoSioInterruptTarget, TwoSioInterruptWiring, TwoSioStraps,
};
use crate::cpu8080::Bus;

use super::memory::{MemoryReadyPhase, S100_OPEN_BUS_VALUE};
use super::serial::sio::SioPort;
use super::{AltairBus, AltairMachine, CLOCK_HZ};

#[path = "two_sio.rs"]
mod two_sio;
use two_sio::{TwoSioBaudTap, TwoSioPort};

// Historical/default addresses retained only for regression clarity. Production
// 88-SIO decode is owned by SioHardwareConfig::address and 88-2SIO decode by
// TwoSioStraps::address.
#[cfg(test)]
const SIO_STATUS_PORT: u8 = 0x00;
#[cfg(test)]
const SIO_DATA_PORT: u8 = 0x01;
#[cfg(test)]
const SIO2_PORT0_STATUS: u8 = 0x10;
#[cfg(test)]
const SIO2_PORT0_DATA: u8 = 0x11;
#[cfg(test)]
const SIO2_PORT1_STATUS: u8 = 0x12;
#[cfg(test)]
const SIO2_PORT1_DATA: u8 = 0x13;
const IO_TRACE_LIMIT: usize = 4096;

pub(crate) const IO_TRACE_IN: u8 = 0;
pub(crate) const IO_TRACE_OUT: u8 = 1;
pub(crate) const IO_TRACE_RX_ENQUEUE: u8 = 2;
pub(crate) const IO_TRACE_TX_COMPLETE: u8 = 3;

#[derive(Clone, Copy, Debug, Default)]
struct IoPortActivity {
    last_in: Option<u8>,
    last_out: Option<u8>,
    in_count: u64,
    out_count: u64,
}

#[derive(Clone, Copy, Debug)]
struct IoTraceEvent {
    sequence: u64,
    kind: u8,
    port: u8,
    value: u8,
    repeat: u32,
}

struct IoTrace {
    enabled: bool,
    events: VecDeque<IoTraceEvent>,
    ports: [IoPortActivity; 256],
    next_sequence: u64,
}

impl Default for IoTrace {
    fn default() -> Self {
        Self {
            enabled: false,
            events: VecDeque::new(),
            ports: [IoPortActivity::default(); 256],
            next_sequence: 1,
        }
    }
}

impl IoTrace {
    fn record(&mut self, kind: u8, port: u8, value: u8) {
        if !self.enabled { return; }
        let activity = &mut self.ports[port as usize];
        match kind {
            IO_TRACE_IN => {
                activity.last_in = Some(value);
                activity.in_count = activity.in_count.saturating_add(1);
            }
            IO_TRACE_OUT => {
                activity.last_out = Some(value);
                activity.out_count = activity.out_count.saturating_add(1);
            }
            _ => {}
        }
        if let Some(last) = self.events.back_mut() {
            if last.kind == kind && last.port == port && last.value == value {
                last.repeat = last.repeat.saturating_add(1);
                return;
            }
        }
        self.events.push_back(IoTraceEvent {
            sequence: self.next_sequence,
            kind,
            port,
            value,
            repeat: 1,
        });
        self.next_sequence = self.next_sequence.saturating_add(1);
        while self.events.len() > IO_TRACE_LIMIT { self.events.pop_front(); }
    }

    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
    fn clear_events(&mut self) {
        self.events.clear();
        self.ports.fill(IoPortActivity::default());
    }
    fn snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.events.iter().map(|e| (e.sequence, e.kind, e.port, e.value, e.repeat)).collect()
    }
    fn port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        let a = self.ports[port as usize];
        (a.last_in, a.last_out, a.in_count, a.out_count)
    }
}

fn card_baud_tap(tap: ConfigTwoSioBaudTap) -> TwoSioBaudTap {
    match tap {
        ConfigTwoSioBaudTap::Baud110 => TwoSioBaudTap::Baud110,
        ConfigTwoSioBaudTap::Baud150 => TwoSioBaudTap::Baud150,
        ConfigTwoSioBaudTap::Baud300 => TwoSioBaudTap::Baud300,
        ConfigTwoSioBaudTap::Baud1200 => TwoSioBaudTap::Baud1200,
        ConfigTwoSioBaudTap::Baud1800 => TwoSioBaudTap::Baud1800,
        ConfigTwoSioBaudTap::Baud2400 => TwoSioBaudTap::Baud2400,
        ConfigTwoSioBaudTap::Baud4800 => TwoSioBaudTap::Baud4800,
        ConfigTwoSioBaudTap::Baud9600 => TwoSioBaudTap::Baud9600,
    }
}

fn configured_two_sio_ports(straps: TwoSioStraps) -> [TwoSioPort; 2] {
    [
        TwoSioPort::new(card_baud_tap(straps.port0_baud)),
        TwoSioPort::new(card_baud_tap(straps.port1_baud)),
    ]
}

pub(super) struct IoDevices {
    /// One physical MITS 88-SIO card built around a finite COM2502 UART.
    sio: SioPort,
    /// Two physical MC6850 channels with independent board baud-generator taps.
    two_sio: [TwoSioPort; 2],
    /// Hardware straps on the installed/dormant 88-2SIO board. A2-A7 select one
    /// four-address block; each ACIA also has its own baud-generator tap.
    two_sio_straps: TwoSioStraps,
    /// Physical DI/EI interrupt wiring. This is separate from MC6850 IRQ state:
    /// an ACIA may request service while its board output is disconnected or sent
    /// to one of the eight 88-VI inputs rather than to the processor PINT line.
    two_sio_interrupt_wiring: TwoSioInterruptWiring,
    serial_board: SerialBoard,
    /// 88-SIO board-level interrupt-enable latch retained from the existing
    /// machine model. Its exact revision-sensitive wiring is audited separately
    /// from the COM2502 status/data/timing closeout.
    sio_control: u8,
    trace: IoTrace,
}

impl Default for IoDevices {
    fn default() -> Self {
        let two_sio_straps = TwoSioStraps::default();
        Self {
            sio: SioPort::default(),
            two_sio: configured_two_sio_ports(two_sio_straps),
            two_sio_straps,
            two_sio_interrupt_wiring: TwoSioInterruptWiring::default(),
            serial_board: SerialBoard::default(),
            sio_control: 0,
            trace: IoTrace::default(),
        }
    }
}

impl IoDevices {
    pub(super) fn configure_serial_board(&mut self, board: SerialBoard) {
        self.serial_board = board;
        self.clear_serial();
    }
    pub(super) fn serial_board(&self) -> SerialBoard { self.serial_board }

    pub(super) fn configure_sio_hardware(&mut self, config: SioHardwareConfig) {
        self.sio.configure(config);
        self.sio_control = 0;
    }
    pub(super) fn sio_hardware(&self) -> SioHardwareConfig { self.sio.config() }

    pub(super) fn configure_two_sio_straps(&mut self, straps: TwoSioStraps) {
        if self.two_sio_straps == straps { return; }
        self.two_sio_straps = straps;
        // Moving jumpers is a physical reconfiguration. Rebuild both ACIAs/card
        // clocks instead of attempting to preserve impossible in-flight state.
        self.two_sio = configured_two_sio_ports(straps);
    }
    pub(super) fn two_sio_straps(&self) -> TwoSioStraps { self.two_sio_straps }

    pub(super) fn configure_two_sio_interrupt_wiring(&mut self, wiring: TwoSioInterruptWiring) {
        self.two_sio_interrupt_wiring = wiring;
    }
    pub(super) fn two_sio_interrupt_wiring(&self) -> TwoSioInterruptWiring {
        self.two_sio_interrupt_wiring
    }

    fn two_sio_port(&self, index: usize) -> Option<&TwoSioPort> {
        (self.serial_board == SerialBoard::TwoSio88)
            .then(|| self.two_sio.get(index))
            .flatten()
    }
    fn two_sio_port_mut(&mut self, index: usize) -> Option<&mut TwoSioPort> {
        if self.serial_board != SerialBoard::TwoSio88 { return None; }
        self.two_sio.get_mut(index)
    }
    fn two_sio_irq(&self, index: usize) -> bool {
        self.two_sio
            .get(index)
            .map_or(false, TwoSioPort::interrupt_request)
    }
    fn two_sio_pint_request(&self) -> bool {
        [0usize, 1].into_iter().any(|index| {
            self.two_sio_interrupt_wiring
                .target(index)
                .map_or(false, TwoSioInterruptTarget::drives_pint)
                && self.two_sio_irq(index)
        })
    }
    pub(super) fn vector_interrupt_requests(&self) -> u8 {
        if self.serial_board != SerialBoard::TwoSio88 { return 0; }
        let mut mask = 0u8;
        for index in [0usize, 1] {
            if !self.two_sio_irq(index) { continue; }
            let Some(level) = self
                .two_sio_interrupt_wiring
                .target(index)
                .and_then(TwoSioInterruptTarget::vector_level)
            else {
                continue;
            };
            mask |= 1u8 << level;
        }
        mask
    }

    /// `(RTS high, BREAK active, CTS high, DCD high)` at the physical MC6850 pins.
    pub(super) fn modem_lines(&self, index: usize) -> Option<(bool, bool, bool, bool)> {
        let port = self.two_sio_port(index)?;
        Some((port.rts_high(), port.break_active(), port.cts_high(), port.dcd_high()))
    }
    pub(super) fn set_modem_inputs(&mut self, index: usize, cts_high: bool, dcd_high: bool) -> bool {
        let Some(port) = self.two_sio_port_mut(index) else { return false; };
        port.set_cts_high(cts_high);
        port.set_dcd_high(dcd_high);
        true
    }

    fn sio_status_port(&self) -> u8 { self.sio.config().address.status() }
    fn sio_data_port(&self) -> u8 { self.sio.config().address.data() }

    fn data_port_for_index(&self, index: usize) -> u8 {
        match (self.serial_board, index) {
            (SerialBoard::Sio88, 0) => self.sio_data_port(),
            (SerialBoard::TwoSio88, 0) => self.two_sio_straps.address.port0_data(),
            (SerialBoard::TwoSio88, 1) => self.two_sio_straps.address.port1_data(),
            (_, 1) => self.two_sio_straps.address.port1_data(),
            _ => self.sio_data_port(),
        }
    }
    fn data_port_index(&self, port: u8) -> Option<usize> {
        match self.serial_board {
            SerialBoard::Sio88 if port == self.sio_data_port() => Some(0),
            SerialBoard::TwoSio88 => match self.two_sio_straps.address.offset(port) {
                Some(1) => Some(0),
                Some(3) => Some(1),
                _ => None,
            },
            _ => None,
        }
    }
    fn two_sio_offset(&self, port: u8) -> Option<u8> {
        if self.serial_board != SerialBoard::TwoSio88 { return None; }
        self.two_sio_straps.address.offset(port)
    }
    fn two_sio_decodes_port(&self, port: u8) -> bool { self.two_sio_offset(port).is_some() }

    pub(super) fn input_wait_states(&self, port: u8) -> u8 {
        // MITS documents the one-Tw PRDY generator on the 88-2SIO. Do not copy
        // that timing onto the earlier 88-SIO without board-specific evidence.
        if self.two_sio_decodes_port(port) { 1 } else { 0 }
    }
    pub(super) fn ready_for_input_t_state(&self, port: u8, input_read: bool, phase: MemoryReadyPhase) -> bool {
        if !input_read || self.input_wait_states(port) == 0 { return true; }
        !matches!(phase, MemoryReadyPhase::T1 | MemoryReadyPhase::T2)
    }
    pub(super) fn advance_t_states(&mut self, t_states: u64) {
        if t_states == 0 { return; }
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.advance_t_states(t_states, CLOCK_HZ),
            SerialBoard::TwoSio88 => {
                for port in &mut self.two_sio { port.advance_t_states(t_states, CLOCK_HZ); }
            }
        }
    }

    pub(super) fn interrupt_request(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => {
                let rx_irq = self.sio_control & 0x01 != 0 && self.sio.rx_full();
                let tx_irq = self.sio_control & 0x02 != 0 && self.sio.tx_buffer_empty();
                rx_irq || tx_irq
            }
            SerialBoard::TwoSio88 => self.two_sio_pint_request(),
        }
    }
    pub(super) const fn direct_interrupt_opcode(&self) -> u8 { 0xff }

    fn input_raw(&mut self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => {
                if port == self.sio_status_port() {
                    self.sio.status()
                } else if port == self.sio_data_port() {
                    self.sio.read_data()
                } else {
                    S100_OPEN_BUS_VALUE
                }
            }
            SerialBoard::TwoSio88 => match self.two_sio_straps.address.offset(port) {
                Some(0) => self.two_sio[0].read_status(),
                Some(1) => self.two_sio[0].read_data(),
                Some(2) => self.two_sio[1].read_status(),
                Some(3) => self.two_sio[1].read_data(),
                _ => S100_OPEN_BUS_VALUE,
            },
        }
    }
    fn peek_input(&self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => {
                if port == self.sio_status_port() {
                    self.sio.status()
                } else if port == self.sio_data_port() {
                    self.sio.peek_data()
                } else {
                    S100_OPEN_BUS_VALUE
                }
            }
            SerialBoard::TwoSio88 => match self.two_sio_straps.address.offset(port) {
                Some(0) => self.two_sio[0].peek_status(),
                Some(1) => self.two_sio[0].peek_data(),
                Some(2) => self.two_sio[1].peek_status(),
                Some(3) => self.two_sio[1].peek_data(),
                _ => S100_OPEN_BUS_VALUE,
            },
        }
    }
    pub(super) fn input(&mut self, port: u8) -> u8 {
        let value = self.input_raw(port);
        self.trace.record(IO_TRACE_IN, port, value);
        value
    }

    fn output_raw(&mut self, port: u8, value: u8) {
        match self.serial_board {
            SerialBoard::Sio88 => {
                if port == self.sio_status_port() {
                    self.sio_control = value & 0x03;
                } else if port == self.sio_data_port() {
                    self.sio.write_data(value);
                }
            }
            SerialBoard::TwoSio88 => match self.two_sio_straps.address.offset(port) {
                Some(0) => self.two_sio[0].write_control(value),
                Some(1) => self.two_sio[0].write_data(value),
                Some(2) => self.two_sio[1].write_control(value),
                Some(3) => self.two_sio[1].write_data(value),
                _ => {}
            },
        }
    }
    pub(super) fn output(&mut self, port: u8, value: u8) {
        self.output_raw(port, value);
        self.trace.record(IO_TRACE_OUT, port, value);
    }

    pub(super) fn serial_receive(&mut self, byte: u8) {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.queue_received_character(byte),
            SerialBoard::TwoSio88 => self.two_sio[0].queue_received_character(byte),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, self.data_port_for_index(0), byte);
    }
    pub(super) fn serial_rx_empty(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.receive_len() == 0,
            SerialBoard::TwoSio88 => self.two_sio[0].receive_len() == 0,
        }
    }
    pub(super) fn serial_rx_len(&self) -> usize {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.receive_len(),
            SerialBoard::TwoSio88 => self.two_sio[0].receive_len(),
        }
    }
    /// Whether the physical receive shift path may begin a new character. An
    /// unread holding register/RDR does not stop either UART's wire and may cause
    /// a real overrun when the next frame completes.
    pub(super) fn serial_rx_line_idle(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.receive_line_idle(),
            SerialBoard::TwoSio88 => self.two_sio[0].receive_line_idle(),
        }
    }
    pub(super) fn serial_tx_front(&self) -> Option<u8> {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.endpoint_tx_front(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_front(),
        }
    }
    pub(super) fn serial_tx_complete(&mut self) -> Option<u8> {
        let completed = match self.serial_board {
            SerialBoard::Sio88 => self.sio.endpoint_tx_complete(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_complete(),
        };
        if let Some(byte) = completed { self.trace.record(IO_TRACE_TX_COMPLETE, self.data_port_for_index(0), byte); }
        completed
    }
    pub(super) fn serial_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.endpoint_tx_pending_or_hardware_busy(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_pending_or_hardware_busy(),
        }
    }

    // The original 88-SIO is a single-channel card. Port 1 only exists on the
    // 88-2SIO and must not be fabricated for the earlier board.
    pub(super) fn port1_receive(&mut self, byte: u8) {
        if self.serial_board != SerialBoard::TwoSio88 { return; }
        self.two_sio[1].queue_received_character(byte);
        self.trace.record(IO_TRACE_RX_ENQUEUE, self.data_port_for_index(1), byte);
    }
    pub(super) fn port1_rx_empty(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => true,
            SerialBoard::TwoSio88 => self.two_sio[1].receive_len() == 0,
        }
    }
    pub(super) fn port1_rx_len(&self) -> usize {
        match self.serial_board {
            SerialBoard::Sio88 => 0,
            SerialBoard::TwoSio88 => self.two_sio[1].receive_len(),
        }
    }
    pub(super) fn port1_rx_line_idle(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => true,
            SerialBoard::TwoSio88 => self.two_sio[1].receive_line_idle(),
        }
    }
    pub(super) fn port1_tx_front(&self) -> Option<u8> {
        match self.serial_board {
            SerialBoard::Sio88 => None,
            SerialBoard::TwoSio88 => self.two_sio[1].endpoint_tx_front(),
        }
    }
    pub(super) fn port1_tx_complete(&mut self) -> Option<u8> {
        if self.serial_board != SerialBoard::TwoSio88 { return None; }
        let completed = self.two_sio[1].endpoint_tx_complete();
        if let Some(byte) = completed { self.trace.record(IO_TRACE_TX_COMPLETE, self.data_port_for_index(1), byte); }
        completed
    }
    pub(super) fn port1_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => false,
            SerialBoard::TwoSio88 => self.two_sio[1].endpoint_tx_pending_or_hardware_busy(),
        }
    }

    pub(super) fn clear_serial(&mut self) {
        self.sio.clear();
        self.two_sio[0].reset();
        self.two_sio[1].reset();
        self.sio_control = 0;
    }

    fn debugger_inject_rx(&mut self, port: u8, byte: u8) -> bool {
        let Some(index) = self.data_port_index(port) else { return false; };
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.debugger_inject_received_character(byte),
            SerialBoard::TwoSio88 => self.two_sio[index].debugger_inject_received_character(byte),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
        true
    }
    fn debugger_clear_rx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else { return false; };
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.clear_receive_for_debugger(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_receive_for_debugger(),
        }
        true
    }
    fn debugger_clear_tx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else { return false; };
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.clear_transmit_for_debugger(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_transmit_for_debugger(),
        }
        true
    }
    fn debugger_complete_tx(&mut self, port: u8) -> Option<u8> {
        let index = self.data_port_index(port)?;
        let byte = match self.serial_board {
            SerialBoard::Sio88 => self.sio.debugger_complete_one_tx()?,
            SerialBoard::TwoSio88 => self.two_sio[index].debugger_complete_one_tx()?,
        };
        self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        Some(byte)
    }
}

impl AltairBus {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        self.io.configure_serial_board(board);
        self.refresh_interrupt_request_line();
    }
    pub fn serial_board(&self) -> SerialBoard { self.io.serial_board() }

    pub fn configure_sio_hardware(&mut self, config: SioHardwareConfig) {
        self.io.configure_sio_hardware(config);
        self.refresh_interrupt_request_line();
    }
    pub fn sio_hardware(&self) -> SioHardwareConfig { self.io.sio_hardware() }

    pub fn configure_two_sio_straps(&mut self, straps: TwoSioStraps) {
        self.io.configure_two_sio_straps(straps);
        self.refresh_interrupt_request_line();
    }
    pub fn two_sio_straps(&self) -> TwoSioStraps { self.io.two_sio_straps() }

    pub fn configure_two_sio_interrupt_wiring(&mut self, wiring: TwoSioInterruptWiring) {
        self.io.configure_two_sio_interrupt_wiring(wiring);
        self.refresh_interrupt_request_line();
    }
    pub fn two_sio_interrupt_wiring(&self) -> TwoSioInterruptWiring {
        self.io.two_sio_interrupt_wiring()
    }
    /// Active raw 88-Vector Interrupt request levels sourced by the 88-2SIO.
    /// Bit n corresponds to VIn. This does not itself assert processor PINT; an
    /// installed 88-VI board must consume/arbitrate these lines separately.
    pub fn two_sio_vector_interrupt_requests(&self) -> u8 {
        self.io.vector_interrupt_requests()
    }

    pub fn serial_modem_lines(&self, port_index: usize) -> Option<(bool, bool, bool, bool)> {
        self.io.modem_lines(port_index)
    }
    pub fn set_serial_modem_inputs(&mut self, port_index: usize, cts_high: bool, dcd_high: bool) -> bool {
        let changed = self.io.set_modem_inputs(port_index, cts_high, dcd_high);
        if changed { self.refresh_interrupt_request_line(); }
        changed
    }

    pub(crate) fn advance_serial_hardware_time(&mut self, t_states: u64) {
        self.io.advance_t_states(t_states);
        self.refresh_interrupt_request_line();
    }
    pub(crate) fn fast_account_io_input_wait(&mut self, port: u8) {
        let wait_states = u32::from(!self.cycle_io_ready(port, true, MemoryReadyPhase::T2));
        self.fast_wait_t_states = self.fast_wait_t_states.saturating_add(wait_states);
    }
    pub(crate) fn cycle_io_ready(&self, port: u8, input_read: bool, phase: MemoryReadyPhase) -> bool {
        self.io.ready_for_input_t_state(port, input_read, phase)
    }

    pub fn serial_port1_receive(&mut self, byte: u8) { self.io.port1_receive(byte); self.refresh_interrupt_request_line(); }
    pub fn serial_port1_rx_empty(&self) -> bool { self.io.port1_rx_empty() }
    pub fn serial_port1_rx_len(&self) -> usize { self.io.port1_rx_len() }
    pub fn serial_port1_rx_line_idle(&self) -> bool { self.io.port1_rx_line_idle() }
    pub fn serial_port1_tx_front(&self) -> Option<u8> { self.io.port1_tx_front() }
    pub fn serial_port1_tx_complete(&mut self) -> Option<u8> {
        let completed = self.io.port1_tx_complete(); self.refresh_interrupt_request_line(); completed
    }
    pub fn serial_port1_tx_busy(&self) -> bool { self.io.port1_tx_busy() }
    pub(crate) fn serial_interrupt_request(&self) -> bool { self.io.interrupt_request() }
    pub(crate) fn serial_interrupt_opcode(&self) -> u8 { self.io.direct_interrupt_opcode() }

    pub fn serial_rx_line_idle(&self) -> bool { self.io.serial_rx_line_idle() }

    pub fn peek_io_port(&self, port: u8) -> u8 {
        if port == 0xff { self.panel.input() } else { self.io.peek_input(port) }
    }
    pub fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) { self.io.trace.port_activity(port) }
    pub fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> { self.io.trace.snapshot() }
    pub fn io_trace_enabled(&self) -> bool { self.io.trace.enabled }
    pub fn set_io_trace_enabled(&mut self, enabled: bool) { self.io.trace.set_enabled(enabled); }
    pub fn clear_io_trace(&mut self) { self.io.trace.clear_events(); }

    pub fn debugger_input_port(&mut self, port: u8) -> u8 {
        let value = <Self as Bus>::input(self, port);
        if port == 0xff { self.io.trace.record(IO_TRACE_IN, port, value); }
        self.refresh_interrupt_request_line();
        value
    }
    pub fn debugger_output_port(&mut self, port: u8, value: u8) {
        <Self as Bus>::output(self, port, value);
        if port == 0xff { self.io.trace.record(IO_TRACE_OUT, port, value); }
        self.refresh_interrupt_request_line();
    }
    pub fn debugger_inject_serial_rx(&mut self, data_port: u8, byte: u8) -> bool {
        let changed = self.io.debugger_inject_rx(data_port, byte);
        if changed { self.refresh_interrupt_request_line(); }
        changed
    }
    pub fn debugger_clear_serial_rx(&mut self, data_port: u8) -> bool {
        let changed = self.io.debugger_clear_rx(data_port);
        if changed { self.refresh_interrupt_request_line(); }
        changed
    }
    pub fn debugger_clear_serial_tx(&mut self, data_port: u8) -> bool {
        let changed = self.io.debugger_clear_tx(data_port);
        if changed { self.refresh_interrupt_request_line(); }
        changed
    }
    pub fn debugger_complete_serial_tx(&mut self, data_port: u8) -> Option<u8> {
        let completed = self.io.debugger_complete_tx(data_port);
        self.refresh_interrupt_request_line();
        completed
    }
}

impl AltairMachine {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        if self.bus.serial_board() == board { return; }
        self.running = false;
        self.bus.configure_serial_board(board);
        self.bus.clear_transient_memory_guards();
        if self.powered { self.reset(); } else { self.cpu.reset(); }
    }
    pub fn serial_board(&self) -> SerialBoard { self.bus.serial_board() }

    pub fn configure_sio_hardware(&mut self, config: SioHardwareConfig) {
        if self.bus.sio_hardware() == config { return; }
        self.running = false;
        self.bus.configure_sio_hardware(config);
        self.bus.clear_transient_memory_guards();
        if self.powered && self.bus.serial_board() == SerialBoard::Sio88 {
            self.reset();
        } else {
            self.cpu.reset();
        }
    }
    pub fn sio_hardware(&self) -> SioHardwareConfig { self.bus.sio_hardware() }

    pub fn configure_two_sio_straps(&mut self, straps: TwoSioStraps) {
        if self.bus.two_sio_straps() == straps { return; }
        self.running = false;
        self.bus.configure_two_sio_straps(straps);
        self.bus.clear_transient_memory_guards();
        if self.powered && self.bus.serial_board() == SerialBoard::TwoSio88 {
            self.reset();
        } else {
            self.cpu.reset();
        }
    }
    pub fn two_sio_straps(&self) -> TwoSioStraps { self.bus.two_sio_straps() }

    pub fn configure_two_sio_interrupt_wiring(&mut self, wiring: TwoSioInterruptWiring) {
        if self.bus.two_sio_interrupt_wiring() == wiring { return; }
        self.running = false;
        self.bus.configure_two_sio_interrupt_wiring(wiring);
        self.bus.clear_transient_memory_guards();
        if self.powered && self.bus.serial_board() == SerialBoard::TwoSio88 {
            self.reset();
        } else {
            self.cpu.reset();
        }
    }
    pub fn two_sio_interrupt_wiring(&self) -> TwoSioInterruptWiring {
        self.bus.two_sio_interrupt_wiring()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        SioAddressPair, SioRevision, TwoSioAddressBlock,
        TwoSioBaudTap as ConfigTwoSioBaudTap, TwoSioInterruptTarget,
        TwoSioInterruptWiring,
    };

    #[test]
    fn default_88_sio_does_not_alias_88_2sio_data_ports() {
        let mut io = IoDevices::default();
        assert_eq!(io.serial_board(), SerialBoard::Sio88);
        io.output(SIO2_PORT0_DATA, b'X');
        io.output(SIO2_PORT1_DATA, b'Y');
        assert!(!io.serial_tx_busy());
        assert!(!io.port1_tx_busy());
        assert_eq!(io.input(SIO2_PORT0_STATUS), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(SIO2_PORT0_DATA), S100_OPEN_BUS_VALUE);
        io.output(SIO_DATA_PORT, b'S');
        assert_eq!(io.serial_tx_front(), None, "88-SIO byte must traverse the COM2502 shift register first");
        io.advance_t_states(200_000);
        assert_eq!(io.serial_tx_front(), Some(b'S'));
    }

    #[test]
    fn sio_rev1_status_uses_d0_d7_without_fabricated_d6() {
        let mut io = IoDevices::default();
        assert_eq!(io.input(SIO_STATUS_PORT), 0x01, "empty Rev1 card: D0 high (not RDA), D7 low (TBMT ready)");
        io.debugger_inject_rx(SIO_DATA_PORT, b'R');
        assert_eq!(io.peek_input(SIO_STATUS_PORT) & 0xc1, 0x00, "RDA ready pulls D0 low and D6 must remain low");
        io.output(SIO_DATA_PORT, b'A');
        assert_eq!(io.peek_input(SIO_STATUS_PORT) & 0xc0, 0x00, "idle shift register immediately frees the holding register");
        io.output(SIO_DATA_PORT, b'B');
        assert_eq!(io.peek_input(SIO_STATUS_PORT) & 0xc0, 0x80, "second byte occupies holding register: D7 only, never fake D6");
    }

    #[test]
    fn sio_address_jumpers_move_status_data_decode_and_open_old_ports() {
        let mut io = IoDevices::default();
        let config = SioHardwareConfig {
            address: SioAddressPair::try_new(0x06).unwrap(),
            ..SioHardwareConfig::default()
        };
        io.configure_sio_hardware(config);
        assert_eq!(io.input(0x00), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(0x01), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(0x06), 0x01);
        assert!(io.debugger_inject_rx(0x07, b'J'));
        assert_eq!(io.peek_input(0x07), b'J');
        assert_eq!(io.input(0x07), b'J');
    }

    #[test]
    fn sio_rev0_ready_bits_use_original_active_high_positions() {
        let mut io = IoDevices::default();
        io.configure_sio_hardware(SioHardwareConfig {
            revision: SioRevision::Rev0,
            ..SioHardwareConfig::default()
        });
        assert_eq!(io.input(SIO_STATUS_PORT), 0x02);
        assert!(io.debugger_inject_rx(SIO_DATA_PORT, b'R'));
        assert_eq!(io.peek_input(SIO_STATUS_PORT) & 0x22, 0x22);
    }

    #[test]
    fn sio_com2502_overrun_replaces_unread_old_character_with_new_one() {
        let mut io = IoDevices::default();
        assert!(io.debugger_inject_rx(SIO_DATA_PORT, b'A'));
        assert!(io.debugger_inject_rx(SIO_DATA_PORT, b'B'));
        assert_eq!(io.peek_input(SIO_STATUS_PORT) & 0x10, 0x10);
        assert_eq!(io.input(SIO_DATA_PORT), b'B');
    }

    #[test]
    fn unmapped_io_reads_open_bus_for_each_installed_serial_board() {
        let mut io = IoDevices::default();
        assert_eq!(io.input(0x7e), S100_OPEN_BUS_VALUE);
        io.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(io.input(SIO_STATUS_PORT), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(SIO_DATA_PORT), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(0x7e), S100_OPEN_BUS_VALUE);
    }

    #[test]
    fn physical_address_strap_moves_decoder_waits_and_open_bus_together() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        let straps = TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        };
        io.configure_two_sio_straps(straps);

        for port in 0x44..=0x47 {
            assert_eq!(io.input_wait_states(port), 1, "selected 88-2SIO block must own PRDY wait");
        }
        for port in 0x10..=0x13 {
            assert_eq!(io.input_wait_states(port), 0, "old block must no longer stretch PRDY");
            assert_eq!(io.input(port), S100_OPEN_BUS_VALUE, "old block must become S-100 open bus");
        }
        assert_eq!(io.input(0x44) & 0x02, 0x02);
        assert_eq!(io.input(0x46) & 0x02, 0x02);
        io.output(0x45, b'0');
        io.output(0x47, b'1');
        assert!(io.serial_tx_busy());
        assert!(io.port1_tx_busy());
        assert_eq!(io.input_wait_states(0xff), 0, "front-panel port must never be decoded by 88-2SIO");
    }

    #[test]
    fn baud_straps_are_independent_per_acia() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.configure_two_sio_straps(TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x10).unwrap(),
            port0_baud: ConfigTwoSioBaudTap::Baud300,
            port1_baud: ConfigTwoSioBaudTap::Baud9600,
        });
        io.output(SIO2_PORT0_STATUS, 0x15); // /16, 8N1
        io.output(SIO2_PORT1_STATUS, 0x15);
        io.serial_receive(b'A');
        io.port1_receive(b'B');

        io.advance_t_states(2_084); // enough for 9600 8N1, nowhere near 300 8N1
        assert_eq!(io.peek_input(SIO2_PORT1_STATUS) & 0x01, 0x01);
        assert_eq!(io.peek_input(SIO2_PORT0_STATUS) & 0x01, 0x00);
        io.advance_t_states(64_583); // total >= 66,667 T-states for 300-baud 10-bit frame
        assert_eq!(io.peek_input(SIO2_PORT0_STATUS) & 0x01, 0x01);
    }

    #[test]
    fn two_sio_modem_pin_levels_are_card_state_not_host_endpoint_state() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(machine.bus.serial_modem_lines(0), Some((false, false, false, false)));
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x51);
        assert_eq!(machine.bus.serial_modem_lines(0), Some((true, false, false, false)));
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x71);
        assert_eq!(machine.bus.serial_modem_lines(0), Some((false, true, false, false)));
        assert!(machine.bus.set_serial_modem_inputs(0, true, false));
        assert_eq!(machine.bus.peek_io_port(SIO2_PORT0_STATUS) & 0x08, 0x08);
    }

    #[test]
    fn dcd_transition_reaches_status_and_canonical_pint() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x91);
        assert!(!machine.bus.cpu_control_lines().interrupt);
        assert!(machine.bus.set_serial_modem_inputs(0, false, true));
        assert_eq!(machine.bus.peek_io_port(SIO2_PORT0_STATUS) & 0x84, 0x84);
        assert!(machine.bus.cpu_control_lines().interrupt);
        assert!(machine.bus.set_serial_modem_inputs(0, false, false));
        let _ = machine.bus.debugger_input_port(SIO2_PORT0_STATUS);
        assert!(machine.bus.cpu_control_lines().interrupt);
        let _ = machine.bus.debugger_input_port(SIO2_PORT0_DATA);
        assert!(!machine.bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn two_sio_rdrf_does_not_make_the_physical_rx_line_busy() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_STATUS, 0x15); // /16, 8N1, Port0 110 baud
        io.serial_receive(b'A');
        assert!(!io.serial_rx_line_idle());
        io.advance_t_states(181_819);
        assert_eq!(io.peek_input(SIO2_PORT0_STATUS) & 0x01, 0x01);
        assert!(io.serial_rx_line_idle(), "completed unread RDR must not fake cable flow control");
        assert!(!io.serial_rx_empty(), "RDR still contains the unread byte");

        io.serial_receive(b'B');
        assert!(!io.serial_rx_line_idle());
        io.advance_t_states(181_819);
        assert_eq!(io.peek_input(SIO2_PORT0_STATUS) & 0x21, 0x01, "overrun is latent until old RDR is read");
        assert_eq!(io.input(SIO2_PORT0_DATA), b'A');
        assert_eq!(io.peek_input(SIO2_PORT0_STATUS) & 0x21, 0x21);
    }

    #[test]
    fn two_sio_selected_input_generates_exactly_one_tw_but_output_does_not_wait() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        for port in [SIO2_PORT0_STATUS, SIO2_PORT0_DATA, SIO2_PORT1_STATUS, SIO2_PORT1_DATA] {
            assert_eq!(io.input_wait_states(port), 1);
        }
        assert_eq!(io.input_wait_states(0x14), 0);
        assert!(!io.ready_for_input_t_state(SIO2_PORT0_STATUS, true, MemoryReadyPhase::T2));
        assert!(io.ready_for_input_t_state(SIO2_PORT0_STATUS, true, MemoryReadyPhase::Tw));
        assert!(io.ready_for_input_t_state(SIO2_PORT0_STATUS, false, MemoryReadyPhase::T2));
    }

    #[test]
    fn sio_control_port_enables_level_sensitive_rx_and_tx_interrupt_sources() {
        let mut io = IoDevices::default();
        io.output(SIO_STATUS_PORT, 0x01);
        io.serial_receive(b'R');
        assert!(!io.interrupt_request(), "RX interrupt waits for the physical frame to reach the holding register");
        io.advance_t_states(200_000);
        assert!(io.interrupt_request());
        assert_eq!(io.input(SIO_DATA_PORT), b'R');
        assert!(!io.interrupt_request());

        io.output(SIO_STATUS_PORT, 0x02);
        assert!(io.interrupt_request(), "empty TX holding register is the level-sensitive ready source");
        io.output(SIO_DATA_PORT, b'T');
        assert!(io.interrupt_request(), "idle transmitter immediately transfers T to the shift register and frees TBMT");
        io.output(SIO_DATA_PORT, b'U');
        assert!(!io.interrupt_request(), "U occupies the finite TX holding register while T shifts");
        io.advance_t_states(200_000);
        assert!(io.interrupt_request(), "U promotes at T's frame boundary and TBMT returns");
        assert_eq!(io.serial_tx_complete(), Some(b'T'));
    }

    #[test]
    fn two_sio_irq_is_routed_after_the_acia_not_fabricated_as_pint() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_STATUS, 0x95); // RX IRQ enabled
        assert!(io.debugger_inject_rx(SIO2_PORT0_DATA, b'A'));
        assert_eq!(io.peek_input(SIO2_PORT0_STATUS) & 0x80, 0x80, "MC6850 IRQ must exist before board routing");

        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Disconnected,
            port1: TwoSioInterruptTarget::Disconnected,
        });
        assert!(!io.interrupt_request(), "disconnected DI must not reach PINT");
        assert_eq!(io.vector_interrupt_requests(), 0);

        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Vi3,
            port1: TwoSioInterruptTarget::Disconnected,
        });
        assert!(!io.interrupt_request(), "VI3 routing must not masquerade as direct PINT");
        assert_eq!(io.vector_interrupt_requests(), 1 << 3);

        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Pint,
            port1: TwoSioInterruptTarget::Disconnected,
        });
        assert!(io.interrupt_request(), "DI->PINT must project the existing ACIA IRQ to the CPU line");
        assert_eq!(io.vector_interrupt_requests(), 0);
    }

    #[test]
    fn di_and_ei_route_independently_and_vi_levels_are_combined_as_lines() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Vi3,
            port1: TwoSioInterruptTarget::Pint,
        });
        io.output(SIO2_PORT0_STATUS, 0x95);
        io.output(SIO2_PORT1_STATUS, 0x95);

        assert!(io.debugger_inject_rx(SIO2_PORT0_DATA, b'0'));
        assert!(!io.interrupt_request(), "Port 0 VI3 alone must not drive PINT");
        assert_eq!(io.vector_interrupt_requests(), 1 << 3);

        assert!(io.debugger_inject_rx(SIO2_PORT1_DATA, b'1'));
        assert!(io.interrupt_request(), "Port 1 EI->PINT must drive PINT independently");
        assert_eq!(io.vector_interrupt_requests(), 1 << 3);

        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Vi3,
            port1: TwoSioInterruptTarget::Vi5,
        });
        assert!(!io.interrupt_request());
        assert_eq!(io.vector_interrupt_requests(), (1 << 3) | (1 << 5));
    }

    #[test]
    fn two_sio_tdre_and_tx_irq_return_before_endpoint_receives_character() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT1_STATUS, 0x35);
        io.output(SIO2_PORT1_DATA, b'T');
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x82, 0x00);
        assert_eq!(io.port1_tx_front(), None);
        io.advance_t_states(209);
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x82, 0x82);
        assert_eq!(io.port1_tx_front(), None);
        io.advance_t_states(2_083);
        assert_eq!(io.port1_tx_front(), Some(b'T'));
        assert!(io.interrupt_request());
        assert_eq!(io.port1_tx_complete(), Some(b'T'));
    }

    #[test]
    fn two_sio_receive_is_card_timed_and_rdr_is_finite() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_STATUS, 0x95);
        io.serial_receive(b'A');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x81, 0);
        io.advance_t_states(181_819);
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x81, 0x81);
        assert_eq!(io.input(SIO2_PORT0_DATA), b'A');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x01, 0);
    }

    #[test]
    fn port1_card_timed_rx_irq_projects_to_canonical_pint() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.bus.debugger_output_port(SIO2_PORT1_STATUS, 0x95);
        assert!(!machine.bus.cpu_control_lines().interrupt);
        machine.bus.serial_port1_receive(b'P');
        assert!(!machine.bus.cpu_control_lines().interrupt);
        machine.bus.advance_serial_hardware_time(2_084);
        assert!(machine.bus.cpu_control_lines().interrupt);
        assert_eq!(machine.bus.debugger_input_port(SIO2_PORT1_DATA), b'P');
        assert!(!machine.bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn debugger_rx_injection_is_immediate_but_still_obeys_mc6850_overrun() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x95);
        assert!(machine.bus.debugger_inject_serial_rx(SIO2_PORT0_DATA, b'A'));
        assert!(machine.bus.cpu_control_lines().interrupt);
        assert!(machine.bus.debugger_inject_serial_rx(SIO2_PORT0_DATA, b'B'));
        assert_eq!(machine.bus.debugger_input_port(SIO2_PORT0_DATA), b'A');
        assert_eq!(machine.bus.peek_io_port(SIO2_PORT0_STATUS) & 0x21, 0x21);
        assert!(machine.bus.debugger_clear_serial_rx(SIO2_PORT0_DATA));
        assert!(!machine.bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn selected_88_2sio_exposes_two_independent_serial_ports() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x02, 0x02);
        io.output(SIO2_PORT0_STATUS, 0x15);
        io.output(SIO2_PORT1_STATUS, 0x15);
        io.output(SIO2_PORT0_DATA, b'0');
        io.output(SIO2_PORT1_DATA, b'1');
        assert!(io.serial_tx_busy());
        assert!(io.port1_tx_busy());
        assert_eq!(io.serial_tx_front(), None);
        assert_eq!(io.port1_tx_front(), None);
    }

    #[test]
    fn changing_serial_board_preserves_ram() {
        let mut machine = AltairMachine::default();
        machine.bus.load(0x0200, &[0x5a]);
        machine.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(machine.bus.read(0x0200), 0x5a);
    }

    #[test]
    fn peek_does_not_consume_88_sio_rx() {
        let mut machine = AltairMachine::default();
        machine.bus.serial_receive(b'Y');
        machine.bus.advance_serial_hardware_time(200_000);
        assert_eq!(machine.bus.peek_io_port(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.serial_rx_len(), 1);
        assert_eq!(machine.bus.input(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.serial_rx_len(), 0);
    }
}
