use std::collections::VecDeque;

use crate::config::SerialBoard;
use crate::cpu8080::Bus;

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

        // Poll loops such as BASIC's IN 00h status wait can execute thousands
        // of identical reads per second. Coalesce adjacent identical events so
        // the useful DATA-port transition remains visible instead of being
        // pushed out of the trace immediately.
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

/// I/O devices currently installed in the emulated machine.
///
/// A fully populated MITS 88-2SIO contains two independent 6850 ACIAs. RusTair
/// therefore keeps separate RX/TX state for Port 0 and Port 1 instead of
/// aliasing both guest-visible port pairs to one host-side serial queue.
pub(super) struct IoDevices {
    serial: [SerialPort; 2],
    serial_board: SerialBoard,
    two_sio_control: [u8; 2],
    trace: IoTrace,
}

impl Default for IoDevices {
    fn default() -> Self {
        Self {
            serial: [SerialPort::default(), SerialPort::default()],
            serial_board: SerialBoard::default(),
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

    fn two_sio_status(&self, index: usize) -> u8 {
        let serial = &self.serial[index];
        (if serial.rx_empty() { 0 } else { 0x01 })
            | (if serial.tx_busy() { 0 } else { 0x02 })
    }

    fn write_two_sio_control(&mut self, index: usize, value: u8) {
        self.two_sio_control[index] = value;

        // MC6850 CR1:CR0 = 11 performs a master reset. We do not yet emulate
        // IRQ/RTS/framing electrically, but reset semantics are guest-visible
        // and must remain independent for the two ACIAs.
        if value & 0x03 == 0x03 {
            self.serial[index].clear();
        }
    }

    fn input_raw(&mut self, port: u8) -> u8 {
        match self.serial_board {
            SerialBoard::Sio88 => match port {
                // MITS 88-SIO status convention used by the S2JS reference.
                // Bit 0 is set when the receive buffer is empty, while bits 6/7
                // are set while the transmit holding register is occupied.
                SIO_STATUS_PORT => {
                    let rx_empty = self.serial[0].rx_empty();
                    let tx_busy = self.serial[0].tx_busy();
                    (if rx_empty { 0x01 } else { 0 }) | (if tx_busy { 0xc0 } else { 0 })
                }
                SIO_DATA_PORT => self.serial[0].read_rx().unwrap_or(0),

                // An absent 88-2SIO must not look TX-ready to software polling
                // either of its status registers.
                SIO2_PORT0_STATUS | SIO2_PORT1_STATUS => 0x00,
                _ => 0,
            },
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.two_sio_status(0),
                SIO2_PORT0_DATA => self.serial[0].read_rx().unwrap_or(0),
                SIO2_PORT1_STATUS => self.two_sio_status(1),
                SIO2_PORT1_DATA => self.serial[1].read_rx().unwrap_or(0),

                // The 88-SIO uses active-low ready flags. Returning all ones
                // for its absent status register keeps software waiting rather
                // than accidentally treating the uninstalled card as ready.
                SIO_STATUS_PORT => 0xff,
                _ => 0,
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
                SIO2_PORT0_STATUS | SIO2_PORT1_STATUS => 0x00,
                _ => 0,
            },
            SerialBoard::TwoSio88 => match port {
                SIO2_PORT0_STATUS => self.two_sio_status(0),
                SIO2_PORT0_DATA => self.serial[0].rx_front().unwrap_or(0),
                SIO2_PORT1_STATUS => self.two_sio_status(1),
                SIO2_PORT1_DATA => self.serial[1].rx_front().unwrap_or(0),
                SIO_STATUS_PORT => 0xff,
                _ => 0,
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
                if port == SIO_DATA_PORT {
                    self.serial[0].write_tx(value);
                }
            }
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

    // Port 0 is the legacy/default console path used by the existing ASR-33
    // integration and by the single-port 88-SIO.
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

// Keep serial-board configuration and Port 1 access next to the device decoder
// they control. Port 0 remains available through AltairBus's existing serial
// API so the ASR-33 path and the 88-SIO keep their established semantics.
impl AltairBus {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        self.io.configure_serial_board(board);
    }

    pub fn serial_board(&self) -> SerialBoard {
        self.io.serial_board()
    }

    pub fn serial_port1_receive(&mut self, byte: u8) {
        self.io.port1_receive(byte);
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
        self.io.port1_tx_complete()
    }

    pub fn serial_port1_tx_busy(&self) -> bool {
        self.io.port1_tx_busy()
    }

    /// Non-invasive port observation for the debugger. Reading a DATA port here
    /// does not consume the receive byte.
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

    /// Perform an intentionally invasive read, exactly like an 8080 IN from the
    /// selected port. DATA-port reads therefore consume one queued RX byte and
    /// the front-panel bus monitor sees the same IN cycle as the CPU would.
    pub fn debugger_input_port(&mut self, port: u8) -> u8 {
        let value = <Self as Bus>::input(self, port);
        if port == 0xff {
            self.io.trace.record(IO_TRACE_IN, port, value);
        }
        value
    }

    /// Perform an intentionally invasive OUT without changing CPU registers.
    /// The front-panel bus monitor sees the same OUT cycle as the CPU would.
    pub fn debugger_output_port(&mut self, port: u8, value: u8) {
        <Self as Bus>::output(self, port, value);
        if port == 0xff {
            self.io.trace.record(IO_TRACE_OUT, port, value);
        }
    }

    pub fn debugger_inject_serial_rx(&mut self, data_port: u8, byte: u8) -> bool {
        self.io.debugger_inject_rx(data_port, byte)
    }

    pub fn debugger_clear_serial_rx(&mut self, data_port: u8) -> bool {
        self.io.debugger_clear_rx(data_port)
    }

    pub fn debugger_clear_serial_tx(&mut self, data_port: u8) -> bool {
        self.io.debugger_clear_tx(data_port)
    }

    pub fn debugger_complete_serial_tx(&mut self, data_port: u8) -> Option<u8> {
        self.io.debugger_complete_tx(data_port)
    }
}

impl AltairMachine {
    /// Swap the installed serial board and reset the CPU/device state without
    /// modifying RAM or the front-panel sense switches.
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        if self.bus.serial_board() == board {
            return;
        }

        self.running = false;
        self.bus.configure_serial_board(board);
        self.bus.clear_transient_memory_guards();
        self.cpu.reset();
        self.bus.force_panel_lamps(0, 0);
        self.wait_led = self.powered;
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

        io.output(SIO_DATA_PORT, b'S');
        assert_eq!(io.serial_tx_front(), Some(b'S'));
    }

    #[test]
    fn selected_88_2sio_exposes_two_independent_serial_ports() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x02, 0x02);
        assert_eq!(io.input(SIO_STATUS_PORT), 0xff);

        io.output(SIO2_PORT0_DATA, b'0');
        io.output(SIO2_PORT1_DATA, b'1');
        assert_eq!(io.serial_tx_front(), Some(b'0'));
        assert_eq!(io.port1_tx_front(), Some(b'1'));

        io.port1_receive(b'B');
        assert_eq!(io.input(SIO2_PORT1_STATUS) & 0x01, 0x01);
        assert_eq!(io.port1_rx_len(), 1);
        assert_eq!(io.input(SIO2_PORT1_DATA), b'B');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x01, 0x00);
    }

    #[test]
    fn two_sio_master_reset_is_per_port() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_DATA, b'0');
        io.output(SIO2_PORT1_DATA, b'1');

        io.output(SIO2_PORT0_STATUS, 0x03);

        assert!(!io.serial_tx_busy());
        assert!(io.port1_tx_busy());
        assert_eq!(io.port1_tx_front(), Some(b'1'));
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

    #[test]
    fn trace_coalesces_repeated_status_polls_but_counts_them() {
        let mut io = IoDevices::default();
        io.trace.set_enabled(true);
        for _ in 0..100 {
            assert_eq!(io.input(SIO_STATUS_PORT), 0x01);
        }
        let events = io.trace.snapshot();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].4, 100);
        assert_eq!(io.trace.port_activity(SIO_STATUS_PORT).2, 100);
    }

    #[test]
    fn debugger_can_inject_rx_without_cpu_side_effects() {
        let mut machine = AltairMachine::default();
        machine.bus.set_io_trace_enabled(true);
        assert!(machine.bus.debugger_inject_serial_rx(SIO_DATA_PORT, b'Y'));
        assert_eq!(machine.bus.peek_io_port(SIO_DATA_PORT), b'Y');
        assert_eq!(machine.bus.debugger_input_port(SIO_DATA_PORT), b'Y');
    }
}
