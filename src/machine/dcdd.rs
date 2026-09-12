//! MITS 88-DCDD physical two-board controller assembly.
//!
//! The controller is deliberately not represented as one register device. MITS
//! split the hardware across two S-100 cards: Board #1 owns address selection and
//! all computer input paths, while Board #2 owns the output-side disk-enable and
//! function/write circuitry. The cards communicate through their documented
//! controller harness, never by holding a software reference to each other.
//!
//! Phase 2 models the fixed 08h-0Ah S-100 register surface with no disk unit
//! attached. Mechanics/media arrive later. In particular, sector-position
//! drivers remain disabled without Head Status, and the power-up contents of the
//! read-data latches are intentionally *undefined* rather than fabricated as a
//! historical constant.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use rand::RngCore;

use crate::s100::{
    S100Card, S100CardClass, S100CardContact, S100CardDescriptor, S100ContactRole, S100Signal,
};
use crate::s100_backplane::{S100BusSample, S100CardDrive, S100ElectricalCard};

pub(crate) const DCDD_STATUS_CONTROL_PORT: u8 = 0x08;
pub(crate) const DCDD_SECTOR_CONTROL_PORT: u8 = 0x09;
pub(crate) const DCDD_DATA_PORT: u8 = 0x0a;

/// MITS defines active controller status as True=0, False=1. With Disk Control
/// disabled every active status function is false, while schematic/guide review
/// fixes the two unused status outputs D3/D4 LOW. Therefore the no-drive status
/// byte is 1110_0111b = E7h, not FFh.
const DISABLED_STATUS: u8 = 0xe7;

const PWR: S100CardContact =
    S100CardContact::new(S100Signal::Plus8V, S100ContactRole::Power);
const GND: S100CardContact =
    S100CardContact::new(S100Signal::Ground, S100ContactRole::Power);

/// Board #1 connector contacts exercised by the Phase-2 circuit.
///
/// The original decode is wired to A8..A15 (8080 I/O cycles duplicate the port
/// byte on both address halves). Keeping only those address inputs here is an
/// architectural guard against silently turning this into a generic low-byte
/// register card. The physical 2 MHz board clock is source-backed too, but its
/// read/write timing circuitry is intentionally deferred until the event-driven
/// disk timing phases; adding CLOC as a hot-path observer before it can affect a
/// Phase-2 output would create needless per-T-state work.
const BOARD1_CONTACTS: &[S100CardContact] = &[
    PWR,
    GND,
    S100CardContact::new(S100Signal::Address(8), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(9), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(10), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(11), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(12), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(13), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(14), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Address(15), S100ContactRole::Input),
    S100CardContact::new(S100Signal::Inp, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Out, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Write, S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataBusIn, S100ContactRole::Input),
    S100CardContact::new(S100Signal::InterruptEnable, S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(1), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(2), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(3), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(4), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(5), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(6), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataIn(7), S100ContactRole::TriStateOutput),
];

/// Board #2 receives the computer output byte directly from S-100. DCL/CD/WDS
/// are *not* synthetic S-100 contacts: Board #1 supplies those strobes over the
/// real inter-board harness. POC clears the Board-2 disk-enable flip-flop.
const BOARD2_CONTACTS: &[S100CardContact] = &[
    PWR,
    GND,
    S100CardContact::new(S100Signal::DataOut(0), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(1), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(2), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(3), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(4), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(5), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(6), S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOut(7), S100ContactRole::Input),
    S100CardContact::new(S100Signal::PowerOnClear, S100ContactRole::Input),
];

static MITS_88_DCDD_BOARD_1: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-dcdd-board-1",
    label: "MITS 88-DCDD Controller Board #1",
    class: S100CardClass::StorageController,
    historical: true,
    contacts: BOARD1_CONTACTS,
};

static MITS_88_DCDD_BOARD_2: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-dcdd-board-2",
    label: "MITS 88-DCDD Controller Board #2",
    class: S100CardClass::StorageController,
    historical: true,
    contacts: BOARD2_CONTACTS,
};

/// External controller-to-disk cable/bus boundary.
///
/// Phase 2 deliberately has no attached Disk Buffer/FD-400, so every addressed
/// drive is unavailable. Later phases extend this boundary rather than handing
/// either S-100 board a disk image or host path.
#[derive(Debug, Default)]
struct Mits88DiskCableBus;

