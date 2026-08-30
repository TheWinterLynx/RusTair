use std::collections::VecDeque;

use crate::config::SerialBoard;
use crate::cpu8080::Bus;
use crate::mc6850::Mc6850;

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
    /// Byte-level model retained exclusively for the older 88-SIO while that
    /// revision-sensitive card is audited separately.
    serial: [SerialPort; 2],
    /// The 88-2SIO contains two independent Motorola 6850 ACIAs. Keeping the
    /// actual chip state here prevents ASR/terminal presentation queues from
    /// masquerading as UART registers.
    two_sio: [Mc6850; 2],
    serial_board: SerialBoard,
    /// Original 88-SIO control channel. D0 enables receive interrupts and D1
    /// enables transmit-ready interrupts in the current pre-revision-audit model.
    sio_control: u8,
    trace: IoTrace,
}

impl Default for IoDevices {
    fn default() -> Self {
        Self {
            serial: [SerialPort::default(), SerialPort::default()],
            two_sio: [Mc6850::default(), Mc6850::default()],
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
    /// the processor advances to T3 on the following clock.
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

    fn two_sio_start_transmitter_if_idle(&mut self, index: usize) {
        // Register ownership is now authentic: guest writes fill TDR, then the
        // ACIA transfers TDR to its separate shift register and TDRE reflects
        // TDR availability rather than endpoint completion. The transfer is
        // presently started at the write boundary; the next closeout slice will
        // put the one-bit transfer delay and full frame duration under the card's
        // emulated baud clock instead of this zero-delay start approximation.
        let _ = self.two_sio[index].transfer_tdr_to_shift_if_idle();
    }

    /// Raw interrupt condition produced by the selected serial board before
    /// motherboard/vector-board routing. Direct PINT operation supplies RST 7;
    /// a future 88-VI/RTC model can consume these same per-board conditions.
    pub(super) fn interrupt_request(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => {
                let rx_irq = self.sio_control & 0x01 != 0 && !self.serial[0].rx_empty();
                let tx_irq = self.sio_control & 0x02 != 0 && !self.serial[0].tx_busy();
                rx_irq || tx_irq
            }
            SerialBoard::TwoSio88 => self.two_sio.iter().any(Mc6850::interrupt_request),
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
                SIO2_PORT0_DATA => {
                    self.two_sio[0].write_data(value);
                    self.two_sio_start_transmitter_if_idle(0);
                }
                SIO2_PORT1_STATUS => self.two_sio[1].write_control(value),
                SIO2_PORT1_DATA => {
                    self.two_sio[1].write_data(value);
                    self.two_sio_start_transmitter_if_idle(1);
                }
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
            SerialBoard::TwoSio88 => self.two_sio[0].receive_character(byte, false, false),
        }
        let port = self.data_port_for_index(0);
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
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
            SerialBoard::TwoSio88 => self.two_sio[0].tx_shift_front(),
        }
    }

    pub(super) fn serial_tx_complete(&mut self) -> Option<u8> {
        let completed = match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].complete_tx(),
            SerialBoard::TwoSio88 => self.two_sio[0].complete_tx_shift(),
        };
        if let Some(byte) = completed {
            let port = self.data_port_for_index(0);
            self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        }
        completed
    }

    pub(super) fn serial_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[0].tx_busy(),
            SerialBoard::TwoSio88 => self.two_sio[0].transmit_busy(),
        }
    }

    pub(super) fn port1_receive(&mut self, byte: u8) {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].receive(byte),
            SerialBoard::TwoSio88 => self.two_sio[1].receive_character(byte, false, false),
        }
        let port = self.data_port_for_index(1);
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
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
            SerialBoard::TwoSio88 => self.two_sio[1].tx_shift_front(),
        }
    }

    pub(super) fn port1_tx_complete(&mut self) -> Option<u8> {
        let completed = match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].complete_tx(),
            SerialBoard::TwoSio88 => self.two_sio[1].complete_tx_shift(),
        };
        if let Some(byte) = completed {
            let port = self.data_port_for_index(1);
            self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        }
        completed
    }

    pub(super) fn port1_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[1].tx_busy(),
            SerialBoard::TwoSio88 => self.two_sio[1].transmit_busy(),
        }
    }

    pub(super) fn clear_serial(&mut self) {
        self.serial[0].clear();
        self.serial[1].clear();
        self.two_sio = [Mc6850::default(), Mc6850::default()];
        self.sio_control = 0;
    }

    fn debugger_inject_rx(&mut self, port: u8, byte: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[index].receive(byte),
            SerialBoard::TwoSio88 => self.two_sio[index].receive_character(byte, false, false),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
        true
    }

    fn debugger_clear_rx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        match self.serial_board {
            SerialBoard::Sio88 => self.serial[index].clear_rx(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_receive_for_debugger(),
        }
        true
    }

    fn debugger_clear_tx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
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
            SerialBoard::TwoSio88 => self.two_sio[index].complete_tx_shift()?,
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

    pub fn serial_board(&self) -> SerialBoard {
        self.io.serial_board()
    }

    pub(crate) fn fast_account_io_input_wait(&mut self, port: u8) {
        // Derive the instruction-level approximation from the same card PRDY
        // predicate used by Cycle. This keeps the two engines from acquiring
        // separate hard-coded 88-2SIO wait policies.
        let wait_states = u32::from(!self.cycle_io_ready(port, true, MemoryReadyPhase::T2));
        self.fast_wait_t_states = self.fast_wait_t_states.saturating_add(wait_states);
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
        assert_eq!(io.direct_interrupt_opcode(), 0xff);
    }

    #[test]
    fn two_sio_acia_control_bits_drive_rx_and_tx_interrupt_conditions() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);

        io.output(SIO2_PORT0_STATUS, 0x80);
        io.serial_receive(b'R');
        assert!(io.interrupt_request());
        assert_eq!(io.input(SIO2_PORT0_DATA), b'R');
        assert!(!io.interrupt_request());

        io.output(SIO2_PORT0_STATUS, 0x20);
        assert!(io.interrupt_request());
        io.output(SIO2_PORT0_DATA, b'T');
        assert_eq!(io.serial_tx_front(), Some(b'T'));
        assert!(io.serial_tx_busy());
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x82, 0x82,
            "TDRE/IRQ describe the empty TDR even while TSR still shifts T");
        assert_eq!(io.serial_tx_complete(), Some(b'T'));
        assert!(io.interrupt_request());
    }

    #[test]
    fn two_sio_has_finite_receive_register_and_reports_delayed_overrun() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(SIO2_PORT0_STATUS, 0x94); // RX IRQ + 8N1 / divide 1

        io.serial_receive(b'A');
        io.serial_receive(b'B');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x21, 0x01);
        assert_eq!(io.input(SIO2_PORT0_DATA), b'A');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x21, 0x21);
        assert_eq!(io.input(SIO2_PORT0_DATA), b'A');
        assert_eq!(io.input(SIO2_PORT0_STATUS) & 0x21, 0x00);
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

        machine.configure_serial_board(SerialBoard::TwoSio88);
        machine.bus.debugger_output_port(SIO2_PORT0_STATUS, 0x14);
        machine.bus.serial_receive(b'Z');
        assert_eq!(machine.bus.peek_io_port(SIO2_PORT0_DATA), b'Z');
        assert_eq!(machine.bus.serial_rx_len(), 1);
        assert_eq!(machine.bus.input(SIO2_PORT0_DATA), b'Z');
        assert_eq!(machine.bus.serial_rx_len(), 0);
    }
}
