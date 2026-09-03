//! Electrical S-100 backplane resolver.
//!
//! This module deliberately knows nothing about CPU, RAM, serial or any other
//! card family. Cards may only observe the resolved 100-contact bus and return
//! what they electrically drive on their declared connector contacts.

use crate::s100::{S100Card, S100CardDescriptor, S100ContactRole, S100Signal};

pub const S100_CONTACT_COUNT: usize = 100;
const PIN_COUNT_WITH_ZERO: usize = S100_CONTACT_COUNT + 1;
const PIN_MASK_WORDS: usize = 2;
pub type S100SlotMask = u32;

type PinMask = [u64; PIN_MASK_WORDS];

// The indexed S-100 nets are intentionally mapped once here for the two hottest
// operations in the emulator. This is the same physical pin mapping returned by
// `S100Signal::pin`; avoiding sixteen/eight enum matches for every CPU delta is
// purely an implementation acceleration.
const ADDRESS_PINS: [u8; 16] = [79, 80, 81, 31, 30, 29, 82, 83, 84, 34, 37, 87, 33, 85, 86, 32];
const DATA_OUT_PINS: [u8; 8] = [36, 35, 88, 89, 38, 39, 40, 90];
const DATA_IN_PINS: [u8; 8] = [95, 94, 41, 42, 91, 92, 93, 43];

#[inline]
fn mask_contains(mask: &PinMask, pin: usize) -> bool {
    mask[pin / 64] & (1u64 << (pin % 64)) != 0
}

#[inline]
fn mask_set(mask: &mut PinMask, pin: usize, set: bool) {
    let word = pin / 64;
    let bit = 1u64 << (pin % 64);
    if set {
        mask[word] |= bit;
    } else {
        mask[word] &= !bit;
    }
}

/// One bit per physical connector. The Altair chassis models currently top out
/// at 18 fitted connectors, so a u32 leaves headroom without allocating on the
/// hot path. This is an execution index for already-decoded hardware, not a
/// software-visible card number or CPU shortcut.
pub const fn s100_slot_mask(slot: usize) -> S100SlotMask {
    if slot == 0 || slot > S100SlotMask::BITS as usize {
        0
    } else {
        1u32 << (slot - 1)
    }
}

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

/// Compact electrical drive set for one card.
///
/// Earlier versions stored a 101-element enum array plus an active bitmap. The
/// exact same four electrical states fit in three bitsets: strong LOW, strong
/// HIGH and the open-collector subset of LOW. This matters because every exact
/// 8080 PHI edge resolves these objects several times.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct S100CardDrive {
    low: PinMask,
    high: PinMask,
    open_collector_low: PinMask,
}

impl Default for S100CardDrive {
    fn default() -> Self {
        Self {
            low: [0; PIN_MASK_WORDS],
            high: [0; PIN_MASK_WORDS],
            open_collector_low: [0; PIN_MASK_WORDS],
        }
    }
}

impl S100CardDrive {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn drive_at(&self, pin: usize) -> S100PinDrive {
        if mask_contains(&self.open_collector_low, pin) {
            S100PinDrive::OpenCollectorLow
        } else if mask_contains(&self.high, pin) {
            S100PinDrive::Driven(true)
        } else if mask_contains(&self.low, pin) {
            S100PinDrive::Driven(false)
        } else {
            S100PinDrive::HighZ
        }
    }

    pub fn pin(&self, pin: u8) -> Option<S100PinDrive> {
        let pin = pin as usize;
        (pin <= S100_CONTACT_COUNT).then(|| self.drive_at(pin))
    }

    #[inline]
    fn set_pin(&mut self, pin: u8, drive: S100PinDrive) {
        let pin = pin as usize;
        debug_assert!(pin <= S100_CONTACT_COUNT);
        mask_set(&mut self.low, pin, false);
        mask_set(&mut self.high, pin, false);
        mask_set(&mut self.open_collector_low, pin, false);
        match drive {
            S100PinDrive::HighZ => {}
            S100PinDrive::Driven(false) => mask_set(&mut self.low, pin, true),
            S100PinDrive::Driven(true) => mask_set(&mut self.high, pin, true),
            S100PinDrive::OpenCollectorLow => {
                mask_set(&mut self.low, pin, true);
                mask_set(&mut self.open_collector_low, pin, true);
            }
        }
    }

