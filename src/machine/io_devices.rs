use std::collections::VecDeque;

use crate::config::SerialBoard;
use crate::cpu8080::Bus;

use super::memory::{MemoryReadyPhase, S100_OPEN_BUS_VALUE};
use super::serial::SerialPort;
use super::{AltairBus, AltairMachine};

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
        if !self.enabled {
            return;
        }

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

        let event = IoTraceEvent {
            sequence: self.next_sequence,
            kind,
            port,
            value,
            repeat: 1,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.events.push_back(event);
        while self.events.len() > IO_TRACE_LIMIT {
            self.events.pop_front();
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn clear_events(&mut self) {
        self.events.clear();
        self.ports.fill(IoPortActivity::default());
    }

    fn snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.events
            .iter()
            .map(|event| {
                (
                    event.sequence,
                    event.kind,
                    event.port,
                    event.value,
                    event.repeat,
                )
            })
            .collect()
    }

    fn port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        let activity = self.ports[port as usize];
        (
            activity.last_in,
            activity.last_out,
            activity.in_count,
            activity.out_count,
        )
    }
}

pub(super) struct IoDevices {
    serial: [SerialPort; 2],
    serial_board: SerialBoard,
    /// Original 88-SIO control channel. D0 enables receive interrupts and D1
    /// enables transmit-ready interrupts. The previous model ignored writes to
    /// port 00h entirely, which made authentic interrupt-driven software
    /// impossible even before PINT was wired through the chassis.
    sio_control: u8,
    /// Motorola 6850 control register image for each 88-2SIO port.
    two_sio_control: [u8; 2],
    trace: IoTrace,
}

impl Default for IoDevices {
    fn default() -> Self {
        Self {
            serial: [SerialPort::default(), SerialPort::default()],
            serial_board: SerialBoard::default(),
            sio_control: 0,
            two_sio_control: [0; 2],
            trace: IoTrace::default(),
        }
    }
}

impl IoDevices {
    pub(super) fn configure_serial_board(&mut self, board: SerialBoard) {
        self.serial_board = board;
        self.clear_serial();
    }

    pub(super) fn serial_board(&self) -> SerialBoard {
        self.serial_board
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
        matches!(
            port,
            SIO2_PORT0_STATUS | SIO2_PORT0_DATA | SIO2_PORT1_STATUS | SIO2_PORT1_DATA
        )
    }

    /// Extra 8080 wait T-states generated by the installed serial card for an
    /// input machine cycle. The March 1977 MITS 88-2SIO manual documents a
    /// 500 ns PRDY stretch on every selected IN and explicitly says it occurs
    /// only during input. At the stock 2 MHz CPU clock that is exactly one TW.
    pub(super) fn input_wait_states(&self, port: u8) -> u8 {
        if self.serial_board == SerialBoard::TwoSio88 && Self::two_sio_decodes_port(port) {
            1
        } else {
            0
        }
    }

    /// Exact PRDY contribution of the 88-2SIO input wait generator. SINP clocks
    /// the board's V flip-flop, holding PRDY low through the READY sampling point
    /// in T2. The resulting PWAIT clears V, so PRDY is high in the first TW and
    /// the processor advances to T3 on the following clock. No persistent host
    /// timer is involved: this is a one-T-state hardware handshake.
    pub(super) fn ready_for_input_t_state(
        &self,
        port: u8,
        input_read: bool,
        phase: MemoryReadyPhase,
    ) -> bool {
        if !input_read || self.input_wait_states(port) == 0 {
            return true;
        }
        !matches!(phase, MemoryReadyPhase::T1 | MemoryReadyPhase::T2)
    }

    fn two_sio_status(&self, index: usize) -> u8 {
        let serial = &self.serial[index];
        (if serial.rx_empty() { 0 } else { 0x01 })
            | (if serial.tx_busy() { 0 } else { 0x02 })
    }

    fn write_two_sio_control(&mut self, index: usize, value: u8) {
        self.two_sio_control[index] = value;
        if value & 0x03 == 0x03 {
            self.serial[index].clear();
        }
    }

