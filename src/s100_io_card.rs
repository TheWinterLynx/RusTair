//! Electrical adapter for register-oriented S-100 I/O cards.
//!
//! This module owns no UART implementation. It translates the actual S-100 bus
//! strobes into one register operation per physical I/O cycle and lets an
//! existing device model supply register contents, side effects and interrupt
//! levels. This prevents the 88-SIO/88-2SIO migration from creating a second
//! COM2502 or MC6850 implementation beside the already-tested one.

use crate::s100::{
    S100Card, S100CardClass, S100CardContact, S100CardDescriptor, S100ContactRole,
    S100Signal,
};
use crate::s100_backplane::{S100BusSample, S100CardDrive, S100ElectricalCard};

const PWR: S100CardContact =
    S100CardContact::new(S100Signal::Plus8V, S100ContactRole::Power);
const GND: S100CardContact =
    S100CardContact::new(S100Signal::Ground, S100ContactRole::Power);

const S100_IO_COMMON_CONTACTS: &[S100CardContact] = &[
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
    S100CardContact::new(S100Signal::PowerOnClear, S100ContactRole::Input),
    S100CardContact::new(
        S100Signal::InterruptRequest,
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(0),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(1),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(2),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(3),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(4),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(5),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(6),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(7),
        S100ContactRole::OpenCollectorOutput,
    ),
];

const S100_2SIO_CONTACTS: &[S100CardContact] = &[
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
    S100CardContact::new(
        S100Signal::InterruptRequest,
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(0),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(1),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(2),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(3),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(4),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(5),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(6),
        S100ContactRole::OpenCollectorOutput,
    ),
    S100CardContact::new(
        S100Signal::VectorInterrupt(7),
        S100ContactRole::OpenCollectorOutput,
    ),
];

/// Live electrical descriptor used by the register-cycle adapter. VI0..VI7 are
/// modeled as possible open-collector destinations of the board's interrupt
/// pads; disconnected wiring simply leaves every VI line released.
pub static MITS_88_SIO_IO_CARD: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-sio-live-io",
    label: "MITS 88-SIO",
    class: S100CardClass::Serial,
    historical: true,
    contacts: S100_IO_COMMON_CONTACTS,
};

