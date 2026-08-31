use super::*;

impl RusTairApp {
    /// Physical receive-line availability. This is intentionally different from
    /// RDR/RDRF emptiness: an MC6850 may have an unread byte in RDR while its
    /// receive shift register / external line is already ready for the next frame.
    pub(in crate::app) fn serial_rx_line_idle_at(&mut self, connection: SerialConnection) -> bool {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_rx_line_idle(port))
            .unwrap_or(true)
    }

    /// Physical RTS level driven by the MC6850 attached to this virtual cable.
    /// Returns None for disconnected cables and for the revision-sensitive 88-SIO,
    /// which must not fabricate MC6850 pins.
    pub(in crate::app) fn serial_rts_high_at(&mut self, connection: SerialConnection) -> Option<bool> {
        Self::backend_serial_port(connection)
            .and_then(|port| self.machine.serial_modem_lines(port))
            .map(|lines| lines.rts_high)
    }

    pub(in crate::app) fn asr_serial_rx_line_idle(&mut self) -> bool {
        let connection = self.asr_connection();
        self.serial_rx_line_idle_at(connection)
    }

    pub(in crate::app) fn asr_serial_rts_high(&mut self) -> Option<bool> {
        let connection = self.asr_connection();
        self.serial_rts_high_at(connection)
    }

    /// Resolve the actual reader motor command from the selected physical
    /// control wiring. Manual mode uses the local reader switch. 88-TYA mode
    /// follows the MC6850 RTS pin directly: HIGH runs ReaderRun+, LOW stops it.
    pub(in crate::app) fn asr_reader_motor_running(&mut self) -> bool {
        let rts_high = self.asr_serial_rts_high();
        self.asr33
            .reader_control
            .effective_running(self.asr33.reader_running, rts_high)
    }

    pub(in crate::app) fn terminal_serial_rx_line_idle(&mut self) -> bool {
        let connection = self.terminal_connection();
        self.serial_rx_line_idle_at(connection)
    }
}
