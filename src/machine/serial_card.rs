use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::config::{SerialBoard, SioHardwareConfig, TwoSioInterruptWiring, TwoSioStraps};
use crate::s100::S100Signal;
use crate::s100_backplane::S100BusSample;
use crate::s100_io_card::{S100IoDeviceLines, S100IoRegisterDevice};

use super::serial_devices::IoDevices;

/// Shared ownership boundary for one physical serial card instance.
///
/// The S-100 slot and host endpoint/debugger handle point at the same finite
/// UART state. No machine-wide UART exists beside this card.
#[derive(Clone)]
pub(crate) struct RuntimeSerialCardHandle {
    state: Rc<RefCell<IoDevices>>,
    connector_dirty: Rc<Cell<bool>>,
    board: SerialBoard,
    base: u8,
}

/// Register-facing half installed in the S-100 backplane.
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

    pub(crate) fn timing_is_quiet(&self) -> bool {
        self.state.borrow().serial_timing_is_quiet()
    }

    #[cfg(test)]
    pub(crate) const fn base(&self) -> u8 {
        self.base
    }

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

    pub(crate) fn tx_front(&self, port_index: usize) -> Option<u8> {
        let state = self.state.borrow();
        match (self.board, port_index) {
            (_, 0) => state.serial_tx_front(),
            (SerialBoard::TwoSio88, 1) => state.port1_tx_front(),
            _ => None,
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

    pub(crate) fn supports_port(&self, port_index: usize) -> bool {
        port_index == 0 || (self.board == SerialBoard::TwoSio88 && port_index == 1)
    }

    pub(crate) fn data_port_matches(&self, port: u8) -> bool {
        let state = self.state.borrow();
        match self.board {
            SerialBoard::Sio88 => port == state.sio_hardware().address.data(),
            SerialBoard::TwoSio88 => {
                matches!(state.two_sio_straps().address.offset(port), Some(1) | Some(3))
            }
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

    pub(crate) fn peek_input(&self, port: u8) -> u8 {
        self.state.borrow().peek_input(port)
    }

    pub(crate) fn debugger_input(&self, port: u8) -> u8 {
        let value = self.state.borrow_mut().input(port);
        self.connector_dirty.set(true);
        value
    }

    pub(crate) fn debugger_output(&self, port: u8, value: u8) {
        self.state.borrow_mut().output(port, value);
        self.connector_dirty.set(true);
    }

    pub(crate) fn clear(&self) {
        self.state.borrow_mut().clear_serial();
        self.connector_dirty.set(true);
    }

    pub(crate) fn modem_lines(&self, port_index: usize) -> Option<(bool, bool, bool, bool)> {
        self.state.borrow().modem_lines(port_index)
    }

    pub(crate) fn set_modem_inputs(
        &self,
        port_index: usize,
        cts: bool,
        dcd: bool,
    ) -> bool {
        let accepted = self
            .state
            .borrow_mut()
            .set_modem_inputs(port_index, cts, dcd);
        self.connector_dirty.set(true);
        accepted
    }

    pub(crate) fn set_receive_break(&self, port_index: usize, active: bool) -> bool {
        let accepted = self
            .state
            .borrow_mut()
            .set_receive_break(port_index, active);
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

    pub(crate) fn io_port_activity(&self, port: u8) -> (Option<u8>, Option<u8>, u64, u64) {
        self.state.borrow().trace_port_activity(port)
    }

    pub(crate) fn io_trace_snapshot(&self) -> Vec<(u64, u8, u8, u8, u32)> {
        self.state.borrow().trace_snapshot()
    }

    pub(crate) fn io_trace_enabled(&self) -> bool {
        self.state.borrow().trace_enabled()
    }

    pub(crate) fn set_io_trace_enabled(&self, enabled: bool) {
        self.state.borrow_mut().set_trace_enabled(enabled);
    }

    pub(crate) fn clear_io_trace(&self) {
        self.state.borrow_mut().clear_trace();
    }
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

    fn external_drive_dirty(&self) -> bool {
        self.handle.connector_dirty.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