pub static MITS_88_2SIO_IO_CARD: S100CardDescriptor = S100CardDescriptor {
    key: "mits-88-2sio-live-io",
    label: "MITS 88-2SIO",
    class: S100CardClass::Serial,
    historical: true,
    contacts: S100_2SIO_CONTACTS,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct S100IoDeviceLines {
    /// Assert the active-low PINT net.
    pub pint: bool,
    /// Bit N asserts the active-low VI(N) net.
    pub vi_asserted: u8,
    /// Assert the active-low/open-collector PRDY contribution. Only cards whose
    /// descriptor declares READY may use this.
    pub ready_low: bool,
}

/// Register-facing boundary supplied by an actual serial/parallel card model.
/// Read/write side effects happen here exactly once per physical bus strobe.
pub trait S100IoRegisterDevice {
    fn read_register(&mut self, offset: u8) -> u8;
    fn write_register(&mut self, offset: u8, value: u8);

    fn bus_lines(&self) -> S100IoDeviceLines {
        S100IoDeviceLines::default()
    }

    /// Observe card-wide bus timing that is not itself a register strobe.
    /// This keeps board-specific edge logic behind the register-device boundary.
    fn observe_bus(&mut self, _sample: &S100BusSample, _selected: bool) -> bool { false }

    /// Return true only for a device whose board-wide logic genuinely needs to
    /// inspect unrelated bus deltas while neither sINP nor sOUT is active. The
    /// adapter itself still preserves POC edges and the transition leaving an
    /// I/O window, so ordinary decoded register cards can remain quiescent.
    fn requires_idle_bus_observation(&self) -> bool { false }

    /// Whether state changed through a host connector or independent device
    /// clock since the adapter last rebuilt its cached S-100 outputs.
    fn external_drive_dirty(&self) -> bool { true }
}

/// Converts S-100 I/O strobes into register operations without owning the
/// register implementation itself.
pub struct S100IoCardAdapter<D> {
    descriptor: &'static S100CardDescriptor,
    base: u8,
    width: u8,
    device: D,
    read_cycle_port: Option<u8>,
    read_drive: Option<u8>,
    write_cycle_port: Option<u8>,
    io_status_active: bool,
    power_on_clear: bool,
    /// Persistent connector output state. Real TTL/tri-state outputs retain
    /// their electrical state until an input or device event changes them; they
    /// are not recomputed merely because the backplane samples the slot again.
    cached_drive: S100CardDrive,
}

impl<D> S100IoCardAdapter<D> {
    pub fn device(&self) -> &D {
        &self.device
    }

    pub fn device_mut(&mut self) -> &mut D {
        &mut self.device
    }

    pub const fn base(&self) -> u8 {
        self.base
    }

    pub const fn width(&self) -> u8 {
        self.width
    }

    fn offset_for_port(&self, port: u8) -> Option<u8> {
        let offset = port.wrapping_sub(self.base);
        (offset < self.width).then_some(offset)
    }

    fn low_address(sample: &S100BusSample) -> Option<u8> {
        let mut port = 0u8;
        for bit in 0..8 {
            if sample.signal_level(S100Signal::Address(bit))? {
                port |= 1 << bit;
            }
        }
        Some(port)
    }

    fn selected_port(&self, sample: &S100BusSample) -> Option<(u8, u8)> {
        let port = Self::low_address(sample)?;
        self.offset_for_port(port).map(|offset| (port, offset))
    }
}

impl<D: S100IoRegisterDevice> S100IoCardAdapter<D> {
    pub fn new(
        descriptor: &'static S100CardDescriptor,
        base: u8,
        width: u8,
        device: D,
    ) -> Self {
        assert!(width != 0, "S-100 I/O card must decode at least one port");
        assert!(
            u16::from(base) + u16::from(width) <= 256,
            "S-100 I/O decode window must fit in A0..A7"
        );
        let mut card = Self {
            descriptor,
            base,
            width,
            device,
            read_cycle_port: None,
            read_drive: None,
            write_cycle_port: None,
            io_status_active: false,
            power_on_clear: false,
            cached_drive: S100CardDrive::new(),
        };
        card.refresh_cached_drive();
        card
    }

    fn refresh_cached_drive(&mut self) {
        let mut drive = S100CardDrive::new();
        if let Some(value) = self.read_drive {
            drive.drive_data_in(value);
        }

        let lines = self.device.bus_lines();
        drive.pull_low(S100Signal::InterruptRequest, lines.pint);
        for level in 0..8 {
            drive.pull_low(
                S100Signal::VectorInterrupt(level),
                lines.vi_asserted & (1 << level) != 0,
            );
        }
        drive.pull_low(S100Signal::Ready, lines.ready_low);
        self.cached_drive = drive;
    }
}

impl<D> S100Card for S100IoCardAdapter<D> {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        self.descriptor
    }
}

impl<D: S100IoRegisterDevice> S100ElectricalCard for S100IoCardAdapter<D> {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        let inp = sample.signal_level(S100Signal::Inp) == Some(true);
        let out = sample.signal_level(S100Signal::Out) == Some(true);
        let io_status_active = inp || out;
        let power_on_clear = sample.signal_level(S100Signal::PowerOnClear) == Some(true);

        // The physical connector still sees every declared input transition.
        // Once the register decoder is outside an I/O status window, however,
        // address/data/DBIN/WAIT activity cannot affect this adapter. Observe the
        // first transition out of a window and both POC edges, then let later
        // unrelated deltas become an O(1) state check instead of re-entering the
        // underlying UART/register model.
        let observe_board = io_status_active
            || self.io_status_active
            || power_on_clear != self.power_on_clear
            || self.device.requires_idle_bus_observation();
        self.io_status_active = io_status_active;
        self.power_on_clear = power_on_clear;
        if !observe_board {
            return;
        }

