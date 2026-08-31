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
}

impl Default for SioDataBits {
    fn default() -> Self { Self::Eight }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioParity { None, Even, Odd }
impl Default for SioParity { fn default() -> Self { Self::None } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioStopBits { One, Two }
impl SioStopBits {
    pub const fn bits(self) -> u8 { match self { Self::One => 1, Self::Two => 2 } }
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
/// MITS documents a selectable range through 25,000 baud and drives the
/// COM2502 with a 16x clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SioBaudRate(u32);

impl SioBaudRate {
    pub const MAX: u32 = 25_000;
    pub const fn try_new(baud: u32) -> Option<Self> {
        if baud <= Self::MAX { Some(Self(baud)) } else { None }
    }
    pub const fn baud(self) -> u32 { self.0 }
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
    fn default_tty_configuration_is_110_baud_8n2() {
        let c = SioHardwareConfig::default();
        assert_eq!(c.interface, SioInterface::TtyC);
        assert_eq!(c.baud.baud(), 110);
        assert_eq!(c.format.frame_bits(), 11);
    }
}
