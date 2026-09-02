//! Runtime S-100 RAM cards shared by the electrical backplane and host tools.
//!
//! The guest never reaches these bytes through a host-side memory API: it sees
//! only DI/DO/MWRT/PRDY on the S-100 bus.  The cloned handle exists for debugger
//! and inspection tools so they can inspect the *same physical card storage*
//! without fabricating CPU cycles or maintaining a shadow flat-memory array.

use std::cell::RefCell;
use std::rc::Rc;

use rand::RngCore;

use crate::config::{FastRamCompatibilityConfig, RamInit};
use crate::s100::{
    S100Card, S100CardDescriptor, S100Signal, FAST_RAM_COMPATIBILITY,
    MITS_1K_STATIC_RAM,
};
use crate::s100_backplane::{S100BusSample, S100CardDrive, S100ElectricalCard};
use crate::s100_memory::{
    S100RamBoardModel, S100RamCardConfig, S100RamTimingModel, MITS_88_16MCD,
    MITS_88_16MCS, MITS_88_4MCD, MITS_88_4MCS, MITS_88_S4K,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeRamConfig {
    Historical(S100RamCardConfig),
    Compatibility(FastRamCompatibilityConfig),
}

impl RuntimeRamConfig {
    pub const fn base_address(self) -> u16 {
        match self {
            Self::Historical(config) => config.base_address,
            Self::Compatibility(config) => config.base_address,
        }
    }

    pub const fn populated_bytes(self) -> usize {
        match self {
            Self::Historical(config) => config.populated_bytes,
            Self::Compatibility(config) => config.populated_bytes,
        }
    }

    pub const fn contains(self, address: u16) -> bool {
        let offset = address.wrapping_sub(self.base_address()) as usize;
        address >= self.base_address() && offset < self.populated_bytes()
    }

    pub const fn read_wait_states(self) -> u8 {
        match self {
            Self::Historical(config) => match config.model.timing_model() {
                S100RamTimingModel::FixedReadWaits(waits) => waits,
                _ => 0,
            },
            Self::Compatibility(config) => config.read_wait_states,
        }
    }

    pub const fn supports_front_panel_protect(self) -> bool {
        match self {
            Self::Historical(config) => config.model.supports_front_panel_protect(),
            // The compatibility card preserves the old RusTair writable/protect
            // semantics without claiming that this is historical MITS hardware.
            Self::Compatibility(_) => true,
        }
    }

    pub const fn historical_model(self) -> Option<S100RamBoardModel> {
        match self {
            Self::Historical(config) => Some(config.model),
            Self::Compatibility(_) => None,
        }
    }
}

#[derive(Debug)]
struct RuntimeRamState {
    config: RuntimeRamConfig,
    bytes: Vec<u8>,
    protected: bool,
    selected_offset: Option<usize>,
    memory_read: bool,
    wait_clocks_remaining: u8,
    previous_sync: bool,
    previous_clock: bool,
    previous_memory_read: bool,
    previous_protect: bool,
    previous_unprotect: bool,
}

impl RuntimeRamState {
    fn new(config: RuntimeRamConfig, init: RamInit) -> Self {
        let mut bytes = vec![0; config.populated_bytes()];
        if init == RamInit::Random {
            rand::rng().fill_bytes(&mut bytes);
        }
        Self {
            config,
            bytes,
            protected: false,
            selected_offset: None,
            memory_read: false,
            wait_clocks_remaining: 0,
            previous_sync: false,
            previous_clock: false,
            previous_memory_read: false,
            previous_protect: false,
            previous_unprotect: false,
        }
    }

    fn read_byte(&self, address: u16) -> Option<u8> {
        if !self.config.contains(address) {
            return None;
        }
        Some(self.bytes[address.wrapping_sub(self.config.base_address()) as usize])
    }

    fn write_byte(&mut self, address: u16, value: u8, respect_protection: bool) -> bool {
        if !self.config.contains(address) || (respect_protection && self.protected) {
            return false;
        }
        let offset = address.wrapping_sub(self.config.base_address()) as usize;
        self.bytes[offset] = value;
        true
    }

    fn reset_timing(&mut self) {
        self.selected_offset = None;
        self.memory_read = false;
        self.wait_clocks_remaining = 0;
        self.previous_sync = false;
        self.previous_clock = false;
        self.previous_memory_read = false;
        self.previous_protect = false;
        self.previous_unprotect = false;
    }
}

#[derive(Clone)]
pub struct RuntimeRamHandle {
    state: Rc<RefCell<RuntimeRamState>>,
}

impl RuntimeRamHandle {
    pub fn config(&self) -> RuntimeRamConfig {
        self.state.borrow().config
    }

    pub fn contains(&self, address: u16) -> bool {
        self.state.borrow().config.contains(address)
    }

    pub fn read_byte(&self, address: u16) -> Option<u8> {
        self.state.borrow().read_byte(address)
    }

    pub fn write_byte(&self, address: u16, value: u8, respect_protection: bool) -> bool {
        self.state
            .borrow_mut()
            .write_byte(address, value, respect_protection)
    }

    pub fn is_protected(&self) -> bool {
        self.state.borrow().protected
    }

    pub fn set_protected(&self, protected: bool) -> bool {
        let mut state = self.state.borrow_mut();
        if !state.config.supports_front_panel_protect() {
            return false;
        }
        state.protected = protected;
        true
    }

    pub fn clear_protection(&self) {
        self.state.borrow_mut().protected = false;
    }

    pub fn initialize(&self, init: RamInit) {
        let mut state = self.state.borrow_mut();
        state.bytes.fill(0);
        if init == RamInit::Random {
            rand::rng().fill_bytes(&mut state.bytes);
        }
        state.protected = false;
        state.reset_timing();
    }

    pub fn load(&self, address: u16, data: &[u8]) -> usize {
        let mut state = self.state.borrow_mut();
        let Some(first) = state.config.contains(address).then_some(
            address.wrapping_sub(state.config.base_address()) as usize,
        ) else {
            return 0;
        };
        let len = data.len().min(state.bytes.len().saturating_sub(first));
        state.bytes[first..first + len].copy_from_slice(&data[..len]);
        len
    }
}

pub struct RuntimeRamCard {
    state: Rc<RefCell<RuntimeRamState>>,
}

impl RuntimeRamCard {
    pub fn historical(
        config: S100RamCardConfig,
        init: RamInit,
    ) -> Result<(Self, RuntimeRamHandle), crate::s100_memory::S100RamConfigError> {
        let config = config.validate()?;
        Ok(Self::from_config(RuntimeRamConfig::Historical(config), init))
    }

    pub fn compatibility(
        config: FastRamCompatibilityConfig,
        init: RamInit,
    ) -> Result<(Self, RuntimeRamHandle), crate::config::S100HardwareConfigError> {
        let config = config.validate()?;
        Ok(Self::from_config(RuntimeRamConfig::Compatibility(config), init))
    }

    fn from_config(config: RuntimeRamConfig, init: RamInit) -> (Self, RuntimeRamHandle) {
        let state = Rc::new(RefCell::new(RuntimeRamState::new(config, init)));
        (
            Self {
                state: Rc::clone(&state),
            },
            RuntimeRamHandle { state },
        )
    }

    fn descriptor_for(config: RuntimeRamConfig) -> &'static S100CardDescriptor {
        match config {
            RuntimeRamConfig::Compatibility(_) => &FAST_RAM_COMPATIBILITY,
            RuntimeRamConfig::Historical(config) => match config.model {
                S100RamBoardModel::Mits1KStatic88Mcs => &MITS_1K_STATIC_RAM,
                S100RamBoardModel::Mits4KDynamic88_4Mcd => &MITS_88_4MCD,
                S100RamBoardModel::Mits4KSynchronous88S4K => &MITS_88_S4K,
                S100RamBoardModel::Mits4KStatic88_4Mcs => &MITS_88_4MCS,
                S100RamBoardModel::Mits16KStatic88_16Mcs => &MITS_88_16MCS,
                S100RamBoardModel::Mits16KDynamic88_16Mcd => &MITS_88_16MCD,
            },
        }
    }
}

