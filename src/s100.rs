//! Common physical contract for cards plugged into RusTair's Altair S-100 bus.
//!
//! This module deliberately models the machine as a chassis/backplane plus
//! plug-in cards.  It is not a generic peripheral API: signal names and pin
//! numbers are the original MITS 8800 system-bus contacts documented in the
//! 1975 Theory of Operation (drawing 880-110 / BUS DEFINITION).

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum S100Signal {
    Plus8V,
    Plus16V,
    Minus16V,
    Ground,
    ExternalReady,
    VectorInterrupt(u8),
    StatusDisable,
    CommandControlDisable,
    Unprotect,
    SingleStep,
    AddressDisable,
    DataOutDisable,
    Phi2,
    Phi1,
    HoldAcknowledge,
    Wait,
    InterruptEnable,
    Address(u8),
    DataOut(u8),
    M1,
    Out,
    Inp,
    MemoryRead,
    HaltAcknowledge,
    Clock,
    SenseSwitchDisable,
    ExternalClear,
    MemoryWrite,
    ProtectStatus,
    Protect,
    Run,
    Ready,
    InterruptRequest,
    Hold,
    Reset,
    Sync,
    Write,
    DataBusIn,
    DataIn(u8),
    InterruptAcknowledge,
    WriteStatus,
    Stack,
    PowerOnClear,
}

