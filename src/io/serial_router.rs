use crate::config::{SerialBoard, SioInterface};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialDevice {
    InternalAsr33,
    TextTerminal,
    ExternalTcp,
    ExternalCom,
}

impl SerialDevice {
    /// Whether this endpoint can truthfully occupy the external connector of the
    /// selected original 88-SIO interface without inventing an unconfigured
    /// physical level converter.
    ///
    /// The built-in ASR-33 is a direct TTY/current-loop endpoint and therefore
    /// requires the C interface. External COM is currently modeled as an
    /// RS-232 host link and therefore requires A. Text Terminal and raw TCP are
    /// explicitly virtual peers: their connector side is instantiated in the
    /// selected A/B/C electrical family, but they remain data-only and never
    /// fabricate the independent Rev0 RIN/ROT ready pulses.
    pub(crate) const fn supports_sio_interface(self, interface: SioInterface) -> bool {
        match self {
            Self::InternalAsr33 => matches!(interface, SioInterface::TtyC),
            Self::ExternalCom => matches!(interface, SioInterface::Rs232A),
            Self::TextTerminal | Self::ExternalTcp => true,
        }
    }

    pub(crate) const fn sio_requirement_label(self) -> &'static str {
        match self {
            Self::InternalAsr33 => "direct ASR-33 cable requires 88-SIO C current loop",
            Self::ExternalCom => "External COM direct cable requires 88-SIO A RS-232",
            Self::TextTerminal => "virtual terminal matches the selected 88-SIO A/B/C interface",
            Self::ExternalTcp => "virtual TCP peer matches the selected 88-SIO A/B/C interface",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialConnection {
    Disconnected,
    Port0,
    Port1,
}

impl SerialConnection {
    pub(crate) const fn is_connected(self) -> bool {
        !matches!(self, Self::Disconnected)
    }
}

/// Models the external serial cables between the installed MITS interface and
/// RusTair's host-side endpoints. Each emulated physical serial port has at
/// most one attached endpoint/cable.
pub(crate) struct SerialRouter {
    asr33: SerialConnection,
    text_terminal: SerialConnection,
    external_tcp: SerialConnection,
    external_com: SerialConnection,
}

impl Default for SerialRouter {
    fn default() -> Self {
        Self {
            asr33: SerialConnection::Port0,
            text_terminal: SerialConnection::Disconnected,
            external_tcp: SerialConnection::Disconnected,
            external_com: SerialConnection::Disconnected,
        }
    }
}

impl SerialRouter {
    pub(crate) fn connection(&self, device: SerialDevice) -> SerialConnection {
        match device {
            SerialDevice::InternalAsr33 => self.asr33,
            SerialDevice::TextTerminal => self.text_terminal,
            SerialDevice::ExternalTcp => self.external_tcp,
            SerialDevice::ExternalCom => self.external_com,
        }
    }

    pub(crate) fn device_on(&self, connection: SerialConnection) -> Option<SerialDevice> {
        if connection == SerialConnection::Disconnected {
            return None;
        }
        if self.asr33 == connection {
            Some(SerialDevice::InternalAsr33)
        } else if self.text_terminal == connection {
            Some(SerialDevice::TextTerminal)
        } else if self.external_tcp == connection {
            Some(SerialDevice::ExternalTcp)
        } else if self.external_com == connection {
            Some(SerialDevice::ExternalCom)
        } else {
            None
        }
    }

    /// Connect an endpoint to a physical port. If the requested port is already
    /// occupied, the existing endpoint is unplugged first, exactly as moving a
    /// single cable would behave.
    pub(crate) fn connect(
        &mut self,
        device: SerialDevice,
        connection: SerialConnection,
    ) -> Option<SerialDevice> {
        let displaced = if connection != SerialConnection::Disconnected {
            self.device_on(connection).filter(|current| *current != device)
        } else {
            None
        };

        if let Some(current) = displaced {
            match current {
                SerialDevice::InternalAsr33 => self.asr33 = SerialConnection::Disconnected,
                SerialDevice::TextTerminal => {
                    self.text_terminal = SerialConnection::Disconnected
                }
                SerialDevice::ExternalTcp => self.external_tcp = SerialConnection::Disconnected,
                SerialDevice::ExternalCom => self.external_com = SerialConnection::Disconnected,
            }
        }

        match device {
            SerialDevice::InternalAsr33 => self.asr33 = connection,
            SerialDevice::TextTerminal => self.text_terminal = connection,
            SerialDevice::ExternalTcp => self.external_tcp = connection,
            SerialDevice::ExternalCom => self.external_com = connection,
        }

        displaced
    }

    /// Replacing the installed serial board also replaces its external cabling.
    /// Host transports may remain enabled, but TCP and COM start electrically
    /// disconnected until the user explicitly attaches them again.
    pub(crate) fn reset_for_board(&mut self, board: SerialBoard) {
        self.external_tcp = SerialConnection::Disconnected;
        self.external_com = SerialConnection::Disconnected;
        match board {
            SerialBoard::Sio88 => {
                self.asr33 = SerialConnection::Port0;
                self.text_terminal = SerialConnection::Disconnected;
            }
            SerialBoard::TwoSio88 => {
                self.asr33 = SerialConnection::Port0;
                self.text_terminal = SerialConnection::Port1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_wiring_connects_only_internal_asr33() {
        let router = SerialRouter::default();
        assert_eq!(
            router.connection(SerialDevice::InternalAsr33),
            SerialConnection::Port0
        );
        assert_eq!(
            router.connection(SerialDevice::TextTerminal),
            SerialConnection::Disconnected
        );
        assert_eq!(
            router.connection(SerialDevice::ExternalTcp),
            SerialConnection::Disconnected
        );
        assert_eq!(
            router.connection(SerialDevice::ExternalCom),
            SerialConnection::Disconnected
        );
    }

    #[test]
    fn two_sio_default_wiring_uses_both_ports_and_leaves_host_endpoints_unplugged() {
        let mut router = SerialRouter::default();
        router.reset_for_board(SerialBoard::TwoSio88);
        assert_eq!(
            router.connection(SerialDevice::InternalAsr33),
            SerialConnection::Port0
        );
        assert_eq!(
            router.connection(SerialDevice::TextTerminal),
            SerialConnection::Port1
        );
        assert_eq!(
            router.connection(SerialDevice::ExternalTcp),
            SerialConnection::Disconnected
        );
        assert_eq!(
            router.connection(SerialDevice::ExternalCom),
            SerialConnection::Disconnected
        );
    }

    #[test]
    fn moving_a_cable_displaces_the_previous_endpoint() {
        let mut router = SerialRouter::default();
        let displaced = router.connect(SerialDevice::ExternalCom, SerialConnection::Port0);
        assert_eq!(displaced, Some(SerialDevice::InternalAsr33));
        assert_eq!(
            router.connection(SerialDevice::ExternalCom),
            SerialConnection::Port0
        );
        assert_eq!(
            router.connection(SerialDevice::InternalAsr33),
            SerialConnection::Disconnected
        );
    }

    #[test]
    fn tcp_and_com_can_use_different_two_sio_ports() {
        let mut router = SerialRouter::default();
        router.reset_for_board(SerialBoard::TwoSio88);
        router.connect(SerialDevice::ExternalTcp, SerialConnection::Port0);
        router.connect(SerialDevice::ExternalCom, SerialConnection::Port1);
        assert_eq!(
            router.connection(SerialDevice::ExternalTcp),
            SerialConnection::Port0
        );
        assert_eq!(
            router.connection(SerialDevice::ExternalCom),
            SerialConnection::Port1
        );
        assert_eq!(
            router.connection(SerialDevice::InternalAsr33),
            SerialConnection::Disconnected
        );
        assert_eq!(
            router.connection(SerialDevice::TextTerminal),
            SerialConnection::Disconnected
        );
    }

    #[test]
    fn original_sio_direct_endpoint_wiring_does_not_invent_level_converters() {
        assert!(SerialDevice::InternalAsr33.supports_sio_interface(SioInterface::TtyC));
        assert!(!SerialDevice::InternalAsr33.supports_sio_interface(SioInterface::Rs232A));
        assert!(!SerialDevice::InternalAsr33.supports_sio_interface(SioInterface::TtlB));

        assert!(SerialDevice::ExternalCom.supports_sio_interface(SioInterface::Rs232A));
        assert!(!SerialDevice::ExternalCom.supports_sio_interface(SioInterface::TtlB));
        assert!(!SerialDevice::ExternalCom.supports_sio_interface(SioInterface::TtyC));

        for interface in [SioInterface::Rs232A, SioInterface::TtlB, SioInterface::TtyC] {
            assert!(SerialDevice::TextTerminal.supports_sio_interface(interface));
            assert!(SerialDevice::ExternalTcp.supports_sio_interface(interface));
        }
    }
}
