//! Electrical S-100 backplane resolver.
//!
//! This module deliberately knows nothing about CPU, RAM, serial or any other
//! card family. Cards may only observe the resolved 100-contact bus and return
//! what they electrically drive on their declared connector contacts.

use crate::s100::{S100Card, S100CardDescriptor, S100ContactRole, S100Signal};

pub const S100_CONTACT_COUNT: usize = 100;
const PIN_COUNT_WITH_ZERO: usize = S100_CONTACT_COUNT + 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum S100PinDrive {
    HighZ,
    /// Ordinary TTL/totem-pole or enabled tri-state output.
    Driven(bool),
    /// Open-collector output actively sinking the net. Released OC outputs are
    /// represented by `HighZ`; they never drive HIGH.
    OpenCollectorLow,
}

impl Default for S100PinDrive {
    fn default() -> Self {
        Self::HighZ
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct S100ResolvedPin {
    /// Physical logic level after resolving drivers and passive bias. `None`
    /// means floating or electrically contended.
    pub level: Option<bool>,
    pub contention: bool,
    pub low_drivers: u16,
    pub high_drivers: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S100CardDrive {
    pins: [S100PinDrive; PIN_COUNT_WITH_ZERO],
}

impl Default for S100CardDrive {
    fn default() -> Self {
        Self {
            pins: [S100PinDrive::HighZ; PIN_COUNT_WITH_ZERO],
        }
    }
}

impl S100CardDrive {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pin(&self, pin: u8) -> Option<S100PinDrive> {
        self.pins.get(pin as usize).copied()
    }

    pub fn drive_signal(&mut self, signal: S100Signal, high: bool) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.pins[pin as usize] = S100PinDrive::Driven(high);
    }

    pub fn drive_tristate(&mut self, signal: S100Signal, level: Option<bool>) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.pins[pin as usize] = level.map_or(S100PinDrive::HighZ, S100PinDrive::Driven);
    }

    pub fn pull_low(&mut self, signal: S100Signal, asserted: bool) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.pins[pin as usize] = if asserted {
            S100PinDrive::OpenCollectorLow
        } else {
            S100PinDrive::HighZ
        };
    }

    pub fn drive_address(&mut self, address: u16) {
        for bit in 0..16 {
            self.drive_signal(S100Signal::Address(bit), address & (1 << bit) != 0);
        }
    }

    pub fn drive_data_out(&mut self, value: u8) {
        for bit in 0..8 {
            self.drive_signal(S100Signal::DataOut(bit), value & (1 << bit) != 0);
        }
    }