impl S100Signal {
    /// Physical edge-connector contact in the original Altair 8800 bus.
    ///
    /// Returns `None` only for an invalid indexed signal (for example A16 or
    /// VI8).  The intentionally unused/undefined contacts are not represented
    /// by `S100Signal` at all.
    pub const fn pin(self) -> Option<u8> {
        match self {
            Self::Plus8V => Some(1),
            Self::Plus16V => Some(2),
            Self::ExternalReady => Some(3),
            Self::VectorInterrupt(bit) => match bit {
                0 => Some(4),
                1 => Some(5),
                2 => Some(6),
                3 => Some(7),
                4 => Some(8),
                5 => Some(9),
                6 => Some(10),
                7 => Some(11),
                _ => None,
            },
            Self::StatusDisable => Some(18),
            Self::CommandControlDisable => Some(19),
            Self::Unprotect => Some(20),
            Self::SingleStep => Some(21),
            Self::AddressDisable => Some(22),
            Self::DataOutDisable => Some(23),
            Self::Phi2 => Some(24),
            Self::Phi1 => Some(25),
            Self::HoldAcknowledge => Some(26),
            Self::Wait => Some(27),
            Self::InterruptEnable => Some(28),
            Self::Address(bit) => match bit {
                0 => Some(79),
                1 => Some(80),
                2 => Some(81),
                3 => Some(31),
                4 => Some(30),
                5 => Some(29),
                6 => Some(82),
                7 => Some(83),
                8 => Some(84),
                9 => Some(34),
                10 => Some(37),
                11 => Some(87),
                12 => Some(33),
                13 => Some(85),
                14 => Some(86),
                15 => Some(32),
                _ => None,
            },
            Self::DataOut(bit) => match bit {
                0 => Some(36),
                1 => Some(35),
                2 => Some(88),
                3 => Some(89),
                4 => Some(38),
                5 => Some(39),
                6 => Some(40),
                7 => Some(90),
                _ => None,
            },
            Self::M1 => Some(44),
            Self::Out => Some(45),
            Self::Inp => Some(46),
            Self::MemoryRead => Some(47),
            Self::HaltAcknowledge => Some(48),
            Self::Clock => Some(49),
            Self::Ground => Some(50),
            // The second +8 V and GND contacts are electrically common with
            // pins 1 and 50.  `pin()` returns the canonical first contact; card
            // descriptors may explicitly include the duplicate supply contact
            // when that distinction matters mechanically.
            Self::Minus16V => Some(52),
            Self::SenseSwitchDisable => Some(53),
            Self::ExternalClear => Some(54),
            Self::MemoryWrite => Some(68),
            Self::ProtectStatus => Some(69),
            Self::Protect => Some(70),
            Self::Run => Some(71),
            Self::Ready => Some(72),
            Self::InterruptRequest => Some(73),
            Self::Hold => Some(74),
            Self::Reset => Some(75),
            Self::Sync => Some(76),
            Self::Write => Some(77),
            Self::DataBusIn => Some(78),
            Self::DataIn(bit) => match bit {
                0 => Some(95),
                1 => Some(94),
                2 => Some(41),
                3 => Some(42),
                4 => Some(91),
                5 => Some(92),
                6 => Some(93),
                7 => Some(43),
                _ => None,
            },
            Self::InterruptAcknowledge => Some(96),
            Self::WriteStatus => Some(97),
            Self::Stack => Some(98),
            Self::PowerOnClear => Some(99),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100ContactRole {
    /// Card only observes this backplane net.
    Input,
    /// Ordinary TTL output from the card.
    Output,
    /// Card output is enabled only when selected; otherwise high impedance.
    TriStateOutput,
    /// Card can assert the shared line and otherwise releases it.
    OpenCollectorOutput,
    /// Unregulated supply or ground contact.
    Power,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S100CardContact {
    pub signal: S100Signal,
    pub role: S100ContactRole,
}

impl S100CardContact {
    pub const fn new(signal: S100Signal, role: S100ContactRole) -> Self {
        Self { signal, role }
    }

    pub const fn pin(self) -> Option<u8> { self.signal.pin() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100CardClass {
    Cpu,
    Memory,
    Serial,
    InterruptController,
    FrontPanel,
    Compatibility,
}

#[derive(Clone, Copy, Debug)]
pub struct S100CardDescriptor {
    pub key: &'static str,
    pub label: &'static str,
    pub class: S100CardClass,
    /// True only when this descriptor names a documented historical board,
    /// rather than a RusTair compatibility profile.
    pub historical: bool,
    /// Electrically modeled S-100 contacts.  This is the code-level equivalent
    /// of tracing the fingers used by the card on its edge connector.
    pub contacts: &'static [S100CardContact],
}

/// Every physical card plugged into the emulated chassis must present this same
/// connector-level identity.  Stateful cards will progressively implement this
/// trait directly as the old aggregate Memory/IoDevices plumbing is removed.
pub trait S100Card {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100CardKind {
    Mits8080Cpu,
    Mits1KStaticRam,
    Mits88Sio,
    Mits88TwoSio,
    /// Existing non-historical zero-wait RAM profile retained while memory is
    /// migrated to explicit historical card instances.
    FastRamCompatibility,
}

impl S100Card for S100CardKind {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        match self {
            Self::Mits8080Cpu => &MITS_8080_CPU,
            Self::Mits1KStaticRam => &MITS_1K_STATIC_RAM,
            Self::Mits88Sio => &MITS_88_SIO,
            Self::Mits88TwoSio => &MITS_88_2SIO,
            Self::FastRamCompatibility => &FAST_RAM_COMPATIBILITY,
        }
    }
}

const PWR: S100CardContact = S100CardContact::new(S100Signal::Plus8V, S100ContactRole::Power);
const GND: S100CardContact = S100CardContact::new(S100Signal::Ground, S100ContactRole::Power);
const A0: S100CardContact = S100CardContact::new(S100Signal::Address(0), S100ContactRole::Input);
const A1: S100CardContact = S100CardContact::new(S100Signal::Address(1), S100ContactRole::Input);
const A2: S100CardContact = S100CardContact::new(S100Signal::Address(2), S100ContactRole::Input);
const A3: S100CardContact = S100CardContact::new(S100Signal::Address(3), S100ContactRole::Input);
const A4: S100CardContact = S100CardContact::new(S100Signal::Address(4), S100ContactRole::Input);
const A5: S100CardContact = S100CardContact::new(S100Signal::Address(5), S100ContactRole::Input);
const A6: S100CardContact = S100CardContact::new(S100Signal::Address(6), S100ContactRole::Input);
const A7: S100CardContact = S100CardContact::new(S100Signal::Address(7), S100ContactRole::Input);

const SERIAL_COMMON: &[S100CardContact] = &[
    PWR,
    GND,
    A0, A1, A2, A3, A4, A5, A6, A7,
    S100CardContact::new(S100Signal::DataOut(0), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(1), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(2), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(3), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(4), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(5), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(6), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(7), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(1), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(2), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(3), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(4), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(5), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(6), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(7), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Inp, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Out, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Write, S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataBusIn, S100ContactRole::Input),
    S100CardContact::new(S100Signal::ExternalClear, S100ContactRole::Input),
    S100CardContact::new(S100Signal::PowerOnClear, S100ContactRole::Input),
    S100CardContact::new(S100Signal::InterruptRequest, S100ContactRole::OpenCollectorOutput),
];

// 88-2SIO has the same CPU/data decode boundary plus its documented one-TW
// PRDY generator.  Keep a separate descriptor even where most contacts match:
// it is a different physical board, not a mode of the 88-SIO.
const TWO_SIO_CONTACTS: &[S100CardContact] = &[
    PWR,
    GND,
    A0, A1, A2, A3, A4, A5, A6, A7,
    S100CardContact::new(S100Signal::DataOut(0), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(1), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(2), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(3), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(4), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(5), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(6), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(7), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(1), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(2), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(3), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(4), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(5), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(6), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(7), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Inp, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Out, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Write, S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataBusIn, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Wait, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Ready, S100ContactRole::OpenCollectorOutput),
    S100CardContact::new(S100Signal::PowerOnClear, S100ContactRole::Input),
    S100CardContact::new(S100Signal::InterruptRequest, S100ContactRole::OpenCollectorOutput),
];

const RAM_CONTACTS: &[S100CardContact] = &[
    PWR,
    GND,
    S100CardContact::new(S100Signal::Address(0), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(1), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(2), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(3), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(4), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(5), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(6), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(7), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(8), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(9), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(10), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(11), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(12), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(13), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(14), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(15), S100ContactRole::Input),
    S100CardContact::new(S100Signal::MemoryRead, S100ContactRole::Input),
    S100CardContact::new(S100Signal::MemoryWrite, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Sync, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Clock, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Protect, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Unprotect, S100ContactRole::Input),
    S100CardContact::new(S100Signal::ProtectStatus, S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Ready, S100ContactRole::OpenCollectorOutput),
    S100CardContact::new(S100Signal::DataOut(0), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(1), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(2), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(3), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(4), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(5), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(6), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(7), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(1), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(2), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(3), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(4), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(5), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(6), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(7), S100ContactRole::TriStateOutput),
];

const CPU_CONTACTS: &[S100CardContact] = &[
    PWR,
    GND,
    S100CardContact::new(S100Signal::Plus16V, S100ContactRole::Power),
    S100CardContact::new(S100Signal::Minus16V, S100ContactRole::Power),
    S100CardContact::new(S100Signal::Ready, S100ContactRole::Input),
    S100CardContact::new(S100Signal::ExternalReady, S100ContactRole::Input),
    S100CardContact::new(S100Signal::InterruptRequest, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Hold, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Reset, S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(1), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(2), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(3), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(4), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(5), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(6), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(7), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Phi2, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Phi1, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Clock, S100ContactRole::Output),
    S100CardContact::new(S100Signal::HoldAcknowledge, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Wait, S100ContactRole::Output),
    S100CardContact::new(S100Signal::InterruptEnable, S100ContactRole::Output),
    S100CardContact::new(S100Signal::M1, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Out, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Inp, S100ContactRole::Output),
    S100CardContact::new(S100Signal::MemoryRead, S100ContactRole::Output),
    S100CardContact::new(S100Signal::HaltAcknowledge, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Sync, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Write, S100ContactRole::Output),
    S100CardContact::new(S100Signal::DataBusIn, S100ContactRole::Output),
    S100CardContact::new(S100Signal::InterruptAcknowledge, S100ContactRole::Output),
    S100CardContact::new(S100Signal::WriteStatus, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Stack, S100ContactRole::Output),
];

pub static MITS_8080_CPU: S100CardDescriptor = S100CardDescriptor {
    key: "mits-8080-cpu",
    label: "MITS 8080 CPU Board",
    class: S100CardClass::Cpu,
    historical: true,
    contacts: CPU_CONTACTS,
};

pub static MITS_1K_STATIC_RAM: S100CardDescriptor = S100CardDescriptor {
    key: "mits-1k-static-ram-1975",
    label: "MITS 1K Static Memory Board (8101)",
    class: S100CardClass::Memory,
    historical: true,
    contacts: RAM_CONTACTS,
};

pub static MITS_88_SIO: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-sio",
    label: "MITS 88-SIO",
    class: S100CardClass::Serial,
    historical: true,
    contacts: SERIAL_COMMON,
};

pub static MITS_88_2SIO: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-2sio",
    label: "MITS 88-2SIO",
    class: S100CardClass::Serial,
    historical: true,
    contacts: TWO_SIO_CONTACTS,
};

pub static FAST_RAM_COMPATIBILITY: S100CardDescriptor = S100CardDescriptor {
    key: "rustair-fast-ram-compat",
    label: "RusTair fast RAM compatibility profile",
    class: S100CardClass::Compatibility,
    historical: false,
    contacts: RAM_CONTACTS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn original_altair_bus_pin_mapping_matches_key_880_110_contacts() {
        assert_eq!(S100Signal::Phi2.pin(), Some(24));
        assert_eq!(S100Signal::Phi1.pin(), Some(25));
        assert_eq!(S100Signal::Clock.pin(), Some(49));
        assert_eq!(S100Signal::ExternalClear.pin(), Some(54));
        assert_eq!(S100Signal::Ready.pin(), Some(72));
        assert_eq!(S100Signal::InterruptRequest.pin(), Some(73));
        assert_eq!(S100Signal::Hold.pin(), Some(74));
        assert_eq!(S100Signal::Reset.pin(), Some(75));
        assert_eq!(S100Signal::Sync.pin(), Some(76));
        assert_eq!(S100Signal::Write.pin(), Some(77));
        assert_eq!(S100Signal::DataBusIn.pin(), Some(78));
        assert_eq!(S100Signal::InterruptAcknowledge.pin(), Some(96));
        assert_eq!(S100Signal::WriteStatus.pin(), Some(97));
        assert_eq!(S100Signal::Stack.pin(), Some(98));
    }

    #[test]
    fn original_altair_address_and_data_contacts_are_not_linearized() {
        assert_eq!(S100Signal::Address(0).pin(), Some(79));
        assert_eq!(S100Signal::Address(3).pin(), Some(31));
        assert_eq!(S100Signal::Address(15).pin(), Some(32));
        assert_eq!(S100Signal::DataOut(0).pin(), Some(36));
        assert_eq!(S100Signal::DataOut(7).pin(), Some(90));
        assert_eq!(S100Signal::DataIn(0).pin(), Some(95));
        assert_eq!(S100Signal::DataIn(7).pin(), Some(43));
    }

    #[test]
    fn every_declared_card_contact_resolves_to_a_real_bus_pin() {
        for card in [
            S100CardKind::Mits8080Cpu,
            S100CardKind::Mits1KStaticRam,
            S100CardKind::Mits88Sio,
            S100CardKind::Mits88TwoSio,
        ] {
            let descriptor = card.s100_descriptor();
            for contact in descriptor.contacts {
                assert!(contact.pin().is_some(), "{} has an invalid contact: {:?}", descriptor.label, contact.signal);
            }
        }
    }

    #[test]
    fn card_descriptors_do_not_claim_the_same_contact_twice() {
        for card in [
            S100CardKind::Mits8080Cpu,
            S100CardKind::Mits1KStaticRam,
            S100CardKind::Mits88Sio,
            S100CardKind::Mits88TwoSio,
        ] {
            let descriptor = card.s100_descriptor();
            let mut pins = HashSet::new();
            for contact in descriptor.contacts {
                let pin = contact.pin().unwrap();
                assert!(pins.insert(pin), "{} duplicates S-100 pin {}", descriptor.label, pin);
            }
        }
    }

    #[test]
    fn two_sio_explicitly_owns_prdy_but_original_sio_does_not() {
        let sio = S100CardKind::Mits88Sio.s100_descriptor();
        let two = S100CardKind::Mits88TwoSio.s100_descriptor();
        assert!(!sio.contacts.iter().any(|c| c.signal == S100Signal::Ready));
        assert!(two.contacts.iter().any(|c| {
            c.signal == S100Signal::Ready && c.role == S100ContactRole::OpenCollectorOutput
        }));
    }

    #[test]
    fn one_common_trait_identifies_every_current_physical_board_family() {
        let cards: [&dyn S100Card; 4] = [
            &S100CardKind::Mits8080Cpu,
            &S100CardKind::Mits1KStaticRam,
            &S100CardKind::Mits88Sio,
            &S100CardKind::Mits88TwoSio,
        ];
        assert!(cards.iter().all(|card| card.s100_descriptor().historical));
    }
}