impl Mits88DiskCableBus {
    #[inline]
    fn drive_available(&self, _address: u8) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Board1HarnessDrive {
    /// Disk Control Latch strobe generated by OUT 08h.
    dcl: bool,
    /// Control Disk strobe generated by OUT 09h.
    cd: bool,
    /// Write Data Strobe generated by OUT 0Ah.
    wds: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Board2HarnessDrive {
    /// Disk Enable line generated by the Board-2 enable flip-flop.
    disk_enable: bool,
}

#[derive(Debug, Default)]
struct Mits88DcddBoard2Circuit {
    selected_drive: u8,
    disk_control_enabled: bool,
}

/// Shared copper/electronics boundary joining Board #1, Board #2 and the external
/// disk cable. Signal ownership remains explicit: each board may only replace
/// the group that it physically drives. The weak Board-2 endpoint represents the
/// receiving pins on that board; it does not give Board #1 a card reference.
#[derive(Debug)]
struct Mits88DcddHarnessState {
    board1: Board1HarnessDrive,
    board2: Board2HarnessDrive,
    board2_sink: Weak<RefCell<Mits88DcddBoard2Circuit>>,
    external_disk_bus: Mits88DiskCableBus,
}

impl Default for Mits88DcddHarnessState {
    fn default() -> Self {
        Self {
            board1: Board1HarnessDrive::default(),
            board2: Board2HarnessDrive::default(),
            board2_sink: Weak::new(),
            external_disk_bus: Mits88DiskCableBus,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Mits88DcddHarness {
    state: Rc<RefCell<Mits88DcddHarnessState>>,
}

impl Mits88DcddHarness {
    pub(crate) fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(Mits88DcddHarnessState::default())),
        }
    }

    pub(crate) fn board1_card(&self) -> Box<dyn S100ElectricalCard> {
        Box::new(Mits88DcddBoard1::new(self.clone()))
    }

    pub(crate) fn board2_card(&self) -> Box<dyn S100ElectricalCard> {
        let circuit = Rc::new(RefCell::new(Mits88DcddBoard2Circuit::default()));
        self.state.borrow_mut().board2_sink = Rc::downgrade(&circuit);
        Box::new(Mits88DcddBoard2 {
            harness: self.clone(),
            circuit,
        })
    }

    /// Drive Board-1 harness outputs and propagate their electrical edge to the
    /// Board-2 receiving circuit immediately. This is the software equivalent of
    /// copper propagation: no runtime/card polling is required, and Board #2
    /// samples its own S-100 DO inputs from the same resolved bus sample.
    fn set_board1_drive(&self, drive: Board1HarnessDrive, sample: &S100BusSample) {
        let (previous, sink) = {
            let mut state = self.state.borrow_mut();
            let previous = state.board1;
            if previous == drive {
                return;
            }
            state.board1 = drive;
            (previous, state.board2_sink.clone())
        };

        if let Some(sink) = sink.upgrade() {
            sink.borrow_mut()
                .observe_harness_edge(previous, drive, sample, self);
        }
    }

    fn board1_drive(&self) -> Board1HarnessDrive {
        self.state.borrow().board1
    }

    fn set_disk_enable(&self, enabled: bool) {
        self.state.borrow_mut().board2.disk_enable = enabled;
    }

    fn disk_enabled(&self) -> bool {
        self.state.borrow().board2.disk_enable
    }

    fn drive_available(&self, address: u8) -> bool {
        self.state.borrow().external_disk_bus.drive_available(address)
    }

    #[cfg(test)]
    fn same_physical_harness(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl Mits88DcddBoard2Circuit {
    fn clear_disk_control(&mut self, harness: &Mits88DcddHarness) {
        self.disk_control_enabled = false;
        harness.set_disk_enable(false);
    }

    fn apply_dcl_rising(&mut self, value: u8, harness: &Mits88DcddHarness) {
        if value & 0x80 != 0 {
            self.clear_disk_control(harness);
            return;
        }

        self.selected_drive = value & 0x0f;
        // MITS explicitly forbids enabling when the cable/drive/power path is
        // absent. Phase 2 has no Disk Buffer attached, so an enable attempt must
        // remain disabled rather than creating a synthetic drive.
        self.disk_control_enabled = harness.drive_available(self.selected_drive);
        harness.set_disk_enable(self.disk_control_enabled);
    }

    fn observe_harness_edge(
        &mut self,
        previous: Board1HarnessDrive,
        current: Board1HarnessDrive,
        sample: &S100BusSample,
        harness: &Mits88DcddHarness,
    ) {
        if current.dcl && !previous.dcl {
            if let Some(value) = sample.data_out() {
                self.apply_dcl_rising(value, harness);
            }
        }

        // CD and WDS edge ownership is established here, but while Disk Control
        // is false they have no Phase-2 state effect. Head/write circuitry is
        // introduced only with the source-backed mechanics/write phases.
        let _cd_rising = current.cd && !previous.cd;
        let _wds_rising = current.wds && !previous.wds;
    }
}

struct Mits88DcddBoard1 {
    harness: Mits88DcddHarness,
    read_drive: Option<u8>,
    /// G3/H1 read-data latch power-up is not specified by the MITS material.
    /// Give the TTL latch an indeterminate power-up byte instead of claiming a
    /// fixed historical value or incorrectly releasing its enabled bus drivers.
    undefined_read_latch: u8,
}

impl Mits88DcddBoard1 {
    fn new(harness: Mits88DcddHarness) -> Self {
        let undefined_read_latch = rand::rng().next_u32() as u8;
        Self {
            harness,
            read_drive: None,
            undefined_read_latch,
        }
    }

    fn high_address(sample: &S100BusSample) -> Option<u8> {
        let mut value = 0u8;
        for bit in 0..8 {
            if sample.signal_level(S100Signal::Address(bit + 8))? {
                value |= 1 << bit;
            }
        }
        Some(value)
    }

    fn decoded_port(sample: &S100BusSample) -> Option<u8> {
        match Self::high_address(sample)? {
            DCDD_STATUS_CONTROL_PORT => Some(DCDD_STATUS_CONTROL_PORT),
            DCDD_SECTOR_CONTROL_PORT => Some(DCDD_SECTOR_CONTROL_PORT),
            DCDD_DATA_PORT => Some(DCDD_DATA_PORT),
            _ => None,
        }
    }

    fn status_byte(&self, sample: &S100BusSample) -> u8 {
        if !self.harness.disk_enabled() {
            return DISABLED_STATUS;
        }

        // With a future enabled drive, D0/D1/D2/D6/D7 will be supplied by the
        // mechanics/electronics state machines. Phase 2 cannot reach this branch
        // because the external cable has no attached Disk Buffer. D5 is already
        // source-backed as observation of S-100 pINTE.
        let mut status = DISABLED_STATUS;
        if sample.signal_level(S100Signal::InterruptEnable) == Some(true) {
            status &= !(1 << 5);
        }
        status
    }

    fn update_read_drive(&mut self, sample: &S100BusSample) {
        let read_active = sample.signal_level(S100Signal::Inp) == Some(true)
            && sample.signal_level(S100Signal::DataBusIn) == Some(true);
        if !read_active {
            self.read_drive = None;
            return;
        }

        self.read_drive = match Self::decoded_port(sample) {
            Some(DCDD_STATUS_CONTROL_PORT) => Some(self.status_byte(sample)),
            // The sector line drivers are enabled by Head Status. With no drive
            // attached HS is false, so IN 09h is genuinely high impedance.
            Some(DCDD_SECTOR_CONTROL_PORT) => None,
            // The read-data line drivers are selected by the read strobe even
            // before valid media data exists. Their latch power-up value is not
            // documented, so drive one indeterminate TTL state rather than
            // fabricating FFh/open bus as historical behavior.
            Some(DCDD_DATA_PORT) => Some(self.undefined_read_latch),
            _ => None,
        };
    }

    fn update_harness_strobes(&self, sample: &S100BusSample) {
        let write_active = sample.signal_level(S100Signal::Out) == Some(true)
            && sample.signal_level(S100Signal::Write) == Some(false);
        let selected = write_active.then(|| Self::decoded_port(sample)).flatten();
        self.harness.set_board1_drive(
            Board1HarnessDrive {
                dcl: selected == Some(DCDD_STATUS_CONTROL_PORT),
                cd: selected == Some(DCDD_SECTOR_CONTROL_PORT),
                wds: selected == Some(DCDD_DATA_PORT),
            },
            sample,
        );
    }
}

impl S100Card for Mits88DcddBoard1 {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        &MITS_88_DCDD_BOARD_1
    }
}

impl S100ElectricalCard for Mits88DcddBoard1 {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        self.update_harness_strobes(sample);
        self.update_read_drive(sample);
    }

    fn drive_s100(&self) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        if let Some(value) = self.read_drive {
            drive.drive_data_in(value);
        }
        drive
    }

    fn external_drive_dirty(&self) -> bool {
        false
    }
}

struct Mits88DcddBoard2 {
    harness: Mits88DcddHarness,
    circuit: Rc<RefCell<Mits88DcddBoard2Circuit>>,
}

impl S100Card for Mits88DcddBoard2 {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        &MITS_88_DCDD_BOARD_2
    }
}

