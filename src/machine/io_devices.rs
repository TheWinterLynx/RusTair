use std::collections::VecDeque;

use crate::config::SerialBoard;
use crate::cpu8080::Bus;

use super::memory::{MemoryReadyPhase, S100_OPEN_BUS_VALUE};
use super::serial::SerialPort;
use super::{AltairBus, AltairMachine, CLOCK_HZ};

#[path = "two_sio.rs"]
mod two_sio;
use two_sio::{TwoSioBaudTap, TwoSioPort};

const SIO_STATUS_PORT: u8 = 0x00;
const SIO_DATA_PORT: u8 = 0x01;
const SIO2_PORT0_STATUS: u8 = 0x10;
const SIO2_PORT0_DATA: u8 = 0x11;
const SIO2_PORT1_STATUS: u8 = 0x12;
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

pub(super) struct IoDevices {
    /// Pre-audit byte-level model retained only for the revision-sensitive 88-SIO.
    serial: [SerialPort; 2],
    /// Two physical MC6850 channels with independent board baud-generator taps.
    /// The defaults match RusTair's default 88-2SIO cabling: Model 33 on Port 0
    /// at 110 baud and the text terminal on Port 1 at 9600 baud. Exposing the
    /// actual per-port strap selector in Configuration remains a closeout item.
    two_sio: [TwoSioPort; 2],
    serial_board: SerialBoard,
    sio_control: u8,
    trace: IoTrace,
}

impl Default for IoDevices {
    fn default() -> Self {
        Self {
            serial: [SerialPort::default(), SerialPort::default()],
            two_sio: [
                TwoSioPort::new(TwoSioBaudTap::Baud110),
                TwoSioPort::new(TwoSioBaudTap::Baud9600),
            ],
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

    fn two_sio_port(&self, index: usize) -> Option<&TwoSioPort> {
        (self.serial_board == SerialBoard::TwoSio88)
            .then(|| self.two_sio.get(index))
            .flatten()
    }

    fn two_sio_port_mut(&mut self, index: usize) -> Option<&mut TwoSioPort> {
        if self.serial_board != SerialBoard::TwoSio88 { return None; }
        self.two_sio.get_mut(index)
    }

    /// `(RTS high, BREAK active, CTS high, DCD high)` at the physical MC6850
    /// pins. None means there is no 88-2SIO ACIA at that channel index.
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

    fn data_port_for_index(&self, index: usize) -> u8 {
        match (self.serial_board, index) {
            (SerialBoard::Sio88, 0) => SIO_DATA_PORT,
            (SerialBoard::TwoSio88, 0) => SIO2_PORT0_DATA,
            (SerialBoard::TwoSio88, 1) => SIO2_PORT1_DATA,
            (_, 1) => SIO2_PORT1_DATA,
            _ => SIO_DATA_PORT,
        }
    }

    fn data_port_index(&self, port: u8) -> Option<usize> {
        match self.serial_board {
            SerialBoard::Sio88 if port == SIO_DATA_PORT => Some(0),
            SerialBoard::TwoSio88 if port == SIO2_PORT0_DATA => Some(0),
            SerialBoard::TwoSio88 if port == SIO2_PORT1_DATA => Some(1),
            _ => None,
        }
    }

    fn two_sio_decodes_port(port: u8) -> bool {
        matches!(port, SIO2_PORT0_STATUS | SIO2_PORT0_DATA | SIO2_PORT1_STATUS | SIO2_PORT1_DATA)
    }

    pub(super) fn input_wait_states(&self, port: u8) -> u8 {
        if self.serial_board == SerialBoard::TwoSio88 && Self::two_sio_decodes_port(port) { 1 } else { 0 }
    }

    pub(super) fn ready_for_input_t_state(&self, port: u8, input_read: bool, phase: MemoryReadyPhase) -> bool {
        if !input_read || self.input_wait_states(port) == 0 { return true; }
        !matches!(phase, MemoryReadyPhase::T1 | MemoryReadyPhase::T2)
    }

    pub(super) fn advance_t_states(&mut self, t_states: u64) {
        if self.serial_board != SerialBoard::TwoSio88 || t_states == 0 { return; }
        for port in &mut self.two_sio {
            port.advance_t_states(t_states, CLOCK_HZ);
        }
    }

    pub(super) fn interrupt_request(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => {
                let rx_irq = self.sio_control & 0x01 != 0 && !self.serial[0].rx_empty();
                let tx_irq = self.sio_control & 0x02 != 0 && !self.serial[0].tx_busy();
                rx_irq || tx_irq
            }
            SerialBoard::TwoSio88 => self.two_sio.iter().any(TwoSioPort::interrupt_request),
        }
    }

    pub(super) const fn direct_interrupt_opcode(&self) -> u8 { 0xff }

    fn input_raw(&mut self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => match port {
                SIO_STATUS_PORT => {
                    let rx_empty = self.serial[0].rx_empty();
                    let tx_busy = self.serial[0].tx_busy();
                    (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
                }
                SIO_DATA_PORT => self.serial[0].read_rx().unwrap_or(0),
                _ => S100_OPEN_BUS_VALUE,
            },
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.two_sio[0].read_status(),
                SIO2_PORT0_DATA => self.two_sio[0].read_data(),
                SIO2_PORT1_STATUS => self.two_sio[1].read_status(),
                SIO2_PORT1_DATA => self.two_sio[1].read_data(),
                _ => S100_OPEN_BUS_VALUE,
            },
        }
    }

