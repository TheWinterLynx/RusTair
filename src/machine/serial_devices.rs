use std::collections::VecDeque;

use crate::config::{
    SerialBoard, SioHardwareConfig, SioRevision, TwoSioBaudTap as ConfigTwoSioBaudTap,
    TwoSioInterruptTarget, TwoSioInterruptWiring, TwoSioStraps,
};

use super::memory::S100_OPEN_BUS_VALUE;
use super::serial::sio::{SioHandshakeLines, SioPort};
use super::CLOCK_HZ;

#[path = "two_sio.rs"]
mod two_sio;
use two_sio::{TwoSioBaudTap, TwoSioPort};

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
        self.events.push_back(IoTraceEvent {
            sequence: self.next_sequence,
            kind,
            port,
            value,
            repeat: 1,
        });
        self.next_sequence = self.next_sequence.saturating_add(1);
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
            .map(|event| (event.sequence, event.kind, event.port, event.value, event.repeat))
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

/// Finite UART state owned by one physical serial card.
///
/// This type is not an Altair bus or a machine-wide I/O singleton. One instance
/// is created inside each installed 88-SIO/88-2SIO card and is shared only with
/// that card's endpoint/debugger handle.
pub(super) struct IoDevices {
    sio: SioPort,
    two_sio: [TwoSioPort; 2],
    two_sio_straps: TwoSioStraps,
    two_sio_interrupt_wiring: TwoSioInterruptWiring,
    serial_board: SerialBoard,
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

    pub(super) fn configure_sio_hardware(&mut self, config: SioHardwareConfig) {
        self.sio.configure(config);
        self.sio_control = 0;
    }

    pub(super) fn sio_hardware(&self) -> SioHardwareConfig {
        self.sio.config()
    }

    pub(super) fn configure_two_sio_straps(&mut self, straps: TwoSioStraps) {
        if self.two_sio_straps == straps {
            return;
        }
        self.two_sio_straps = straps;
        self.two_sio = configured_two_sio_ports(straps);
    }

    pub(super) fn two_sio_straps(&self) -> TwoSioStraps {
        self.two_sio_straps
    }

    pub(super) fn configure_two_sio_interrupt_wiring(
        &mut self,
        wiring: TwoSioInterruptWiring,
    ) {
        self.two_sio_interrupt_wiring = wiring;
    }

    fn two_sio_port(&self, index: usize) -> Option<&TwoSioPort> {
        (self.serial_board == SerialBoard::TwoSio88)
            .then(|| self.two_sio.get(index))
            .flatten()
    }

    fn two_sio_port_mut(&mut self, index: usize) -> Option<&mut TwoSioPort> {
        if self.serial_board != SerialBoard::TwoSio88 {
            return None;
        }
        self.two_sio.get_mut(index)
    }

    fn two_sio_irq(&self, index: usize) -> bool {
        self.two_sio
            .get(index)
            .is_some_and(TwoSioPort::interrupt_request)
    }

    fn two_sio_pint_request(&self) -> bool {
        [0usize, 1].into_iter().any(|index| {
            self.two_sio_interrupt_wiring
                .target(index)
                .is_some_and(TwoSioInterruptTarget::drives_pint)
                && self.two_sio_irq(index)
        })
    }

    fn sio_interrupt_sources(&self) -> (bool, bool) {
        if self.serial_board != SerialBoard::Sio88 {
            return (false, false);
        }
        let status = self.sio.status();
        let input = self.sio_control & 0x01 != 0 && status & 0x01 == 0;
        let output = self.sio_control & 0x02 != 0 && status & 0x80 == 0;
        (input, output)
    }