    #[inline]
    fn active_word(&self, word: usize) -> u64 {
        self.low[word] | self.high[word]
    }

    #[inline]
    fn strong_word(&self, word: usize) -> u64 {
        self.active_word(word) & !self.open_collector_low[word]
    }

    fn for_each_active_pin(&self, mut visit: impl FnMut(usize)) {
        for word_index in 0..PIN_MASK_WORDS {
            let mut bits = self.active_word(word_index);
            while bits != 0 {
                let offset = bits.trailing_zeros() as usize;
                let pin = word_index * 64 + offset;
                if (1..=S100_CONTACT_COUNT).contains(&pin) {
                    visit(pin);
                }
                bits &= bits - 1;
            }
        }
    }

    pub fn drive_signal(&mut self, signal: S100Signal, high: bool) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.set_pin(pin, S100PinDrive::Driven(high));
    }

    pub fn drive_tristate(&mut self, signal: S100Signal, level: Option<bool>) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.set_pin(
            pin,
            level.map_or(S100PinDrive::HighZ, S100PinDrive::Driven),
        );
    }

    pub fn pull_low(&mut self, signal: S100Signal, asserted: bool) {
        let pin = signal.pin().expect("valid S-100 signal");
        self.set_pin(
            pin,
            if asserted {
                S100PinDrive::OpenCollectorLow
            } else {
                S100PinDrive::HighZ
            },
        );
    }

    pub fn drive_address(&mut self, address: u16) {
        for (bit, pin) in ADDRESS_PINS.iter().copied().enumerate() {
            self.set_pin(pin, S100PinDrive::Driven(address & (1u16 << bit) != 0));
        }
    }

    pub fn drive_data_out(&mut self, value: u8) {
        for (bit, pin) in DATA_OUT_PINS.iter().copied().enumerate() {
            self.set_pin(pin, S100PinDrive::Driven(value & (1u8 << bit) != 0));
        }
    }

    pub fn drive_data_in(&mut self, value: u8) {
        for (bit, pin) in DATA_IN_PINS.iter().copied().enumerate() {
            self.set_pin(pin, S100PinDrive::Driven(value & (1u8 << bit) != 0));
        }
    }
}

/// Compact resolved backplane sample.
///
/// Public callers still observe `S100ResolvedPin`; internally level/contention
/// are bitsets and only the driver counts remain byte arrays. Eighteen physical
/// slots plus chassis wiring fit safely in u8 while the public counters retain
/// their original u16 type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S100BusSample {
    defined: PinMask,
    high: PinMask,
    contention: PinMask,
    low_drivers: [u8; PIN_COUNT_WITH_ZERO],
    high_drivers: [u8; PIN_COUNT_WITH_ZERO],
}

impl Default for S100BusSample {
    fn default() -> Self {
        Self {
            defined: [0; PIN_MASK_WORDS],
            high: [0; PIN_MASK_WORDS],
            contention: [0; PIN_MASK_WORDS],
            low_drivers: [0; PIN_COUNT_WITH_ZERO],
            high_drivers: [0; PIN_COUNT_WITH_ZERO],
        }
    }
}

impl S100BusSample {
    #[inline]
    fn set_level(&mut self, pin: usize, level: Option<bool>, contention: bool) {
        mask_set(&mut self.contention, pin, contention);
        match level {
            Some(high) if !contention => {
                mask_set(&mut self.defined, pin, true);
                mask_set(&mut self.high, pin, high);
            }
            _ => {
                mask_set(&mut self.defined, pin, false);
                mask_set(&mut self.high, pin, false);
            }
        }
    }

    #[inline]
    fn refresh_driven_level(&mut self, pin: usize) {
        let low = self.low_drivers[pin] != 0;
        let high = self.high_drivers[pin] != 0;
        let contention = low && high;
        let level = if contention {
            None
        } else if low {
            Some(false)
        } else if high {
            Some(true)
        } else {
            None
        };
        self.set_level(pin, level, contention);
    }

