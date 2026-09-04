//! Electrical S-100 backplane resolver.
//!
//! This module deliberately knows nothing about CPU, RAM, serial or any other
//! card family. Cards may only observe the resolved 100-contact bus and return
//! what they electrically drive on their declared connector contacts.

use crate::s100::{S100Card, S100CardDescriptor, S100ContactRole, S100Signal};

pub const S100_CONTACT_COUNT: usize = 100;
const PIN_MASK_WORDS: usize = 2;
const DRIVER_COUNT_BITS: usize = 8;
pub type S100SlotMask = u32;

type PinMask = [u64; PIN_MASK_WORDS];
type DriverCountPlanes = [PinMask; DRIVER_COUNT_BITS];

// The indexed S-100 nets are intentionally mapped once here for the hottest
// operations in the emulator. This is the same physical pin mapping returned by
// `S100Signal::pin`; avoiding sixteen/eight enum matches for every CPU delta is
// purely an implementation acceleration.
const ADDRESS_PINS: [u8; 16] = [79, 80, 81, 31, 30, 29, 82, 83, 84, 34, 37, 87, 33, 85, 86, 32];
const DATA_OUT_PINS: [u8; 8] = [36, 35, 88, 89, 38, 39, 40, 90];
const DATA_IN_PINS: [u8; 8] = [95, 94, 41, 42, 91, 92, 93, 43];
const ADDRESS_PIN_MASK: PinMask = [0x27e0000000, 0x00ff8000];
const DATA_OUT_PIN_MASK: PinMask = [0x1d800000000, 0x07000000];
const DATA_IN_PIN_MASK: PinMask = [0xe0000000000, 0xf8000000];

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

#[inline]
fn masks_intersect(a: &PinMask, b: &PinMask) -> bool {
    (a[0] & b[0]) != 0 || (a[1] & b[1]) != 0
}

#[inline]
fn add_driver_mask(planes: &mut DriverCountPlanes, word: usize, mask: u64) {
    let mut carry = mask;
    for plane in planes.iter_mut() {
        if carry == 0 {
            break;
        }
        let next_carry = plane[word] & carry;
        plane[word] ^= carry;
        carry = next_carry;
    }
    debug_assert_eq!(carry, 0, "S-100 driver count overflow");
}

/// Subtract one driver from every pin selected by `mask`. The bit-sliced binary
/// counter makes this the exact inverse of `add_driver_mask`, 64 contacts at a
/// time. Production therefore updates driver counts when a drive changes instead
/// of rebuilding all counts from every installed card on every digital delta.
#[inline]
fn sub_driver_mask(planes: &mut DriverCountPlanes, word: usize, mask: u64) {
    let mut borrow = mask;
    for plane in planes.iter_mut() {
        if borrow == 0 {
            break;
        }
        let next_borrow = (!plane[word]) & borrow;
        plane[word] ^= borrow;
        borrow = next_borrow;
    }
    debug_assert_eq!(borrow, 0, "S-100 driver count underflow");
}

#[inline]
fn driver_count(planes: &DriverCountPlanes, pin: usize) -> u16 {
    let word = pin / 64;
    let bit = 1u64 << (pin % 64);
    let mut count = 0u16;
    for (index, plane) in planes.iter().enumerate() {
        if plane[word] & bit != 0 {
            count |= 1u16 << index;
        }
    }
    count
}

#[inline]
fn any_driver_mask(planes: &DriverCountPlanes, word: usize) -> u64 {
    planes.iter().fold(0, |mask, plane| mask | plane[word])
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

/// Logical connector changes produced by one resolver delta. The two machine
/// words correspond to the 100 physical contacts; no card-family knowledge is
/// encoded here. Driver-count-only changes deliberately do not wake hardware:
/// TTL inputs react to the resolved electrical state, not to how many equal
/// sources happen to be holding that same level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct S100BusChange {
    pins: PinMask,
}

