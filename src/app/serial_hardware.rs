use super::*;

impl RusTairApp {
    /// Apply the physical 88-SIO jumper/UART configuration.
    ///
    /// Revision, interface variant, address decode, baud preset, COM2502 word
    /// format and interrupt routing are board-level wiring. They therefore cannot
    /// be changed while the emulated chassis is powered, just as the 88-2SIO
    /// strap controls cannot.
    pub(in crate::app) fn apply_sio_hardware(&mut self, config: crate::config::SioHardwareConfig) {
        if self.config.machine.sio_hardware == config { return; }
        if self.machine.powered() {
            self.status = "Power OFF the Altair before changing 88-SIO hardware wiring".into();
            return;
        }

        // The built-in ASR-33 represents a direct TTY/current-loop device. If
        // the physical card is changed to the A (RS-232) or B (TTL) interface,
        // keeping that same virtual cable attached would silently invent a level
        // converter. Unplug it instead; reconnecting through an explicit future
        // adapter remains possible without falsifying the base hardware.
        let disconnect_asr = self.config.machine.serial_board == SerialBoard::Sio88
            && config.interface != crate::config::SioInterface::TtyC
            && self.asr_connection().is_connected();

        self.config.machine.sio_hardware = config;
        self.machine.configure_sio_hardware(config);
        if disconnect_asr {
            self.serial_router.connect(
                SerialDevice::InternalAsr33,
                SerialConnection::Disconnected,
            );
        }
        self.asr33.tx_started = None;
        self.asr33.answerback.clear();
        self.terminal.tx_started = None;
        self.external_serial.reset_line_timing();
        self.external_com.reset_line_timing();
        let now = Instant::now();
        self.last_tick = now;
        self.execution_clock.reset_at(now);
        self.status = format!(
            "88-SIO hardware: {} · {:02X}h/{:02X}h · {} · {} · {} · IN→{} · OUT→{}{}",
            config.revision.label(),
            config.address.status(),
            config.address.data(),
            config.interface.label(),
            config.baud.label(),
            config.format.label(),
            config.interrupt_wiring.input.label(),
            config.interrupt_wiring.output.label(),
            if disconnect_asr {
                " · ASR-33 cable disconnected: direct connection requires 88-SIO C current loop"
            } else {
                ""
            },
        );
    }

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

    /// Continuous MC6850 spacing/BREAK output at the selected cable boundary.
    pub(in crate::app) fn serial_break_active_at(&mut self, connection: SerialConnection) -> Option<bool> {
        Self::backend_serial_port(connection)
            .and_then(|port| self.machine.serial_modem_lines(port))
            .map(|lines| lines.break_active)
    }

    /// Drive the physical CTS/DCD input levels presented to the installed ACIA.
    /// The caller supplies literal MC6850 TTL levels, not RS-232 assertion state.
    pub(in crate::app) fn serial_set_modem_inputs_at(
        &mut self,
        connection: SerialConnection,
        cts_high: bool,
        dcd_high: bool,
    ) -> bool {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_set_modem_inputs(port, cts_high, dcd_high))
            .unwrap_or(false)
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