    /// Raw interrupt condition produced by the selected serial board before
    /// motherboard/vector-board routing. Direct PINT operation supplies RST 7;
    /// a future 88-VI/RTC model can consume these same per-board conditions and
    /// replace the direct vectoring policy without changing the UART model.
    pub(super) fn interrupt_request(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => {
                let rx_irq = self.sio_control & 0x01 != 0 && !self.serial[0].rx_empty();
                let tx_irq = self.sio_control & 0x02 != 0 && !self.serial[0].tx_busy();
                rx_irq || tx_irq
            }
            SerialBoard::TwoSio88 => (0..2).any(|index| {
                let control = self.two_sio_control[index];
                let rx_irq = control & 0x80 != 0 && !self.serial[index].rx_empty();
                // MC6850 transmitter-control bits CR6:CR5 = 01 enable the
                // transmitter-empty interrupt while keeping RTS asserted.
                let tx_irq = control & 0x60 == 0x20 && !self.serial[index].tx_busy();
                rx_irq || tx_irq
            }),
        }
    }

    pub(super) const fn direct_interrupt_opcode(&self) -> u8 {
        // The stock direct-PINT Altair interrupt path forces RST 7 (0038h).
        0xff
    }

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
                SIO2_PORT0_STATUS => self.two_sio_status(0),
                SIO2_PORT0_DATA => self.serial[0].read_rx().unwrap_or(0),
                SIO2_PORT1_STATUS => self.two_sio_status(1),
                SIO2_PORT1_DATA => self.serial[1].read_rx().unwrap_or(0),
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
                SIO2_PORT0_STATUS => self.two_sio_status(0),
                SIO2_PORT0_DATA => self.serial[0].rx_front().unwrap_or(0),
                SIO2_PORT1_STATUS => self.two_sio_status(1),
                SIO2_PORT1_DATA => self.serial[1].rx_front().unwrap_or(0),
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
                SIO2_PORT0_STATUS => self.write_two_sio_control(0, value),
                SIO2_PORT0_DATA => self.serial[0].write_tx(value),
                SIO2_PORT1_STATUS => self.write_two_sio_control(1, value),
                SIO2_PORT1_DATA => self.serial[1].write_tx(value),
                _ => {}
            },
        }
    }

    pub(super) fn output(&mut self, port: u8, value: u8) {
        self.output_raw(port, value);
        self.trace.record(IO_TRACE_OUT, port, value);
    }

    pub(super) fn serial_receive(&mut self, byte: u8) {
        self.serial[0].receive(byte);
        let port = self.data_port_for_index(0);
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
    }

    pub(super) fn serial_rx_empty(&self) -> bool {
        self.serial[0].rx_empty()
    }

    pub(super) fn serial_rx_len(&self) -> usize {
        self.serial[0].rx_len()
    }

    pub(super) fn serial_tx_front(&self) -> Option<u8> {
        self.serial[0].tx_front()
    }

    pub(super) fn serial_tx_complete(&mut self) -> Option<u8> {
        let completed = self.serial[0].complete_tx();
        if let Some(byte) = completed {
            let port = self.data_port_for_index(0);
            self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        }
        completed
    }

    pub(super) fn serial_tx_busy(&self) -> bool {
        self.serial[0].tx_busy()
    }

    pub(super) fn port1_receive(&mut self, byte: u8) {
        self.serial[1].receive(byte);
        let port = self.data_port_for_index(1);
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
    }

    pub(super) fn port1_rx_empty(&self) -> bool {
        self.serial[1].rx_empty()
    }

    pub(super) fn port1_rx_len(&self) -> usize {
        self.serial[1].rx_len()
    }

    pub(super) fn port1_tx_front(&self) -> Option<u8> {
        self.serial[1].tx_front()
    }

    pub(super) fn port1_tx_complete(&mut self) -> Option<u8> {
        let completed = self.serial[1].complete_tx();
        if let Some(byte) = completed {
            let port = self.data_port_for_index(1);
            self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        }
        completed
    }

    pub(super) fn port1_tx_busy(&self) -> bool {
        self.serial[1].tx_busy()
    }

    pub(super) fn clear_serial(&mut self) {
        self.serial[0].clear();
        self.serial[1].clear();
        self.sio_control = 0;
        self.two_sio_control.fill(0);
    }

    fn debugger_inject_rx(&mut self, port: u8, byte: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        self.serial[index].receive(byte);
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
        true
    }

    fn debugger_clear_rx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        self.serial[index].clear_rx();
        true
    }

    fn debugger_clear_tx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        self.serial[index].clear_tx();
        true
    }

    fn debugger_complete_tx(&mut self, port: u8) -> Option<u8> {
        let index = self.data_port_index(port)?;
        let byte = self.serial[index].complete_tx()?;
        self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        Some(byte)
    }
}

