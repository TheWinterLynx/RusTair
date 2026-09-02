//! Historical S-100 RAM card instances.
//!
//! Machine capacity is not a card type. A real Altair may contain several RAM
//! cards of different models, populations and address straps. This module keeps
//! those identities explicit so the chassis can eventually persist an actual
//! slot inventory instead of one aggregate `RamSize` plus a global timing mode.

use crate::s100::{
    S100Card, S100CardClass, S100CardContact, S100CardDescriptor, S100ContactRole,
    S100Signal, MITS_1K_STATIC_RAM,
};
use crate::s100_backplane::{S100BusSample, S100CardDrive, S100ElectricalCard};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100RamBoardModel {
    /// Original 8800 static board. The basic kit shipped with one 256-byte bank
    /// populated; pairs of Intel 8101s expand it in 256-byte steps to 1 KiB.
    Mits1KStatic88Mcs,
    /// MITS 88-4MCD, the early 4 KiB dynamic board.
    Mits4KDynamic88_4Mcd,
    /// MITS 88-S4K synchronous 4 KiB dynamic board.
    Mits4KSynchronous88S4K,
    /// MITS 88-4MCS 4 KiB static board using 2102A-4 RAMs.
    Mits4KStatic88_4Mcs,
    /// MITS 88-16MCS 16 KiB static board.
    Mits16KStatic88_16Mcs,
    /// MITS 88-16MCD 16 KiB dynamic board.
    Mits16KDynamic88_16Mcd,
}