impl S100BusChange {
    pub fn is_empty(self) -> bool {
        self.pins == [0; PIN_MASK_WORDS]
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
/// The four connector states fit in three bitsets: strong LOW, strong HIGH and
/// the open-collector subset of LOW. A complete S-100 connector therefore moves
/// as six machine words rather than a 101-element enum array.
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

/// Resolved backplane sample.
///
/// LOW/HIGH/contention and even per-pin driver counts are represented as bit
/// planes. Resolution therefore processes 64 physical contacts in parallel. The
/// public per-pin API is unchanged and reconstructs exact driver counts lazily.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S100BusSample {
    defined: PinMask,
    high: PinMask,
    contention: PinMask,
    low_driver_counts: DriverCountPlanes,
    high_driver_counts: DriverCountPlanes,
    cached_address: Option<u16>,
    cached_data_out: Option<u8>,
    cached_data_in: Option<u8>,
}

impl Default for S100BusSample {
    fn default() -> Self {
        Self {
            defined: [0; PIN_MASK_WORDS],
            high: [0; PIN_MASK_WORDS],
            contention: [0; PIN_MASK_WORDS],
            low_driver_counts: [[0; PIN_MASK_WORDS]; DRIVER_COUNT_BITS],
            high_driver_counts: [[0; PIN_MASK_WORDS]; DRIVER_COUNT_BITS],
            cached_address: None,
            cached_data_out: None,
            cached_data_in: None,
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

    fn set_resolved_pin(&mut self, pin: usize, resolved: S100ResolvedPin) {
        debug_assert_eq!(resolved.low_drivers, 0, "passive bias cannot own a driver");
        debug_assert_eq!(resolved.high_drivers, 0, "passive bias cannot own a driver");
        self.set_level(pin, resolved.level, resolved.contention);
    }

    #[inline]
    fn clear_driver_counts(&mut self) {
        self.low_driver_counts = [[0; PIN_MASK_WORDS]; DRIVER_COUNT_BITS];
        self.high_driver_counts = [[0; PIN_MASK_WORDS]; DRIVER_COUNT_BITS];
    }

    #[inline]
    fn add_drive(&mut self, drive: &S100CardDrive) {
        for word in 0..PIN_MASK_WORDS {
            add_driver_mask(&mut self.low_driver_counts, word, drive.low[word]);
            add_driver_mask(&mut self.high_driver_counts, word, drive.high[word]);
        }
    }

    #[inline]
    fn replace_drive(&mut self, old: &S100CardDrive, new: &S100CardDrive) {
        for word in 0..PIN_MASK_WORDS {
            let removed_low = old.low[word] & !new.low[word];
            let added_low = new.low[word] & !old.low[word];
            let removed_high = old.high[word] & !new.high[word];
            let added_high = new.high[word] & !old.high[word];
            if removed_low != 0 {
                sub_driver_mask(&mut self.low_driver_counts, word, removed_low);
            }
            if added_low != 0 {
                add_driver_mask(&mut self.low_driver_counts, word, added_low);
            }
            if removed_high != 0 {
                sub_driver_mask(&mut self.high_driver_counts, word, removed_high);
            }
            if added_high != 0 {
                add_driver_mask(&mut self.high_driver_counts, word, added_high);
            }
        }
    }

    fn finalize_driver_levels(&mut self, passive_defined: PinMask, passive_high: PinMask) {
        for word in 0..PIN_MASK_WORDS {
            let low = any_driver_mask(&self.low_driver_counts, word);
            let high = any_driver_mask(&self.high_driver_counts, word);
            let driven = low | high;
            let contention = low & high;
            self.contention[word] = contention;
            self.defined[word] =
                (passive_defined[word] & !driven) | (driven & !contention);
            self.high[word] =
                (passive_high[word] & !driven) | (high & !low);
        }
        self.refresh_cached_buses();
    }

    fn refresh_cached_buses(&mut self) {
        self.cached_address = self.resolved_pin_bits_uncached(&ADDRESS_PINS);
        self.cached_data_out = self
            .resolved_pin_bits_uncached(&DATA_OUT_PINS)
            .map(|value| value as u8);
        self.cached_data_in = self
            .resolved_pin_bits_uncached(&DATA_IN_PINS)
            .map(|value| value as u8);
    }

    fn refresh_cached_buses_for_change(&mut self, change: S100BusChange) {
        if masks_intersect(&change.pins, &ADDRESS_PIN_MASK) {
            self.cached_address = self.resolved_pin_bits_uncached(&ADDRESS_PINS);
        }
        if masks_intersect(&change.pins, &DATA_OUT_PIN_MASK) {
            self.cached_data_out = self
                .resolved_pin_bits_uncached(&DATA_OUT_PINS)
                .map(|value| value as u8);
        }
        if masks_intersect(&change.pins, &DATA_IN_PIN_MASK) {
            self.cached_data_in = self
                .resolved_pin_bits_uncached(&DATA_IN_PINS)
                .map(|value| value as u8);
        }
    }

    fn electrical_change_from(
        &self,
        old_defined: PinMask,
        old_high: PinMask,
        old_contention: PinMask,
    ) -> S100BusChange {
        let mut pins = [0; PIN_MASK_WORDS];
        for word in 0..PIN_MASK_WORDS {
            pins[word] = (self.defined[word] ^ old_defined[word])
                | (self.high[word] ^ old_high[word])
                | (self.contention[word] ^ old_contention[word]);
        }
        S100BusChange { pins }
    }

    fn finalize_incremental(
        &mut self,
        passive_defined: PinMask,
        passive_high: PinMask,
        old_defined: PinMask,
        old_high: PinMask,
        old_contention: PinMask,
    ) -> S100BusChange {
        for word in 0..PIN_MASK_WORDS {
            let low = any_driver_mask(&self.low_driver_counts, word);
            let high = any_driver_mask(&self.high_driver_counts, word);
            let driven = low | high;
            let contention = low & high;
            self.contention[word] = contention;
            self.defined[word] =
                (passive_defined[word] & !driven) | (driven & !contention);
            self.high[word] =
                (passive_high[word] & !driven) | (high & !low);
        }
        let change = self.electrical_change_from(old_defined, old_high, old_contention);
        self.refresh_cached_buses_for_change(change);
        change
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
            low_drivers: driver_count(&self.low_driver_counts, pin),
            high_drivers: driver_count(&self.high_driver_counts, pin),
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

    fn resolved_pin_bits_uncached(&self, pins: &[u8]) -> Option<u16> {
        let mut value = 0u16;
        for (bit, pin) in pins.iter().copied().enumerate() {
            if self.pin_level(pin)? {
                value |= 1u16 << bit;
            }
        }
        Some(value)
    }

    pub fn address(&self) -> Option<u16> {
        self.cached_address
    }

    pub fn data_out(&self) -> Option<u8> {
        self.cached_data_out
    }

    pub fn data_in(&self) -> Option<u8> {
        self.cached_data_in
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

    /// Whether an event outside the S-100 input path may have changed this
    /// card's connector outputs. The conservative default preserves the existing
    /// semantics for ordinary cards; devices with an explicit dirty latch can
    /// make idle refresh a zero-work predicate instead of rebuilding a drive.
    fn external_drive_dirty(&self) -> bool {
        true
    }

    /// Refresh connector outputs after state changed outside the S-100 input
    /// path (for example a character arriving at a serial connector). Most
    /// cards have no such boundary and can return their persistent drive.
    fn refresh_external_drive(&mut self) -> S100CardDrive {
        self.drive_s100()
    }
}

pub struct S100Slot {
    number: usize,
    card: Option<Box<dyn S100ElectricalCard>>,
    strong_outputs: PinMask,
    open_collector_outputs: PinMask,
    input_sensitivity: PinMask,
    cached_drive: S100CardDrive,
    /// Drive currently folded into the incremental resolver. It deliberately
    /// remains separate from `cached_drive`: a card may change state after
    /// observing a delta and the next resolver delta then applies exactly that
    /// old->new connector transition.
    resolved_drive: S100CardDrive,
}

impl S100Slot {
    fn new(number: usize) -> Self {
        Self {
            number,
            card: None,
            strong_outputs: [0; PIN_MASK_WORDS],
            open_collector_outputs: [0; PIN_MASK_WORDS],
            input_sensitivity: [0; PIN_MASK_WORDS],
            cached_drive: S100CardDrive::new(),
            resolved_drive: S100CardDrive::new(),
        }
    }

    fn allow(mask: &mut PinMask, pin: u8) {
        mask_set(mask, pin as usize, true);
    }

    fn set_drive_contract(&mut self, descriptor: &'static S100CardDescriptor) {
        self.strong_outputs = [0; PIN_MASK_WORDS];
        self.open_collector_outputs = [0; PIN_MASK_WORDS];
        self.input_sensitivity = [0; PIN_MASK_WORDS];
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
                S100ContactRole::Input => {
                    Self::allow(&mut self.input_sensitivity, pin);
                }
                S100ContactRole::Power => {}
            }
        }
    }

    fn clear_drive_contract(&mut self) {
        self.strong_outputs = [0; PIN_MASK_WORDS];
        self.open_collector_outputs = [0; PIN_MASK_WORDS];
        self.input_sensitivity = [0; PIN_MASK_WORDS];
        self.cached_drive = S100CardDrive::new();
        self.resolved_drive = S100CardDrive::new();
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
    /// Pre-biased no-driver sample. Each resolve starts from these passive levels
    /// and then folds active card drives into bit-sliced driver counters.
    passive_sample: S100BusSample,
    sample: S100BusSample,
    /// One bit per physically occupied connector. Hot paths iterate this mask
    /// directly instead of rescanning empty chassis connectors.
    occupied_slots: S100SlotMask,
    /// For each physical contact, the slots that actually declare that contact
    /// as an INPUT. This is the compiled fan-out graph for event propagation.
    input_observers: [S100SlotMask; S100_CONTACT_COUNT + 1],
    /// Slots whose cached connector output differs from what is currently folded
    /// into the persistent resolver. Selection changes are added separately.
    dirty_drive_slots: S100SlotMask,
    /// Transaction mask whose contributions are currently folded into `sample`.
    incremental_selected: S100SlotMask,
    /// Non-slot Display/Control contribution currently folded into `sample`.
    incremental_chassis_drive: S100CardDrive,
    /// General/diagnostic resolvers can invalidate the persistent accumulator;
    /// the next production delta rebuilds it once and then resumes O(changes).
    incremental_valid: bool,
}

impl S100Backplane {
    pub fn new(slot_count: usize) -> Self {
        let mut backplane = Self {
            slots: (0..slot_count)
                .map(|index| S100Slot::new(index + 1))
                .collect(),
            passive_sample: S100BusSample::default(),
            sample: S100BusSample::default(),
            occupied_slots: 0,
            input_observers: [0; S100_CONTACT_COUNT + 1],
            dirty_drive_slots: 0,
            incremental_selected: 0,
            incremental_chassis_drive: S100CardDrive::new(),
            incremental_valid: false,
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
        backplane.passive_sample.refresh_cached_buses();
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

    fn update_observer_index(&mut self, sensitivity: PinMask, slot_bit: S100SlotMask, add: bool) {
        for word in 0..PIN_MASK_WORDS {
            let mut bits = sensitivity[word];
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let pin = word * 64 + bit;
                if pin > S100_CONTACT_COUNT {
                    continue;
                }
                if add {
                    self.input_observers[pin] |= slot_bit;
                } else {
                    self.input_observers[pin] &= !slot_bit;
                }
            }
        }
    }

    #[inline]
    fn observers_for_change(&self, change: S100BusChange) -> S100SlotMask {
        let mut observers = 0;
        for word in 0..PIN_MASK_WORDS {
            let mut bits = change.pins[word];
            while bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let pin = word * 64 + bit;
                if pin <= S100_CONTACT_COUNT {
                    observers |= self.input_observers[pin];
                }
            }
        }
        observers
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
        self.passive_sample.refresh_cached_buses();
        self.incremental_valid = false;
    }

    pub fn insert(
        &mut self,
        slot: usize,
        card: Box<dyn S100ElectricalCard>,
    ) -> Result<(), S100BackplaneError> {
        let slot_count = self.slots.len();
        let slot_bit = s100_slot_mask(slot);
        let sensitivity;
        {
            let target = self
                .slots
                .get_mut(slot.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or(S100BackplaneError::InvalidSlot { slot, slot_count })?;
            if target.card.is_some() {
                return Err(S100BackplaneError::SlotOccupied { slot });
            }
            target.set_drive_contract(card.s100_descriptor());
            let drive = card.drive_s100();
            if let Err(error) = validate_card_drive(target, &drive) {
                target.clear_drive_contract();
                return Err(error);
            }
            target.cached_drive = drive;
            target.resolved_drive = S100CardDrive::new();
            sensitivity = target.input_sensitivity;
            target.card = Some(card);
        }
        self.occupied_slots |= slot_bit;
        self.dirty_drive_slots |= slot_bit;
        self.update_observer_index(sensitivity, slot_bit, true);
        self.incremental_valid = false;
        Ok(())
    }

    pub fn remove(
        &mut self,
        slot: usize,
    ) -> Result<Option<Box<dyn S100ElectricalCard>>, S100BackplaneError> {
        let slot_count = self.slots.len();
        let slot_bit = s100_slot_mask(slot);
        let sensitivity;
        let card;
        {
            let target = self
                .slots
                .get_mut(slot.checked_sub(1).unwrap_or(usize::MAX))
                .ok_or(S100BackplaneError::InvalidSlot { slot, slot_count })?;
            sensitivity = target.input_sensitivity;
            card = target.card.take();
            target.clear_drive_contract();
        }
        self.update_observer_index(sensitivity, slot_bit, false);
        self.occupied_slots &= !slot_bit;
        self.dirty_drive_slots &= !slot_bit;
        self.incremental_valid = false;
        Ok(card)
    }

    #[inline]
    fn begin_resolution(sample: &mut S100BusSample, passive: &S100BusSample) -> (PinMask, PinMask) {
        sample.clone_from(passive);
        sample.clear_driver_counts();
        (passive.defined, passive.high)
    }

    fn resolve_against_passive(
        passive: &S100BusSample,
        drives: &[S100CardDrive],
    ) -> S100BusSample {
        let mut sample = passive.clone();
        let passive_defined = passive.defined;
        let passive_high = passive.high;
        sample.clear_driver_counts();
        for drive in drives {
            sample.add_drive(drive);
        }
        sample.finalize_driver_levels(passive_defined, passive_high);
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

    /// Observe only physically occupied slots in the selected set. Iterating the
    /// u32 mask avoids rescanning empty connectors in an 18-slot chassis.
    pub fn observe_selected_cards(&mut self, selected: S100SlotMask) {
        let observed = &self.sample;
        let mut pending = selected & self.occupied_slots;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            pending &= pending - 1;
            if let Some(card) = self.slots[index].card.as_mut() {
                card.observe_s100(observed);
            }
        }
    }

    /// Re-sample card outputs only for slots whose state may have changed outside
    /// the backplane (for example CPU package pins or an asynchronously advanced
    /// UART). Cards that can prove their external side is clean skip the virtual
    /// refresh and S100CardDrive copy/compare entirely.
    pub fn refresh_cached_drives(
        &mut self,
        selected: S100SlotMask,
    ) -> Result<S100SlotMask, S100BackplaneError> {
        let mut changed = 0;
        let mut pending = selected & self.occupied_slots;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            let bit = 1u32 << index;
            pending &= pending - 1;
            let slot = &mut self.slots[index];
            let Some(card) = slot.card.as_mut() else {
                continue;
            };
            if !card.external_drive_dirty() {
                continue;
            }
            let drive = card.refresh_external_drive();
            if drive != slot.cached_drive {
                validate_card_drive(slot, &drive)?;
                slot.cached_drive = drive;
                changed |= bit;
            }
        }
        self.dirty_drive_slots |= changed;
        Ok(changed)
    }

    /// Push a connector drive that the chassis fabric has already obtained from
    /// that exact physical card instance. This is not a synthetic bus source: it
    /// updates the normal slot cache, validates the same connector contract, and
    /// the drive still reaches every other card only through normal S-100
    /// resolution. It removes a redundant virtual `drive_s100()` round-trip when
    /// the caller just changed a card's non-S-100 side and already has its drive.
    pub(crate) fn update_cached_slot_drive(
        &mut self,
        slot: usize,
        drive: S100CardDrive,
    ) -> Result<bool, S100BackplaneError> {
        let slot_count = self.slots.len();
        let bit = s100_slot_mask(slot);
        let target = self
            .slots
            .get_mut(slot.checked_sub(1).unwrap_or(usize::MAX))
            .ok_or(S100BackplaneError::InvalidSlot { slot, slot_count })?;
        validate_card_drive(target, &drive)?;
        if drive == target.cached_drive {
            return Ok(false);
        }
        target.cached_drive = drive;
        self.dirty_drive_slots |= bit;
        Ok(true)
    }

    /// Observe exactly the slots wired to changed input contacts plus any slots
    /// forced by a non-S100-side event (for example Intel package pins). The
    /// per-contact fan-out was compiled when cards were inserted, so no chassis
    /// scan or per-slot sensitivity test occurs on the propagation hot path.
    pub fn observe_changed_cards(
        &mut self,
        change: S100BusChange,
        forced: S100SlotMask,
        selected: S100SlotMask,
    ) -> Result<S100SlotMask, S100BackplaneError> {
        let observed = &self.sample;
        let mut pending = (forced | self.observers_for_change(change))
            & selected
            & self.occupied_slots;
        let mut drive_changed = 0;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            let bit = 1u32 << index;
            pending &= pending - 1;
            let slot = &mut self.slots[index];
            let Some(card) = slot.card.as_mut() else {
                continue;
            };
            card.observe_s100(observed);
            let drive = card.drive_s100();
            if drive != slot.cached_drive {
                // The previous cached drive was already contract-checked at
                // insertion or at the last connector transition. Revalidate only
                // when the card actually proposes a new electrical drive.
                validate_card_drive(slot, &drive)?;
                slot.cached_drive = drive;
                drive_changed |= bit;
            }
        }
        self.dirty_drive_slots |= drive_changed;
        Ok(drive_changed)
    }

    fn rebuild_incremental(
        &mut self,
        selected: S100SlotMask,
        chassis_drive: S100CardDrive,
    ) -> S100BusChange {
        let old_defined = self.sample.defined;
        let old_high = self.sample.high;
        let old_contention = self.sample.contention;
        let (passive_defined, passive_high) =
            Self::begin_resolution(&mut self.sample, &self.passive_sample);
        let mut pending = self.occupied_slots;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            let bit = 1u32 << index;
            pending &= pending - 1;
            let slot = &mut self.slots[index];
            let drive = if selected & bit != 0 {
                slot.cached_drive
            } else {
                S100CardDrive::new()
            };
            self.sample.add_drive(&drive);
            slot.resolved_drive = drive;
        }
        self.sample.add_drive(&chassis_drive);
        self.sample
            .finalize_driver_levels(passive_defined, passive_high);
        self.incremental_selected = selected;
        self.incremental_chassis_drive = chassis_drive;
        self.dirty_drive_slots = 0;
        self.incremental_valid = true;
        self.sample
            .electrical_change_from(old_defined, old_high, old_contention)
    }

    fn resolve_incremental(
        &mut self,
        selected: S100SlotMask,
        chassis_drive: S100CardDrive,
    ) -> S100BusChange {
        if !self.incremental_valid {
            return self.rebuild_incremental(selected, chassis_drive);
        }

        let old_defined = self.sample.defined;
        let old_high = self.sample.high;
        let old_contention = self.sample.contention;

        let selection_changed = selected ^ self.incremental_selected;
        let mut pending = (self.dirty_drive_slots | selection_changed) & self.occupied_slots;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            let bit = 1u32 << index;
            pending &= pending - 1;
            let slot = &mut self.slots[index];
            let target = if selected & bit != 0 {
                slot.cached_drive
            } else {
                S100CardDrive::new()
            };
            if target != slot.resolved_drive {
                self.sample.replace_drive(&slot.resolved_drive, &target);
                slot.resolved_drive = target;
            }
            self.dirty_drive_slots &= !bit;
        }

        if chassis_drive != self.incremental_chassis_drive {
            self.sample
                .replace_drive(&self.incremental_chassis_drive, &chassis_drive);
            self.incremental_chassis_drive = chassis_drive;
        }
        self.incremental_selected = selected;

        self.sample.finalize_incremental(
            self.passive_sample.defined,
            self.passive_sample.high,
            old_defined,
            old_high,
            old_contention,
        )
    }

