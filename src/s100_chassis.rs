//! Historical Altair chassis and S-100 motherboard topology.
//!
//! Slot count is a property of the physical chassis/backplane assembly, not a
//! global emulator constant.  The original Altair 8800 used four-slot expander
//! motherboards and could fit up to four of them in the base chassis.  The
//! 8800a/8800b family replaced those sections with a single 18-position
//! motherboard.  MITS sold 8800b assembled systems with different numbers of
//! edge connectors populated on that same board.

use crate::s100_backplane::S100Backplane;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AltairChassisModel {
    /// Original 1975 Altair 8800 with one to four 4-slot expander motherboards.
    Altair8800,
    /// 8800a: transitional front-panel update with the later 18-slot motherboard.
    Altair8800A,
    /// Altair 8800b with the single-piece 18-position motherboard.
    Altair8800B,
}

impl AltairChassisModel {
    pub const ALL: [Self; 3] = [Self::Altair8800, Self::Altair8800A, Self::Altair8800B];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Altair8800 => "MITS Altair 8800",
            Self::Altair8800A => "MITS Altair 8800a",
            Self::Altair8800B => "MITS Altair 8800b",
        }
    }

    pub const fn motherboard(self) -> S100MotherboardKind {
        match self {
            Self::Altair8800 => S100MotherboardKind::FourSlotExpanderSections,
            Self::Altair8800A | Self::Altair8800B => S100MotherboardKind::Single18Slot,
        }
    }

    pub const fn physical_slot_positions(self) -> usize {
        match self {
            // Four 88-EC boards fit in the original base chassis, four slots each.
            Self::Altair8800 => 16,
            Self::Altair8800A | Self::Altair8800B => 18,
        }
    }
}

impl Default for AltairChassisModel {
    fn default() -> Self {
        Self::Altair8800
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100MotherboardKind {
    /// Original 88-EC arrangement: electrically common bus sections, four
    /// connector positions per motherboard.
    FourSlotExpanderSections,
    /// Later single-piece 18-position motherboard used by 8800a/8800b.
    Single18Slot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S100ChassisConfig {
    pub model: AltairChassisModel,
    /// Edge connectors actually fitted and therefore usable.  This is distinct
    /// from etched motherboard positions on 8800a/8800b systems.
    pub fitted_connectors: usize,
}

impl S100ChassisConfig {
    pub const fn original_8800(expander_sections: usize) -> Self {
        Self {
            model: AltairChassisModel::Altair8800,
            fitted_connectors: expander_sections * 4,
        }
    }

    pub const fn altair_8800a() -> Self {
        Self {
            model: AltairChassisModel::Altair8800A,
            fitted_connectors: 18,
        }
    }

    pub const fn altair_8800b(fitted_connectors: usize) -> Self {
        Self {
            model: AltairChassisModel::Altair8800B,
            fitted_connectors,
        }
    }

    pub fn validate(self) -> Result<Self, S100ChassisConfigError> {
        let valid = match self.model {
            AltairChassisModel::Altair8800 => {
                matches!(self.fitted_connectors, 4 | 8 | 12 | 16)
            }
            AltairChassisModel::Altair8800A => self.fitted_connectors == 18,
            // MITS documentation/brochures explicitly describe 6, 12 and 18
            // connector assembled 8800b configurations on the 18-position board.
            AltairChassisModel::Altair8800B => {
                matches!(self.fitted_connectors, 6 | 12 | 18)
            }
        };
        if !valid {
            return Err(S100ChassisConfigError::InvalidConnectorPopulation {
                model: self.model,
                fitted_connectors: self.fitted_connectors,
            });
        }
        Ok(self)
    }

    pub const fn physical_slot_positions(self) -> usize {
        self.model.physical_slot_positions()
    }

    pub const fn motherboard(self) -> S100MotherboardKind {
        self.model.motherboard()
    }

    /// Create the electrically usable backplane.  Card identity is deliberately
    /// absent here: the chassis only decides how many connector positions exist.
    pub fn empty_backplane(self) -> Result<S100Backplane, S100ChassisConfigError> {
        let config = self.validate()?;
        Ok(S100Backplane::new(config.fitted_connectors))
    }
}

impl Default for S100ChassisConfig {
    fn default() -> Self {
        // The original Altair shipped with one four-slot expander motherboard;
        // additional 88-EC boards were options.  Four slots are enough for the
        // CPU plus a small memory/I/O starter system and avoid pretending that
        // unused expansion hardware was installed.
        Self::original_8800(1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100ChassisConfigError {
    InvalidConnectorPopulation {
        model: AltairChassisModel,
        fitted_connectors: usize,
    },
}

/// UI-facing inventory skeleton.  It deliberately stores connector occupancy
/// independently from runtime card objects so configuration/persistence can be
/// edited while POWER is off and then materialized into a fresh backplane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S100SlotInventory<T> {
    chassis: S100ChassisConfig,
    slots: Vec<Option<T>>,
}

impl<T> S100SlotInventory<T> {
    pub fn new(chassis: S100ChassisConfig) -> Result<Self, S100ChassisConfigError> {
        let chassis = chassis.validate()?;
        let slots = (0..chassis.fitted_connectors).map(|_| None).collect();
        Ok(Self { chassis, slots })
    }

    pub fn chassis(&self) -> S100ChassisConfig {
        self.chassis
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn slots(&self) -> &[Option<T>] {
        &self.slots
    }

    pub fn slot(&self, number: usize) -> Option<&T> {
        self.slots.get(number.checked_sub(1)?).and_then(Option::as_ref)
    }

    pub fn set_slot(&mut self, number: usize, card: Option<T>) -> Result<(), usize> {
        let index = number.checked_sub(1).ok_or(number)?;
        let slot = self.slots.get_mut(index).ok_or(number)?;
        *slot = card;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_8800_uses_four_slot_expander_sections_up_to_sixteen_cards() {
        for (sections, slots) in [(1, 4), (2, 8), (3, 12), (4, 16)] {
            let config = S100ChassisConfig::original_8800(sections).validate().unwrap();
            assert_eq!(config.fitted_connectors, slots);
            assert_eq!(config.physical_slot_positions(), 16);
            assert_eq!(config.motherboard(), S100MotherboardKind::FourSlotExpanderSections);
        }
        assert!(S100ChassisConfig::original_8800(5).validate().is_err());
    }

    #[test]
    fn altair_8800b_keeps_eighteen_positions_separate_from_fitted_connectors() {
        for connectors in [6, 12, 18] {
            let config = S100ChassisConfig::altair_8800b(connectors).validate().unwrap();
            assert_eq!(config.physical_slot_positions(), 18);
            assert_eq!(config.fitted_connectors, connectors);
            assert_eq!(config.motherboard(), S100MotherboardKind::Single18Slot);
            assert_eq!(config.empty_backplane().unwrap().slot_count(), connectors);
        }
        assert!(S100ChassisConfig::altair_8800b(16).validate().is_err());
    }

    #[test]
    fn slot_inventory_is_one_based_like_physical_slot_labels() {
        let mut inventory = S100SlotInventory::new(S100ChassisConfig::altair_8800b(18)).unwrap();
        inventory.set_slot(1, Some("CPU")).unwrap();
        inventory.set_slot(18, Some("RAM")).unwrap();
        assert_eq!(inventory.slot(1), Some(&"CPU"));
        assert_eq!(inventory.slot(18), Some(&"RAM"));
        assert!(inventory.set_slot(19, Some("bad")).is_err());
    }
}