impl S100RamBoardModel {
    pub const ALL: [Self; 6] = [
        Self::Mits1KStatic88Mcs,
        Self::Mits4KDynamic88_4Mcd,
        Self::Mits4KSynchronous88S4K,
        Self::Mits4KStatic88_4Mcs,
        Self::Mits16KStatic88_16Mcs,
        Self::Mits16KDynamic88_16Mcd,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mits1KStatic88Mcs => "MITS 88-MCS / 88-1MCS 1K Static RAM",
            Self::Mits4KDynamic88_4Mcd => "MITS 88-4MCD 4K Dynamic RAM",
            Self::Mits4KSynchronous88S4K => "MITS 88-S4K Synchronous 4K RAM",
            Self::Mits4KStatic88_4Mcs => "MITS 88-4MCS 4K Static RAM",
            Self::Mits16KStatic88_16Mcs => "MITS 88-16MCS 16K Static RAM",
            Self::Mits16KDynamic88_16Mcd => "MITS 88-16MCD 16K Dynamic RAM",
        }
    }

    pub const fn capacity_bytes(self) -> usize {
        match self {
            Self::Mits1KStatic88Mcs => 1024,
            Self::Mits4KDynamic88_4Mcd
            | Self::Mits4KSynchronous88S4K
            | Self::Mits4KStatic88_4Mcs => 4 * 1024,
            Self::Mits16KStatic88_16Mcs | Self::Mits16KDynamic88_16Mcd => 16 * 1024,
        }
    }

    /// Hardware address-selection quantum. This is deliberately independent of
    /// how many RAM chips happen to be populated on a board.
    pub const fn address_granularity(self) -> usize {
        match self {
            Self::Mits1KStatic88Mcs => 1024,
            Self::Mits4KDynamic88_4Mcd
            | Self::Mits4KSynchronous88S4K
            | Self::Mits4KStatic88_4Mcs => 4 * 1024,
            Self::Mits16KStatic88_16Mcs | Self::Mits16KDynamic88_16Mcd => 16 * 1024,
        }
    }

    pub const fn valid_population(self, bytes: usize) -> bool {
        match self {
            Self::Mits1KStatic88Mcs => {
                bytes >= 256 && bytes <= 1024 && bytes % 256 == 0
            }
            _ => bytes == self.capacity_bytes(),
        }
    }

    pub const fn timing_model(self) -> S100RamTimingModel {
        match self {
            // Original 8101 board explicitly slows every read by two 8080 waits.
            Self::Mits1KStatic88Mcs => S100RamTimingModel::FixedReadWaits(2),
            // 88-4MCD refreshes every 32 clock periods. A CPU access colliding
            // with refresh may receive one or two waits; that collision engine
            // is kept explicit instead of silently pretending this is static RAM.
            Self::Mits4KDynamic88_4Mcd => S100RamTimingModel::RefreshCollision {
                interval_clocks: 32,
                min_waits: 1,
                max_waits: 2,
            },
            // These documented boards normally run the 2 MHz Altair without
            // processor wait states.
            Self::Mits4KSynchronous88S4K
            | Self::Mits4KStatic88_4Mcs
            | Self::Mits16KStatic88_16Mcs
            | Self::Mits16KDynamic88_16Mcd => S100RamTimingModel::NoWait,
        }
    }

    pub const fn refresh_model(self) -> S100RamRefreshModel {
        match self {
            Self::Mits1KStatic88Mcs
            | Self::Mits4KStatic88_4Mcs
            | Self::Mits16KStatic88_16Mcs => S100RamRefreshModel::None,
            Self::Mits4KDynamic88_4Mcd => S100RamRefreshModel::CpuClockEvery32,
            Self::Mits4KSynchronous88S4K => S100RamRefreshModel::CpuSynchronous,
            Self::Mits16KDynamic88_16Mcd => S100RamRefreshModel::OnBoardCrystal,
        }
    }

    /// The first electrical migration implements exact fixed/no-wait behavior.
    /// Dynamic refresh failure/collision details remain visible as unfinished
    /// hardware rather than being hidden behind a generic no-wait profile.
    pub const fn timing_fully_implemented(self) -> bool {
        !matches!(
            self,
            Self::Mits4KDynamic88_4Mcd | Self::Mits4KSynchronous88S4K
        )
    }

    pub const fn supports_front_panel_protect(self) -> bool {
        matches!(
            self,
            Self::Mits1KStatic88Mcs
                | Self::Mits4KDynamic88_4Mcd
                | Self::Mits4KStatic88_4Mcs
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100RamTimingModel {
    NoWait,
    FixedReadWaits(u8),
    RefreshCollision {
        interval_clocks: u16,
        min_waits: u8,
        max_waits: u8,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100RamRefreshModel {
    None,
    CpuClockEvery32,
    CpuSynchronous,
    OnBoardCrystal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S100RamCardConfig {
    pub model: S100RamBoardModel,
    pub base_address: u16,
    /// Physically installed bytes on this board. Only the original 1K board is
    /// currently documented as a normal partially-populated configuration.
    pub populated_bytes: usize,
}

impl S100RamCardConfig {
    pub const fn fully_populated(model: S100RamBoardModel, base_address: u16) -> Self {
        Self {
            model,
            base_address,
            populated_bytes: model.capacity_bytes(),
        }
    }

    pub const fn with_population(
        model: S100RamBoardModel,
        base_address: u16,
        populated_bytes: usize,
    ) -> Self {
        Self { model, base_address, populated_bytes }
    }

    pub fn validate(self) -> Result<Self, S100RamConfigError> {
        let quantum = self.model.address_granularity();
        if self.base_address as usize % quantum != 0 {
            return Err(S100RamConfigError::MisalignedBase {
                base_address: self.base_address,
                required_granularity: quantum,
            });
        }
        let end = u32::from(self.base_address) + self.model.capacity_bytes() as u32;
        if end > 0x1_0000 {
            return Err(S100RamConfigError::AddressWindowExceeds64K);
        }
        if !self.model.valid_population(self.populated_bytes) {
            return Err(S100RamConfigError::InvalidPopulation {
                model: self.model,
                populated_bytes: self.populated_bytes,
            });
        }
        Ok(self)
    }

    pub const fn address_window_contains(self, address: u16) -> bool {
        let offset = address.wrapping_sub(self.base_address) as usize;
        address >= self.base_address && offset < self.model.capacity_bytes()
    }

    pub const fn populated_address_contains(self, address: u16) -> bool {
        let offset = address.wrapping_sub(self.base_address) as usize;
        address >= self.base_address && offset < self.populated_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100RamConfigError {
    MisalignedBase { base_address: u16, required_granularity: usize },
    AddressWindowExceeds64K,
    InvalidPopulation { model: S100RamBoardModel, populated_bytes: usize },
}

pub struct S100RamCard {
    config: S100RamCardConfig,
    bytes: Vec<u8>,
    protected: bool,
    selected_offset: Option<usize>,
    memory_read: bool,
    wait_clocks_remaining: u8,
    previous_sync: bool,
    previous_clock: bool,
}

impl S100RamCard {
    pub fn new(config: S100RamCardConfig) -> Result<Self, S100RamConfigError> {
        let config = config.validate()?;
        Ok(Self {
            bytes: vec![0; config.populated_bytes],
            config,
            protected: false,
            selected_offset: None,
            memory_read: false,
            wait_clocks_remaining: 0,
            previous_sync: false,
            previous_clock: false,
        })
    }

    pub fn config(&self) -> S100RamCardConfig { self.config }
    pub fn model(&self) -> S100RamBoardModel { self.config.model }
    pub fn is_protected(&self) -> bool { self.protected }

    pub fn set_protected(&mut self, protected: bool) -> bool {
        if !self.model().supports_front_panel_protect() { return false; }
        self.protected = protected;
        true
    }

    pub fn read_byte(&self, address: u16) -> Option<u8> {
        if !self.config.populated_address_contains(address) { return None; }
        Some(self.bytes[address.wrapping_sub(self.config.base_address) as usize])
    }

    pub fn write_byte(&mut self, address: u16, value: u8) -> bool {
        if self.protected || !self.config.populated_address_contains(address) { return false; }
        let offset = address.wrapping_sub(self.config.base_address) as usize;
        self.bytes[offset] = value;
        true
    }

    fn fixed_read_waits(&self) -> u8 {
        match self.model().timing_model() {
            S100RamTimingModel::FixedReadWaits(waits) => waits,
            _ => 0,
        }
    }
}

impl S100Card for S100RamCard {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        match self.model() {
            S100RamBoardModel::Mits1KStatic88Mcs => &MITS_1K_STATIC_RAM,
            S100RamBoardModel::Mits4KDynamic88_4Mcd => &MITS_88_4MCD,
            S100RamBoardModel::Mits4KSynchronous88S4K => &MITS_88_S4K,
            S100RamBoardModel::Mits4KStatic88_4Mcs => &MITS_88_4MCS,
            S100RamBoardModel::Mits16KStatic88_16Mcs => &MITS_88_16MCS,
            S100RamBoardModel::Mits16KDynamic88_16Mcd => &MITS_88_16MCD,
        }
    }
}

impl S100ElectricalCard for S100RamCard {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        let sync = sample.signal_level(S100Signal::Sync) == Some(true);
        let clock = sample.signal_level(S100Signal::Clock) == Some(true);
        let sync_rising = sync && !self.previous_sync;
        let clock_rising = clock && !self.previous_clock;

        self.selected_offset = sample.address().and_then(|address| {
            self.config
                .populated_address_contains(address)
                .then_some(address.wrapping_sub(self.config.base_address) as usize)
        });
        self.memory_read = self.selected_offset.is_some()
            && sample.signal_level(S100Signal::MemoryRead) == Some(true);

        if let (Some(address), Some(value)) = (sample.address(), sample.data_out()) {
            if sample.signal_level(S100Signal::MemoryWrite) == Some(true) {
                self.write_byte(address, value);
            }
        }

        let fixed_waits = self.fixed_read_waits();
        if !self.memory_read || fixed_waits == 0 {
            self.wait_clocks_remaining = 0;
        } else if sync_rising {
            self.wait_clocks_remaining = fixed_waits;
        } else if clock_rising && self.wait_clocks_remaining != 0 {
            self.wait_clocks_remaining -= 1;
        }

        self.previous_sync = sync;
        self.previous_clock = clock;
    }

    fn drive_s100(&self) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        if self.memory_read {
            if let Some(offset) = self.selected_offset {
                drive.drive_data_in(self.bytes[offset]);
            }
        }
        if self.wait_clocks_remaining != 0 {
            drive.pull_low(S100Signal::Ready, true);
        }
        drive
    }
}

const PWR: S100CardContact = S100CardContact::new(S100Signal::Plus8V, S100ContactRole::Power);
const P16: S100CardContact = S100CardContact::new(S100Signal::Plus16V, S100ContactRole::Power);
const M16: S100CardContact = S100CardContact::new(S100Signal::Minus16V, S100ContactRole::Power);
const GND: S100CardContact = S100CardContact::new(S100Signal::Ground, S100ContactRole::Power);

macro_rules! memory_contacts {
    ($($extra:expr),* $(,)?) => {
        &[
            PWR, P16, M16, GND,
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
            $($extra),*
        ]
    };
}

const MCD_CONTACTS: &[S100CardContact] = memory_contacts!(
    S100CardContact::new(S100Signal::Sync, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Clock, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Ready, S100ContactRole::OpenCollectorOutput),
    S100CardContact::new(S100Signal::Protect, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Unprotect, S100ContactRole::Input),
    S100CardContact::new(S100Signal::ProtectStatus, S100ContactRole::TriStateOutput),
);
const S4K_CONTACTS: &[S100CardContact] = memory_contacts!(
    S100CardContact::new(S100Signal::Sync, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Clock, S100ContactRole::Input),
);
const MCS4_CONTACTS: &[S100CardContact] = memory_contacts!(
    S100CardContact::new(S100Signal::Protect, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Unprotect, S100ContactRole::Input),
    S100CardContact::new(S100Signal::ProtectStatus, S100ContactRole::TriStateOutput),
);
const MCS16_CONTACTS: &[S100CardContact] = memory_contacts!();
const MCD16_CONTACTS: &[S100CardContact] = memory_contacts!(
    S100CardContact::new(S100Signal::Clock, S100ContactRole::Input),
);

pub static MITS_88_4MCD: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-4mcd",
    label: "MITS 88-4MCD 4K Dynamic RAM",
    class: S100CardClass::Memory,
    historical: true,
    contacts: MCD_CONTACTS,
};

pub static MITS_88_S4K: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-s4k",
    label: "MITS 88-S4K Synchronous 4K RAM",
    class: S100CardClass::Memory,
    historical: true,
    contacts: S4K_CONTACTS,
};

pub static MITS_88_4MCS: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-4mcs",
    label: "MITS 88-4MCS 4K Static RAM",
    class: S100CardClass::Memory,
    historical: true,
    contacts: MCS4_CONTACTS,
};

pub static MITS_88_16MCS: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-16mcs",
    label: "MITS 88-16MCS 16K Static RAM",
    class: S100CardClass::Memory,
    historical: true,
    contacts: MCS16_CONTACTS,
};

