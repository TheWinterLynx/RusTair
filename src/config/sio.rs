/// Logic revision of the original MITS 88-SIO.
///
/// Rev 0 exposes the COM2502 buffer-ready flags directly (active HIGH on
/// status bits D5/D1). The later Rev 1/status-word modification moves those
/// indications to D0/D7 and inverts them (active LOW), which is the form used
/// by most later MITS software.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioRevision {
    Rev0,
    Rev1,
}

impl SioRevision {
    pub const ALL: [Self; 2] = [Self::Rev0, Self::Rev1];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rev0 => "Rev 0 — original status word",
            Self::Rev1 => "Rev 1 — modified status word",
        }
    }

    const fn persistence_key(self) -> &'static str {
        match self { Self::Rev0 => "rev0", Self::Rev1 => "rev1" }
    }

    fn from_persistence_key(value: &str) -> Option<Self> {
        Some(match value { "rev0" => Self::Rev0, "rev1" => Self::Rev1, _ => return None })
    }
}

impl Default for SioRevision {
    fn default() -> Self { Self::Rev1 }
}

/// External electrical interface fitted to an 88-SIO card.
///
/// The UART/CPU-visible logic is common; A/B/C select the line-interface
/// circuitry described by MITS.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioInterface {
    /// 88-SIO A: EIA/RS-232 level interface.
    Rs232A,
    /// 88-SIO B: TTL level interface.
    TtlB,
    /// 88-SIO C: TTY/current-loop oriented interface.
    TtyC,
}

impl SioInterface {
    pub const ALL: [Self; 3] = [Self::Rs232A, Self::TtlB, Self::TtyC];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rs232A => "88-SIO A — RS-232 levels",
            Self::TtlB => "88-SIO B — TTL levels",
            Self::TtyC => "88-SIO C — TTY/current-loop interface",
        }
    }

    pub const fn persistence_key(self) -> &'static str {
        match self {
            Self::Rs232A => "a-rs232",
            Self::TtlB => "b-ttl",
            Self::TtyC => "c-tty",
        }
    }

    pub fn from_persistence_key(value: &str) -> Option<Self> {
        Some(match value {
            "a-rs232" => Self::Rs232A,
            "b-ttl" => Self::TtlB,
            "c-tty" => Self::TtyC,
            _ => return None,
        })
    }
}

impl Default for SioInterface {
    fn default() -> Self { Self::TtyC }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioDataBits { Five, Six, Seven, Eight }

impl SioDataBits {
    pub const ALL: [Self; 4] = [Self::Five, Self::Six, Self::Seven, Self::Eight];
    pub const fn bits(self) -> u8 {
        match self { Self::Five => 5, Self::Six => 6, Self::Seven => 7, Self::Eight => 8 }
    }
    pub const fn label(self) -> &'static str {
        match self { Self::Five => "5 data bits", Self::Six => "6 data bits", Self::Seven => "7 data bits", Self::Eight => "8 data bits" }
    }
    fn from_bits(bits: u8) -> Option<Self> {
        Some(match bits { 5 => Self::Five, 6 => Self::Six, 7 => Self::Seven, 8 => Self::Eight, _ => return None })
    }
}

impl Default for SioDataBits {
    fn default() -> Self { Self::Eight }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioParity { None, Even, Odd }
impl SioParity {
    pub const ALL: [Self; 3] = [Self::None, Self::Even, Self::Odd];
    pub const fn label(self) -> &'static str {
        match self { Self::None => "No parity", Self::Even => "Even parity", Self::Odd => "Odd parity" }
    }
    const fn persistence_key(self) -> &'static str {
        match self { Self::None => "none", Self::Even => "even", Self::Odd => "odd" }
    }
    fn from_persistence_key(value: &str) -> Option<Self> {
        Some(match value { "none" => Self::None, "even" => Self::Even, "odd" => Self::Odd, _ => return None })
    }
}
impl Default for SioParity { fn default() -> Self { Self::None } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioStopBits { One, Two }
impl SioStopBits {
    pub const ALL: [Self; 2] = [Self::One, Self::Two];
    pub const fn bits(self) -> u8 { match self { Self::One => 1, Self::Two => 2 } }
    pub const fn label(self) -> &'static str { match self { Self::One => "1 stop bit", Self::Two => "2 stop bits" } }
    fn from_bits(bits: u8) -> Option<Self> { Some(match bits { 1 => Self::One, 2 => Self::Two, _ => return None }) }
}
impl Default for SioStopBits { fn default() -> Self { Self::Two } }

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct SioWordFormat {
    pub data_bits: SioDataBits,
    pub parity: SioParity,
    pub stop_bits: SioStopBits,
}

impl SioWordFormat {
    pub const fn frame_bits(self) -> u8 {
        1 + self.data_bits.bits() + match self.parity { SioParity::None => 0, _ => 1 } + self.stop_bits.bits()
    }

