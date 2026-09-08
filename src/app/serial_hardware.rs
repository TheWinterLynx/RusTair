use super::*;
use crate::config::S100InstalledCardConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) enum PhysicalSerialPortKind {
    SioStatus,
    SioData,
    TwoSioStatus(u8),
    TwoSioData(u8),
}

impl PhysicalSerialPortKind {
    pub(in crate::app) const fn is_status(self) -> bool {
        matches!(self, Self::SioStatus | Self::TwoSioStatus(_))
    }

    pub(in crate::app) const fn is_data(self) -> bool {
        matches!(self, Self::SioData | Self::TwoSioData(_))
    }

    pub(in crate::app) const fn label(self) -> &'static str {
        match self {
            Self::SioStatus => "MITS 88-SIO status",
            Self::SioData => "MITS 88-SIO data",
            Self::TwoSioStatus(0) => "MITS 88-2SIO Port 0 status/control",
            Self::TwoSioData(0) => "MITS 88-2SIO Port 0 data",
            Self::TwoSioStatus(1) => "MITS 88-2SIO Port 1 status/control",
            Self::TwoSioData(1) => "MITS 88-2SIO Port 1 data",
            Self::TwoSioStatus(_) => "MITS 88-2SIO status/control",
            Self::TwoSioData(_) => "MITS 88-2SIO data",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::app) struct PhysicalSerialPortBinding {
    pub(in crate::app) slot: usize,
    pub(in crate::app) port: u8,
    pub(in crate::app) kind: PhysicalSerialPortKind,
}

impl RusTairApp {
    /// Return every physical S-100 serial-card decoder that responds to `port`.
    /// Multiple bindings are deliberate: overlapping I/O decoders are legal on
    /// the electrical bus and debugger UIs must expose, not hide, contention.
    pub(in crate::app) fn physical_serial_port_bindings(
        &self,
        port: u8,
    ) -> Vec<PhysicalSerialPortBinding> {
        let mut bindings = Vec::new();
        for (slot, card) in self.config.machine.s100_hardware.serial_slots() {
            match card {
                S100InstalledCardConfig::Mits88Sio(config) => {
                    if port == config.address.status() {
                        bindings.push(PhysicalSerialPortBinding {
                            slot,
                            port,
                            kind: PhysicalSerialPortKind::SioStatus,
                        });
                    }
                    if port == config.address.data() {
                        bindings.push(PhysicalSerialPortBinding {
                            slot,
                            port,
                            kind: PhysicalSerialPortKind::SioData,
                        });
                    }
                }
                S100InstalledCardConfig::Mits88TwoSio { straps, .. } => {
                    for (mapped_port, kind) in [
                        (
                            straps.address.port0_status(),
                            PhysicalSerialPortKind::TwoSioStatus(0),
                        ),
                        (
                            straps.address.port0_data(),
                            PhysicalSerialPortKind::TwoSioData(0),
                        ),
                        (
                            straps.address.port1_status(),
                            PhysicalSerialPortKind::TwoSioStatus(1),
                        ),
                        (
                            straps.address.port1_data(),
                            PhysicalSerialPortKind::TwoSioData(1),
                        ),
                    ] {
                        if port == mapped_port {
                            bindings.push(PhysicalSerialPortBinding {
                                slot,
                                port,
                                kind,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        bindings
    }

    pub(in crate::app) fn physical_serial_inventory_label(&self) -> String {
        let cards = self
            .config
            .machine
            .s100_hardware
            .serial_slots()
            .map(|(slot, card)| match card {
                S100InstalledCardConfig::Mits88Sio(config) => format!(
                    "Slot {slot} · 88-SIO [{:02X}h/{:02X}h]",
                    config.address.status(),
                    config.address.data()
                ),
                S100InstalledCardConfig::Mits88TwoSio { straps, .. } => format!(
                    "Slot {slot} · 88-2SIO [{:02X}h-{:02X}h]",
                    straps.address.base(),
                    straps.address.base().saturating_add(3)
                ),
                _ => unreachable!("serial_slots returned a non-serial card"),
            })
            .collect::<Vec<_>>();
        match cards.as_slice() {
            [] => "No serial card installed".into(),
            [only] => only.clone(),
            _ => format!("{} serial cards · {}", cards.len(), cards.join(" · ")),
        }
    }

    pub(in crate::app) fn first_physical_serial_data_port(&self) -> Option<u8> {
        self.config
            .machine
            .s100_hardware
            .serial_slots()
            .next()
            .map(|(_, card)| match card {
                S100InstalledCardConfig::Mits88Sio(config) => config.address.data(),
                S100InstalledCardConfig::Mits88TwoSio { straps, .. } => {
                    straps.address.port0_data()
                }
                _ => unreachable!("serial_slots returned a non-serial card"),
            })
    }

    /// Physical receive-line availability. This is intentionally different from
    /// RDR/RDRF emptiness: an MC6850 may have an unread byte in RDR while its
    /// receive shift register / external line is already ready for the next frame.
    pub(in crate::app) fn serial_rx_line_idle_at(&mut self, connection: SerialConnection) -> bool {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_rx_line_idle(port))
            .unwrap_or(true)
    }

    /// Drive the attached UART's receive wire to continuous SPACE/BREAK or back
    /// to MARK. This is deliberately a physical line operation rather than a
    /// special byte, and works for either installed serial-card family.
    pub(in crate::app) fn serial_set_receive_break_at(
        &mut self,
        connection: SerialConnection,
        active: bool,
    ) -> bool {
        Self::backend_serial_port(connection)
            .map(|port| self.machine.serial_set_receive_break(port, active))
            .unwrap_or(false)
    }

    /// Physical RTS level driven by the MC6850 attached to this virtual cable.
    /// Returns None for disconnected cables and for the revision-sensitive 88-SIO,
    /// which must not fabricate MC6850 pins.
    pub(in crate::app) fn serial_rts_high_at(
        &mut self,
        connection: SerialConnection,
    ) -> Option<bool> {
        Self::backend_serial_port(connection)
            .and_then(|port| self.machine.serial_modem_lines(port))
            .map(|lines| lines.rts_high)
    }

    /// Continuous MC6850 spacing/BREAK output at the selected cable boundary.
    pub(in crate::app) fn serial_break_active_at(
        &mut self,
        connection: SerialConnection,
    ) -> Option<bool> {
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

    pub(in crate::app) fn asr_serial_set_receive_break(&mut self, active: bool) -> bool {
        let connection = self.asr_connection();
        self.serial_set_receive_break_at(connection, active)
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
