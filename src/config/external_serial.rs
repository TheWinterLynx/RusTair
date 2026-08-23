use std::net::Ipv4Addr;
use std::time::Duration;

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

    /// Conventional asynchronous 8N1 pacing: one start bit, eight data bits,
    /// one stop bit. The internal ASR-33 keeps its separate authentic 11-unit
    /// 110-baud timing model.
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
    pub allow_multiple_clients: bool,
}

impl Default for ExternalSerialConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_scope: TcpListenScope::Loopback,
            tcp_port: 8800,
            speed: ExternalSerialSpeed::Baud9600,
            allow_multiple_clients: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_tcp_is_safe_by_default() {
        let config = ExternalSerialConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.listen_scope, TcpListenScope::Loopback);
        assert_eq!(config.tcp_port, 8800);
        assert!(!config.allow_multiple_clients);
    }

    #[test]
    fn external_serial_pacing_uses_ten_bit_frames() {
        assert_eq!(
            ExternalSerialSpeed::Baud300.char_time(),
            Duration::from_secs_f64(10.0 / 300.0)
        );
    }
}