    pub fn drive_data_in(&mut self, value: u8) {
        for bit in 0..8 {
            self.drive_signal(S100Signal::DataIn(bit), value & (1 << bit) != 0);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S100BusSample {
    pins: [S100ResolvedPin; PIN_COUNT_WITH_ZERO],
}

impl Default for S100BusSample {
    fn default() -> Self {
        Self {
            pins: [S100ResolvedPin::default(); PIN_COUNT_WITH_ZERO],
        }
    }
}

impl S100BusSample {
    pub fn pin(&self, pin: u8) -> Option<S100ResolvedPin> {
        self.pins.get(pin as usize).copied()
    }

    pub fn signal(&self, signal: S100Signal) -> S100ResolvedPin {
        signal
            .pin()
            .and_then(|pin| self.pin(pin))
            .unwrap_or_default()
    }

    pub fn signal_level(&self, signal: S100Signal) -> Option<bool> {
        let pin = self.signal(signal);
        (!pin.contention).then_some(pin.level).flatten()
    }

    pub fn signal_is_contended(&self, signal: S100Signal) -> bool {
        self.signal(signal).contention
    }

    fn resolved_bits(&self, signal: impl Fn(u8) -> S100Signal, count: u8) -> Option<u16> {
        let mut value = 0u16;
        for bit in 0..count {
            let high = self.signal_level(signal(bit))?;
            if high {
                value |= 1u16 << bit;
            }
        }
        Some(value)
    }

    pub fn address(&self) -> Option<u16> {
        self.resolved_bits(S100Signal::Address, 16)
    }

    pub fn data_out(&self) -> Option<u8> {
        self.resolved_bits(S100Signal::DataOut, 8)
            .map(|value| value as u8)
    }

    pub fn data_in(&self) -> Option<u8> {
        self.resolved_bits(S100Signal::DataIn, 8)
            .map(|value| value as u8)
    }

    pub fn data_in_or(&self, open_bus: u8) -> u8 {
        self.data_in().unwrap_or(open_bus)
    }

    pub fn contended_pins(&self) -> impl Iterator<Item = u8> + '_ {
        (1..=S100_CONTACT_COUNT as u8).filter(|&pin| self.pins[pin as usize].contention)
    }
}

/// Stateful electrical card contract. This is intentionally separate from host
/// APIs such as debugger or serial endpoints: these two methods are the card's
/// only route to other S-100 hardware.
pub trait S100ElectricalCard: S100Card {
    /// Observe one already-resolved bus sample and update only internal card
    /// state. A card must not mutate another card or the backplane here.
    fn observe_s100(&mut self, _sample: &S100BusSample) {}

    /// Return the levels this card currently drives. Everything omitted remains
    /// high impedance/released.
    fn drive_s100(&self) -> S100CardDrive;
}

pub struct S100Slot {
    number: usize,
    card: Option<Box<dyn S100ElectricalCard>>,
}

impl S100Slot {
    fn new(number: usize) -> Self {
        Self { number, card: None }
    }

    pub fn number(&self) -> usize {
        self.number
    }

    pub fn is_empty(&self) -> bool {
        self.card.is_none()
    }

    pub fn descriptor(&self) -> Option<&'static S100CardDescriptor> {
        self.card.as_ref().map(|card| card.s100_descriptor())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum S100BackplaneError {
    InvalidSlot {
        slot: usize,
        slot_count: usize,
    },
    SlotOccupied {
        slot: usize,
    },
    /// The card attempted to drive a contact it did not declare as an output,
    /// or used an electrical drive incompatible with that declared role.
    IllegalCardDrive {
        slot: usize,
        pin: u8,
        drive: S100PinDrive,
    },
}

pub struct S100Backplane {
    slots: Vec<S100Slot>,
    passive_bias: [Option<bool>; PIN_COUNT_WITH_ZERO],
    sample: S100BusSample,
}

impl S100Backplane {
    pub fn new(slot_count: usize) -> Self {
        let mut backplane = Self {
            slots: (0..slot_count)
                .map(|index| S100Slot::new(index + 1))
                .collect(),
            passive_bias: [None; PIN_COUNT_WITH_ZERO],
            sample: S100BusSample::default(),
        };

        // Shared open-collector nets are released HIGH in the normal MITS bus.
        // This is passive bus bias, not ownership by any CPU/RAM/I/O card.
        for signal in [
            S100Signal::ExternalReady,
            S100Signal::Ready,
            S100Signal::InterruptRequest,
        ] {
            backplane.set_passive_bias(signal, Some(true));
        }
        for level in 0..8 {
            backplane.set_passive_bias(S100Signal::VectorInterrupt(level), Some(true));
        }
        backplane
    }

    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    pub fn slots(&self) -> &[S100Slot] {
        &self.slots
    }

    pub fn sample(&self) -> &S100BusSample {
        &self.sample
    }

    pub fn set_passive_bias(&mut self, signal: S100Signal, level: Option<bool>) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.passive_bias[pin as usize] = level;
    }

    pub fn insert(
        &mut self,
        slot: usize,
        card: Box<dyn S100ElectricalCard>,
    ) -> Result<(), S100BackplaneError> {
        let slot_count = self.slots.len();
        let target = self
            .slots
            .get_mut(slot.checked_sub(1).unwrap_or(usize::MAX))
            .ok_or(S100BackplaneError::InvalidSlot { slot, slot_count })?;
        if target.card.is_some() {
            return Err(S100BackplaneError::SlotOccupied { slot });
        }
        target.card = Some(card);
        Ok(())
    }

    pub fn remove(
        &mut self,
        slot: usize,
    ) -> Result<Option<Box<dyn S100ElectricalCard>>, S100BackplaneError> {
        let slot_count = self.slots.len();
        let target = self
            .slots
            .get_mut(slot.checked_sub(1).unwrap_or(usize::MAX))
            .ok_or(S100BackplaneError::InvalidSlot { slot, slot_count })?;
        Ok(target.card.take())
    }

    /// Resolve arbitrary drive sets against this backplane's passive biases.
    /// This contains no card-type switches and is also used for non-slot chassis
    /// wiring such as the Display/Control board connector.
    pub fn resolve_drive_sets(&self, drives: &[S100CardDrive]) -> S100BusSample {
        let mut sample = S100BusSample::default();
        for pin in 1..=S100_CONTACT_COUNT {
            let mut lows = 0u16;
            let mut highs = 0u16;
            for drive in drives {
                match drive.pins[pin] {
                    S100PinDrive::HighZ => {}
                    S100PinDrive::Driven(false) | S100PinDrive::OpenCollectorLow => {
                        lows = lows.saturating_add(1);
                    }
                    S100PinDrive::Driven(true) => {
                        highs = highs.saturating_add(1);
                    }
                }
            }

            let contention = lows != 0 && highs != 0;
            let level = if contention {
                None
            } else if lows != 0 {
                Some(false)
            } else if highs != 0 {
                Some(true)
            } else {
                self.passive_bias[pin]
            };
            sample.pins[pin] = S100ResolvedPin {
                level,
                contention,
                low_drivers: lows,
                high_drivers: highs,
            };
        }
        sample
    }

    /// Let every slotted card observe the currently resolved electrical sample
    /// exactly once. Separating observation from resolution is important for
    /// edge-triggered cards: a chassis may need a second combinational resolve
    /// after RAM/serial outputs change without replaying the same clock edge.
    pub fn observe_cards(&mut self) {
        let observed = self.sample.clone();
        for slot in &mut self.slots {
            if let Some(card) = slot.card.as_mut() {
                card.observe_s100(&observed);
            }
        }
    }

    /// Resolve the current drives of every slotted card plus optional chassis
    /// wiring that is not itself an S-100 slot (for example Display/Control).
    /// Slotted card drives are still checked against their connector contracts.
    pub fn resolve_current_drives(
        &mut self,
        chassis_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        let mut drives = Vec::with_capacity(self.slots.len() + chassis_drives.len());
        for slot in &self.slots {
            let Some(card) = slot.card.as_ref() else {
                continue;
            };
            let drive = card.drive_s100();
            validate_card_drive(slot.number, card.s100_descriptor(), &drive)?;
            drives.push(drive);
        }
        drives.extend(chassis_drives.iter().cloned());
        self.sample = self.resolve_drive_sets(&drives);
        Ok(&self.sample)
    }

    /// One digital propagation step retained for simple callers/tests: every
    /// card observes the previous sample once, then all drives are resolved.
    pub fn step(&mut self) -> Result<&S100BusSample, S100BackplaneError> {
        self.observe_cards();
        self.resolve_current_drives(&[])
    }
}

fn validate_card_drive(
    slot: usize,
    descriptor: &'static S100CardDescriptor,
    drive: &S100CardDrive,
) -> Result<(), S100BackplaneError> {
    for pin in 1..=S100_CONTACT_COUNT as u8 {
        let actual = drive.pins[pin as usize];
        if actual == S100PinDrive::HighZ {
            continue;
        }

        let role = descriptor
            .contacts
            .iter()
            .find(|contact| contact.pin() == Some(pin))
            .map(|contact| contact.role);

        let legal = match (role, actual) {
            (
                Some(S100ContactRole::Output | S100ContactRole::TriStateOutput),
                S100PinDrive::Driven(_),
            ) => true,
            (Some(S100ContactRole::OpenCollectorOutput), S100PinDrive::OpenCollectorLow) => true,
            _ => false,
        };
        if !legal {
            return Err(S100BackplaneError::IllegalCardDrive {
                slot,
                pin,
                drive: actual,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100::{S100CardClass, S100CardContact};

    const READY_OC: &[S100CardContact] = &[S100CardContact::new(
        S100Signal::Ready,
        S100ContactRole::OpenCollectorOutput,
    )];
    const DI0_TRI: &[S100CardContact] = &[S100CardContact::new(
        S100Signal::DataIn(0),
        S100ContactRole::TriStateOutput,
    )];
    const A0_INPUT: &[S100CardContact] = &[S100CardContact::new(
        S100Signal::Address(0),
        S100ContactRole::Input,
    )];

    static READY_CARD: S100CardDescriptor = S100CardDescriptor {
        key: "test-ready",
        label: "test READY card",
        class: S100CardClass::Compatibility,
        historical: false,
        contacts: READY_OC,
    };
    static DI_CARD: S100CardDescriptor = S100CardDescriptor {
        key: "test-di",
        label: "test DI card",
        class: S100CardClass::Compatibility,
        historical: false,
        contacts: DI0_TRI,
    };
    static INPUT_ONLY_CARD: S100CardDescriptor = S100CardDescriptor {
        key: "test-input-only",
        label: "test input-only card",
        class: S100CardClass::Compatibility,
        historical: false,
        contacts: A0_INPUT,
    };

    struct FixedCard {
        descriptor: &'static S100CardDescriptor,
        drive: S100CardDrive,
    }

    impl S100Card for FixedCard {
        fn s100_descriptor(&self) -> &'static S100CardDescriptor {
            self.descriptor
        }
    }

    impl S100ElectricalCard for FixedCard {
        fn drive_s100(&self) -> S100CardDrive {
            self.drive.clone()
        }
    }

    fn ready_card(pull_low: bool) -> FixedCard {
        let mut drive = S100CardDrive::new();
        drive.pull_low(S100Signal::Ready, pull_low);
        FixedCard {
            descriptor: &READY_CARD,
            drive,
        }
    }

    #[test]
    fn two_open_collectors_require_every_source_to_release_before_prdy_rises() {
        let mut backplane = S100Backplane::new(2);
        backplane.insert(1, Box::new(ready_card(true))).unwrap();
        backplane.insert(2, Box::new(ready_card(false))).unwrap();
        backplane.step().unwrap();
        assert_eq!(
            backplane.sample().signal_level(S100Signal::Ready),
            Some(false)
        );

        backplane.remove(1).unwrap();
        backplane.step().unwrap();
        assert_eq!(
            backplane.sample().signal_level(S100Signal::Ready),
            Some(true)
        );
    }

    #[test]
    fn one_tri_state_driver_places_its_value_on_data_in() {
        let backplane = S100Backplane::new(0);
        let mut drive = S100CardDrive::new();
        drive.drive_signal(S100Signal::DataIn(0), true);
        let sample = backplane.resolve_drive_sets(&[drive]);
        assert_eq!(sample.signal_level(S100Signal::DataIn(0)), Some(true));
    }

    #[test]
    fn conflicting_tri_state_drivers_report_contention_instead_of_last_writer_wins() {
        let backplane = S100Backplane::new(0);
        let mut high = S100CardDrive::new();
        high.drive_signal(S100Signal::DataIn(0), true);
        let mut low = S100CardDrive::new();
        low.drive_signal(S100Signal::DataIn(0), false);
        let sample = backplane.resolve_drive_sets(&[high, low]);
        assert!(sample.signal_is_contended(S100Signal::DataIn(0)));
        assert_eq!(sample.signal_level(S100Signal::DataIn(0)), None);
    }

    #[test]
    fn vector_interrupt_lines_resolve_independently() {
        let mut backplane = S100Backplane::new(0);
        let mut drive = S100CardDrive::new();
        drive.pull_low(S100Signal::VectorInterrupt(3), true);
        let sample = backplane.resolve_drive_sets(&[drive]);
        assert_eq!(
            sample.signal_level(S100Signal::VectorInterrupt(3)),
            Some(false)
        );
        assert_eq!(
            sample.signal_level(S100Signal::VectorInterrupt(4)),
            Some(true)
        );
        backplane.set_passive_bias(S100Signal::VectorInterrupt(4), None);
        let sample = backplane.resolve_drive_sets(&[]);
        assert_eq!(sample.signal_level(S100Signal::VectorInterrupt(4)), None);
    }

    #[test]
    fn slot_order_cannot_change_open_collector_resolution() {
        let mut a = S100Backplane::new(2);
        a.insert(1, Box::new(ready_card(true))).unwrap();
        a.insert(2, Box::new(ready_card(false))).unwrap();
        a.step().unwrap();

        let mut b = S100Backplane::new(2);
        b.insert(1, Box::new(ready_card(false))).unwrap();
        b.insert(2, Box::new(ready_card(true))).unwrap();
        b.step().unwrap();

        assert_eq!(a.sample(), b.sample());
    }

    #[test]
    fn card_cannot_drive_a_contact_declared_only_as_input() {
        let mut drive = S100CardDrive::new();
        drive.drive_signal(S100Signal::Address(0), true);
        let card = FixedCard {
            descriptor: &INPUT_ONLY_CARD,
            drive,
        };
        let mut backplane = S100Backplane::new(1);
        backplane.insert(1, Box::new(card)).unwrap();
        assert!(matches!(
            backplane.step(),
            Err(S100BackplaneError::IllegalCardDrive {
                slot: 1,
                pin: 79,
                ..
            })
        ));
    }

    #[test]
    fn descriptor_role_is_checked_for_tri_state_card_output() {
        let mut drive = S100CardDrive::new();
        drive.drive_signal(S100Signal::DataIn(0), true);
        let card = FixedCard {
            descriptor: &DI_CARD,
            drive,
        };
        let mut backplane = S100Backplane::new(1);
        backplane.insert(1, Box::new(card)).unwrap();
        backplane.step().unwrap();
        assert_eq!(
            backplane.sample().signal_level(S100Signal::DataIn(0)),
            Some(true)
        );
    }

    #[test]
    fn observation_can_be_followed_by_combinational_reresolve_without_replaying_edge() {
        let mut backplane = S100Backplane::new(1);
        backplane.insert(1, Box::new(ready_card(true))).unwrap();
        backplane.resolve_current_drives(&[]).unwrap();
        assert_eq!(backplane.sample().signal_level(S100Signal::Ready), Some(false));
        backplane.observe_cards();
        backplane.resolve_current_drives(&[]).unwrap();
        assert_eq!(backplane.sample().signal_level(S100Signal::Ready), Some(false));
    }
}
