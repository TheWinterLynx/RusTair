//! Physical configuration for the MITS 88-2SIO board.
//!
//! The March 1977 MITS manual states that A2-A7 select one aligned block of
//! four I/O addresses and A0/A1 select the ACIA register/port within that block.
//! Address FFh belongs to the Altair front-panel sense switches, so RusTair does
//! not offer the FCh-FFh block as an installable 88-2SIO configuration.
//!
//! The same assembly documentation also requires each serial port to be
//! hardwired independently for one of three electrical interconnects: RS-232,
//! TTL, or TTY 20 mA current loop. These are physical board/cable choices, not
//! endpoint software preferences and not hidden level converters.
//!
//! Interrupt wiring is deliberately represented separately from address/baud
//! straps. The assembly manual exposes DI (Port 0) and EI (Port 1) pads that may
//! be left disconnected, wired to the single PINT line, or wired to one of the
//! eight 88-Vector Interrupt inputs VI0..VI7.

/// One of the eight baud-generator taps silk-screened on the MITS 88-2SIO.
///
/// These are physical board straps. The MC6850 CR1:CR0 control bits still apply
/// /1, /16 or /64 to the selected clock; selecting `Baud110` therefore gives
/// 110 baud in the normal /16 mode and 27.5 baud in /64 mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TwoSioBaudTap {
    Baud110,
    Baud150,
    Baud300,
    Baud1200,
    Baud1800,
    Baud2400,
    Baud4800,
    Baud9600,
}

impl TwoSioBaudTap {
    pub const ALL: [Self; 8] = [
        Self::Baud110,
        Self::Baud150,
        Self::Baud300,
        Self::Baud1200,
        Self::Baud1800,
        Self::Baud2400,
        Self::Baud4800,
        Self::Baud9600,
    ];

    pub const fn baud(self) -> u32 {
        match self {
            Self::Baud110 => 110,
            Self::Baud150 => 150,
            Self::Baud300 => 300,
            Self::Baud1200 => 1_200,
            Self::Baud1800 => 1_800,
            Self::Baud2400 => 2_400,
            Self::Baud4800 => 4_800,
            Self::Baud9600 => 9_600,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Baud110 => "110",
            Self::Baud150 => "150",
            Self::Baud300 => "300",
            Self::Baud1200 => "1200",
            Self::Baud1800 => "1800",
            Self::Baud2400 => "2400",
            Self::Baud4800 => "4800",
            Self::Baud9600 => "9600",
        }
    }
}

impl Default for TwoSioBaudTap {
    fn default() -> Self { Self::Baud110 }
}

/// Electrical signal interconnect hardwired on one 88-2SIO port.
///
/// This is the digital family at the external connector boundary. Exact RS-232
/// voltages, TTL thresholds and current-loop current magnitude are analog
/// non-claims; the important fidelity invariant is that these three families
/// are not silently interchangeable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TwoSioSignalInterface {
    Rs232,
    Ttl,
    Tty20mA,
}

impl TwoSioSignalInterface {
    pub const ALL: [Self; 3] = [Self::Rs232, Self::Ttl, Self::Tty20mA];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Rs232 => "RS-232 voltage levels",
            Self::Ttl => "TTL voltage levels",
            Self::Tty20mA => "TTY 20 mA current loop",
        }
    }

    pub const fn persistence_key(self) -> &'static str {
        match self {
            Self::Rs232 => "rs232",
            Self::Ttl => "ttl",
            Self::Tty20mA => "tty20ma",
        }
    }

    pub fn from_persistence_key(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rs232" | "rs-232" => Some(Self::Rs232),
            "ttl" => Some(Self::Ttl),
            "tty20ma" | "tty" | "current_loop" | "current-loop" => Some(Self::Tty20mA),
            _ => None,
        }
    }
}

impl Default for TwoSioSignalInterface {
    fn default() -> Self { Self::Rs232 }
}