    fn set_resolved_pin(&mut self, pin: usize, resolved: S100ResolvedPin) {
        self.low_drivers[pin] = resolved.low_drivers.min(u8::MAX as u16) as u8;
        self.high_drivers[pin] = resolved.high_drivers.min(u8::MAX as u16) as u8;
        self.set_level(pin, resolved.level, resolved.contention);
    }

    pub fn pin(&self, pin: u8) -> Option<S100ResolvedPin> {
        let pin = pin as usize;
        if pin > S100_CONTACT_COUNT {
            return None;
        }
        let contention = mask_contains(&self.contention, pin);
        let level = if contention || !mask_contains(&self.defined, pin) {
            None
        } else {
            Some(mask_contains(&self.high, pin))
        };
        Some(S100ResolvedPin {
            level,
            contention,
            low_drivers: u16::from(self.low_drivers[pin]),
            high_drivers: u16::from(self.high_drivers[pin]),
        })
    }

    pub fn signal(&self, signal: S100Signal) -> S100ResolvedPin {
        signal
            .pin()
            .and_then(|pin| self.pin(pin))
            .unwrap_or_default()
    }

    pub fn signal_level(&self, signal: S100Signal) -> Option<bool> {
        let pin = signal.pin()? as usize;
        if mask_contains(&self.contention, pin) || !mask_contains(&self.defined, pin) {
            None
        } else {
            Some(mask_contains(&self.high, pin))
        }
    }

    pub fn signal_is_contended(&self, signal: S100Signal) -> bool {
        signal
            .pin()
            .is_some_and(|pin| mask_contains(&self.contention, pin as usize))
    }

    #[inline]
    fn pin_level(&self, pin: u8) -> Option<bool> {
        let pin = pin as usize;
        if mask_contains(&self.contention, pin) || !mask_contains(&self.defined, pin) {
            None
        } else {
            Some(mask_contains(&self.high, pin))
        }
    }

    fn resolved_pin_bits(&self, pins: &[u8]) -> Option<u16> {
        let mut value = 0u16;
        for (bit, pin) in pins.iter().copied().enumerate() {
            if self.pin_level(pin)? {
                value |= 1u16 << bit;
            }
        }
        Some(value)
    }

    pub fn address(&self) -> Option<u16> {
        self.resolved_pin_bits(&ADDRESS_PINS)
    }

    pub fn data_out(&self) -> Option<u8> {
        self.resolved_pin_bits(&DATA_OUT_PINS).map(|value| value as u8)
    }

    pub fn data_in(&self) -> Option<u8> {
        self.resolved_pin_bits(&DATA_IN_PINS).map(|value| value as u8)
    }

    pub fn data_in_or(&self, open_bus: u8) -> u8 {
        self.data_in().unwrap_or(open_bus)
    }

    pub fn contended_pins(&self) -> impl Iterator<Item = u8> + '_ {
        (1..=S100_CONTACT_COUNT as u8)
            .filter(|&pin| mask_contains(&self.contention, pin as usize))
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
    strong_outputs: PinMask,
    open_collector_outputs: PinMask,
}

impl S100Slot {
    fn new(number: usize) -> Self {
        Self {
            number,
            card: None,
            strong_outputs: [0; PIN_MASK_WORDS],
            open_collector_outputs: [0; PIN_MASK_WORDS],
        }
    }

    fn allow(mask: &mut PinMask, pin: u8) {
        mask_set(mask, pin as usize, true);
    }

    fn set_drive_contract(&mut self, descriptor: &'static S100CardDescriptor) {
        self.strong_outputs = [0; PIN_MASK_WORDS];
        self.open_collector_outputs = [0; PIN_MASK_WORDS];
        for contact in descriptor.contacts {
            let Some(pin) = contact.pin() else {
                continue;
            };
            match contact.role {
                S100ContactRole::Output | S100ContactRole::TriStateOutput => {
                    Self::allow(&mut self.strong_outputs, pin);
                }
                S100ContactRole::OpenCollectorOutput => {
                    Self::allow(&mut self.open_collector_outputs, pin);
                }
                S100ContactRole::Input | S100ContactRole::Power => {}
            }
        }
    }

