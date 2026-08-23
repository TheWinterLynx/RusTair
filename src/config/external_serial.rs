use std::net::Ipv4Addr;
use std::time::Duration;

use super::terminal::TerminalDuplex;

/// Character handling at the host-side serial endpoint.
///
/// Altair BASIC 3.2 emits terminal text with bit 7 used/set on some output
/// bytes. Period terminals treated the link as 7-bit ASCII, and the ASR-33 was
/// an uppercase-only terminal. Keep that historically useful behavior as the
/// default, while retaining case-preserving 7-bit and byte-transparent modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalSerialCharacterMode {
    Asr33Uppercase,
    SevenBitAscii,
    Raw8Bit,
}

impl ExternalSerialCharacterMode {
    pub const ALL: [Self; 3] = [
        Self::Asr33Uppercase,
        Self::SevenBitAscii,
        Self::Raw8Bit,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Asr33Uppercase => "ASR-33 style (7-bit + uppercase input)",
            Self::SevenBitAscii => "7-bit ASCII (preserve case)",
            Self::Raw8Bit => "Raw 8-bit bytes",
        }
    }

    /// Transform a byte travelling from the host terminal into the emulated
    /// UART. ASR-33 mode models the uppercase-only keyboard rather than merely
    /// masking bit 7.
    pub const fn rx_transform(self, byte: u8) -> u8 {
        match self {
            Self::Asr33Uppercase => {
                let byte = byte & 0x7f;
                if byte >= b'a' && byte <= b'z' {
                    byte - 0x20
                } else {
                    byte
                }
            }
            Self::SevenBitAscii => byte & 0x7f,
            Self::Raw8Bit => byte,
        }
    }

    /// Transform a byte travelling from the Altair to the host terminal. Both
    /// historical terminal modes strip bit 7, but do not rewrite printable
    /// output case: uppercase normalization represents the ASR-33 keyboard on
    /// input, not a general-purpose mutation of guest output.
    pub const fn tx_transform(self, byte: u8) -> u8 {
        match self {
            Self::Asr33Uppercase | Self::SevenBitAscii => byte & 0x7f,
            Self::Raw8Bit => byte,
        }
    }
}

impl Default for ExternalSerialCharacterMode {
    fn default() -> Self {
        Self::Asr33Uppercase
    }
}

/// Host-side pacing for the raw TCP serial endpoint. This does not change the
/// emulated MITS interface hardware; it only limits how quickly bytes cross the
/// host transport boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalSerialSpeed {
    Instant,
    Baud110,
    Baud300,
    Baud1200,
    Baud2400,
    Baud9600,
}

impl ExternalSerialSpeed {
    pub const ALL: [Self; 6] = [
        Self::Instant,
        Self::Baud110,
        Self::Baud300,
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud9600,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Instant => "Instant",
            Self::Baud110 => "110 baud",
            Self::Baud300 => "300 baud",
            Self::Baud1200 => "1200 baud",
            Self::Baud2400 => "2400 baud",
            Self::Baud9600 => "9600 baud",
        }
    }

    pub const fn baud(self) -> Option<u32> {
        match self {
            Self::Instant => None,
            Self::Baud110 => Some(110),
            Self::Baud300 => Some(300),
            Self::Baud1200 => Some(1_200),
            Self::Baud2400 => Some(2_400),
            Self::Baud9600 => Some(9_600),
        }
    }

    /// Conventional asynchronous pacing. Ten bit-times per character is a
    /// practical host-transport model (start + character/parity + stop). The
    /// internal ASR-33 keeps its separate authentic 11-unit 110-baud timing.
    pub fn char_time(self) -> Duration {
        match self.baud() {
            Some(baud) => Duration::from_secs_f64(10.0 / baud as f64),
            None => Duration::ZERO,
        }
    }
}

impl Default for ExternalSerialSpeed {
    fn default() -> Self {
        Self::Baud9600
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TcpListenScope {
    Loopback,
    AllInterfaces,
}

impl TcpListenScope {
    pub const ALL: [Self; 2] = [Self::Loopback, Self::AllInterfaces];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Loopback => "This computer only (127.0.0.1)",
            Self::AllInterfaces => "LAN / all interfaces (0.0.0.0)",
        }
    }

    pub const fn bind_ipv4(self) -> Ipv4Addr {
        match self {
            Self::Loopback => Ipv4Addr::LOCALHOST,
            Self::AllInterfaces => Ipv4Addr::UNSPECIFIED,
        }
    }
}

impl Default for TcpListenScope {
    fn default() -> Self {
        Self::Loopback
    }
}

/// Configuration for the host-side raw TCP endpoint. The endpoint is disabled
/// by default so RusTair never opens a listening socket unless the user asks it
/// to. Multiple network clients are also opt-in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalSerialConfig {
    pub enabled: bool,
    pub listen_scope: TcpListenScope,
    pub tcp_port: u16,
    pub speed: ExternalSerialSpeed,
    pub character_mode: ExternalSerialCharacterMode,
    pub duplex: TerminalDuplex,
    pub allow_multiple_clients: bool,
}

impl Default for ExternalSerialConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_scope: TcpListenScope::Loopback,
            tcp_port: 8800,
            speed: ExternalSerialSpeed::Baud9600,
            character_mode: ExternalSerialCharacterMode::Asr33Uppercase,
            duplex: TerminalDuplex::FullDuplexRemoteEcho,
            allow_multiple_clients: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_tcp_is_safe_and_asr33_friendly_by_default() {
        let config = ExternalSerialConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.listen_scope, TcpListenScope::Loopback);
        assert_eq!(config.tcp_port, 8800);
        assert_eq!(
            config.character_mode,
            ExternalSerialCharacterMode::Asr33Uppercase
        );
        assert_eq!(config.duplex, TerminalDuplex::FullDuplexRemoteEcho);
        assert!(!config.allow_multiple_clients);
    }

    #[test]
    fn asr33_mode_strips_bit_seven_and_uppercases_host_input() {
        assert_eq!(
            ExternalSerialCharacterMode::Asr33Uppercase.rx_transform(b'y'),
            b'Y'
        );
        assert_eq!(
            ExternalSerialCharacterMode::Asr33Uppercase.rx_transform(0xe5),
            b'E'
        );
        assert_eq!(
            ExternalSerialCharacterMode::Asr33Uppercase.tx_transform(0xc5),
            b'E'
        );
        assert_eq!(
            ExternalSerialCharacterMode::Asr33Uppercase.tx_transform(b'y'),
            b'y'
        );
    }

    #[test]
    fn seven_bit_mode_preserves_case_and_raw_mode_preserves_all_bits() {
        assert_eq!(
            ExternalSerialCharacterMode::SevenBitAscii.rx_transform(b'y'),
            b'y'
        );
        assert_eq!(
            ExternalSerialCharacterMode::SevenBitAscii.tx_transform(0xc5),
            b'E'
        );
        assert_eq!(
            ExternalSerialCharacterMode::Raw8Bit.rx_transform(0xe5),
            0xe5
        );
        assert_eq!(
            ExternalSerialCharacterMode::Raw8Bit.tx_transform(0xe5),
            0xe5
        );
    }

    #[test]
    fn external_serial_pacing_uses_ten_bit_frames() {
        assert_eq!(
            ExternalSerialSpeed::Baud300.char_time(),
            Duration::from_secs_f64(10.0 / 300.0)
        );
    }
}