impl S100Card for RuntimeRamCard {
    fn s100_descriptor(&self) -> &'static S100CardDescriptor {
        Self::descriptor_for(self.state.borrow().config)
    }
}

impl S100ElectricalCard for RuntimeRamCard {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        let mut state = self.state.borrow_mut();
        let sync = sample.signal_level(S100Signal::Sync) == Some(true);
        let clock = sample.signal_level(S100Signal::Clock) == Some(true);
        let protect = sample.signal_level(S100Signal::Protect) == Some(true);
        let unprotect = sample.signal_level(S100Signal::Unprotect) == Some(true);
        let sync_rising = sync && !state.previous_sync;
        let clock_rising = clock && !state.previous_clock;

        state.selected_offset = sample.address().and_then(|address| {
            state
                .config
                .contains(address)
                .then_some(address.wrapping_sub(state.config.base_address()) as usize)
        });
        let memory_read = state.selected_offset.is_some()
            && sample.signal_level(S100Signal::MemoryRead) == Some(true);

        // The Display/Control board generates MWRT. A selected RAM card sees the
        // resulting bus line; it does not infer writes from CPU package state.
        if let (Some(address), Some(value)) = (sample.address(), sample.data_out()) {
            if sample.signal_level(S100Signal::MemoryWrite) == Some(true) {
                let _ = state.write_byte(address, value, true);
            }
        }