impl AltairBus {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        self.io.configure_serial_board(board);
        self.refresh_interrupt_request_line();
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.io.serial_board()
    }

    pub(crate) fn fast_account_io_input_wait(&mut self, port: u8) {
        self.fast_wait_t_states = self
            .fast_wait_t_states
            .saturating_add(u32::from(self.io.input_wait_states(port)));
    }

    pub(crate) fn cycle_io_ready(
        &self,
        port: u8,
        input_read: bool,
        phase: MemoryReadyPhase,
    ) -> bool {
        self.io.ready_for_input_t_state(port, input_read, phase)
    }

    pub fn serial_port1_receive(&mut self, byte: u8) {
        self.io.port1_receive(byte);
        self.refresh_interrupt_request_line();
    }

    pub fn serial_port1_rx_empty(&self) -> bool {
        self.io.port1_rx_empty()
    }

    pub fn serial_port1_rx_len(&self) -> usize {
        self.io.port1_rx_len()
    }

    pub fn serial_port1_tx_front(&self) -> Option<u8> {
        self.io.port1_tx_front()
    }

    pub fn serial_port1_tx_complete(&mut self) -> Option<u8> {
        let completed = self.io.port1_tx_complete();
        self.refresh_interrupt_request_line();
        completed
    }

    pub fn serial_port1_tx_busy(&self) -> bool {
        self.io.port1_tx_busy()
    }

    pub(crate) fn serial_interrupt_request(&self) -> bool {
        self.io.interrupt_request()
    }

    pub(crate) fn serial_interrupt_opcode(&self) -> u8 {
        self.io.direct_interrupt_opcode()
    }

    pub fn peek_io_port(&self, port: u8) -> u8 {
        if port == 0xff {
            self.panel.input()
        } else {
            self.io.peek_input(port)
        }
    }

    pub fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        self.io.trace.port_activity(port)
    }

    pub fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.io.trace.snapshot()
    }

    pub fn io_trace_enabled(&self) -> bool {
        self.io.trace.enabled
    }

    pub fn set_io_trace_enabled(&mut self, enabled: bool) {
        self.io.trace.set_enabled(enabled);
    }

    pub fn clear_io_trace(&mut self) {
        self.io.trace.clear_events();
    }

    pub fn debugger_input_port(&mut self, port: u8) -> u8 {
        let value = <Self as Bus>::input(self, port);
        if port == 0xff {
            self.io.trace.record(IO_TRACE_IN, port, value);
        }
        self.refresh_interrupt_request_line();
        value
    }

    pub fn debugger_output_port(&mut self, port: u8, value: u8) {
        <Self as Bus>::output(self, port, value);
        if port == 0xff {
            self.io.trace.record(IO_TRACE_OUT, port, value);
        }
        self.refresh_interrupt_request_line();
    }

    pub fn debugger_inject_serial_rx(&mut self, data_port: u8, byte: u8) -> bool {
        let changed = self.io.debugger_inject_rx(data_port, byte);
        if changed {
            self.refresh_interrupt_request_line();
        }
        changed
    }

    pub fn debugger_clear_serial_rx(&mut self, data_port: u8) -> bool {
        let changed = self.io.debugger_clear_rx(data_port);
        if changed {
            self.refresh_interrupt_request_line();
        }
        changed
    }

    pub fn debugger_clear_serial_tx(&mut self, data_port: u8) -> bool {
        let changed = self.io.debugger_clear_tx(data_port);
        if changed {
            self.refresh_interrupt_request_line();
        }
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
        if self.bus.serial_board() == board {
            return;
        }

        self.running = false;
        self.bus.configure_serial_board(board);
        self.bus.clear_transient_memory_guards();
        if self.powered {
            self.reset();
        } else {
            self.cpu.reset();
        }
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.bus.serial_board()
    }
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
        assert_eq!(io.input(SIO2_PORT1_STATUS), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(SIO2_PORT1_DATA), S100_OPEN_BUS_VALUE);
        io.output(SIO_DATA_PORT, b'S');
        assert_eq!(io.serial_tx_front(), Some(b'S'));
    }

    #[test]
    fn unmapped_io_reads_open_bus_for_each_installed_serial_board() {
        let mut io = IoDevices::default();
        assert_eq!(io.input(0x7e), S100_OPEN_BUS_VALUE);
        assert_eq!(io.peek_input(0x7e), S100_OPEN_BUS_VALUE);
        io.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(io.input(SIO_STATUS_PORT), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(SIO_DATA_PORT), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(0x7e), S100_OPEN_BUS_VALUE);
        assert_eq!(io.peek_input(0x7e), S100_OPEN_BUS_VALUE);
    }

    #[test]
    fn two_sio_selected_input_generates_exactly_one_tw_but_output_does_not_wait() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(io.input_wait_states(SIO2_PORT0_STATUS), 1);
        assert_eq!(io.input_wait_states(SIO2_PORT0_DATA), 1);
        assert_eq!(io.input_wait_states(SIO2_PORT1_STATUS), 1);
        assert_eq!(io.input_wait_states(SIO2_PORT1_DATA), 1);
        assert_eq!(io.input_wait_states(0x14), 0);

        assert!(!io.ready_for_input_t_state(SIO2_PORT0_STATUS, true, MemoryReadyPhase::T1));
        assert!(!io.ready_for_input_t_state(SIO2_PORT0_STATUS, true, MemoryReadyPhase::T2));
        assert!(io.ready_for_input_t_state(SIO2_PORT0_STATUS, true, MemoryReadyPhase::Tw));
        assert!(io.ready_for_input_t_state(SIO2_PORT0_STATUS, true, MemoryReadyPhase::T3));
        assert!(io.ready_for_input_t_state(SIO2_PORT0_STATUS, false, MemoryReadyPhase::T2));
        assert!(io.ready_for_input_t_state(0x14, true, MemoryReadyPhase::T2));

        io.configure_serial_board(SerialBoard::Sio88);
        assert_eq!(io.input_wait_states(SIO_STATUS_PORT), 0);
        assert!(io.ready_for_input_t_state(SIO_STATUS_PORT, true, MemoryReadyPhase::T2));
    }

    #[test]
    fn sio_control_port_enables_level_sensitive_rx_and_tx_interrupt_sources() {
        let mut io = IoDevices::default();
        assert!(!io.interrupt_request());

        io.output(SIO_STATUS_PORT, 0x01); // D0: receive interrupt enable
        io.serial_receive(b'R');
        assert!(io.interrupt_request());
        assert_eq!(io.input(SIO_DATA_PORT), b'R');
        assert!(!io.interrupt_request(), "reading the receive buffer removes the RX interrupt condition");

        io.output(SIO_STATUS_PORT, 0x02); // D1: transmit-ready interrupt enable
        assert!(io.interrupt_request(), "an empty transmitter is ready and therefore requests service");
        io.output(SIO_DATA_PORT, b'T');
        assert!(!io.interrupt_request(), "loading the transmitter removes the ready condition");
        assert_eq!(io.serial_tx_complete(), Some(b'T'));
        assert!(io.interrupt_request(), "completed transmission restores the ready interrupt condition");
        assert_eq!(io.direct_interrupt_opcode(), 0xff);
    }

    #[test]
    fn two_sio_acia_control_bits_drive_rx_and_tx_interrupt_conditions() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);

        io.output(SIO2_PORT0_STATUS, 0x80); // CR7: RX IRQ enable
        io.serial_receive(b'R');
        assert!(io.interrupt_request());
        assert_eq!(io.input(SIO2_PORT0_DATA), b'R');
        assert!(!io.interrupt_request());

        io.output(SIO2_PORT0_STATUS, 0x20); // CR6:CR5 = 01: TX empty IRQ enable
        assert!(io.interrupt_request());
        io.output(SIO2_PORT0_DATA, b'T');
        assert!(!io.interrupt_request());
        assert_eq!(io.serial_tx_complete(), Some(b'T'));
        assert!(io.interrupt_request());
    }

    #[test]
    fn selected_88_2sio_exposes_two_independent_serial_ports() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO_STATUS_PORT), S100_OPEN_BUS_VALUE);
        io.output(SIO2_PORT0_DATA, b'0');
        io.output(SIO2_PORT1_DATA, b'1');
        assert_eq!(io.serial_tx_front(), Some(b'0'));
        assert_eq!(io.port1_tx_front(), Some(b'1'));
    }

    #[test]
    fn port1_irq_projects_to_canonical_pint_immediately() {
        let mut machine = AltairMachine::default();
        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.bus.debugger_output_port(SIO2_PORT1_STATUS, 0x80);
        assert!(!machine.bus.cpu_control_lines().interrupt);

        machine.bus.serial_port1_receive(b'P');
        assert!(machine.bus.cpu_control_lines().interrupt);

        assert_eq!(machine.bus.debugger_input_port(SIO2_PORT1_DATA), b'P');
        assert!(!machine.bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn debugger_uart_mutations_refresh_canonical_pint() {
        let mut machine = AltairMachine::default();
        machine.bus.debugger_output_port(SIO_STATUS_PORT, 0x01);
        assert!(!machine.bus.cpu_control_lines().interrupt);

        assert!(machine.bus.debugger_inject_serial_rx(SIO_DATA_PORT, b'D'));
        assert!(machine.bus.cpu_control_lines().interrupt);
        assert!(machine.bus.debugger_clear_serial_rx(SIO_DATA_PORT));
        assert!(!machine.bus.cpu_control_lines().interrupt);
    }

    #[test]
    fn changing_serial_board_preserves_ram() {
        let mut machine = AltairMachine::default();
        machine.bus.load(0x0200, &[0x5a]);
        machine.configure_serial_board(SerialBoard::TwoSio88);
        assert_eq!(machine.bus.read(0x0200), 0x5a);
        assert_eq!(machine.serial_board(), SerialBoard::TwoSio88);
    }

    #[test]
    fn peek_does_not_consume_serial_rx() {
        let mut machine = AltairMachine::default();
        machine.bus.serial_receive(b'Y');
        assert_eq!(machine.bus.peek_io_port(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.serial_rx_len(), 1);
        assert_eq!(machine.bus.input(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.serial_rx_len(), 0);
    }
}
