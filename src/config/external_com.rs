use std::time::Duration;

use super::{ExternalSerialCharacterMode, TerminalDuplex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComDataBits {
    Five,
    Six,
    Seven,
    Eight,
}

impl ComDataBits {
    pub const ALL: [Self; 4] = [Self::Five, Self::Six, Self::Seven, Self::Eight];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
        }
    }

    pub const fn bits(self) -> u32 {
        match self {
            Self::Five => 5,
            Self::Six => 6,
            Self::Seven => 7,
            Self::Eight => 8,
        }
    }
}

impl Default for ComDataBits {
    fn default() -> Self {
        Self::Eight
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComParity {
    None,
    Odd,
    Even,
}

impl ComParity {
    pub const ALL: [Self; 3] = [Self::None, Self::Odd, Self::Even];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Odd => "Odd",
            Self::Even => "Even",
        }
    }

    pub const fn frame_bits(self) -> u32 {
        if matches!(self, Self::None) { 0 } else { 1 }
    }
}

impl Default for ComParity {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComStopBits {
    One,
    Two,
}

impl ComStopBits {
    pub const ALL: [Self; 2] = [Self::One, Self::Two];

    pub const fn label(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Two => "2",
        }
    }

    pub const fn bits(self) -> u32 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

impl Default for ComStopBits {
    fn default() -> Self {
        Self::One
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComFlowControl {
    None,
    Software,
    Hardware,
}

impl ComFlowControl {
    pub const ALL: [Self; 3] = [Self::None, Self::Software, Self::Hardware];

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Software => "XON/XOFF",
            Self::Hardware => "RTS/CTS",
        }
    }
}

impl Default for ComFlowControl {
    fn default() -> Self {
        Self::None
    }
}

/// Physical wiring of the host COM modem-input pins to an emulated MC6850.
///
/// MITS explicitly instructs installers to jumper CTS and DCD to ground when
/// they are not connected. `Grounded` is therefore the historically safe
/// no-modem default rather than an emulator convenience.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ComModemInputMode {
    #[default]
    Grounded,
    HostPins,
}

impl ComModemInputMode {
    pub const ALL: [Self; 2] = [Self::Grounded, Self::HostPins];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Grounded => "Grounded — MITS no-modem jumpers",
            Self::HostPins => "Follow host CTS / Carrier Detect",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalComConfig {
    pub enabled: bool,
    pub port_name: String,
    pub baud_rate: u32,
    pub data_bits: ComDataBits,
    pub parity: ComParity,
    pub stop_bits: ComStopBits,
    pub flow_control: ComFlowControl,
    pub modem_inputs: ComModemInputMode,
    pub character_mode: ExternalSerialCharacterMode,
    pub duplex: TerminalDuplex,
}

impl ExternalComConfig {
    pub fn frame_bits(&self) -> u32 {
        1 + self.data_bits.bits() + self.parity.frame_bits() + self.stop_bits.bits()
    }

    pub fn char_time(&self) -> Duration {
        if self.baud_rate == 0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(self.frame_bits() as f64 / self.baud_rate as f64)
        }
    }

    pub fn framing_label(&self) -> String {
        let parity = match self.parity {
            ComParity::None => "N",
            ComParity::Odd => "O",
            ComParity::Even => "E",
        };
        format!(
            "{} {}{}{}",
            self.baud_rate,
            self.data_bits.label(),
            parity,
            self.stop_bits.label()
        )
    }
}

impl Default for ExternalComConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port_name: String::new(),
            baud_rate: 9_600,
            data_bits: ComDataBits::Eight,
            parity: ComParity::None,
            stop_bits: ComStopBits::One,
            flow_control: ComFlowControl::None,
            modem_inputs: ComModemInputMode::Grounded,
            character_mode: ExternalSerialCharacterMode::Asr33Uppercase,
            duplex: TerminalDuplex::FullDuplexRemoteEcho,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_time_uses_real_frame_width() {
        let config = ExternalComConfig {
            baud_rate: 110,
            data_bits: ComDataBits::Seven,
            parity: ComParity::Even,
            stop_bits: ComStopBits::Two,
            ..ExternalComConfig::default()
        };
        assert_eq!(config.frame_bits(), 11);
        assert_eq!(config.char_time(), Duration::from_secs_f64(11.0 / 110.0));

        let config = ExternalComConfig::default();
        assert_eq!(config.frame_bits(), 10);
        assert_eq!(config.char_time(), Duration::from_secs_f64(10.0 / 9_600.0));
    }

    #[test]
    fn unconnected_modem_inputs_default_to_mits_ground_jumpers() {
        assert_eq!(
            ExternalComConfig::default().modem_inputs,
            ComModemInputMode::Grounded
        );
    }
}