    fn peek_input(&self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => match port {
                SIO_STATUS_PORT => {
                    let rx_empty = self.serial[0].rx_empty();
                    let tx_busy = self.serial[0].tx_busy();
                    (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
                }
                SIO_DATA_PORT => self.serial[0].rx_front().unwrap_or(0),
                _ => S100_OPEN_BUS_VALUE,
            },
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.two_sio[0].peek_status(),
                SIO2_PORT0_DATA => self.two_sio[0].peek_data(),
                SIO2_PORT1_STATUS => self.two_sio[1].peek_status(),
                SIO2_PORT1_DATA => self.two_sio[1].peek_data(),
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
            SerialBoard::Sio88 => match port {
                SIO_STATUS_PORT => self.sio_control = value & 0x03,
                SIO_DATA_PORT => self.serial[0].write_tx(value),
                _ => {}
            },
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.two_sio[0].write_control(value),
                SIO2_PORT0_DATA => self.two_sio[0].write_data(value),
                SIO2_PORT1_STATUS => self.two_sio[1].write_control(value),
                SIO2_PORT1_DATA => self.two_sio[1].write_data(value),
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
            SerialBoard::Sio88 => self.serial[0].receive(byte),
            SerialBoard::TwoSio88 => self.two_sio[0].queue_received_character(byte),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, self.data_port_for_index(0), byte);
    }

