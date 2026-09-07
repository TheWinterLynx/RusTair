use crate::config::SerialBoard;

use super::AltairBus;

impl AltairBus {
    /// Raw VI levels sourced by the installed 88-2SIO. They remain separate
    /// from processor PINT until a real 88-VI card is installed to arbitrate them.
    pub fn two_sio_vector_interrupt_requests(&self) -> u8 {
        if self.memory.primary_serial_board() == Some(SerialBoard::TwoSio88) {
            self.memory.serial_vector_interrupt_requests()
        } else {
            0
        }
    }

    /// Raw VI levels sourced by the installed 88-SIO.
    pub fn sio_vector_interrupt_requests(&self) -> u8 {
        if self.memory.primary_serial_board() == Some(SerialBoard::Sio88) {
            self.memory.serial_vector_interrupt_requests()
        } else {
            0
        }
    }

    pub fn serial_modem_lines(&self, port_index: usize) -> Option<(bool, bool, bool, bool)> {
        self.memory.serial_modem_lines(port_index)
    }

    pub fn set_serial_modem_inputs(
        &mut self,
        port_index: usize,
        cts_high: bool,
        dcd_high: bool,
    ) -> bool {
        self.memory
            .set_serial_modem_inputs(port_index, cts_high, dcd_high)
    }

    pub fn set_serial_receive_break(&mut self, port_index: usize, active: bool) -> bool {
        self.memory.set_serial_receive_break(port_index, active)
    }

    /// `(RIN ready latched, ROT ready latched, BIN high, BOT high)` at the
    /// logical 88-SIO board/interface boundary.
    pub fn sio_handshake_lines(&self) -> Option<(bool, bool, bool, bool)> {
        self.memory
            .sio_handshake_lines()
            .map(|lines| (lines.1, lines.2, lines.4, lines.5))
    }

    pub fn pulse_sio_input_device_ready(&mut self) -> bool {
        self.memory.pulse_sio_input_device_ready()
    }

    pub fn pulse_sio_output_device_ready(&mut self) -> bool {
        self.memory.pulse_sio_output_device_ready()
    }

    pub(crate) fn advance_serial_hardware_time(&mut self, t_states: u64) {
        self.memory.advance_serial_time(t_states);
    }

    pub fn serial_port1_receive(&mut self, byte: u8) {
        let _ = self.memory.serial_receive(1, byte);
    }

    pub fn serial_port1_rx_empty(&self) -> bool {
        self.memory.serial_rx_empty(1)
    }

    pub fn serial_port1_rx_len(&self) -> usize {
        self.memory.serial_rx_len(1)
    }

    pub fn serial_port1_rx_line_idle(&self) -> bool {
        self.memory.serial_rx_line_idle(1)
    }

    pub fn serial_port1_tx_front(&self) -> Option<u8> {
        self.memory.serial_tx_front(1)
    }

    pub fn serial_port1_tx_complete(&mut self) -> Option<u8> {
        self.memory.serial_tx_complete(1)
    }

    pub fn serial_port1_tx_busy(&self) -> bool {
        self.memory.serial_tx_busy(1)
    }

    pub fn serial_rx_line_idle(&self) -> bool {
        self.memory.serial_rx_line_idle(0)
    }

    pub fn peek_io_port(&self, port: u8) -> u8 {
        if port == 0xff {
            self.panel.input()
        } else {
            self.memory.peek_io_port(port)
        }
    }

    pub fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        if port == 0xff {
            (None, None, 0, 0)
        } else {
            self.memory.io_port_activity(port)
        }
    }

    pub fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.memory.io_trace_snapshot()
    }

    pub fn io_trace_enabled(&self) -> bool {
        self.memory.io_trace_enabled()
    }

    pub fn set_io_trace_enabled(&mut self, enabled: bool) {
        self.memory.set_io_trace_enabled(enabled);
    }

    pub fn clear_io_trace(&mut self) {
        self.memory.clear_io_trace();
    }

    pub fn debugger_input_port(&mut self, port: u8) -> u8 {
        if port == 0xff {
            self.panel.input()
        } else {
            self.memory.debugger_input_port(port)
        }
    }

    pub fn debugger_output_port(&mut self, port: u8, value: u8) {
        if port != 0xff {
            self.memory.debugger_output_port(port, value);
        }
    }

    pub fn debugger_inject_serial_rx(&mut self, data_port: u8, byte: u8) -> bool {
        self.memory.debugger_inject_serial_rx(data_port, byte)
    }

    pub fn debugger_clear_serial_rx(&mut self, data_port: u8) -> bool {
        self.memory.debugger_clear_serial_rx(data_port)
    }

    pub fn debugger_clear_serial_tx(&mut self, data_port: u8) -> bool {
        self.memory.debugger_clear_serial_tx(data_port)
    }

    pub fn debugger_complete_serial_tx(&mut self, data_port: u8) -> Option<u8> {
        self.memory.debugger_complete_serial_tx(data_port)
    }
}