    fn clear_drive_contract(&mut self) {
        self.strong_outputs = [0; PIN_MASK_WORDS];
        self.open_collector_outputs = [0; PIN_MASK_WORDS];
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
    /// Pre-biased no-driver sample. Each resolve starts by copying this fixed
    /// electrical baseline and then touches only pins that somebody drives.
    passive_sample: S100BusSample,
    sample: S100BusSample,
}

impl S100Backplane {
    pub fn new(slot_count: usize) -> Self {
        let mut backplane = Self {
            slots: (0..slot_count)
                .map(|index| S100Slot::new(index + 1))
                .collect(),
            passive_sample: S100BusSample::default(),
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
        backplane.sample.clone_from(&backplane.passive_sample);
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
        let pin = signal.pin().expect("valid S-100 signal") as usize;
        self.passive_sample.set_resolved_pin(
            pin,
            S100ResolvedPin {
                level,
                contention: false,
                low_drivers: 0,
                high_drivers: 0,
            },
        );
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
        target.set_drive_contract(card.s100_descriptor());
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
        let card = target.card.take();
        target.clear_drive_contract();
        Ok(card)
    }

    #[inline]
    fn apply_drive(sample: &mut S100BusSample, drive: &S100CardDrive) {
        for word_index in 0..PIN_MASK_WORDS {
            let mut lows = drive.low[word_index];
            while lows != 0 {
                let offset = lows.trailing_zeros() as usize;
                let pin = word_index * 64 + offset;
                if (1..=S100_CONTACT_COUNT).contains(&pin) {
                    sample.low_drivers[pin] = sample.low_drivers[pin].saturating_add(1);
                    sample.refresh_driven_level(pin);
                }
                lows &= lows - 1;
            }

            let mut highs = drive.high[word_index];
            while highs != 0 {
                let offset = highs.trailing_zeros() as usize;
                let pin = word_index * 64 + offset;
                if (1..=S100_CONTACT_COUNT).contains(&pin) {
                    sample.high_drivers[pin] = sample.high_drivers[pin].saturating_add(1);
                    sample.refresh_driven_level(pin);
                }
                highs &= highs - 1;
            }
        }
    }

    fn resolve_against_passive(
        passive: &S100BusSample,
        drives: &[S100CardDrive],
    ) -> S100BusSample {
        let mut sample = passive.clone();
        for drive in drives {
            Self::apply_drive(&mut sample, drive);
        }
        sample
    }

    /// Resolve arbitrary drive sets against this backplane's passive biases.
    /// This contains no card-type switches and is also used for non-slot chassis
    /// wiring such as the Display/Control board connector.
    pub fn resolve_drive_sets(&self, drives: &[S100CardDrive]) -> S100BusSample {
        Self::resolve_against_passive(&self.passive_sample, drives)
    }

    /// Let every slotted card observe the currently resolved electrical sample
    /// exactly once.
    pub fn observe_cards(&mut self) {
        self.observe_selected_cards(S100SlotMask::MAX);
    }

    /// Observe only cards whose already-compiled decode mask says they can be
    /// affected by this transaction. Real slots still see the wires in parallel;
    /// this mask merely avoids serially executing impossible responders in the
    /// software model.
    pub fn observe_selected_cards(&mut self, selected: S100SlotMask) {
        let observed = &self.sample;
        for slot in &mut self.slots {
            if selected & s100_slot_mask(slot.number) == 0 {
                continue;
            }
            if let Some(card) = slot.card.as_mut() {
                card.observe_s100(observed);
            }
        }
    }

    /// Resolve the current drives of every slotted card plus optional chassis
    /// wiring that is not itself an S-100 slot (for example Display/Control).
    pub fn resolve_current_drives(
        &mut self,
        chassis_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        self.resolve_selected_drives(S100SlotMask::MAX, chassis_drives)
    }

    /// Resolve only predecoded participating slots. The resolver itself remains
    /// card-family agnostic: it receives only a connector mask and still applies
    /// the same pin-role validation, tri-state, open-collector and contention
    /// rules to every selected electrical drive.
    pub fn resolve_selected_drives(
        &mut self,
        selected: S100SlotMask,
        chassis_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        self.sample.clone_from(&self.passive_sample);
        for slot in &self.slots {
            if selected & s100_slot_mask(slot.number) == 0 {
                continue;
            }
            let Some(card) = slot.card.as_ref() else {
                continue;
            };
            let drive = card.drive_s100();
            validate_card_drive(slot, &drive)?;
            Self::apply_drive(&mut self.sample, &drive);
        }
        for drive in chassis_drives {
            Self::apply_drive(&mut self.sample, drive);
        }
        Ok(&self.sample)
    }

    /// One digital propagation step retained for simple callers/tests: every
    /// card observes the previous sample once, then all drives are resolved.
    pub fn step(&mut self) -> Result<&S100BusSample, S100BackplaneError> {
        self.observe_cards();
        self.resolve_current_drives(&[])
    }
}

#[inline]
fn first_pin(word_index: usize, bits: u64) -> usize {
    word_index * 64 + bits.trailing_zeros() as usize
}

fn validate_card_drive(slot: &S100Slot, drive: &S100CardDrive) -> Result<(), S100BackplaneError> {
    // Valid built-in cards overwhelmingly take the no-error path. Checking the
    // declared connector contract a machine word at a time preserves the exact
    // runtime guard without walking every driven address/status/data pin.
    for word_index in 0..PIN_MASK_WORDS {
        let illegal_strong = drive.strong_word(word_index) & !slot.strong_outputs[word_index];
        if illegal_strong != 0 {
            let pin = first_pin(word_index, illegal_strong);
            return Err(S100BackplaneError::IllegalCardDrive {
                slot: slot.number,
                pin: pin as u8,
                drive: drive.drive_at(pin),
            });
        }

        let illegal_open =
            drive.open_collector_low[word_index] & !slot.open_collector_outputs[word_index];
        if illegal_open != 0 {
            let pin = first_pin(word_index, illegal_open);
            return Err(S100BackplaneError::IllegalCardDrive {
                slot: slot.number,
                pin: pin as u8,
                drive: S100PinDrive::OpenCollectorLow,
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
            self.drive
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
    fn compact_drive_preserves_all_four_electrical_states() {
        let mut drive = S100CardDrive::new();
        assert_eq!(drive.pin(72), Some(S100PinDrive::HighZ));
        drive.drive_signal(S100Signal::Ready, true);
        assert_eq!(drive.pin(72), Some(S100PinDrive::Driven(true)));
        drive.drive_signal(S100Signal::Ready, false);
        assert_eq!(drive.pin(72), Some(S100PinDrive::Driven(false)));
        drive.pull_low(S100Signal::Ready, true);
        assert_eq!(drive.pin(72), Some(S100PinDrive::OpenCollectorLow));
        drive.pull_low(S100Signal::Ready, false);
        assert_eq!(drive.pin(72), Some(S100PinDrive::HighZ));
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
        let pin = sample.signal(S100Signal::DataIn(0));
        assert_eq!(pin.low_drivers, 1);
        assert_eq!(pin.high_drivers, 1);
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

    #[test]
    fn predecoded_slot_mask_skips_impossible_responders_without_changing_resolution() {
        let mut backplane = S100Backplane::new(2);
        backplane.insert(1, Box::new(ready_card(true))).unwrap();
        backplane.insert(2, Box::new(ready_card(false))).unwrap();

        backplane
            .resolve_selected_drives(s100_slot_mask(2), &[])
            .unwrap();
        assert_eq!(backplane.sample().signal_level(S100Signal::Ready), Some(true));

        backplane
            .resolve_selected_drives(s100_slot_mask(1) | s100_slot_mask(2), &[])
            .unwrap();
        assert_eq!(backplane.sample().signal_level(S100Signal::Ready), Some(false));
    }
}