/// Where one MC6850 IRQ output is physically wired on the 88-2SIO PCB.
///
/// `Pint` means the corresponding DI/EI pad is hard-wired to the Altair's
/// single processor interrupt request line. `Vi0`..`Vi7` mean the pad is wired
/// to an 88-Vector Interrupt input; an installed 88-VI board is a separate
/// system component and is responsible for turning that level into the opcode
/// presented during interrupt acknowledge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TwoSioInterruptTarget {
    Disconnected,
    Pint,
    Vi0,
    Vi1,
    Vi2,
    Vi3,
    Vi4,
    Vi5,
    Vi6,
    Vi7,
}

impl TwoSioInterruptTarget {
    pub const ALL: [Self; 10] = [
        Self::Disconnected,
        Self::Pint,
        Self::Vi0,
        Self::Vi1,
        Self::Vi2,
        Self::Vi3,
        Self::Vi4,
        Self::Vi5,
        Self::Vi6,
        Self::Vi7,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "No interrupt connection",
            Self::Pint => "PINT — single-level processor interrupt",
            Self::Vi0 => "88-VI level 0 (VI0)",
            Self::Vi1 => "88-VI level 1 (VI1)",
            Self::Vi2 => "88-VI level 2 (VI2)",
            Self::Vi3 => "88-VI level 3 (VI3)",
            Self::Vi4 => "88-VI level 4 (VI4)",
            Self::Vi5 => "88-VI level 5 (VI5)",
            Self::Vi6 => "88-VI level 6 (VI6)",
            Self::Vi7 => "88-VI level 7 (VI7)",
        }
    }

    pub const fn persistence_key(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Pint => "pint",
            Self::Vi0 => "vi0",
            Self::Vi1 => "vi1",
            Self::Vi2 => "vi2",
            Self::Vi3 => "vi3",
            Self::Vi4 => "vi4",
            Self::Vi5 => "vi5",
            Self::Vi6 => "vi6",
            Self::Vi7 => "vi7",
        }
    }

    pub fn from_persistence_key(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "disconnected" | "none" => Some(Self::Disconnected),
            "pint" => Some(Self::Pint),
            "vi0" => Some(Self::Vi0),
            "vi1" => Some(Self::Vi1),
            "vi2" => Some(Self::Vi2),
            "vi3" => Some(Self::Vi3),
            "vi4" => Some(Self::Vi4),
            "vi5" => Some(Self::Vi5),
            "vi6" => Some(Self::Vi6),
            "vi7" => Some(Self::Vi7),
            _ => None,
        }
    }

    pub const fn drives_pint(self) -> bool { matches!(self, Self::Pint) }

    pub const fn vector_level(self) -> Option<u8> {
        match self {
            Self::Vi0 => Some(0),
            Self::Vi1 => Some(1),
            Self::Vi2 => Some(2),
            Self::Vi3 => Some(3),
            Self::Vi4 => Some(4),
            Self::Vi5 => Some(5),
            Self::Vi6 => Some(6),
            Self::Vi7 => Some(7),
            Self::Disconnected | Self::Pint => None,
        }
    }
}

impl Default for TwoSioInterruptTarget {
    fn default() -> Self {
        // Preserve RusTair's pre-routing behavior once the new wiring is applied:
        // both ACIAs historically projected to the shared PINT path in the model.
        Self::Pint
    }
}

/// Physical DI/EI jumper wiring for both ACIAs.
///
/// MITS names the Port 0 request pad DI and the Port 1 request pad EI. Each is
/// independently wireable, so one port may be disconnected while the other
/// uses PINT, or the two may feed different 88-VI levels.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct TwoSioInterruptWiring {
    pub port0: TwoSioInterruptTarget,
    pub port1: TwoSioInterruptTarget,
}

impl TwoSioInterruptWiring {
    pub const fn target(self, index: usize) -> Option<TwoSioInterruptTarget> {
        match index {
            0 => Some(self.port0),
            1 => Some(self.port1),
            _ => None,
        }
    }
}

/// A2-A7 address-select strap block on an 88-2SIO.
///
/// `base` is always aligned to four. Valid installable bases are 00h through
/// F8h. FCh is deliberately rejected because the decoded block would include
/// FFh, the Altair front-panel sense-switch port that MITS says should not be
/// used for the 88-2SIO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TwoSioAddressBlock {
    base: u8,
}