    /// Resolve cached card outputs. The production hot path keeps exact driver
    /// counts persistently and applies only connector drive deltas. With the
    /// normal single Display/Control contribution this is O(changed drives), not
    /// O(all selected cards × all driven pins). Multi-drive diagnostic callers
    /// retain the general rebuild path.
    pub fn resolve_cached_selected_drives(
        &mut self,
        selected: S100SlotMask,
        chassis_drives: &[S100CardDrive],
    ) -> S100BusChange {
        if chassis_drives.len() <= 1 {
            return self.resolve_incremental(
                selected,
                chassis_drives.first().copied().unwrap_or_default(),
            );
        }

        self.incremental_valid = false;
        let old_defined = self.sample.defined;
        let old_high = self.sample.high;
        let old_contention = self.sample.contention;
        let (passive_defined, passive_high) =
            Self::begin_resolution(&mut self.sample, &self.passive_sample);
        let mut pending = selected & self.occupied_slots;
        while pending != 0 {
            let index = pending.trailing_zeros() as usize;
            pending &= pending - 1;
            self.sample.add_drive(&self.slots[index].cached_drive);
        }
        for drive in chassis_drives {
            self.sample.add_drive(drive);
        }
        self.sample
            .finalize_driver_levels(passive_defined, passive_high);
        self.sample
            .electrical_change_from(old_defined, old_high, old_contention)
    }

