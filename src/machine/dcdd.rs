//! MITS 88-DCDD physical two-board controller assembly.
//!
//! Phase 1 intentionally models topology only.  The July 1977 MITS operator's
//! guide documents two separate S-100 controller boards joined by a dedicated
//! controller harness which also owns the external 37-pin / 18-pair disk cable
//! boundary.  Register decode and active S-100 contacts are introduced in Phase
//! 2 from the schematics; until then both fitted cards are electrically quiescent
//! except for their modeled supply contacts.

use std::rc::Rc;

use crate::s100::{
    S100Card, S100CardClass, S100CardContact, S100CardDescriptor, S100ContactRole, S100Signal,
};
use crate::s100_backplane::{S100CardDrive, S100ElectricalCard};

const PHASE1_SUPPLY_CONTACTS: &[S100CardContact] = &[
    S100CardContact::new(S100Signal::Plus8V, S100ContactRole::Power),
    S100CardContact::new(S100Signal::Ground, S100ContactRole::Power),
];

static MITS_88_DCDD_BOARD_1: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-dcdd-board-1",
    label: "MITS 88-DCDD Controller Board #1",
    class: S100CardClass::StorageController,
    historical: true,
    contacts: PHASE1_SUPPLY_CONTACTS,
};

static MITS_88_DCDD_BOARD_2: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-dcdd-board-2",
    label: "MITS 88-DCDD Controller Board #2",
    class: S100CardClass::StorageController,
    historical: true,
    contacts: PHASE1_SUPPLY_CONTACTS,
};

/// External controller-to-disk cable/bus boundary.
///
/// It is deliberately a distinct physical object even though Phase 1 has no
/// active disk signals yet.  Later phases extend this object rather than giving
/// either S-100 board a direct drive/image reference.
#[derive(Debug, Default)]
struct Mits88DiskCableBus;

/// Shared copper/electronics boundary joining Board #1, Board #2 and the external
/// disk cable.  There is one instance per validated 88-DCDD controller pair.
#[derive(Debug, Default)]
struct Mits88DcddHarnessState {
    external_disk_bus: Mits88DiskCableBus,
}

/// Cloneable handle to the one documented controller harness.
///
/// Board objects clone this handle; they never contain references to each other.
/// `Rc` is sufficient because the S-100 fabric is single-threaded emulated
/// hardware. Dynamic harness state will gain interior mutability only when Phase
/// 2 introduces source-backed signal lines.
#[derive(Clone, Debug)]
pub(crate) struct Mits88DcddHarness {
    state: Rc<Mits88DcddHarnessState>,
}

impl Mits88DcddHarness {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(Mits88DcddHarnessState::default()),
        }
    }

    pub(crate) fn board1_card(&self) -> Box<dyn S100ElectricalCard> {
        Box::new(Mits88DcddBoard1 {
            harness: self.clone(),
        })
    }

    pub(crate) fn board2_card(&self) -> Box<dyn S100ElectricalCard> {
        Box::new(Mits88DcddBoard2 {
            harness: self.clone(),
        })
    }

    #[inline]
    fn phase1_bus_boundary(&self) -> &Mits88DiskCableBus {
        &self.state.external_disk_bus
    }

    #[cfg(test)]
    fn same_physical_harness(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

struct Mits88DcddBoard1 {
    harness: Mits88DcddHarness,
}

impl S100Card for Mits88DcddBoard1 {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        &MITS_88_DCDD_BOARD_1
    }
}

impl S100ElectricalCard for Mits88DcddBoard1 {
    fn drive_s100(&self) -> S100CardDrive {
        // Touching the shared boundary here makes the ownership relationship
        // explicit without inventing any Phase-2 electrical behavior.
        let _ = self.harness.phase1_bus_boundary();
        S100CardDrive::new()
    }

    fn external_drive_dirty(&self) -> bool {
        false
    }
}

struct Mits88DcddBoard2 {
    harness: Mits88DcddHarness,
}

impl S100Card for Mits88DcddBoard2 {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        &MITS_88_DCDD_BOARD_2
    }
}

impl S100ElectricalCard for Mits88DcddBoard2 {
    fn drive_s100(&self) -> S100CardDrive {
        let _ = self.harness.phase1_bus_boundary();
        S100CardDrive::new()
    }

    fn external_drive_dirty(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100_backplane::S100PinDrive;

    #[test]
    fn both_controller_boards_share_one_harness_not_each_other() {
        let harness = Mits88DcddHarness::new();
        let board1 = Mits88DcddBoard1 {
            harness: harness.clone(),
        };
        let board2 = Mits88DcddBoard2 {
            harness: harness.clone(),
        };

        assert!(board1.harness.same_physical_harness(&board2.harness));
    }

    #[test]
    fn phase1_descriptors_are_distinct_historical_storage_controller_cards() {
        let harness = Mits88DcddHarness::new();
        let board1 = Mits88DcddBoard1 {
            harness: harness.clone(),
        };
        let board2 = Mits88DcddBoard2 { harness };
        let descriptor1 = board1.s100_descriptor();
        let descriptor2 = board2.s100_descriptor();

        assert_ne!(descriptor1.key, descriptor2.key);
        assert_eq!(descriptor1.class, S100CardClass::StorageController);
        assert_eq!(descriptor2.class, S100CardClass::StorageController);
        assert!(descriptor1.historical && descriptor2.historical);
        assert_eq!(descriptor1.contacts, PHASE1_SUPPLY_CONTACTS);
        assert_eq!(descriptor2.contacts, PHASE1_SUPPLY_CONTACTS);
    }

    #[test]
    fn phase1_cards_drive_no_s100_signal_and_need_no_external_refresh() {
        let harness = Mits88DcddHarness::new();
        let board1 = Mits88DcddBoard1 {
            harness: harness.clone(),
        };
        let board2 = Mits88DcddBoard2 { harness };

        for board in [&board1 as &dyn S100ElectricalCard, &board2 as &dyn S100ElectricalCard] {
            let drive = board.drive_s100();
            for pin in 1..=100 {
                assert_eq!(drive.pin(pin), Some(S100PinDrive::HighZ));
            }
            assert!(!board.external_drive_dirty());
        }
    }
}