        // A0..A7 are physically connected and remain part of the card's input
        // sensitivity. Their decode only has a behavioral consequence while an
        // I/O status line is active.
        let selected = if io_status_active {
            self.selected_port(sample)
        } else {
            None
        };
        let mut drive_dirty = self.device.observe_bus(sample, selected.is_some());
        let previous_read_drive = self.read_drive;

        // sINP is the latched I/O-read status; DBIN is the actual 8080 input
        // strobe. Only their overlap constitutes the register read. Keeping the
        // port latched while that strobe remains active prevents multiple
        // resolver deltas from clearing RDA or consuming a FIFO more than once.
        let read_active = inp && sample.signal_level(S100Signal::DataBusIn) == Some(true);
        if read_active {
            if let Some((port, offset)) = selected {
                if self.read_cycle_port != Some(port) {
                    self.read_drive = Some(self.device.read_register(offset));
                    self.read_cycle_port = Some(port);
                    drive_dirty = true;
                }
            } else {
                self.read_cycle_port = None;
                self.read_drive = None;
            }
        } else {
            self.read_cycle_port = None;
            self.read_drive = None;
        }

        // sOUT is the latched I/O-write status and pWR is active LOW. The write
        // occurs once when that physical combination first becomes active for a
        // selected port; holding pWR low across another propagation delta does
        // not duplicate the device write.
        let write_active = out && sample.signal_level(S100Signal::Write) == Some(false);
        if write_active {
            if let (Some((port, offset)), Some(value)) = (selected, sample.data_out()) {
                if self.write_cycle_port != Some(port) {
                    self.device.write_register(offset, value);
                    self.write_cycle_port = Some(port);
                    drive_dirty = true;
                }
            }
        } else {
            self.write_cycle_port = None;
        }

