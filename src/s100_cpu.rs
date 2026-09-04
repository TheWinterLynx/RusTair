//! Live MITS 8080 CPU-board model for the electrical S-100 backplane.
//!
//! The Intel 8080 package is not an S-100 card.  The MITS processor board sits
//! between the package pins and the 100-contact bus: it buffers the 16-bit
//! address and split DI/DO data paths, clocks the 8212 status latch, generates
//! the non-overlapping clocks, and honours the original bus-disable inputs.
//! Keeping that boundary explicit lets Fast and Cycle Accurate remain execution
//! engines for the same physical board instead of becoming two different
//! machines.

use std::cell::RefCell;
use std::rc::Rc;

use crate::cpu8080_cycle::{Cpu8080Inputs, Cpu8080Pins};
use crate::s100::{
    S100Card, S100CardClass, S100CardContact, S100CardDescriptor, S100ContactRole,
    S100Signal,
};
use crate::s100_backplane::{S100BusSample, S100CardDrive, S100ElectricalCard};

const CPU_CONTACTS: &[S100CardContact] = &[
    S100CardContact::new(S100Signal::Plus8V, S100ContactRole::Power),
    S100CardContact::new(S100Signal::Plus16V, S100ContactRole::Power),
    S100CardContact::new(S100Signal::Minus16V, S100ContactRole::Power),
    S100CardContact::new(S100Signal::Ground, S100ContactRole::Power),
    S100CardContact::new(S100Signal::ExternalReady, S100ContactRole::Input),
    S100CardContact::new(S100Signal::StatusDisable, S100ContactRole::Input),
    S100CardContact::new(
        S100Signal::CommandControlDisable,
        S100ContactRole::Input,
    ),
    S100CardContact::new(S100Signal::AddressDisable, S100ContactRole::Input),
    S100CardContact::new(S100Signal::DataOutDisable, S100ContactRole::Input),
    S100CardContact::new(S100Signal::Ready, S100ContactRole::Input),
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
    S100CardContact::new(S100Signal::Address(0), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(1), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(2), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(3), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(4), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(5), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(6), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(7), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(8), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(9), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(10), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(11), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(12), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(13), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(14), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::Address(15), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(0), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(1), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(2), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(3), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(4), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(5), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(6), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::DataOut(7), S100ContactRole::TriStateOutput),
    S100CardContact::new(S100Signal::M1, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Out, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Inp, S100ContactRole::Output),
    S100CardContact::new(S100Signal::MemoryRead, S100ContactRole::Output),
    S100CardContact::new(S100Signal::HaltAcknowledge, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Sync, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Write, S100ContactRole::Output),
    S100CardContact::new(S100Signal::DataBusIn, S100ContactRole::Output),
    S100CardContact::new(
        S100Signal::InterruptAcknowledge,
        S100ContactRole::Output,
    ),
    S100CardContact::new(S100Signal::WriteStatus, S100ContactRole::Output),
    S100CardContact::new(S100Signal::Stack, S100ContactRole::Output),
];

/// Complete connector contract used by the live MITS CPU-card implementation.
///
/// The earlier `s100::MITS_8080_CPU` descriptor predates live electrical cards
/// and intentionally remains untouched while callers migrate.  New chassis code
/// must use this descriptor because it includes the address/data drivers and the
/// four original active-low bus-disable inputs.
pub static MITS_8080_CPU_BOARD: S100CardDescriptor = S100CardDescriptor {
    key: "mits-8080-cpu",
    label: "MITS 8080 CPU Board",
    class: S100CardClass::Cpu,
    historical: true,
    contacts: CPU_CONTACTS,
};

#[derive(Clone, Copy, Debug)]
struct Mits8080CpuBoardState {
    pins: Cpu8080Pins,
    inputs: Cpu8080Inputs,
    status_word: u8,
    cloc: bool,
    status_disabled: bool,
    command_disabled: bool,
    address_disabled: bool,
    data_out_disabled: bool,
    /// Connector outputs are physical state, not a temporary rendering. Keep
    /// them latched and mutate only nets affected by a package/input transition.
    cached_drive: S100CardDrive,
}