    pub(super) fn interrupt_request(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => {
                let (input, output) = self.sio_interrupt_sources();
                let wiring = self.sio.config().interrupt_wiring;
                (input && wiring.input.drives_pint())
                    || (output && wiring.output.drives_pint())
            }
            SerialBoard::TwoSio88 => self.two_sio_pint_request(),
        }
    }

    pub(super) fn vector_interrupt_requests(&self) -> u8 {
        if self.serial_board == SerialBoard::Sio88 {
            let (input, output) = self.sio_interrupt_sources();
            let wiring = self.sio.config().interrupt_wiring;
            let mut mask = 0u8;
            if input {
                if let Some(level) = wiring.input.vector_level() {
                    mask |= 1u8 << level;
                }
            }
            if output {
                if let Some(level) = wiring.output.vector_level() {
                    mask |= 1u8 << level;
                }
            }
            return mask;
        }

        let mut mask = 0u8;
        for index in [0usize, 1] {
            if !self.two_sio_irq(index) {
                continue;
            }
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

    pub(super) fn modem_lines(&self, index: usize) -> Option<(bool, bool, bool, bool)> {
        let port = self.two_sio_port(index)?;
        Some((
            port.rts_high(),
            port.break_active(),
            port.cts_high(),
            port.dcd_high(),
        ))
    }

    pub(super) fn set_modem_inputs(
        &mut self,
        index: usize,
        cts_high: bool,
        dcd_high: bool,
    ) -> bool {
        let Some(port) = self.two_sio_port_mut(index) else {
            return false;
        };
        port.set_cts_high(cts_high);
        port.set_dcd_high(dcd_high);
        true
    }

    pub(super) fn set_receive_break(&mut self, index: usize, active: bool) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 if index == 0 => {
                self.sio.set_receive_break(active);
                true
            }
            SerialBoard::TwoSio88 => {
                let Some(port) = self.two_sio.get_mut(index) else {
                    return false;
                };
                port.set_receive_break(active);
                true
            }
            _ => false,
        }
    }

    pub(super) fn sio_handshake_lines(&self) -> Option<SioHandshakeLines> {
        (self.serial_board == SerialBoard::Sio88).then(|| self.sio.handshake_lines())
    }

    pub(super) fn pulse_sio_input_device_ready(&mut self) -> bool {
        if self.serial_board != SerialBoard::Sio88
            || self.sio.config().revision != SioRevision::Rev0
        {
            return false;
        }
        self.sio.pulse_input_device_ready();
        true
    }

    pub(super) fn pulse_sio_output_device_ready(&mut self) -> bool {
        if self.serial_board != SerialBoard::Sio88
            || self.sio.config().revision != SioRevision::Rev0
        {
            return false;
        }
        self.sio.pulse_output_device_ready();
        true
    }

    fn sio_status_port(&self) -> u8 {
        self.sio.config().address.status()
    }

    fn sio_data_port(&self) -> u8 {
        self.sio.config().address.data()
    }

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

    pub(super) fn advance_t_states(&mut self, t_states: u64) {
        if t_states == 0 {
            return;
        }
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.advance_t_states(t_states, CLOCK_HZ),
            SerialBoard::TwoSio88 => {
                for port in &mut self.two_sio {
                    port.advance_t_states(t_states, CLOCK_HZ);
                }
            }
        }
    }

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

    pub(super) fn peek_input(&self, port: u8) -> u8 {
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
        self.trace
            .record(IO_TRACE_RX_ENQUEUE, self.data_port_for_index(0), byte);
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
        if let Some(byte) = completed {
            self.trace
                .record(IO_TRACE_TX_COMPLETE, self.data_port_for_index(0), byte);
        }
        completed
    }

    pub(super) fn serial_tx_busy(&self) -> bool {
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.endpoint_tx_pending_or_hardware_busy(),
            SerialBoard::TwoSio88 => self.two_sio[0].endpoint_tx_pending_or_hardware_busy(),
        }
    }

    pub(super) fn port1_receive(&mut self, byte: u8) {
        if self.serial_board != SerialBoard::TwoSio88 {
            return;
        }
        self.two_sio[1].queue_received_character(byte);
        self.trace
            .record(IO_TRACE_RX_ENQUEUE, self.data_port_for_index(1), byte);
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
        if self.serial_board != SerialBoard::TwoSio88 {
            return None;
        }
        let completed = self.two_sio[1].endpoint_tx_complete();
        if let Some(byte) = completed {
            self.trace
                .record(IO_TRACE_TX_COMPLETE, self.data_port_for_index(1), byte);
        }
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

    pub(super) fn trace_port_activity(
        &self,
        port: u8,
    ) -> (Option<u8>, Option<u8>, u64, u64) {
        self.trace.port_activity(port)
    }

    pub(super) fn trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.trace.snapshot()
    }

    pub(super) fn trace_enabled(&self) -> bool {
        self.trace.enabled
    }

    pub(super) fn set_trace_enabled(&mut self, enabled: bool) {
        self.trace.set_enabled(enabled);
    }

    pub(super) fn clear_trace(&mut self) {
        self.trace.clear_events();
    }

    pub(super) fn debugger_inject_rx(&mut self, port: u8, byte: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.debugger_inject_received_character(byte),
            SerialBoard::TwoSio88 => self.two_sio[index].debugger_inject_received_character(byte),
        }
        self.trace.record(IO_TRACE_RX_ENQUEUE, port, byte);
        true
    }

    pub(super) fn debugger_clear_rx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.clear_receive_for_debugger(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_receive_for_debugger(),
        }
        true
    }

    pub(super) fn debugger_clear_tx(&mut self, port: u8) -> bool {
        let Some(index) = self.data_port_index(port) else {
            return false;
        };
        match self.serial_board {
            SerialBoard::Sio88 => self.sio.clear_transmit_for_debugger(),
            SerialBoard::TwoSio88 => self.two_sio[index].clear_transmit_for_debugger(),
        }
        true
    }

    pub(super) fn debugger_complete_tx(&mut self, port: u8) -> Option<u8> {
        let index = self.data_port_index(port)?;
        let byte = match self.serial_board {
            SerialBoard::Sio88 => self.sio.debugger_complete_one_tx()?,
            SerialBoard::TwoSio88 => self.two_sio[index].debugger_complete_one_tx()?,
        };
        self.trace.record(IO_TRACE_TX_COMPLETE, port, byte);
        Some(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SioAddressPair, TwoSioAddressBlock};

    #[test]
    fn rev1_status_uses_d0_d7_without_fabricated_d6() {
        let mut io = IoDevices::default();
        assert_eq!(io.input(0x00), 0x01);
        assert!(io.debugger_inject_rx(0x01, b'R'));
        assert_eq!(io.peek_input(0x00) & 0xc1, 0x00);
        io.output(0x01, b'A');
        io.output(0x01, b'B');
        assert_eq!(io.peek_input(0x00) & 0xc0, 0x80);
    }

    #[test]
    fn sio_address_jumpers_move_the_device_decoder() {
        let mut io = IoDevices::default();
        io.configure_sio_hardware(SioHardwareConfig {
            address: SioAddressPair::try_new(0x06).unwrap(),
            ..SioHardwareConfig::default()
        });
        assert_eq!(io.input(0x00), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(0x06), 0x01);
        assert!(io.debugger_inject_rx(0x07, b'J'));
        assert_eq!(io.input(0x07), b'J');
    }

    #[test]
    fn two_sio_interrupt_routing_is_card_local() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.output(0x10, 0x95);
        assert!(io.debugger_inject_rx(0x11, b'A'));
        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Vi3,
            port1: TwoSioInterruptTarget::Disconnected,
        });
        assert!(!io.interrupt_request());
        assert_eq!(io.vector_interrupt_requests(), 1 << 3);
        io.configure_two_sio_interrupt_wiring(TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Pint,
            port1: TwoSioInterruptTarget::Disconnected,
        });
        assert!(io.interrupt_request());
        assert_eq!(io.vector_interrupt_requests(), 0);
    }

    #[test]
    fn two_sio_straps_move_decode_and_keep_ports_independent() {
        let mut io = IoDevices::default();
        io.configure_serial_board(SerialBoard::TwoSio88);
        io.configure_two_sio_straps(TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        });
        assert_eq!(io.input(0x10), S100_OPEN_BUS_VALUE);
        assert_eq!(io.input(0x44) & 0x02, 0x02);
        assert_eq!(io.input(0x46) & 0x02, 0x02);
        io.output(0x45, b'0');
        io.output(0x47, b'1');
        assert!(io.serial_tx_busy());
        assert!(io.port1_tx_busy());
    }
}
