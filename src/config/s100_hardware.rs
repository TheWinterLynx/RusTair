use super::machine::{CpuBoard, RamBoardProfile, RamSize, SerialBoard};
use super::sio::SioHardwareConfig;
use super::two_sio::{TwoSioInterruptWiring, TwoSioStraps};
use crate::s100_chassis::{AltairChassisModel, S100ChassisConfig, S100ChassisConfigError};
use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig, S100RamConfigError};

pub const MAX_S100_SLOTS: usize = 18;

/// Physical card families that can be selected in the chassis editor.
///
/// Aggregate capacities such as "8K RAM" intentionally do not appear here:
/// those are machine totals assembled from one or more real cards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100InstalledCardKind {
    Mits8080Cpu,
    Mits1KStatic88Mcs,
    Mits4KDynamic88_4Mcd,
    Mits4KSynchronous88S4K,
    Mits4KStatic88_4Mcs,
    Mits16KStatic88_16Mcs,
    Mits16KDynamic88_16Mcd,
    Mits88Sio,
    Mits88TwoSio,
    /// Non-historical RAM used only for explicit compatibility assemblies and
    /// migration of old aggregate RusTair configurations.
    FastRamCompatibility,
}

impl S100InstalledCardKind {
    pub const ALL: [Self; 10] = [
        Self::Mits8080Cpu,
        Self::Mits1KStatic88Mcs,
        Self::Mits4KDynamic88_4Mcd,
        Self::Mits4KSynchronous88S4K,
        Self::Mits4KStatic88_4Mcs,
        Self::Mits16KStatic88_16Mcs,
        Self::Mits16KDynamic88_16Mcd,
        Self::Mits88Sio,
        Self::Mits88TwoSio,
        Self::FastRamCompatibility,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mits8080Cpu => "MITS 8080 CPU Board",
            Self::Mits1KStatic88Mcs => "MITS 88-MCS / 88-1MCS 1K Static RAM",
            Self::Mits4KDynamic88_4Mcd => "MITS 88-4MCD 4K Dynamic RAM",
            Self::Mits4KSynchronous88S4K => "MITS 88-S4K Synchronous 4K RAM",
            Self::Mits4KStatic88_4Mcs => "MITS 88-4MCS 4K Static RAM",
            Self::Mits16KStatic88_16Mcs => "MITS 88-16MCS 16K Static RAM",
            Self::Mits16KDynamic88_16Mcd => "MITS 88-16MCD 16K Dynamic RAM",
            Self::Mits88Sio => "MITS 88-SIO",
            Self::Mits88TwoSio => "MITS 88-2SIO",
            Self::FastRamCompatibility => "Fast RAM compatibility (non-historical)",
        }
    }

    pub const fn historical(self) -> bool {
        !matches!(self, Self::FastRamCompatibility)
    }

    pub const fn ram_model(self) -> Option<S100RamBoardModel> {
        Some(match self {
            Self::Mits1KStatic88Mcs => S100RamBoardModel::Mits1KStatic88Mcs,
            Self::Mits4KDynamic88_4Mcd => S100RamBoardModel::Mits4KDynamic88_4Mcd,
            Self::Mits4KSynchronous88S4K => S100RamBoardModel::Mits4KSynchronous88S4K,
            Self::Mits4KStatic88_4Mcs => S100RamBoardModel::Mits4KStatic88_4Mcs,
            Self::Mits16KStatic88_16Mcs => S100RamBoardModel::Mits16KStatic88_16Mcs,
            Self::Mits16KDynamic88_16Mcd => S100RamBoardModel::Mits16KDynamic88_16Mcd,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FastRamCompatibilityConfig {
    pub base_address: u16,
    pub populated_bytes: usize,
    /// Retained only so an old global 1K-board timing profile can migrate
    /// without silently becoming zero-wait memory when it cannot be expressed
    /// as a realistic set of physical boards.
    pub read_wait_states: u8,
}

impl FastRamCompatibilityConfig {
    pub const fn no_wait(base_address: u16, populated_bytes: usize) -> Self {
        Self { base_address, populated_bytes, read_wait_states: 0 }
    }

    pub fn validate(self) -> Result<Self, S100HardwareConfigError> {
        if self.populated_bytes == 0
            || u32::from(self.base_address) + self.populated_bytes as u32 > 0x1_0000
        {
            return Err(S100HardwareConfigError::InvalidCompatibilityRamWindow {
                base_address: self.base_address,
                populated_bytes: self.populated_bytes,
            });
        }
        Ok(self)
    }
}

/// Persistable physical configuration of one fitted S-100 connector.
///
/// Serial-card strap state belongs to the card instance, not to the machine as a
/// global singleton. This permits independently strapped 88-SIO/88-2SIO boards
/// to coexist without adding card-family branches to the electrical backplane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100InstalledCardConfig {
    Mits8080Cpu,
    Ram(S100RamCardConfig),
    Mits88Sio(SioHardwareConfig),
    Mits88TwoSio {
        straps: TwoSioStraps,
        interrupt_wiring: TwoSioInterruptWiring,
    },
    FastRamCompatibility(FastRamCompatibilityConfig),
}

impl S100InstalledCardConfig {
    pub const fn kind(self) -> S100InstalledCardKind {
        match self {
            Self::Mits8080Cpu => S100InstalledCardKind::Mits8080Cpu,
            Self::Ram(config) => match config.model {
                S100RamBoardModel::Mits1KStatic88Mcs => S100InstalledCardKind::Mits1KStatic88Mcs,
                S100RamBoardModel::Mits4KDynamic88_4Mcd => S100InstalledCardKind::Mits4KDynamic88_4Mcd,
                S100RamBoardModel::Mits4KSynchronous88S4K => S100InstalledCardKind::Mits4KSynchronous88S4K,
                S100RamBoardModel::Mits4KStatic88_4Mcs => S100InstalledCardKind::Mits4KStatic88_4Mcs,
                S100RamBoardModel::Mits16KStatic88_16Mcs => S100InstalledCardKind::Mits16KStatic88_16Mcs,
                S100RamBoardModel::Mits16KDynamic88_16Mcd => S100InstalledCardKind::Mits16KDynamic88_16Mcd,
            },
            Self::Mits88Sio(_) => S100InstalledCardKind::Mits88Sio,
            Self::Mits88TwoSio { .. } => S100InstalledCardKind::Mits88TwoSio,
            Self::FastRamCompatibility(_) => S100InstalledCardKind::FastRamCompatibility,
        }
    }

    pub fn default_for_kind(kind: S100InstalledCardKind) -> Self {
        match kind {
            S100InstalledCardKind::Mits8080Cpu => Self::Mits8080Cpu,
            S100InstalledCardKind::Mits88Sio => Self::Mits88Sio(SioHardwareConfig::default()),
            S100InstalledCardKind::Mits88TwoSio => Self::Mits88TwoSio {
                straps: TwoSioStraps::default(),
                interrupt_wiring: TwoSioInterruptWiring::default(),
            },
            S100InstalledCardKind::FastRamCompatibility => {
                Self::FastRamCompatibility(FastRamCompatibilityConfig::no_wait(0, 8 * 1024))
            }
            ram_kind => {
                let model = ram_kind.ram_model().expect("RAM kind");
                Self::Ram(S100RamCardConfig::fully_populated(model, 0))
            }
        }
    }

    pub fn validate(self) -> Result<Self, S100HardwareConfigError> {
        match self {
            Self::Ram(config) => {
                config
                    .validate()
                    .map_err(S100HardwareConfigError::InvalidRamCard)?;
            }
            Self::FastRamCompatibility(config) => {
                config.validate()?;
            }
            _ => {}
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S100HardwareConfig {
    pub chassis: S100ChassisConfig,
    /// Fixed-size persisted storage; only indices below `fitted_connectors` are
    /// electrically present. The fixed 18-entry form keeps MachineConfig Copy
    /// and avoids allocating just to inspect/persist the hardware setup.
    pub slots: [Option<S100InstalledCardConfig>; MAX_S100_SLOTS],
}

impl S100HardwareConfig {
    pub fn empty(chassis: S100ChassisConfig) -> Result<Self, S100HardwareConfigError> {
        let chassis = chassis
            .validate()
            .map_err(S100HardwareConfigError::InvalidChassis)?;
        Ok(Self { chassis, slots: [None; MAX_S100_SLOTS] })
    }

    pub const fn fitted_connectors(self) -> usize {
        self.chassis.fitted_connectors
    }

    pub fn slot(self, number: usize) -> Option<S100InstalledCardConfig> {
        if number == 0 || number > self.chassis.fitted_connectors {
            return None;
        }
        self.slots[number - 1]
    }

    pub fn set_slot(
        &mut self,
        number: usize,
        card: Option<S100InstalledCardConfig>,
    ) -> Result<(), S100HardwareConfigError> {
        if number == 0 || number > self.chassis.fitted_connectors {
            return Err(S100HardwareConfigError::InvalidSlot {
                slot: number,
                fitted_connectors: self.chassis.fitted_connectors,
            });
        }
        if let Some(card) = card {
            card.validate()?;
        }
        self.slots[number - 1] = card;
        Ok(())
    }

    pub fn set_chassis(&mut self, chassis: S100ChassisConfig) -> Result<(), S100HardwareConfigError> {
        let chassis = chassis
            .validate()
            .map_err(S100HardwareConfigError::InvalidChassis)?;
        for index in chassis.fitted_connectors..MAX_S100_SLOTS {
            if self.slots[index].is_some() {
                return Err(S100HardwareConfigError::CardOutsideFittedConnectors {
                    slot: index + 1,
                    fitted_connectors: chassis.fitted_connectors,
                });
            }
        }
        self.chassis = chassis;
        Ok(())
    }

    pub fn installed_cards(self) -> impl Iterator<Item = (usize, S100InstalledCardConfig)> {
        let count = self.chassis.fitted_connectors;
        self.slots
            .into_iter()
            .take(count)
            .enumerate()
            .filter_map(|(index, card)| card.map(|card| (index + 1, card)))
    }

    pub fn cpu_slots(self) -> impl Iterator<Item = usize> {
        self.installed_cards().filter_map(|(slot, card)| {
            matches!(card, S100InstalledCardConfig::Mits8080Cpu).then_some(slot)
        })
    }

    /// The one CPU board physically installed in the fitted S-100 connectors.
    /// A temporarily invalid POWER-OFF edit may return `None`; validated runtime
    /// configurations contain exactly one CPU card.
    pub fn active_cpu_board_slot(self) -> Option<(usize, CpuBoard)> {
        let mut boards = self.installed_cards().filter_map(|(slot, card)| match card {
            S100InstalledCardConfig::Mits8080Cpu => Some((slot, CpuBoard::Mits8080)),
            _ => None,
        });
        let board = boards.next()?;
        boards.next().is_none().then_some(board)
    }

    pub fn active_cpu_board(self) -> Option<CpuBoard> {
        self.active_cpu_board_slot().map(|(_, board)| board)
    }

    pub fn installed_ram_bytes(self) -> usize {
        self.installed_cards()
            .map(|(_, card)| match card {
                S100InstalledCardConfig::Ram(config) => config.populated_bytes,
                S100InstalledCardConfig::FastRamCompatibility(config) => config.populated_bytes,
                _ => 0,
            })
            .sum()
    }

    /// Validate the persisted electrical assembly. RAM address overlap is not an
    /// error here: mis-strapped real cards are representable and the electrical
    /// backplane must expose their DI contention at runtime.
    pub fn validate(self) -> Result<Self, S100HardwareConfigError> {
        self.chassis
            .validate()
            .map_err(S100HardwareConfigError::InvalidChassis)?;
        for index in self.chassis.fitted_connectors..MAX_S100_SLOTS {
            if self.slots[index].is_some() {
                return Err(S100HardwareConfigError::CardOutsideFittedConnectors {
                    slot: index + 1,
                    fitted_connectors: self.chassis.fitted_connectors,
                });
            }
        }
        for (_, card) in self.installed_cards() {
            card.validate()?;
        }
        let cpu_count = self.cpu_slots().count();
        if cpu_count != 1 {
            return Err(S100HardwareConfigError::UnsupportedCpuCardCount(cpu_count));
        }
        Ok(self)
    }

    /// Migration-only conversion from pre-v5 aggregate settings. It deliberately
    /// creates a compatibility assembly instead of inventing historical boards:
    /// an old `8K + Mits1K timing` value never described which eight boards or
    /// slots physically existed.
    pub(crate) fn from_legacy_globals(
        ram_size: RamSize,
        ram_profile: RamBoardProfile,
        serial_board: SerialBoard,
        sio_hardware: SioHardwareConfig,
        two_sio_straps: TwoSioStraps,
        two_sio_interrupt_wiring: TwoSioInterruptWiring,
    ) -> Self {
        let chassis = S100ChassisConfig::original_8800(1);
        let mut slots = [None; MAX_S100_SLOTS];
        slots[0] = Some(S100InstalledCardConfig::Mits8080Cpu);
        slots[1] = Some(S100InstalledCardConfig::FastRamCompatibility(
            FastRamCompatibilityConfig {
                base_address: 0,
                populated_bytes: ram_size.bytes(),
                read_wait_states: ram_profile.read_wait_states(),
            },
        ));
        slots[2] = Some(match serial_board {
            SerialBoard::Sio88 => S100InstalledCardConfig::Mits88Sio(sio_hardware),
            SerialBoard::TwoSio88 => S100InstalledCardConfig::Mits88TwoSio {
                straps: two_sio_straps,
                interrupt_wiring: two_sio_interrupt_wiring,
            },
        });
        Self { chassis, slots }
    }

    pub fn historical_8800b_18_slot_starter() -> Self {
        let mut config = Self::empty(S100ChassisConfig::altair_8800b(18)).expect("valid 8800b");
        config.slots[0] = Some(S100InstalledCardConfig::Mits8080Cpu);
        config.slots[1] = Some(S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits16KStatic88_16Mcs,
            0x0000,
        )));
        config.slots[2] = Some(S100InstalledCardConfig::Mits88TwoSio {
            straps: TwoSioStraps::default(),
            interrupt_wiring: TwoSioInterruptWiring::default(),
        });
        config
    }
}

impl Default for S100HardwareConfig {
    fn default() -> Self {
        Self::from_legacy_globals(
            RamSize::default(),
            RamBoardProfile::default(),
            SerialBoard::default(),
            SioHardwareConfig::default(),
            TwoSioStraps::default(),
            TwoSioInterruptWiring::default(),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100HardwareConfigError {
    InvalidChassis(S100ChassisConfigError),
    InvalidSlot { slot: usize, fitted_connectors: usize },
    CardOutsideFittedConnectors { slot: usize, fitted_connectors: usize },
    InvalidRamCard(S100RamConfigError),
    InvalidCompatibilityRamWindow { base_address: u16, populated_bytes: usize },
    UnsupportedCpuCardCount(usize),
}

/// UI-friendly connector populations documented by the chassis model.
pub const fn fitted_connector_choices(model: AltairChassisModel) -> &'static [usize] {
    match model {
        AltairChassisModel::Altair8800 => &[4, 8, 12, 16],
        AltairChassisModel::Altair8800A => &[18],
        AltairChassisModel::Altair8800B => &[6, 12, 18],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_capacity_is_not_a_historical_card_kind() {
        assert!(!S100InstalledCardKind::ALL
            .iter()
            .any(|kind| kind.label() == "8K RAM"));
        assert_eq!(
            S100InstalledCardKind::Mits16KStatic88_16Mcs.ram_model(),
            Some(S100RamBoardModel::Mits16KStatic88_16Mcs)
        );
    }

    #[test]
    fn installed_card_validation_preserves_the_outer_card_variant() {
        let ram = S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits4KStatic88_4Mcs,
            0x1000,
        ));
        assert_eq!(ram.validate().unwrap(), ram);

        let compatibility = S100InstalledCardConfig::FastRamCompatibility(
            FastRamCompatibilityConfig::no_wait(0x2000, 0x1000),
        );
        assert_eq!(compatibility.validate().unwrap(), compatibility);
    }

    #[test]
    fn default_inventory_preserves_old_machine_semantics_without_claiming_history() {
        let config = S100HardwareConfig::default();
        assert_eq!(config.chassis, S100ChassisConfig::original_8800(1));
        assert_eq!(config.cpu_slots().collect::<Vec<_>>(), vec![1]);
        assert_eq!(config.installed_ram_bytes(), RamSize::K8.bytes());
        assert!(matches!(
            config.slot(2),
            Some(S100InstalledCardConfig::FastRamCompatibility(_))
        ));
        assert!(matches!(config.slot(3), Some(S100InstalledCardConfig::Mits88Sio(_))));
        config.validate().unwrap();
    }

    #[test]
    fn active_cpu_board_identity_and_slot_come_only_from_installed_s100_card() {
        let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
        hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::FastRamCompatibility(
                    FastRamCompatibilityConfig::no_wait(0, 0x1000),
                )),
            )
            .unwrap();
        hardware.set_slot(5, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
        let hardware = hardware.validate().unwrap();
        assert_eq!(hardware.active_cpu_board_slot(), Some((5, CpuBoard::Mits8080)));
        assert_eq!(hardware.active_cpu_board(), Some(CpuBoard::Mits8080));
    }

    #[test]
    fn each_serial_board_owns_its_own_physical_straps() {
        let mut config = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(18)).unwrap();
        config.set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
        config.set_slot(2, Some(S100InstalledCardConfig::Mits88Sio(SioHardwareConfig::default()))).unwrap();
        config.set_slot(3, Some(S100InstalledCardConfig::Mits88Sio(SioHardwareConfig::default()))).unwrap();
        config.set_slot(4, Some(S100InstalledCardConfig::Mits88TwoSio {
            straps: TwoSioStraps::default(),
            interrupt_wiring: TwoSioInterruptWiring::default(),
        })).unwrap();
        config.validate().unwrap();
        assert_eq!(config.installed_cards().count(), 4);
    }

    #[test]
    fn shrinking_chassis_never_silently_drops_cards() {
        let mut config = S100HardwareConfig::historical_8800b_18_slot_starter();
        config.set_slot(18, Some(S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(
            S100RamBoardModel::Mits1KStatic88Mcs,
            0xfc00,
        )))).unwrap();
        assert!(matches!(
            config.set_chassis(S100ChassisConfig::altair_8800b(12)),
            Err(S100HardwareConfigError::CardOutsideFittedConnectors { slot: 18, .. })
        ));
        assert!(config.slot(18).is_some());
    }

    #[test]
    fn overlapping_ram_is_preserved_for_electrical_contention_instead_of_rejected() {
        let mut config = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(18)).unwrap();
        config.set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
        let ram = S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0x0000);
        config.set_slot(2, Some(S100InstalledCardConfig::Ram(ram))).unwrap();
        config.set_slot(3, Some(S100InstalledCardConfig::Ram(ram))).unwrap();
        config.validate().unwrap();
    }

    #[test]
    fn connector_choices_follow_the_selected_chassis() {
        assert_eq!(fitted_connector_choices(AltairChassisModel::Altair8800), &[4, 8, 12, 16]);
        assert_eq!(fitted_connector_choices(AltairChassisModel::Altair8800A), &[18]);
        assert_eq!(fitted_connector_choices(AltairChassisModel::Altair8800B), &[6, 12, 18]);
    }
}
