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
}

impl Default for Mits8080CpuBoardState {
    fn default() -> Self {
        Self {
            pins: Cpu8080Pins::default(),
            inputs: Cpu8080Inputs::default(),
            status_word: 0,
            cloc: false,
            status_disabled: false,
            command_disabled: false,
            address_disabled: false,
            data_out_disabled: false,
        }
    }
}

/// Host-side handle to the Intel-package side of the physical CPU card.
///
/// This handle is deliberately not a bus API.  Execution engines may update the
/// 8080 package pins and sample the package inputs; other S-100 cards never see
/// this handle and can communicate with the CPU only through the backplane.
#[derive(Clone)]
pub struct Mits8080CpuBoardHandle {
    state: Rc<RefCell<Mits8080CpuBoardState>>,
}

impl Mits8080CpuBoardHandle {
    pub fn set_package_pins(&self, pins: Cpu8080Pins) {
        let mut state = self.state.borrow_mut();
        state.pins = pins;
        // The buffered 2 MHz CLOC follows the board oscillator.  At the digital
        // boundary used by the existing exact core, PHI1 rising is its high
        // transition and PHI2 rising its low transition; dead time retains the
        // previous CLOC level.
        if pins.phi1 {
            state.cloc = true;
        } else if pins.phi2 {
            state.cloc = false;
        }
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

    fn status_bit(word: u8, mask: u8) -> bool {
        word & mask != 0
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
        // board.  The backplane supplies their normal released-high bias.
        let prdy = sample.signal_level(S100Signal::Ready).unwrap_or(true);
        let xrdy = sample
            .signal_level(S100Signal::ExternalReady)
            .unwrap_or(true);
        state.inputs.ready = prdy && xrdy;

        // Original PINT is active-low at the connector.  HOLD and RESET are
        // consumed as asserted-high processor inputs at this board boundary.
        state.inputs.interrupt =
            sample.signal_level(S100Signal::InterruptRequest) == Some(false);
        state.inputs.hold = sample.signal_level(S100Signal::Hold) == Some(true);
        state.inputs.reset = sample.signal_level(S100Signal::Reset) == Some(true);
        state.inputs.data_in = sample.data_in().unwrap_or(0xff);

        // The four bus-disable controls are active-low.  Floating means inactive
        // on an ordinary single-master Altair, which is also the useful default
        // before a front panel or DMA master is attached to the live backplane.
        state.status_disabled =
            sample.signal_level(S100Signal::StatusDisable) == Some(false);
        state.command_disabled =
            sample.signal_level(S100Signal::CommandControlDisable) == Some(false);
        state.address_disabled =
            sample.signal_level(S100Signal::AddressDisable) == Some(false);
        state.data_out_disabled =
            sample.signal_level(S100Signal::DataOutDisable) == Some(false);

        // The MITS board's 8212 latches the 8080 status byte at SYNC + PHI1.
        if state.pins.phi1 && state.pins.sync {
            if let Some(word) = state.pins.data_out {
                state.status_word = word;
            }
        }
    }

    fn drive_s100(&self) -> S100CardDrive {
        let state = self.state.borrow();
        let pins = state.pins;
        let mut drive = S100CardDrive::new();

        drive.drive_signal(S100Signal::Phi1, pins.phi1);
        drive.drive_signal(S100Signal::Phi2, pins.phi2);
        drive.drive_signal(S100Signal::Clock, state.cloc);
        drive.drive_signal(S100Signal::Wait, pins.wait);
        drive.drive_signal(S100Signal::InterruptEnable, pins.inte);

        if !state.command_disabled {
            drive.drive_signal(S100Signal::HoldAcknowledge, pins.hlda);
            drive.drive_signal(S100Signal::Sync, pins.sync);
            // S-100 pWR is the active-low 8080 /WR level itself.
            drive.drive_signal(S100Signal::Write, pins.wr_n);
            drive.drive_signal(S100Signal::DataBusIn, pins.dbin);
        }

        if !state.address_disabled {
            if let Some(address) = pins.address {
                drive.drive_address(address);
            }
        }

        if !state.data_out_disabled {
            if let Some(value) = pins.data_out {
                drive.drive_data_out(value);
            }
        }

        if !state.status_disabled {
            let status = state.status_word;
            drive.drive_signal(S100Signal::MemoryRead, Self::status_bit(status, 0x80));
            drive.drive_signal(S100Signal::Inp, Self::status_bit(status, 0x40));
            drive.drive_signal(S100Signal::M1, Self::status_bit(status, 0x20));
            drive.drive_signal(S100Signal::Out, Self::status_bit(status, 0x10));
            drive.drive_signal(
                S100Signal::HaltAcknowledge,
                Self::status_bit(status, 0x08),
            );
            drive.drive_signal(S100Signal::Stack, Self::status_bit(status, 0x04));
            drive.drive_signal(S100Signal::WriteStatus, Self::status_bit(status, 0x02));
            drive.drive_signal(
                S100Signal::InterruptAcknowledge,
                Self::status_bit(status, 0x01),
            );
        }

        drive
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
            self.drive.clone()
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