impl S100ElectricalCard for Mits88DcddBoard2 {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        if sample.signal_level(S100Signal::PowerOnClear) == Some(true) {
            self.circuit.borrow_mut().clear_disk_control(&self.harness);
        }
    }

    fn drive_s100(&self) -> S100CardDrive {
        // Board #2 owns no Phase-2 S-100 output. Its outputs go to Board #1 or
        // the external disk cable over the dedicated controller harness.
        S100CardDrive::new()
    }

    fn external_drive_dirty(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100_backplane::{S100Backplane, S100PinDrive};

    fn io_sample(port: u8, inp: bool, out: bool, dbin: bool, wr_n: bool, data: u8) -> S100BusSample {
        let backplane = S100Backplane::new(0);
        let mut source = S100CardDrive::new();
        source.drive_address(u16::from(port) << 8);
        source.drive_data_out(data);
        source.drive_signal(S100Signal::Inp, inp);
        source.drive_signal(S100Signal::Out, out);
        source.drive_signal(S100Signal::DataBusIn, dbin);
        source.drive_signal(S100Signal::Write, wr_n);
        source.drive_signal(S100Signal::InterruptEnable, false);
        backplane.resolve_drive_sets(&[source])
    }

    fn board2_fixture(
        harness: &Mits88DcddHarness,
    ) -> (Mits88DcddBoard2, Rc<RefCell<Mits88DcddBoard2Circuit>>) {
        let circuit = Rc::new(RefCell::new(Mits88DcddBoard2Circuit::default()));
        harness.state.borrow_mut().board2_sink = Rc::downgrade(&circuit);
        (
            Mits88DcddBoard2 {
                harness: harness.clone(),
                circuit: circuit.clone(),
            },
            circuit,
        )
    }

    #[test]
    fn both_controller_boards_share_one_harness_not_each_other() {
        let harness = Mits88DcddHarness::new();
        let board1 = Mits88DcddBoard1::new(harness.clone());
        let (board2, _) = board2_fixture(&harness);

        assert!(board1.harness.same_physical_harness(&board2.harness));
    }

    #[test]
    fn descriptors_split_real_s100_ownership_between_the_two_boards() {
        let harness = Mits88DcddHarness::new();
        let board1 = Mits88DcddBoard1::new(harness.clone());
        let (board2, _) = board2_fixture(&harness);
        let descriptor1 = board1.s100_descriptor();
        let descriptor2 = board2.s100_descriptor();

        assert_ne!(descriptor1.key, descriptor2.key);
        assert_eq!(descriptor1.class, S100CardClass::StorageController);
        assert_eq!(descriptor2.class, S100CardClass::StorageController);
        assert!(descriptor1.historical && descriptor2.historical);
        assert!(descriptor1.contacts.iter().any(|c| c.signal == S100Signal::Address(15)));
        assert!(!descriptor1.contacts.iter().any(|c| c.signal == S100Signal::DataOut(0)));
        assert!(descriptor2.contacts.iter().any(|c| c.signal == S100Signal::DataOut(0)));
        assert!(!descriptor2.contacts.iter().any(|c| c.signal == S100Signal::DataIn(0)));
    }

    #[test]
    fn board1_decodes_the_documented_high_address_byte_not_a_generic_low_byte() {
        let harness = Mits88DcddHarness::new();
        let mut board1 = Mits88DcddBoard1::new(harness);

        let high_selected = io_sample(0x08, true, false, true, true, 0);
        board1.observe_s100(&high_selected);
        assert_eq!(board1.read_drive, Some(DISABLED_STATUS));

        let backplane = S100Backplane::new(0);
        let mut low_only = S100CardDrive::new();
        low_only.drive_address(0x0008);
        low_only.drive_signal(S100Signal::Inp, true);
        low_only.drive_signal(S100Signal::DataBusIn, true);
        low_only.drive_signal(S100Signal::Write, true);
        low_only.drive_signal(S100Signal::Out, false);
        low_only.drive_signal(S100Signal::InterruptEnable, false);
        board1.observe_s100(&backplane.resolve_drive_sets(&[low_only]));
        assert_eq!(board1.read_drive, None);
    }

    #[test]
    fn disabled_no_drive_status_is_exactly_e7_and_sector_port_is_high_z() {
        let harness = Mits88DcddHarness::new();
        let mut board1 = Mits88DcddBoard1::new(harness);

        board1.observe_s100(&io_sample(0x08, true, false, true, true, 0));
        assert_eq!(board1.read_drive, Some(0xe7));
        assert_eq!(
            board1
                .drive_s100()
                .pin(S100Signal::DataIn(0).pin().unwrap()),
            Some(S100PinDrive::Driven(true))
        );

        board1.observe_s100(&io_sample(0x09, true, false, true, true, 0));
        assert_eq!(board1.read_drive, None);
        let drive = board1.drive_s100();
        for bit in 0..8 {
            assert_eq!(
                drive.pin(S100Signal::DataIn(bit).pin().unwrap()),
                Some(S100PinDrive::HighZ)
            );
        }
    }

    #[test]
    fn board1_generates_only_the_documented_harness_strobe_for_each_out_port() {
        let harness = Mits88DcddHarness::new();
        let mut board1 = Mits88DcddBoard1::new(harness.clone());

        board1.observe_s100(&io_sample(0x08, false, true, false, false, 0x00));
        assert_eq!(
            harness.board1_drive(),
            Board1HarnessDrive {
                dcl: true,
                cd: false,
                wds: false
            }
        );
        board1.observe_s100(&io_sample(0x09, false, true, false, false, 0x00));
        assert_eq!(
            harness.board1_drive(),
            Board1HarnessDrive {
                dcl: false,
                cd: true,
                wds: false
            }
        );
        board1.observe_s100(&io_sample(0x0a, false, true, false, false, 0x00));
        assert_eq!(
            harness.board1_drive(),
            Board1HarnessDrive {
                dcl: false,
                cd: false,
                wds: true
            }
        );
        board1.observe_s100(&io_sample(0x0b, false, true, false, false, 0x00));
        assert_eq!(harness.board1_drive(), Board1HarnessDrive::default());
    }

    #[test]
    fn harness_edge_reaches_board2_independent_of_card_observation_order() {
        let harness = Mits88DcddHarness::new();
        let mut board1 = Mits88DcddBoard1::new(harness.clone());
        let (_board2, circuit) = board2_fixture(&harness);

        let select = io_sample(0x08, false, true, false, false, 0x03);
        board1.observe_s100(&select);

        assert_eq!(circuit.borrow().selected_drive, 3);
        assert!(!circuit.borrow().disk_control_enabled);
        assert!(!harness.disk_enabled());
    }

    #[test]
    fn board2_cannot_enable_a_missing_drive_and_d7_or_poc_clear_disk_control() {
        let harness = Mits88DcddHarness::new();
        let mut board1 = Mits88DcddBoard1::new(harness.clone());
        let (mut board2, circuit) = board2_fixture(&harness);

        let select = io_sample(0x08, false, true, false, false, 0x03);
        board1.observe_s100(&select);
        assert_eq!(circuit.borrow().selected_drive, 3);
        assert!(!circuit.borrow().disk_control_enabled);
        assert!(!harness.disk_enabled());

        // Release then assert a distinct DCL edge with the documented clear bit.
        board1.observe_s100(&io_sample(0x08, false, true, false, true, 0x83));
        circuit.borrow_mut().disk_control_enabled = true;
        harness.set_disk_enable(true);
        board1.observe_s100(&io_sample(0x08, false, true, false, false, 0x83));
        assert!(!circuit.borrow().disk_control_enabled);
        assert!(!harness.disk_enabled());

        circuit.borrow_mut().disk_control_enabled = true;
        harness.set_disk_enable(true);
        let backplane = S100Backplane::new(0);
        let mut poc = S100CardDrive::new();
        poc.drive_signal(S100Signal::PowerOnClear, true);
        board2.observe_s100(&backplane.resolve_drive_sets(&[poc]));
        assert!(!circuit.borrow().disk_control_enabled);
        assert!(!harness.disk_enabled());
    }

    #[test]
    fn data_port_drives_an_indeterminate_latch_instead_of_claiming_open_bus() {
        let harness = Mits88DcddHarness::new();
        let mut board1 = Mits88DcddBoard1::new(harness);
        board1.undefined_read_latch = 0x5a;
        board1.observe_s100(&io_sample(0x0a, true, false, true, true, 0));
        assert_eq!(board1.read_drive, Some(0x5a));
        assert_eq!(
            board1
                .drive_s100()
                .pin(S100Signal::DataIn(1).pin().unwrap()),
            Some(S100PinDrive::Driven(true))
        );
    }

    #[test]
    fn phase2_cards_have_no_asynchronous_s100_refresh_path() {
        let harness = Mits88DcddHarness::new();
        let board1 = Mits88DcddBoard1::new(harness.clone());
        let (board2, _) = board2_fixture(&harness);
        assert!(!board1.external_drive_dirty());
        assert!(!board2.external_drive_dirty());
    }
}