    pub(super) fn serial_rx_empty(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].rx_empty(),
            SerialBoard::TwoSio88 => self.two_sio[0].receive_len() == 0,
        }
    }
    pub(super) fn serial_rx_len(&self) -> usize {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].rx_len(),
            SerialBoard::TwoSio88 => self.two_sio[0].receive_len(),
        }
    }
    pub(super) fn serial_tx_front(&self) -> Option<u8> {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].tx_front(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_front(),
        }
    }
    pub(super) fn serial_tx_complete(&mut self) -> Option<u8> {
        let completed = match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].complete_tx(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_complete(),
        };
        if let Some(byte) = completed {
            self.trace.record(IO_TRACE_TX_COMPLETE, self.data_port_for_index(0), byte);
        }
        completed
    }
    pub(super) fn serial_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].tx_busy(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_pending_or_hardware_busy(),
        }
    }

    pub(super) fn port1_receive(&mut self, byte: u8) {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].receive(byte),
            SerialBoard::TwoSio88 => self.two_sio[1].queue_received_character(byte),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, self.data_port_for_index(1), byte);
    }
    pub(super) fn port1_rx_empty(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].rx_empty(),
            SerialBoard::TwoSio88 => self.two_sio[1].receive_len() == 0,
        }
    }
    pub(super) fn port1_rx_len(&self) -> usize {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].rx_len(),
            SerialBoard::TwoSio88 => self.two_sio[1].receive_len(),
        }
    }
    pub(super) fn port1_tx_front(&self) -> Option<u8> {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].tx_front(),
            SerialBoard::TwoSio88 => self.two_sio[1].endpoint_tx_front(),
        }
    }
    pub(super) fn port1_tx_complete(&mut self) -> Option<u8> {
        let completed = match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].complete_tx(),
            SerialBoard::TwoSio88 => self.two_sio[1].endpoint_tx_complete(),
        };
        if let Some(byte) = completed {
            self.trace.record(IO_TRACE_TX_COMPLETE, self.data_port_for_index(1), byte);
        }
        completed
    }
    pub(super) fn port1_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].tx_busy(),
            SerialBoard::TwoSio88 => self.two_sio[1].endpoint_tx_pending_or_hardware_busy(),
        }
    }

    pub(super) fn clear_serial(&mut self) {
        self.serial[0].clear();
        self.serial[1].clear();
        self.two_sio[0].reset();
        self.two_sio[1].reset();
        self.sio_control = 0;
    }

    fn debugger_inject_rx(&mut self, port: u8, byte: u8) -> bool {
        let Some(index) = self.data_port_index(port) else { return false; };
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[index].receive(byte),
            SerialBoard::TwoSio88 => self.two_sio[index].debugger_inject_received_character(byte),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
        true
    }

    fn debugger_clear_rx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else { return false; };
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[index].clear_rx(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_receive_for_debugger(),
        }
        true
    }

    fn debugger_clear_tx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else { return false; };
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[index].clear_tx(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_transmit_for_debugger(),
        }
        true
    }

    fn debugger_complete_tx(&mut self, port: u8) -> Option<u8> {
        let index = self.data_port_index(port)?;
        let byte = match self.serial_board {
            SerialBoard::Sio88 => self.serial[index].complete_tx()?,
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

    /// `(RTS high, BREAK active, CTS high, DCD high)` for one installed
    /// 88-2SIO ACIA. The 88-SIO has different hardware and therefore returns
    /// None instead of fabricating MC6850 modem pins.
    pub fn serial_modem_lines(&self, port_index: usize) -> Option<(bool, bool, bool, bool)> {
        self.io.modem_lines(port_index)
    }

    /// Drive the external active-low modem inputs as physical TTL levels. MITS
    /// specifies grounded CTS/DCD for unused 88-2SIO inputs, represented by the
    /// default `(false, false)` state.
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
    pub fn serial_port1_tx_front(&self) -> Option<u8> { self.io.port1_tx_front() }
    pub fn serial_port1_tx_complete(&mut self) -> Option<u8> {
        let completed = self.io.port1_tx_complete(); self.refresh_interrupt_request_line(); completed
    }
    pub fn serial_port1_tx_busy(&self) -> bool { self.io.port1_tx_busy() }
    pub(crate) fn serial_interrupt_request(&self) -> bool { self.io.interrupt_request() }
    pub(crate) fn serial_interrupt_opcode(&self) -> u8 { self.io.direct_interrupt_opcode() }

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(io.serial_tx_front(), Some(b'S'));
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
    fn two_sio_modem_pin_levels_are_card_state_not_host_endpoint_state() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(machine.bus.serial_modem_lines(0), Some((false, false, false, false)));
        assert_eq!(machine.bus.serial_modem_lines(1), Some((false, false, false, false)));
        assert_eq!(machine.bus.serial_modem_lines(2), None);

        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x51);
        assert_eq!(machine.bus.serial_modem_lines(0), Some((true, false, false, false)));
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x71);
        assert_eq!(machine.bus.serial_modem_lines(0), Some((false, true, false, false)));

        assert!(machine.bus.set_serial_modem_inputs(0, true, false));
        assert_eq!(machine.bus.serial_modem_lines(0), Some((false, true, true, false)));
        assert_eq!(machine.bus.peek_io_port(SIO2_PORT0_STATUS) & 0x08, 0x08);
    }

    #[test]
    fn dcd_transition_reaches_status_and_canonical_pint() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x91); // RX IRQ enabled, 8N2, /16
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
        assert!(io.interrupt_request());
        assert_eq!(io.input(SIO_DATA_PORT), b'R');
        assert!(!io.interrupt_request());
        io.output(SIO_STATUS_PORT, 0x02);
        assert!(io.interrupt_request());
        io.output(SIO_DATA_PORT, b'T');
        assert!(!io.interrupt_request());
        assert_eq!(io.serial_tx_complete(), Some(b'T'));
        assert!(io.interrupt_request());
    }

    #[test]
    fn two_sio_tdre_and_tx_irq_return_before_endpoint_receives_character() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT1_STATUS, 0x35); // 8N1, TX IRQ, /16; Port1 strap=9600
        io.output(SIO2_PORT1_DATA, b'T');
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x82, 0x00);
        assert_eq!(io.port1_tx_front(), None);

        io.advance_t_states(209);
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x82, 0x82);
        assert_eq!(io.port1_tx_front(), None, "TSR is still shifting after TDRE returns");

        io.advance_t_states(2_083);
        assert_eq!(io.port1_tx_front(), Some(b'T'));
        assert!(io.interrupt_request());
        assert_eq!(io.port1_tx_complete(), Some(b'T'));
    }

    #[test]
    fn two_sio_receive_is_card_timed_and_rdr_is_finite() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_STATUS, 0x95); // RX IRQ, 8N1, /16; Port0 strap=110
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
        machine.bus.debugger_output_port(SIO2_PORT1_STATUS, 0x95); // RX IRQ, 8N1, /16
        assert!(!machine.bus.cpu_control_lines().interrupt);

        machine.bus.serial_port1_receive(b'P');
        assert!(!machine.bus.cpu_control_lines().interrupt, "wire start must not set RDRF/PINT before a full frame");
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
        assert_eq!(machine.bus.peek_io_port(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.serial_rx_len(), 1);
        assert_eq!(machine.bus.input(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.serial_rx_len(), 0);
    }
}