impl TwoSioAddressBlock {
    pub const DEFAULT: Self = Self { base: 0x10 };

    pub const fn try_new(base: u8) -> Option<Self> {
        if base & 0x03 != 0 || base > 0xf8 {
            None
        } else {
            Some(Self { base })
        }
    }

    pub const fn base(self) -> u8 { self.base }
    pub const fn port0_status(self) -> u8 { self.base }
    pub const fn port0_data(self) -> u8 { self.base + 1 }
    pub const fn port1_status(self) -> u8 { self.base + 2 }
    pub const fn port1_data(self) -> u8 { self.base + 3 }

    pub const fn contains(self, port: u8) -> bool {
        port >= self.base && port <= self.base + 3
    }

    /// A0/A1 decoded offset inside the four-address board block.
    pub const fn offset(self, port: u8) -> Option<u8> {
        if self.contains(port) { Some(port - self.base) } else { None }
    }
}

impl Default for TwoSioAddressBlock {
    fn default() -> Self { Self::DEFAULT }
}

/// Physical address, baud and line-interface straps/hardwiring for one MITS
/// 88-2SIO board.
///
/// Interrupt DI/EI wiring is intentionally a separate `TwoSioInterruptWiring`
/// value because the assembly manual treats it as signal interconnect wiring to
/// the chassis interrupt system rather than part of the port setup bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TwoSioStraps {
    pub address: TwoSioAddressBlock,
    pub port0_baud: TwoSioBaudTap,
    pub port1_baud: TwoSioBaudTap,
    pub port0_interface: TwoSioSignalInterface,
    pub port1_interface: TwoSioSignalInterface,
}

impl TwoSioStraps {
    pub const fn port_status(self, index: usize) -> Option<u8> {
        match index {
            0 => Some(self.address.port0_status()),
            1 => Some(self.address.port1_status()),
            _ => None,
        }
    }

    pub const fn port_data(self, index: usize) -> Option<u8> {
        match index {
            0 => Some(self.address.port0_data()),
            1 => Some(self.address.port1_data()),
            _ => None,
        }
    }

    pub const fn port_interface(self, index: usize) -> Option<TwoSioSignalInterface> {
        match index {
            0 => Some(self.port0_interface),
            1 => Some(self.port1_interface),
            _ => None,
        }
    }
}