    pub fn label(self) -> String {
        let parity = match self.parity { SioParity::None => 'N', SioParity::Even => 'E', SioParity::Odd => 'O' };
        format!("{}{}{}", self.data_bits.bits(), parity, self.stop_bits.bits())
    }
}

/// Even control/status address selected by the seven 88-SIO address jumpers.
/// The data channel is always the following odd address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SioAddressPair { base: u8 }

impl SioAddressPair {
    pub const fn try_new(base: u8) -> Option<Self> {
        if base & 1 == 0 { Some(Self { base }) } else { None }
    }
    pub const fn base(self) -> u8 { self.base }
    pub const fn status(self) -> u8 { self.base }
    pub const fn data(self) -> u8 { self.base.wrapping_add(1) }
    pub const fn contains(self, port: u8) -> bool { port == self.status() || port == self.data() }
}

impl Default for SioAddressPair {
    fn default() -> Self { Self { base: 0x00 } }
}

/// Nominal serial bit rate produced by the 88-SIO baud generator.
///
/// The board uses a 12-bit preset counter and a 16x UART clock. MITS published
/// a standard wiring table for 110, 150, 300, 600, 1200, 2400, 4800, 9600 and
/// 19200 baud, while also documenting how a different preset count can produce
/// another rate up to 25,000 baud. `STANDARD` is therefore the authentic menu
/// of published presets, not the complete electrical state space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SioBaudRate(u32);

impl SioBaudRate {
    pub const MAX: u32 = 25_000;
    pub const STANDARD: [Self; 9] = [
        Self(110), Self(150), Self(300), Self(600), Self(1_200), Self(2_400),
        Self(4_800), Self(9_600), Self(19_200),
    ];

    pub const fn try_new(baud: u32) -> Option<Self> {
        if baud <= Self::MAX { Some(Self(baud)) } else { None }
    }
    pub const fn baud(self) -> u32 { self.0 }
    pub fn label(self) -> String { format!("{} baud", self.0) }
    pub fn is_standard(self) -> bool {
        matches!(self.0, 110 | 150 | 300 | 600 | 1_200 | 2_400 | 4_800 | 9_600 | 19_200)
    }
}

impl Default for SioBaudRate {
    fn default() -> Self { Self(110) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct SioHardwareConfig {
    pub revision: SioRevision,
    pub interface: SioInterface,
    pub address: SioAddressPair,
    pub baud: SioBaudRate,
    pub format: SioWordFormat,
}

impl SioHardwareConfig {
    /// Compact persistence form for one atomic physical card configuration.
    /// Invalid or partial text is rejected as a whole so configuration loading
    /// can retain a known-safe default rather than constructing a hybrid board.
    pub fn persistence_key(self) -> String {
        format!(
            "{},{},{:02X},{},{},{},{}",
            self.revision.persistence_key(),
            self.interface.persistence_key(),
            self.address.base(),
            self.baud.baud(),
            self.format.data_bits.bits(),
            self.format.parity.persistence_key(),
            self.format.stop_bits.bits(),
        )
    }

    pub fn from_persistence_key(value: &str) -> Option<Self> {
        let mut fields = value.split(',');
        let revision = SioRevision::from_persistence_key(fields.next()?)?;
        let interface = SioInterface::from_persistence_key(fields.next()?)?;
        let address = SioAddressPair::try_new(u8::from_str_radix(fields.next()?, 16).ok()?)?;
        let baud = SioBaudRate::try_new(fields.next()?.parse().ok()?)?;
        let data_bits = SioDataBits::from_bits(fields.next()?.parse().ok()?)?;
        let parity = SioParity::from_persistence_key(fields.next()?)?;
        let stop_bits = SioStopBits::from_bits(fields.next()?.parse().ok()?)?;
        if fields.next().is_some() { return None; }
        Some(Self { revision, interface, address, baud, format: SioWordFormat { data_bits, parity, stop_bits } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_default_is_the_common_modified_rev1_board() {
        assert_eq!(SioRevision::default(), SioRevision::Rev1);
    }

    #[test]
    fn address_jumpers_select_even_status_and_following_odd_data_port() {
        let p = SioAddressPair::try_new(0x06).unwrap();
        assert_eq!(p.status(), 0x06);
        assert_eq!(p.data(), 0x07);
        assert!(SioAddressPair::try_new(0x07).is_none());
        assert_eq!(SioAddressPair::try_new(0xfe).unwrap().data(), 0xff);
    }

    #[test]
    fn baud_generator_range_matches_mits_documented_ceiling() {
        assert_eq!(SioBaudRate::try_new(25_000).unwrap().baud(), 25_000);
        assert!(SioBaudRate::try_new(25_001).is_none());
    }

    #[test]
    fn published_mits_baud_table_is_exposed_without_forbidding_custom_presets() {
        assert_eq!(SioBaudRate::STANDARD.map(SioBaudRate::baud), [110, 150, 300, 600, 1_200, 2_400, 4_800, 9_600, 19_200]);
        assert!(SioBaudRate::try_new(4_800).unwrap().is_standard());
        assert!(!SioBaudRate::try_new(2_000).unwrap().is_standard());
    }

    #[test]
    fn default_tty_configuration_is_110_baud_8n2() {
        let c = SioHardwareConfig::default();
        assert_eq!(c.interface, SioInterface::TtyC);
        assert_eq!(c.baud.baud(), 110);
        assert_eq!(c.format.frame_bits(), 11);
        assert_eq!(c.format.label(), "8N2");
    }

    #[test]
    fn persistence_round_trip_is_atomic_and_rejects_partial_hardware() {
        let config = SioHardwareConfig {
            revision: SioRevision::Rev0,
            interface: SioInterface::Rs232A,
            address: SioAddressPair::try_new(0x06).unwrap(),
            baud: SioBaudRate::try_new(9_600).unwrap(),
            format: SioWordFormat { data_bits: SioDataBits::Seven, parity: SioParity::Even, stop_bits: SioStopBits::One },
        };
        assert_eq!(SioHardwareConfig::from_persistence_key(&config.persistence_key()), Some(config));
        assert!(SioHardwareConfig::from_persistence_key("rev0,a-rs232,07,9600,7,even,1").is_none());
        assert!(SioHardwareConfig::from_persistence_key("rev0,a-rs232,06,9600,7,even").is_none());
    }
}