    /// Resolve the current drives of every slotted card plus optional chassis
    /// wiring that is not itself an S-100 slot (for example Display/Control).
    pub fn resolve_current_drives(
        &mut self,
        chassis_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        self.resolve_selected_drives(S100SlotMask::MAX, chassis_drives)
    }

    /// Compatibility/general resolver. It refreshes every selected card before
    /// resolving, preserving the old API semantics for tests and low-frequency
    /// callers. Production Cycle/Fast hot paths use the cached event API above.
    pub fn resolve_selected_drives(
        &mut self,
        selected: S100SlotMask,
        chassis_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        self.refresh_cached_drives(selected)?;
        let _ = self.resolve_cached_selected_drives(selected, chassis_drives);
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
    use std::cell::RefCell;
    use std::rc::Rc;

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
    const REACTIVE_CONTACTS: &[S100CardContact] = &[
        S100CardContact::new(S100Signal::Address(0), S100ContactRole::Input),
        S100CardContact::new(S100Signal::DataIn(0), S100ContactRole::TriStateOutput),
    ];

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
    static REACTIVE_CARD: S100CardDescriptor = S100CardDescriptor {
        key: "test-reactive",
        label: "test reactive card",
        class: S100CardClass::Compatibility,
        historical: false,
        contacts: REACTIVE_CONTACTS,
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

    struct ReactiveCard {
        observations: Rc<RefCell<usize>>,
        a0: bool,
    }

    impl S100Card for ReactiveCard {
        fn s100_descriptor(&self) -> &'static S100CardDescriptor {
            &REACTIVE_CARD
        }
    }

    impl S100ElectricalCard for ReactiveCard {
        fn observe_s100(&mut self, sample: &S100BusSample) {
            *self.observations.borrow_mut() += 1;
            self.a0 = sample.signal_level(S100Signal::Address(0)) == Some(true);
        }

        fn drive_s100(&self) -> S100CardDrive {
            let mut drive = S100CardDrive::new();
            drive.drive_tristate(S100Signal::DataIn(0), Some(self.a0));
            drive
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
        assert_eq!(backplane.sample().signal(S100Signal::Ready).low_drivers, 1);

        backplane.remove(1).unwrap();
        backplane.step().unwrap();
        assert_eq!(
            backplane.sample().signal_level(S100Signal::Ready),
            Some(true)
        );
        assert_eq!(backplane.sample().signal(S100Signal::Ready).low_drivers, 0);
    }

    #[test]
    fn one_tri_state_driver_places_its_value_on_data_in() {
        let backplane = S100Backplane::new(0);
        let mut drive = S100CardDrive::new();
        drive.drive_signal(S100Signal::DataIn(0), true);
        let sample = backplane.resolve_drive_sets(&[drive]);
        assert_eq!(sample.signal_level(S100Signal::DataIn(0)), Some(true));
        assert_eq!(sample.signal(S100Signal::DataIn(0)).high_drivers, 1);
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
    fn bit_sliced_driver_counts_preserve_multiple_same_level_sources() {
        let backplane = S100Backplane::new(0);
        let mut a = S100CardDrive::new();
        let mut b = S100CardDrive::new();
        let mut c = S100CardDrive::new();
        a.drive_signal(S100Signal::DataIn(0), true);
        b.drive_signal(S100Signal::DataIn(0), true);
        c.drive_signal(S100Signal::DataIn(0), true);
        let sample = backplane.resolve_drive_sets(&[a, b, c]);
        let pin = sample.signal(S100Signal::DataIn(0));
        assert_eq!(pin.level, Some(true));
        assert!(!pin.contention);
        assert_eq!(pin.high_drivers, 3);
        assert_eq!(pin.low_drivers, 0);
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
        assert!(matches!(
            backplane.insert(1, Box::new(card)),
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
    fn cached_event_path_wakes_only_cards_connected_to_changed_inputs() {
        let observations = Rc::new(RefCell::new(0usize));
        let card = ReactiveCard {
            observations: Rc::clone(&observations),
            a0: false,
        };
        let mut backplane = S100Backplane::new(1);
        backplane.insert(1, Box::new(card)).unwrap();

        let mut address = S100CardDrive::new();
        address.drive_signal(S100Signal::Address(0), true);
        let change = backplane.resolve_cached_selected_drives(S100SlotMask::MAX, &[address]);
        assert!(!change.is_empty());
        let changed_drives = backplane
            .observe_changed_cards(change, 0, S100SlotMask::MAX)
            .unwrap();
        assert_eq!(changed_drives, s100_slot_mask(1));
        assert_eq!(*observations.borrow(), 1);

        let _ = backplane.resolve_cached_selected_drives(S100SlotMask::MAX, &[address]);
        let stable = backplane.resolve_cached_selected_drives(S100SlotMask::MAX, &[address]);
        assert!(stable.is_empty());
        backplane
            .observe_changed_cards(stable, 0, S100SlotMask::MAX)
            .unwrap();
        assert_eq!(*observations.borrow(), 1);
    }

    #[test]
    fn incremental_resolver_removes_departing_slot_driver_counts_exactly() {
        let mut backplane = S100Backplane::new(2);
        backplane.insert(1, Box::new(ready_card(true))).unwrap();
        backplane.insert(2, Box::new(ready_card(true))).unwrap();
        backplane
            .resolve_selected_drives(s100_slot_mask(1) | s100_slot_mask(2), &[])
            .unwrap();
        assert_eq!(backplane.sample().signal(S100Signal::Ready).low_drivers, 2);
        backplane
            .resolve_selected_drives(s100_slot_mask(1), &[])
            .unwrap();
        assert_eq!(backplane.sample().signal(S100Signal::Ready).low_drivers, 1);
        backplane.resolve_selected_drives(0, &[]).unwrap();
        assert_eq!(backplane.sample().signal(S100Signal::Ready).low_drivers, 0);
        assert_eq!(backplane.sample().signal_level(S100Signal::Ready), Some(true));
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