        if drive_dirty || self.read_drive != previous_read_drive {
            self.refresh_cached_drive();
        }
    }

    fn drive_s100(&self) -> S100CardDrive {
        self.cached_drive
    }

    fn external_drive_dirty(&self) -> bool {
        self.device.external_drive_dirty()
    }

    fn refresh_external_drive(&mut self) -> S100CardDrive {
        if self.device.external_drive_dirty() {
            self.refresh_cached_drive();
        }
        self.cached_drive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100_backplane::{s100_slot_mask, S100Backplane};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct FakeState {
        reads: usize,
        writes: Vec<(u8, u8)>,
        read_values: [u8; 4],
        lines: S100IoDeviceLines,
        bus_observations: usize,
    }

    #[derive(Clone)]
    struct FakeDevice(Rc<RefCell<FakeState>>);

    impl S100IoRegisterDevice for FakeDevice {
        fn read_register(&mut self, offset: u8) -> u8 {
            let mut state = self.0.borrow_mut();
            state.reads += 1;
            state.read_values[offset as usize]
        }

        fn write_register(&mut self, offset: u8, value: u8) {
            self.0.borrow_mut().writes.push((offset, value));
        }

        fn bus_lines(&self) -> S100IoDeviceLines {
            self.0.borrow().lines
        }

        fn observe_bus(&mut self, _sample: &S100BusSample, _selected: bool) -> bool {
            self.0.borrow_mut().bus_observations += 1;
            false
        }
    }

    fn drive_io(port: u8, inp: bool, out: bool, dbin: bool, wr_n: bool, data: u8) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.drive_address(u16::from(port));
        drive.drive_signal(S100Signal::Inp, inp);
        drive.drive_signal(S100Signal::Out, out);
        drive.drive_signal(S100Signal::DataBusIn, dbin);
        drive.drive_signal(S100Signal::Write, wr_n);
        drive.drive_data_out(data);
        drive
    }

    fn drive_io_low_address_only(
        port: u8,
        inp: bool,
        out: bool,
        dbin: bool,
        wr_n: bool,
        data: u8,
    ) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        for bit in 0..8 {
            drive.drive_signal(S100Signal::Address(bit), port & (1 << bit) != 0);
        }
        drive.drive_signal(S100Signal::Inp, inp);
        drive.drive_signal(S100Signal::Out, out);
        drive.drive_signal(S100Signal::DataBusIn, dbin);
        drive.drive_signal(S100Signal::Write, wr_n);
        drive.drive_data_out(data);
        drive
    }

    #[test]
    fn idle_bus_deltas_do_not_reenter_the_register_device() {
        let state = Rc::new(RefCell::new(FakeState::default()));
        let card = S100IoCardAdapter::new(
            &MITS_88_2SIO_IO_CARD,
            0x44,
            4,
            FakeDevice(Rc::clone(&state)),
        );
        let mut backplane = S100Backplane::new(2);
        backplane.insert(2, Box::new(card)).unwrap();
        let selected = s100_slot_mask(2);

        let idle_a = drive_io(0x44, false, false, false, true, 0x11);
        backplane.resolve_selected_drives(selected, &[idle_a]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().bus_observations, 0);

        let idle_b = drive_io(0x45, false, false, true, true, 0x22);
        backplane.resolve_selected_drives(selected, &[idle_b]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().bus_observations, 0);

        let io_window = drive_io(0x44, true, false, false, true, 0);
        backplane.resolve_selected_drives(selected, &[io_window]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().bus_observations, 1);

        let leave_io = drive_io(0x44, false, false, false, true, 0);
        backplane.resolve_selected_drives(selected, &[leave_io]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().bus_observations, 2);

        let idle_c = drive_io(0x46, false, false, true, true, 0x33);
        backplane.resolve_selected_drives(selected, &[idle_c]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().bus_observations, 2);
    }

    #[test]
    fn input_register_side_effect_occurs_once_even_across_multiple_bus_deltas() {
        let state = Rc::new(RefCell::new(FakeState::default()));
        state.borrow_mut().read_values[1] = 0x5a;
        let card = S100IoCardAdapter::new(
            &MITS_88_SIO_IO_CARD,
            0x06,
            2,
            FakeDevice(Rc::clone(&state)),
        );
        let mut backplane = S100Backplane::new(3);
        backplane.insert(2, Box::new(card)).unwrap();
        let selected = s100_slot_mask(2);

        let before_dbin = drive_io(0x07, true, false, false, true, 0);
        backplane.resolve_selected_drives(selected, &[before_dbin]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().reads, 0);

        let dbin = drive_io(0x07, true, false, true, true, 0);
        backplane.resolve_selected_drives(selected, &[dbin.clone()]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().reads, 1);

        backplane.resolve_selected_drives(selected, &[dbin.clone()]).unwrap();
        assert_eq!(backplane.sample().data_in(), Some(0x5a));
        backplane.observe_selected_cards(selected);
        backplane.resolve_selected_drives(selected, &[dbin]).unwrap();
        assert_eq!(state.borrow().reads, 1, "same DBIN strobe must not read twice");
        assert_eq!(backplane.sample().data_in(), Some(0x5a));
    }

    #[test]
    fn io_decode_uses_only_a0_through_a7() {
        let state = Rc::new(RefCell::new(FakeState::default()));
        state.borrow_mut().read_values[0] = 0x3c;
        let card = S100IoCardAdapter::new(
            &MITS_88_SIO_IO_CARD,
            0x44,
            2,
            FakeDevice(Rc::clone(&state)),
        );
        let mut backplane = S100Backplane::new(2);
        backplane.insert(2, Box::new(card)).unwrap();
        let selected = s100_slot_mask(2);
        let dbin = drive_io_low_address_only(0x44, true, false, true, true, 0);

        backplane.resolve_selected_drives(selected, &[dbin.clone()]).unwrap();
        backplane.observe_selected_cards(selected);
        backplane.resolve_selected_drives(selected, &[dbin]).unwrap();

        assert_eq!(state.borrow().reads, 1);
        assert_eq!(backplane.sample().data_in(), Some(0x3c));
        for bit in 8..16 {
            assert_eq!(
                backplane.sample().signal_level(S100Signal::Address(bit)),
                None,
                "A{bit} may float without affecting an A0..A7 I/O decoder"
            );
        }
    }

    #[test]
    fn output_register_side_effect_occurs_once_while_pwr_remains_asserted() {
        let state = Rc::new(RefCell::new(FakeState::default()));
        let card = S100IoCardAdapter::new(
            &MITS_88_SIO_IO_CARD,
            0x06,
            2,
            FakeDevice(Rc::clone(&state)),
        );
        let mut backplane = S100Backplane::new(3);
        backplane.insert(2, Box::new(card)).unwrap();
        let selected = s100_slot_mask(2);

        let before_pwr = drive_io(0x07, false, true, false, true, 0xa5);
        backplane.resolve_selected_drives(selected, &[before_pwr]).unwrap();
        backplane.observe_selected_cards(selected);
        assert!(state.borrow().writes.is_empty());

        let pwr = drive_io(0x07, false, true, false, false, 0xa5);
        backplane.resolve_selected_drives(selected, &[pwr.clone()]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().writes, vec![(1, 0xa5)]);

        backplane.resolve_selected_drives(selected, &[pwr]).unwrap();
        backplane.observe_selected_cards(selected);
        assert_eq!(state.borrow().writes, vec![(1, 0xa5)], "held pWR must not duplicate DATA OUT");
    }

    #[test]
    fn overlapping_io_cards_contend_on_di_instead_of_becoming_first_match_wins() {
        let a = Rc::new(RefCell::new(FakeState::default()));
        let b = Rc::new(RefCell::new(FakeState::default()));
        a.borrow_mut().read_values[0] = 0x00;
        b.borrow_mut().read_values[0] = 0xff;
        let mut backplane = S100Backplane::new(4);
        backplane
            .insert(
                2,
                Box::new(S100IoCardAdapter::new(
                    &MITS_88_SIO_IO_CARD,
                    0x44,
                    2,
                    FakeDevice(Rc::clone(&a)),
                )),
            )
            .unwrap();
        backplane
            .insert(
                4,
                Box::new(S100IoCardAdapter::new(
                    &MITS_88_2SIO_IO_CARD,
                    0x44,
                    4,
                    FakeDevice(Rc::clone(&b)),
                )),
            )
            .unwrap();
        let selected = s100_slot_mask(2) | s100_slot_mask(4);
        let dbin = drive_io(0x44, true, false, true, true, 0);

        backplane.resolve_selected_drives(selected, &[dbin.clone()]).unwrap();
        backplane.observe_selected_cards(selected);
        backplane.resolve_selected_drives(selected, &[dbin]).unwrap();

        assert_eq!(a.borrow().reads, 1);
        assert_eq!(b.borrow().reads, 1);
        assert!(backplane.sample().signal_is_contended(S100Signal::DataIn(0)));
        assert!(backplane.sample().signal_is_contended(S100Signal::DataIn(7)));
        assert_eq!(backplane.sample().data_in(), None);
    }

    #[test]
    fn interrupt_and_ready_outputs_are_real_open_collector_bus_lines() {
        let state = Rc::new(RefCell::new(FakeState::default()));
        state.borrow_mut().lines = S100IoDeviceLines {
            pint: true,
            vi_asserted: 1 << 3,
            ready_low: true,
        };
        let card = S100IoCardAdapter::new(
            &MITS_88_2SIO_IO_CARD,
            0x10,
            4,
            FakeDevice(Rc::clone(&state)),
        );
        let mut backplane = S100Backplane::new(2);
        backplane.insert(2, Box::new(card)).unwrap();
        backplane.resolve_current_drives(&[]).unwrap();

        assert_eq!(
            backplane.sample().signal_level(S100Signal::InterruptRequest),
            Some(false)
        );
        assert_eq!(
            backplane.sample().signal_level(S100Signal::VectorInterrupt(3)),
            Some(false)
        );
        assert_eq!(
            backplane.sample().signal_level(S100Signal::VectorInterrupt(2)),
            Some(true)
        );
        assert_eq!(backplane.sample().signal_level(S100Signal::Ready), Some(false));
    }
}