pub static MITS_88_16MCD: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-16mcd",
    label: "MITS 88-16MCD 16K Dynamic RAM",
    class: S100CardClass::Memory,
    historical: true,
    contacts: MCD16_CONTACTS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100_backplane::S100Backplane;

    fn memory_read_master(address: u16, sync: bool, clock: bool) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.drive_address(address);
        drive.drive_signal(S100Signal::MemoryRead, true);
        drive.drive_signal(S100Signal::MemoryWrite, false);
        drive.drive_signal(S100Signal::Sync, sync);
        drive.drive_signal(S100Signal::Clock, clock);
        drive
    }

    #[test]
    fn historical_catalog_is_not_collapsed_to_one_kilobyte_cards() {
        let capacities: Vec<usize> = S100RamBoardModel::ALL
            .into_iter()
            .map(S100RamBoardModel::capacity_bytes)
            .collect();
        assert!(capacities.contains(&1024));
        assert!(capacities.contains(&(4 * 1024)));
        assert!(capacities.contains(&(16 * 1024)));
        assert!(S100RamBoardModel::ALL.into_iter().all(|model| {
            S100RamCard::new(S100RamCardConfig::fully_populated(model, 0)).is_ok()
        }));
    }

    #[test]
    fn original_one_k_board_accepts_historical_256_byte_population_steps() {
        for bytes in [256, 512, 768, 1024] {
            assert!(S100RamCardConfig::with_population(
                S100RamBoardModel::Mits1KStatic88Mcs,
                0x0400,
                bytes,
            )
            .validate()
            .is_ok());
        }
        assert!(matches!(
            S100RamCardConfig::with_population(
                S100RamBoardModel::Mits1KStatic88Mcs,
                0,
                128,
            )
            .validate(),
            Err(S100RamConfigError::InvalidPopulation { .. })
        ));
    }

    #[test]
    fn address_straps_use_each_historical_boards_decode_quantum() {
        assert!(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits1KStatic88Mcs,
            0x0400,
        )
        .validate()
        .is_ok());
        assert!(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits4KStatic88_4Mcs,
            0x1000,
        )
        .validate()
        .is_ok());
        assert!(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits16KStatic88_16Mcs,
            0x4000,
        )
        .validate()
        .is_ok());
        assert!(matches!(
            S100RamCardConfig::fully_populated(
                S100RamBoardModel::Mits16KStatic88_16Mcs,
                0x1000,
            )
            .validate(),
            Err(S100RamConfigError::MisalignedBase { .. })
        ));
    }

    #[test]
    fn selected_ram_card_drives_di_through_the_same_backplane_resolver() {
        let mut ram = S100RamCard::new(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits4KStatic88_4Mcs,
            0x1000,
        ))
        .unwrap();
        assert!(ram.write_byte(0x1234, 0x5a));

        let backplane = S100Backplane::new(0);
        let master = memory_read_master(0x1234, false, false);
        let observed = backplane.resolve_drive_sets(&[master.clone()]);
        ram.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[master, ram.drive_s100()]);
        assert_eq!(resolved.data_in(), Some(0x5a));
    }

    #[test]
    fn unselected_ram_card_releases_di() {
        let mut ram = S100RamCard::new(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits4KStatic88_4Mcs,
            0x1000,
        ))
        .unwrap();
        let backplane = S100Backplane::new(0);
        let master = memory_read_master(0x3000, false, false);
        let observed = backplane.resolve_drive_sets(&[master.clone()]);
        ram.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[master, ram.drive_s100()]);
        assert_eq!(resolved.data_in(), None);
    }

    #[test]
    fn overlapping_ram_straps_surface_real_di_contention() {
        let config = S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits4KStatic88_4Mcs,
            0x0000,
        );
        let mut a = S100RamCard::new(config).unwrap();
        let mut b = S100RamCard::new(config).unwrap();
        a.write_byte(0x0010, 0x00);
        b.write_byte(0x0010, 0xff);

        let backplane = S100Backplane::new(0);
        let master = memory_read_master(0x0010, false, false);
        let observed = backplane.resolve_drive_sets(&[master.clone()]);
        a.observe_s100(&observed);
        b.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[
            master,
            a.drive_s100(),
            b.drive_s100(),
        ]);
        assert!((0..8).all(|bit| resolved.signal_is_contended(S100Signal::DataIn(bit))));
    }

    #[test]
    fn one_k_static_board_pulls_prdy_for_its_documented_two_read_waits() {
        let mut ram = S100RamCard::new(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits1KStatic88Mcs,
            0,
        ))
        .unwrap();
        let backplane = S100Backplane::new(0);

        let sync = memory_read_master(0x0010, true, false);
        let observed = backplane.resolve_drive_sets(&[sync.clone()]);
        ram.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[sync, ram.drive_s100()]);
        assert_eq!(resolved.signal_level(S100Signal::Ready), Some(false));

        for expected_low in [true, false] {
            let low_clock = memory_read_master(0x0010, false, false);
            let observed = backplane.resolve_drive_sets(&[low_clock]);
            ram.observe_s100(&observed);
            let high_clock = memory_read_master(0x0010, false, true);
            let observed = backplane.resolve_drive_sets(&[high_clock.clone()]);
            ram.observe_s100(&observed);
            let resolved = backplane.resolve_drive_sets(&[high_clock, ram.drive_s100()]);
            assert_eq!(
                resolved.signal_level(S100Signal::Ready),
                Some(!expected_low),
            );
        }
    }

    #[test]
    fn dynamic_refresh_behavior_is_explicit_not_silently_flattened_to_fast_ram() {
        assert_eq!(
            S100RamBoardModel::Mits4KDynamic88_4Mcd.timing_model(),
            S100RamTimingModel::RefreshCollision {
                interval_clocks: 32,
                min_waits: 1,
                max_waits: 2,
            }
        );
        assert!(!S100RamBoardModel::Mits4KDynamic88_4Mcd.timing_fully_implemented());
        assert_eq!(
            S100RamBoardModel::Mits16KDynamic88_16Mcd.refresh_model(),
            S100RamRefreshModel::OnBoardCrystal,
        );
    }
}