impl Default for Mits8080CpuBoardState {
    fn default() -> Self {
        let mut state = Self {
            pins: Cpu8080Pins::default(),
            inputs: Cpu8080Inputs::default(),
            status_word: 0,
            cloc: false,
            status_disabled: false,
            command_disabled: false,
            address_disabled: false,
            data_out_disabled: false,
            cached_drive: S100CardDrive::new(),
        };
        state.rebuild_drive();
        state
    }
}

impl Mits8080CpuBoardState {
    #[inline]
    fn status_bit(word: u8, mask: u8) -> bool {
        word & mask != 0
    }

    fn drive_address(&mut self) {
        for bit in 0..16 {
            let level = if self.address_disabled {
                None
            } else {
                self.pins
                    .address
                    .map(|address| address & (1u16 << bit) != 0)
            };
            self.cached_drive
                .drive_tristate(S100Signal::Address(bit), level);
        }
    }

    fn drive_data_out(&mut self) {
        for bit in 0..8 {
            let level = if self.data_out_disabled {
                None
            } else {
                self.pins
                    .data_out
                    .map(|value| value & (1u8 << bit) != 0)
            };
            self.cached_drive
                .drive_tristate(S100Signal::DataOut(bit), level);
        }
    }

    fn drive_command_group(&mut self) {
        for (signal, level) in [
            (S100Signal::HoldAcknowledge, self.pins.hlda),
            (S100Signal::Sync, self.pins.sync),
            (S100Signal::Write, self.pins.wr_n),
            (S100Signal::DataBusIn, self.pins.dbin),
        ] {
            self.cached_drive.drive_tristate(
                signal,
                (!self.command_disabled).then_some(level),
            );
        }
    }

    fn drive_status_group(&mut self) {
        let status = self.status_word;
        for (signal, mask) in [
            (S100Signal::MemoryRead, 0x80),
            (S100Signal::Inp, 0x40),
            (S100Signal::M1, 0x20),
            (S100Signal::Out, 0x10),
            (S100Signal::HaltAcknowledge, 0x08),
            (S100Signal::Stack, 0x04),
            (S100Signal::WriteStatus, 0x02),
            (S100Signal::InterruptAcknowledge, 0x01),
        ] {
            self.cached_drive.drive_tristate(
                signal,
                (!self.status_disabled).then_some(Self::status_bit(status, mask)),
            );
        }
    }

    fn rebuild_drive(&mut self) {
        self.cached_drive = S100CardDrive::new();
        self.cached_drive
            .drive_signal(S100Signal::Phi1, self.pins.phi1);
        self.cached_drive
            .drive_signal(S100Signal::Phi2, self.pins.phi2);
        self.cached_drive
            .drive_signal(S100Signal::Clock, self.cloc);
        self.cached_drive
            .drive_signal(S100Signal::Wait, self.pins.wait);
        self.cached_drive
            .drive_signal(S100Signal::InterruptEnable, self.pins.inte);
        self.drive_command_group();
        self.drive_address();
        self.drive_data_out();
        self.drive_status_group();
    }

