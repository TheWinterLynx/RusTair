use crate::config::SerialBoard;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialDevice {
    InternalAsr33,
    TextTerminal,
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
/// RusTair's host-side terminal devices.
///
/// A physical serial port may have at most one attached device. The single-port
/// 88-SIO therefore exposes only Port 0, while a fully populated 88-2SIO exposes
/// independent Port 0 and Port 1 connections.
pub(crate) struct SerialRouter {
    asr33: SerialConnection,
    text_terminal: SerialConnection,
}

impl Default for SerialRouter {
    fn default() -> Self {
        Self {
            asr33: SerialConnection::Port0,
            text_terminal: SerialConnection::Disconnected,
        }
    }
}

impl SerialRouter {
    pub(crate) fn connection(&self, device: SerialDevice) -> SerialConnection {
        match device {
            SerialDevice::InternalAsr33 => self.asr33,
            SerialDevice::TextTerminal => self.text_terminal,
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
        } else {
            None
        }
    }

    /// Connect a device to a physical port. If the requested port is already
    /// occupied, the existing device is unplugged first, exactly as moving a
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
            }
        }

        match device {
            SerialDevice::InternalAsr33 => self.asr33 = connection,
            SerialDevice::TextTerminal => self.text_terminal = connection,
        }

        displaced
    }

    /// A board swap represents physically replacing the serial interface, so
    /// start from a deterministic, useful cable layout for that board.
    pub(crate) fn reset_for_board(&mut self, board: SerialBoard) {
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
    fn default_wiring_connects_asr33_to_single_sio_port() {
        let router = SerialRouter::default();
        assert_eq!(
            router.connection(SerialDevice::InternalAsr33),
            SerialConnection::Port0
        );
        assert_eq!(
            router.connection(SerialDevice::TextTerminal),
            SerialConnection::Disconnected
        );
    }

    #[test]
    fn two_sio_default_wiring_uses_both_ports() {
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
    }

    #[test]
    fn moving_a_cable_displaces_the_previous_device() {
        let mut router = SerialRouter::default();
        let displaced = router.connect(SerialDevice::TextTerminal, SerialConnection::Port0);
        assert_eq!(displaced, Some(SerialDevice::InternalAsr33));
        assert_eq!(
            router.connection(SerialDevice::InternalAsr33),
            SerialConnection::Disconnected
        );
        assert_eq!(
            router.connection(SerialDevice::TextTerminal),
            SerialConnection::Port0
        );
    }
}