        if state.selected_offset.is_some() && state.config.supports_front_panel_protect() {
            if protect && !state.previous_protect {
                state.protected = true;
            }
            if unprotect && !state.previous_unprotect {
                state.protected = false;
            }
        }

        let fixed_waits = state.config.read_wait_states();
        if !memory_read || fixed_waits == 0 {
            state.wait_clocks_remaining = 0;
        } else if sync_rising || (!state.previous_memory_read && sync) {
            // sMEMR is produced by the CPU-board 8212 *after* SYNC+PHI1. The
            // second condition models that same-edge propagation without asking
            // the RAM card to predict the CPU status byte before it exists.
            state.wait_clocks_remaining = fixed_waits;
        } else if clock_rising && state.wait_clocks_remaining != 0 {
            state.wait_clocks_remaining -= 1;
        }

        state.memory_read = memory_read;
        state.previous_memory_read = memory_read;
        state.previous_sync = sync;
        state.previous_clock = clock;
        state.previous_protect = protect;
        state.previous_unprotect = unprotect;
    }

    fn drive_s100(&self) -> S100CardDrive {
        let state = self.state.borrow();
        let mut drive = S100CardDrive::new();
        if state.memory_read {
            if let Some(offset) = state.selected_offset {
                drive.drive_data_in(state.bytes[offset]);
            }
        }
        if state.wait_clocks_remaining != 0 {
            drive.pull_low(S100Signal::Ready, true);
        }
        if state.protected && state.selected_offset.is_some() && state.config.supports_front_panel_protect() {
            drive.drive_tristate(S100Signal::ProtectStatus, Some(true));
        }
        drive
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::s100_backplane::S100Backplane;

    fn read_drive(address: u16, sync: bool, clock: bool) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.drive_address(address);
        drive.drive_signal(S100Signal::MemoryRead, true);
        drive.drive_signal(S100Signal::MemoryWrite, false);
        drive.drive_signal(S100Signal::Sync, sync);
        drive.drive_signal(S100Signal::Clock, clock);
        drive
    }

    #[test]
    fn host_handle_and_electrical_card_share_one_storage_instance() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        assert!(handle.write_byte(0x0123, 0x5a, false));

        let backplane = S100Backplane::new(0);
        let master = read_drive(0x0123, false, false);
        let observed = backplane.resolve_drive_sets(&[master.clone()]);
        card.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[master, card.drive_s100()]);
        assert_eq!(resolved.data_in(), Some(0x5a));
    }

    #[test]
    fn overlapping_handles_remain_distinct_and_bus_exposes_contention() {
        let config = S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0);
        let (mut a, ah) = RuntimeRamCard::historical(config, RamInit::Zeroed).unwrap();
        let (mut b, bh) = RuntimeRamCard::historical(config, RamInit::Zeroed).unwrap();
        ah.write_byte(0x0010, 0x00, false);
        bh.write_byte(0x0010, 0xff, false);
        let backplane = S100Backplane::new(0);
        let master = read_drive(0x0010, false, false);
        let observed = backplane.resolve_drive_sets(&[master.clone()]);
        a.observe_s100(&observed);
        b.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[master, a.drive_s100(), b.drive_s100()]);
        assert!((0..8).all(|bit| resolved.signal_is_contended(S100Signal::DataIn(bit))));
    }

    #[test]
    fn compatibility_ram_is_explicit_but_uses_same_electrical_data_path() {
        let (mut card, handle) = RuntimeRamCard::compatibility(
            FastRamCompatibilityConfig::no_wait(0, 8 * 1024),
            RamInit::Zeroed,
        )
        .unwrap();
        handle.write_byte(0x1234, 0xa5, false);
        let backplane = S100Backplane::new(0);
        let master = read_drive(0x1234, false, false);
        let observed = backplane.resolve_drive_sets(&[master.clone()]);
        card.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[master, card.drive_s100()]);
        assert_eq!(resolved.data_in(), Some(0xa5));
        assert!(matches!(handle.config(), RuntimeRamConfig::Compatibility(_)));
    }

    #[test]
    fn protection_lives_on_the_shared_physical_card() {
        let (card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits1KStatic88Mcs, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        drop(card);
        assert!(handle.set_protected(true));
        assert!(!handle.write_byte(0x0010, 0x55, true));
        assert!(handle.write_byte(0x0010, 0x55, false));
        assert_eq!(handle.read_byte(0x0010), Some(0x55));
    }
}