    fn set_package_pins(&mut self, pins: Cpu8080Pins) {
        let old_pins = self.pins;
        let old_status = self.status_word;
        let old_cloc = self.cloc;
        self.pins = pins;

        // The buffered 2 MHz CLOC follows the board oscillator. At our digital
        // boundary PHI1 rising is its high transition and PHI2 rising its low
        // transition; dead time retains the preceding oscillator level.
        if pins.phi1 {
            self.cloc = true;
        } else if pins.phi2 {
            self.cloc = false;
        }

        // The 8212 belongs to the CPU board, not the backplane. It clocks from
        // the package-side SYNC+PHI1 event, so latch it here before this edge is
        // propagated onto S-100 rather than forcing a second whole-bus delta.
        if pins.phi1 && pins.sync {
            if let Some(word) = pins.data_out {
                self.status_word = word;
            }
        }

        if pins.phi1 != old_pins.phi1 {
            self.cached_drive.drive_signal(S100Signal::Phi1, pins.phi1);
        }
        if pins.phi2 != old_pins.phi2 {
            self.cached_drive.drive_signal(S100Signal::Phi2, pins.phi2);
        }
        if self.cloc != old_cloc {
            self.cached_drive.drive_signal(S100Signal::Clock, self.cloc);
        }
        if pins.wait != old_pins.wait {
            self.cached_drive.drive_signal(S100Signal::Wait, pins.wait);
        }
        if pins.inte != old_pins.inte {
            self.cached_drive
                .drive_signal(S100Signal::InterruptEnable, pins.inte);
        }
        if pins.hlda != old_pins.hlda
            || pins.sync != old_pins.sync
            || pins.wr_n != old_pins.wr_n
            || pins.dbin != old_pins.dbin
        {
            self.drive_command_group();
        }
        if pins.address != old_pins.address {
            self.drive_address();
        }
        if pins.data_out != old_pins.data_out {
            self.drive_data_out();
        }
        if self.status_word != old_status {
            self.drive_status_group();
        }
    }
}

/// Host-side handle to the Intel-package side of the physical CPU card.
///
/// This handle is deliberately not a bus API. Execution engines may update the
/// 8080 package pins and sample the package inputs; other S-100 cards never see
/// this handle and can communicate with the CPU only through the backplane.
#[derive(Clone)]
pub struct Mits8080CpuBoardHandle {
    state: Rc<RefCell<Mits8080CpuBoardState>>,
}

impl Mits8080CpuBoardHandle {
    pub fn set_package_pins(&self, pins: Cpu8080Pins) {
        self.state.borrow_mut().set_package_pins(pins);
    }

    /// Update the Intel-package side and return the CPU board's resulting
    /// connector drive in the same borrow. `S100RuntimeFabric` uses this to push
    /// the already-computed physical connector state into the backplane cache
    /// instead of immediately re-entering the card through a virtual drive call.
    /// No other card is visible through this handle.
    pub(crate) fn set_package_pins_and_connector_drive(
        &self,
        pins: Cpu8080Pins,
    ) -> S100CardDrive {
        let mut state = self.state.borrow_mut();
        state.set_package_pins(pins);
        state.cached_drive
    }

    pub fn package_pins(&self) -> Cpu8080Pins {
        self.state.borrow().pins
    }

    pub fn package_inputs(&self) -> Cpu8080Inputs {
        self.state.borrow().inputs
    }

    pub fn latched_status_word(&self) -> u8 {
        self.state.borrow().status_word
    }
}

pub struct Mits8080CpuBoard {
    state: Rc<RefCell<Mits8080CpuBoardState>>,
}

impl Mits8080CpuBoard {
    pub fn new() -> (Self, Mits8080CpuBoardHandle) {
        let state = Rc::new(RefCell::new(Mits8080CpuBoardState::default()));
        (
            Self {
                state: Rc::clone(&state),
            },
            Mits8080CpuBoardHandle { state },
        )
    }
}

impl Default for Mits8080CpuBoard {
    fn default() -> Self {
        Self::new().0
    }
}

impl S100Card for Mits8080CpuBoard {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        &MITS_8080_CPU_BOARD
    }
}

impl S100ElectricalCard for Mits8080CpuBoard {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        let mut state = self.state.borrow_mut();

        // pRDY and XRDY are both active-high readiness inputs to the processor
        // board. The backplane supplies their normal released-high bias.
        let prdy = sample.signal_level(S100Signal::Ready).unwrap_or(true);
        let xrdy = sample
            .signal_level(S100Signal::ExternalReady)
            .unwrap_or(true);
        state.inputs.ready = prdy && xrdy;

        // Original PINT is active-low at the connector. HOLD and RESET are
        // consumed as asserted-high processor inputs at this board boundary.
        state.inputs.interrupt =
            sample.signal_level(S100Signal::InterruptRequest) == Some(false);
        state.inputs.hold = sample.signal_level(S100Signal::Hold) == Some(true);
        state.inputs.reset = sample.signal_level(S100Signal::Reset) == Some(true);
        state.inputs.data_in = sample.data_in().unwrap_or(0xff);