impl Default for TwoSioStraps {
    fn default() -> Self {
        Self {
            address: TwoSioAddressBlock::DEFAULT,
            port0_baud: TwoSioBaudTap::Baud110,
            port1_baud: TwoSioBaudTap::Baud9600,
            // Preserve the intended default physical installation: the built-in
            // Model 33 on Port 0 uses its 20 mA loop, while Port 1 is a normal
            // RS-232 serial connector. Virtual endpoints may explicitly adapt.
            port0_interface: TwoSioSignalInterface::Tty20mA,
            port1_interface: TwoSioSignalInterface::Rs232,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_strap_is_one_aligned_four_port_block() {
        let block = TwoSioAddressBlock::try_new(0x44).expect("68 decimal / 44h is MITS manual example");
        assert_eq!(block.port0_status(), 0x44);
        assert_eq!(block.port0_data(), 0x45);
        assert_eq!(block.port1_status(), 0x46);
        assert_eq!(block.port1_data(), 0x47);
        assert_eq!(block.offset(0x44), Some(0));
        assert_eq!(block.offset(0x47), Some(3));
        assert_eq!(block.offset(0x48), None);
    }

    #[test]
    fn address_strap_rejects_unaligned_and_front_panel_conflicting_blocks() {
        assert!(TwoSioAddressBlock::try_new(0x10).is_some());
        assert!(TwoSioAddressBlock::try_new(0xf8).is_some());
        assert!(TwoSioAddressBlock::try_new(0x11).is_none());
        assert!(TwoSioAddressBlock::try_new(0xfc).is_none());
    }

    #[test]
    fn default_straps_preserve_existing_installation() {
        let straps = TwoSioStraps::default();
        assert_eq!(straps.address.base(), 0x10);
        assert_eq!(straps.port0_baud, TwoSioBaudTap::Baud110);
        assert_eq!(straps.port1_baud, TwoSioBaudTap::Baud9600);
        assert_eq!(straps.port0_interface, TwoSioSignalInterface::Tty20mA);
        assert_eq!(straps.port1_interface, TwoSioSignalInterface::Rs232);
    }

    #[test]
    fn physical_taps_are_the_eight_mits_silkscreen_rates() {
        let rates = TwoSioBaudTap::ALL.map(TwoSioBaudTap::baud);
        assert_eq!(rates, [110, 150, 300, 1_200, 1_800, 2_400, 4_800, 9_600]);
    }

    #[test]
    fn each_port_can_be_hardwired_for_any_documented_signal_family() {
        assert_eq!(TwoSioSignalInterface::ALL.len(), 3);
        let straps = TwoSioStraps {
            port0_interface: TwoSioSignalInterface::Ttl,
            port1_interface: TwoSioSignalInterface::Tty20mA,
            ..TwoSioStraps::default()
        };
        assert_eq!(straps.port_interface(0), Some(TwoSioSignalInterface::Ttl));
        assert_eq!(straps.port_interface(1), Some(TwoSioSignalInterface::Tty20mA));
        assert_eq!(straps.port_interface(2), None);
    }

    #[test]
    fn signal_interface_persistence_keys_round_trip_and_reject_unknown_values() {
        for interface in TwoSioSignalInterface::ALL {
            assert_eq!(
                TwoSioSignalInterface::from_persistence_key(interface.persistence_key()),
                Some(interface)
            );
        }
        assert_eq!(TwoSioSignalInterface::from_persistence_key("current-loop"), Some(TwoSioSignalInterface::Tty20mA));
        assert_eq!(TwoSioSignalInterface::from_persistence_key("usb"), None);
    }

    #[test]
    fn interrupt_wiring_models_disconnected_pint_and_all_eight_vi_levels() {
        assert_eq!(TwoSioInterruptTarget::ALL.len(), 10);
        assert!(!TwoSioInterruptTarget::Disconnected.drives_pint());
        assert!(TwoSioInterruptTarget::Pint.drives_pint());
        assert_eq!(TwoSioInterruptTarget::Disconnected.vector_level(), None);
        assert_eq!(TwoSioInterruptTarget::Pint.vector_level(), None);
        for (level, target) in [
            TwoSioInterruptTarget::Vi0,
            TwoSioInterruptTarget::Vi1,
            TwoSioInterruptTarget::Vi2,
            TwoSioInterruptTarget::Vi3,
            TwoSioInterruptTarget::Vi4,
            TwoSioInterruptTarget::Vi5,
            TwoSioInterruptTarget::Vi6,
            TwoSioInterruptTarget::Vi7,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(target.vector_level(), Some(level as u8));
            assert!(!target.drives_pint());
        }
    }

    #[test]
    fn interrupt_wiring_is_independent_for_di_and_ei() {
        let wiring = TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Disconnected,
            port1: TwoSioInterruptTarget::Vi3,
        };
        assert_eq!(wiring.target(0), Some(TwoSioInterruptTarget::Disconnected));
        assert_eq!(wiring.target(1), Some(TwoSioInterruptTarget::Vi3));
        assert_eq!(wiring.target(2), None);
    }

    #[test]
    fn interrupt_wiring_default_preserves_previous_pint_projection() {
        let wiring = TwoSioInterruptWiring::default();
        assert_eq!(wiring.port0, TwoSioInterruptTarget::Pint);
        assert_eq!(wiring.port1, TwoSioInterruptTarget::Pint);
    }

    #[test]
    fn interrupt_target_persistence_keys_round_trip_and_reject_unknown_values() {
        for target in TwoSioInterruptTarget::ALL {
            assert_eq!(
                TwoSioInterruptTarget::from_persistence_key(target.persistence_key()),
                Some(target)
            );
        }
        assert_eq!(TwoSioInterruptTarget::from_persistence_key("NONE"), Some(TwoSioInterruptTarget::Disconnected));
        assert_eq!(TwoSioInterruptTarget::from_persistence_key("irq7"), None);
    }
}