        // Only these four backplane inputs can change what the CPU card drives.
        // Normal DI/READY/PINT/HOLD/RESET observations therefore do not rebuild
        // an identical 100-contact drive after every resolver delta.
        let status_disabled =
            sample.signal_level(S100Signal::StatusDisable) == Some(false);
        let command_disabled =
            sample.signal_level(S100Signal::CommandControlDisable) == Some(false);
        let address_disabled =
            sample.signal_level(S100Signal::AddressDisable) == Some(false);
        let data_out_disabled =
            sample.signal_level(S100Signal::DataOutDisable) == Some(false);
        let drive_controls_changed = status_disabled != state.status_disabled
            || command_disabled != state.command_disabled
            || address_disabled != state.address_disabled
            || data_out_disabled != state.data_out_disabled;
        state.status_disabled = status_disabled;
        state.command_disabled = command_disabled;
        state.address_disabled = address_disabled;
        state.data_out_disabled = data_out_disabled;
        if drive_controls_changed {
            state.rebuild_drive();
        }
    }

    #[inline]
    fn drive_s100(&self) -> S100CardDrive {
        self.state.borrow().cached_drive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100_backplane::S100Backplane;

    const SOURCE_CONTACTS: &[S100CardContact] = &[
        S100CardContact::new(S100Signal::ExternalReady, S100ContactRole::OpenCollectorOutput),
        S100CardContact::new(S100Signal::Ready, S100ContactRole::OpenCollectorOutput),
        S100CardContact::new(
            S100Signal::InterruptRequest,
            S100ContactRole::OpenCollectorOutput,
        ),
        S100CardContact::new(S100Signal::Hold, S100ContactRole::Output),
        S100CardContact::new(S100Signal::Reset, S100ContactRole::Output),
        S100CardContact::new(S100Signal::StatusDisable, S100ContactRole::Output),
        S100CardContact::new(
            S100Signal::CommandControlDisable,
            S100ContactRole::Output,
        ),
        S100CardContact::new(S100Signal::AddressDisable, S100ContactRole::Output),
        S100CardContact::new(S100Signal::DataOutDisable, S100ContactRole::Output),
        S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(1), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(2), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(3), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(4), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(5), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(6), S100ContactRole::TriStateOutput),
        S100CardContact::new(S100Signal::DataIn(7), S100ContactRole::TriStateOutput),
    ];

    static SOURCE_DESCRIPTOR: S100CardDescriptor = S100CardDescriptor {
        key: "test-s100-source",
        label: "test S-100 source",
        class: S100CardClass::Compatibility,
        historical: false,
        contacts: SOURCE_CONTACTS,
    };

    struct SourceCard {
        drive: S100CardDrive,
    }

    impl S100Card for SourceCard {
        fn s100_descriptor(&self) -> &'static S100CardDescriptor {
            &SOURCE_DESCRIPTOR
        }
    }

    impl S100ElectricalCard for SourceCard {
        fn drive_s100(&self) -> S100CardDrive {
            self.drive
        }
    }

    fn role(signal: S100Signal) -> Option<S100ContactRole> {
        MITS_8080_CPU_BOARD
            .contacts
            .iter()
            .find(|contact| contact.signal == signal)
            .map(|contact| contact.role)
    }

    #[test]
    fn live_cpu_descriptor_declares_the_real_bus_master_paths() {
        assert_eq!(
            role(S100Signal::Address(15)),
            Some(S100ContactRole::TriStateOutput)
        );
        assert_eq!(
            role(S100Signal::DataOut(7)),
            Some(S100ContactRole::TriStateOutput)
        );
        assert_eq!(role(S100Signal::DataIn(7)), Some(S100ContactRole::Input));
        assert_eq!(
            role(S100Signal::AddressDisable),
            Some(S100ContactRole::Input)
        );
        assert_eq!(
            role(S100Signal::DataOutDisable),
            Some(S100ContactRole::Input)
        );
        assert_eq!(
            role(S100Signal::StatusDisable),
            Some(S100ContactRole::Input)
        );
        assert_eq!(
            role(S100Signal::CommandControlDisable),
            Some(S100ContactRole::Input)
        );
    }

    #[test]
    fn cpu_board_drives_address_data_status_and_clock_only_through_its_slot() {
        let (card, handle) = Mits8080CpuBoard::new();
        let mut backplane = S100Backplane::new(4);
        backplane.insert(1, Box::new(card)).unwrap();

        handle.set_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(0x5aa5),
            data_out: Some(0xa2),
            sync: true,
            dbin: false,
            wr_n: true,
            inte: true,
            wait: false,
            hlda: false,
        });
        backplane.step().unwrap();

        let sample = backplane.sample();
        assert_eq!(sample.address(), Some(0x5aa5));
        assert_eq!(sample.data_out(), Some(0xa2));
        assert_eq!(sample.signal_level(S100Signal::Phi1), Some(true));
        assert_eq!(sample.signal_level(S100Signal::Phi2), Some(false));
        assert_eq!(sample.signal_level(S100Signal::Clock), Some(true));
        assert_eq!(sample.signal_level(S100Signal::MemoryRead), Some(true));
        assert_eq!(sample.signal_level(S100Signal::M1), Some(true));
        assert_eq!(sample.signal_level(S100Signal::WriteStatus), Some(true));
        assert_eq!(handle.latched_status_word(), 0xa2);
    }

    #[test]
    fn cpu_package_inputs_are_sampled_from_resolved_backplane_nets() {
        let (card, handle) = Mits8080CpuBoard::new();
        let mut source = S100CardDrive::new();
        source.drive_data_in(0x5a);
        source.pull_low(S100Signal::Ready, true);
        source.pull_low(S100Signal::ExternalReady, false);
        source.pull_low(S100Signal::InterruptRequest, true);
        source.drive_signal(S100Signal::Hold, true);
        source.drive_signal(S100Signal::Reset, true);

        let mut backplane = S100Backplane::new(4);
        backplane.insert(1, Box::new(card)).unwrap();
        backplane
            .insert(2, Box::new(SourceCard { drive: source }))
            .unwrap();

        // First propagation places the external source on the bus; the second
        // lets the CPU board consume that already-resolved electrical state.
        backplane.step().unwrap();
        backplane.step().unwrap();

        assert_eq!(
            handle.package_inputs(),
            Cpu8080Inputs {
                data_in: 0x5a,
                ready: false,
                interrupt: true,
                hold: true,
                reset: true,
            }
        );
    }

    #[test]
    fn active_low_bus_disable_inputs_release_cpu_owned_nets() {
        let (card, handle) = Mits8080CpuBoard::new();
        handle.set_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(0x1234),
            data_out: Some(0xa2),
            sync: true,
            dbin: true,
            wr_n: false,
            inte: true,
            wait: false,
            hlda: false,
        });

        let mut source = S100CardDrive::new();
        source.drive_signal(S100Signal::StatusDisable, false);
        source.drive_signal(S100Signal::CommandControlDisable, false);
        source.drive_signal(S100Signal::AddressDisable, false);
        source.drive_signal(S100Signal::DataOutDisable, false);

        let mut backplane = S100Backplane::new(4);
        backplane.insert(1, Box::new(card)).unwrap();
        backplane
            .insert(2, Box::new(SourceCard { drive: source }))
            .unwrap();
        backplane.step().unwrap();
        backplane.step().unwrap();

        let sample = backplane.sample();
        assert_eq!(sample.address(), None);
        assert_eq!(sample.data_out(), None);
        assert_eq!(sample.signal_level(S100Signal::MemoryRead), None);
        assert_eq!(sample.signal_level(S100Signal::Sync), None);
        assert_eq!(sample.signal_level(S100Signal::Write), None);
        assert_eq!(sample.signal_level(S100Signal::DataBusIn), None);
        // Clock, WAIT and INTE are not among the four disabled bus groups.
        assert_eq!(sample.signal_level(S100Signal::Phi1), Some(true));
        assert_eq!(sample.signal_level(S100Signal::InterruptEnable), Some(true));
    }
}
