//! Runtime S-100 RAM cards shared by the electrical backplane and host tools.
//!
//! The guest never reaches these bytes through a host-side memory API: it sees
//! only DI/DO/MWRT/PRDY on the S-100 bus. The cloned handle exists for debugger
//! and inspection tools so they can inspect the *same physical card storage*
//! without fabricating CPU cycles or maintaining a shadow flat-memory array.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rand::RngCore;

use crate::config::{FastRamCompatibilityConfig, RamInit};
use crate::s100::{
    FAST_RAM_COMPATIBILITY, MITS_1K_STATIC_RAM, S100Card, S100CardDescriptor, S100Signal,
};
use crate::s100_backplane::{S100BusSample, S100CardDrive, S100ElectricalCard};
use crate::s100_memory::{
    MITS_88_4MCD, MITS_88_4MCS, MITS_88_16MCD, MITS_88_16MCS, MITS_88_S4K, S100RamBoardModel,
    S100RamCardConfig, S100RamTimingModel,
};

const LEGACY_COMPATIBILITY_PROTECTION_UNIT: usize = 1024;
// The S4K divider advances every other 8080 T-cycle and requests refresh after
// 32 divider counts. One PHI2 rising edge occurs per T-state, so the physical
// request cadence is 64 PHI2 rising edges, approximately 32 us at 2 MHz.
const S4K_REFRESH_PHI2_EDGES: u16 = 64;

type RamDriveSignature = (Option<u8>, bool, bool);

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
            // This non-historical migration card deliberately preserves RusTair's
            // legacy 1 KiB protection blocks. Real MITS cards remain card-scoped.
            Self::Compatibility(_) => true,
        }
    }

    pub const fn protection_unit_bytes(self) -> usize {
        match self {
            Self::Historical(_) => self.populated_bytes(),
            Self::Compatibility(_) => LEGACY_COMPATIBILITY_PROTECTION_UNIT,
        }
    }

    pub const fn protection_unit_count(self) -> usize {
        let unit = self.protection_unit_bytes();
        self.populated_bytes().div_ceil(unit)
    }

    pub const fn historical_model(self) -> Option<S100RamBoardModel> {
        match self {
            Self::Historical(config) => Some(config.model),
            Self::Compatibility(_) => None,
        }
    }
}

/// Clock/refresh state is shared independently from byte storage so Adaptive
/// Full can advance the exact same physical card timing without taking a dynamic
/// `RefCell` borrow around unrelated RAM/protection/cache state on every machine
/// cycle. `Cell` is sufficient because the emulator and S-100 fabric are
/// single-threaded and this state is copied atomically at each host operation.
#[derive(Clone, Copy, Debug, Default)]
struct RuntimeRamRefreshState {
    previous_sync: bool,
    previous_clock: bool,
    previous_phi2: bool,
    previous_m1: bool,
    previous_data_bus_in: bool,
    refresh_clock_count: u16,
    refresh_pending: bool,
    refresh_active: bool,
    refresh_collision_waits: u8,
    #[cfg(test)]
    refresh_cycles: u64,
    /// Full needs to know whether an internal T-state is the hidden T4 of the
    /// current semantic M1. Exact Partial locates the same slot from pDBIN.
    m1_phi2_count: u8,
}

impl RuntimeRamRefreshState {
    #[inline(always)]
    fn complete_refresh_cycle(&mut self) {
        self.refresh_pending = false;
        #[cfg(test)]
        {
            self.refresh_cycles = self.refresh_cycles.saturating_add(1);
        }
    }

    fn advance_refresh_timing(
        &mut self,
        model: Option<S100RamBoardModel>,
        sync_rising: bool,
        clock_rising: bool,
        phi2_rising: bool,
        m1: bool,
        data_bus_in_falling: bool,
        run: bool,
        halt_ack: bool,
    ) {
        match model {
            Some(S100RamBoardModel::Mits4KDynamic88_4Mcd) => {
                let S100RamTimingModel::RefreshCollision {
                    interval_clocks,
                    min_waits,
                    max_waits,
                } = S100RamBoardModel::Mits4KDynamic88_4Mcd.timing_model()
                else {
                    unreachable!("88-4MCD must retain refresh-collision timing")
                };

                // CLOC rises at PHI1 while processor SYNC for the same T1 becomes
                // visible later, at PHI2. Preserve which collision phase the most
                // recent CLOC established until that SYNC edge arrives. A refresh
                // that becomes due on the immediately preceding CLOC is the
                // documented two-WAIT case; an older pending refresh is one WAIT.
                if clock_rising {
                    self.refresh_clock_count += 1;
                    if self.refresh_clock_count >= interval_clocks {
                        self.refresh_clock_count = 0;
                        self.refresh_pending = true;
                        self.refresh_collision_waits = max_waits;
                    } else if self.refresh_pending {
                        self.refresh_collision_waits = min_waits;
                    }
                }

                if sync_rising {
                    self.refresh_active = self.refresh_pending;
                    if self.refresh_active {
                        if self.refresh_collision_waits == 0 {
                            self.refresh_collision_waits = min_waits;
                        }
                        self.complete_refresh_cycle();
                    } else {
                        self.refresh_collision_waits = 0;
                    }
                }
            }
            Some(S100RamBoardModel::Mits4KSynchronous88S4K) => {
                if phi2_rising {
                    self.refresh_clock_count += 1;
                    if self.refresh_clock_count >= S4K_REFRESH_PHI2_EDGES {
                        self.refresh_clock_count = 0;
                        self.refresh_pending = true;
                    }
                }

                // Later MITS artwork (the November 1976 factory correction)
                // identifies the hidden T4 from the trailing edge of pDBIN while
                // sM1 is asserted. This remains exact even when sM1 never drops
                // between adjacent fetch cycles and when WAIT extends T2/TW.
                let hidden_m1_t4 = run && m1 && data_bus_in_falling;
                let parked_refresh_slot = (!run || halt_ack) && phi2_rising;
                if self.refresh_pending && (hidden_m1_t4 || parked_refresh_slot) {
                    self.complete_refresh_cycle();
                }
            }
            _ => {}
        }
    }

    #[inline(always)]
    fn full_advance_four_mcd_clocks(&mut self, clocks: u32) {
        if clocks == 0 {
            return;
        }
        let S100RamTimingModel::RefreshCollision {
            interval_clocks,
            min_waits,
            max_waits,
        } = S100RamBoardModel::Mits4KDynamic88_4Mcd.timing_model()
        else {
            unreachable!("88-4MCD must retain refresh-collision timing")
        };
        let interval = u32::from(interval_clocks);
        let total = u32::from(self.refresh_clock_count).saturating_add(clocks);
        let crossed_refresh = total >= interval;
        if crossed_refresh {
            self.refresh_pending = true;
        }
        self.refresh_clock_count = (total % interval) as u16;
        if self.refresh_pending {
            self.refresh_collision_waits = if crossed_refresh && total % interval == 0 {
                max_waits
            } else {
                min_waits
            };
        } else {
            self.refresh_collision_waits = 0;
        }
    }

    #[inline(always)]
    fn full_four_mcd_machine_cycle(
        &mut self,
        config: RuntimeRamConfig,
        address: u16,
        memory_access: bool,
        base_t_states: u32,
    ) -> u32 {
        debug_assert!(base_t_states != 0);
        let S100RamTimingModel::RefreshCollision {
            min_waits,
            max_waits,
            ..
        } = S100RamBoardModel::Mits4KDynamic88_4Mcd.timing_model()
        else {
            unreachable!("88-4MCD must retain refresh-collision timing")
        };

        // T1 PHI1 advances CLOC, then T1 PHI2 exposes SYNC. The shared collision
        // latch therefore carries exactly the same phase information as Partial.
        self.full_advance_four_mcd_clocks(1);
        let refresh_at_sync = self.refresh_pending;
        let selected_access = memory_access && config.contains(address);
        let waits = if refresh_at_sync && selected_access {
            match self.refresh_collision_waits {
                waits if waits == max_waits => max_waits,
                waits if waits == min_waits => min_waits,
                _ => min_waits,
            }
        } else {
            0
        };

        if refresh_at_sync {
            self.complete_refresh_cycle();
        }
        self.refresh_active = false;
        self.refresh_collision_waits = 0;
        self.full_advance_four_mcd_clocks(
            base_t_states
                .saturating_sub(1)
                .saturating_add(u32::from(waits)),
        );
        self.previous_sync = false;
        self.previous_clock = false;
        u32::from(waits)
    }

    /// Arithmetic PHI2 fast-forward for the corrected 88-S4K. A semantic M1
    /// callback is an exact machine-cycle boundary even if the latched sM1 bus
    /// level remains high across adjacent fetches. Internal T-states continue
    /// that same M1 without resetting the T4 position. Full advances at most one
    /// 64-edge refresh period in any one arithmetic chunk.
    #[inline(always)]
    fn full_advance_s4k_phi2(&mut self, edges: u32, m1: bool, new_machine_cycle: bool) {
        if m1 && new_machine_cycle {
            self.m1_phi2_count = 0;
        }
        if edges == 0 {
            self.previous_m1 = m1;
            self.previous_phi2 = false;
            return;
        }
        debug_assert!(edges <= u32::from(S4K_REFRESH_PHI2_EDGES));

        let pending_before = self.refresh_pending;
        let count_before = u32::from(self.refresh_clock_count);
        let to_due = u32::from(S4K_REFRESH_PHI2_EDGES) - count_before;
        let became_due = to_due <= edges;
        let total = count_before + edges;
        self.refresh_clock_count = (total % u32::from(S4K_REFRESH_PHI2_EDGES)) as u16;
        if became_due {
            self.refresh_pending = true;
        }

        if m1 {
            let before_m1 = u32::from(self.m1_phi2_count);
            let hidden_edge = (before_m1 < 4).then_some(4 - before_m1);
            self.m1_phi2_count = before_m1
                .saturating_add(edges)
                .min(u32::from(u8::MAX)) as u8;
            if let Some(hidden_edge) = hidden_edge {
                let hidden_slot_reached = edges >= hidden_edge;
                let pending_at_hidden = pending_before || (became_due && to_due <= hidden_edge);
                if hidden_slot_reached && pending_at_hidden {
                    self.complete_refresh_cycle();
                }
            }
        }

        self.previous_m1 = m1;
        self.previous_phi2 = false;
    }

    #[inline(always)]
    fn full_machine_cycle_timing(
        &mut self,
        config: RuntimeRamConfig,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        match config.historical_model() {
            Some(S100RamBoardModel::Mits4KDynamic88_4Mcd) => {
                let waits = self.full_four_mcd_machine_cycle(
                    config,
                    address,
                    memory_access,
                    base_t_states,
                );
                self.previous_m1 = m1;
                self.previous_phi2 = false;
                waits
            }
            Some(S100RamBoardModel::Mits4KSynchronous88S4K) => {
                self.full_advance_s4k_phi2(base_t_states, m1, true);
                0
            }
            _ => 0,
        }
    }

    #[inline(always)]
    fn full_internal_t_states(&mut self, config: RuntimeRamConfig, t_states: u32) {
        if t_states == 0 {
            return;
        }
        match config.historical_model() {
            Some(S100RamBoardModel::Mits4KDynamic88_4Mcd) => {
                self.full_advance_four_mcd_clocks(t_states);
            }
            Some(S100RamBoardModel::Mits4KSynchronous88S4K) => {
                self.full_advance_s4k_phi2(t_states, self.previous_m1, false);
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
struct RuntimeRamState {
    config: RuntimeRamConfig,
    bytes: Vec<u8>,
    protected: Vec<bool>,
    selected_offset: Option<usize>,
    memory_read: bool,
    wait_clocks_remaining: u8,
    previous_memory_read: bool,
    previous_protect: bool,
    previous_unprotect: bool,
    /// Physical connector output is persistent state. Rebuilding eight DI pins
    /// plus PRDY/PROT on every observation was pure host work when those outputs
    /// had not changed. Keep the exact drive latched just like the CPU board.
    cached_drive: S100CardDrive,
}

impl RuntimeRamState {
    fn new(config: RuntimeRamConfig, init: RamInit) -> Self {
        let mut bytes = vec![0; config.populated_bytes()];
        if init == RamInit::Random {
            rand::rng().fill_bytes(&mut bytes);
        }
        let mut state = Self {
            config,
            bytes,
            protected: vec![false; config.protection_unit_count()],
            selected_offset: None,
            memory_read: false,
            wait_clocks_remaining: 0,
            previous_memory_read: false,
            previous_protect: false,
            previous_unprotect: false,
            cached_drive: S100CardDrive::new(),
        };
        state.rebuild_cached_drive();
        state
    }

    fn offset_for(&self, address: u16) -> Option<usize> {
        self.config
            .contains(address)
            .then_some(address.wrapping_sub(self.config.base_address()) as usize)
    }

    fn protection_index_for_offset(&self, offset: usize) -> usize {
        offset / self.config.protection_unit_bytes()
    }

    fn is_offset_protected(&self, offset: usize) -> bool {
        self.protected
            .get(self.protection_index_for_offset(offset))
            .copied()
            .unwrap_or(false)
    }

    fn is_protected(&self, address: u16) -> bool {
        self.offset_for(address)
            .is_some_and(|offset| self.is_offset_protected(offset))
    }

    fn set_protected_raw(&mut self, address: u16, protected: bool) -> bool {
        if !self.config.supports_front_panel_protect() {
            return false;
        }
        let Some(offset) = self.offset_for(address) else {
            return false;
        };
        let index = self.protection_index_for_offset(offset);
        let Some(latch) = self.protected.get_mut(index) else {
            return false;
        };
        *latch = protected;
        true
    }

    fn read_byte(&self, address: u16) -> Option<u8> {
        let offset = self.offset_for(address)?;
        Some(self.bytes[offset])
    }

    fn write_byte_raw(&mut self, address: u16, value: u8, respect_protection: bool) -> bool {
        let Some(offset) = self.offset_for(address) else {
            return false;
        };
        if respect_protection && self.is_offset_protected(offset) {
            return false;
        }
        self.bytes[offset] = value;
        true
    }

    #[inline]
    fn drive_signature(&self) -> RamDriveSignature {
        let data_in = if self.memory_read {
            self.selected_offset.map(|offset| self.bytes[offset])
        } else {
            None
        };
        let ready_low = self.wait_clocks_remaining != 0;
        let protect_high = self.selected_offset.is_some_and(|offset| {
            self.config.supports_front_panel_protect() && self.is_offset_protected(offset)
        });
        (data_in, ready_low, protect_high)
    }

    fn rebuild_cached_drive(&mut self) {
        let (data_in, ready_low, protect_high) = self.drive_signature();
        let mut drive = S100CardDrive::new();
        if let Some(value) = data_in {
            drive.drive_data_in(value);
        }
        drive.pull_low(S100Signal::Ready, ready_low);
        if protect_high {
            drive.drive_tristate(S100Signal::ProtectStatus, Some(true));
        }
        self.cached_drive = drive;
    }

    #[inline]
    fn refresh_cached_drive_if_changed(&mut self, before: RamDriveSignature) {
        if self.drive_signature() != before {
            self.rebuild_cached_drive();
        }
    }

    fn reset_timing(&mut self) {
        self.selected_offset = None;
        self.memory_read = false;
        self.wait_clocks_remaining = 0;
        self.previous_memory_read = false;
        self.previous_protect = false;
        self.previous_unprotect = false;
    }
}

#[derive(Clone)]
pub struct RuntimeRamHandle {
    state: Rc<RefCell<RuntimeRamState>>,
    refresh: Rc<Cell<RuntimeRamRefreshState>>,
    config: RuntimeRamConfig,
}

impl RuntimeRamHandle {
    pub fn config(&self) -> RuntimeRamConfig {
        self.config
    }

    pub fn contains(&self, address: u16) -> bool {
        self.config.contains(address)
    }

    pub fn read_byte(&self, address: u16) -> Option<u8> {
        self.state.borrow().read_byte(address)
    }

    pub fn write_byte(&self, address: u16, value: u8, respect_protection: bool) -> bool {
        let mut state = self.state.borrow_mut();
        let before = state.drive_signature();
        let written = state.write_byte_raw(address, value, respect_protection);
        state.refresh_cached_drive_if_changed(before);
        written
    }

    pub fn is_protected(&self, address: u16) -> bool {
        self.state.borrow().is_protected(address)
    }

    pub fn set_protected(&self, address: u16, protected: bool) -> bool {
        let mut state = self.state.borrow_mut();
        let before = state.drive_signature();
        let changed = state.set_protected_raw(address, protected);
        state.refresh_cached_drive_if_changed(before);
        changed
    }

    pub fn clear_protection(&self) {
        let mut state = self.state.borrow_mut();
        let before = state.drive_signature();
        state.protected.fill(false);
        state.refresh_cached_drive_if_changed(before);
    }

    pub fn initialize(&self, init: RamInit) {
        {
            let mut state = self.state.borrow_mut();
            state.bytes.fill(0);
            if init == RamInit::Random {
                rand::rng().fill_bytes(&mut state.bytes);
            }
            state.protected.fill(false);
            state.reset_timing();
            state.rebuild_cached_drive();
        }
        self.refresh.set(RuntimeRamRefreshState::default());
    }

    pub fn load(&self, address: u16, data: &[u8]) -> usize {
        let mut state = self.state.borrow_mut();
        let Some(first) = state.offset_for(address) else {
            return 0;
        };
        let before = state.drive_signature();
        let len = data.len().min(state.bytes.len().saturating_sub(first));
        state.bytes[first..first + len].copy_from_slice(&data[..len]);
        state.refresh_cached_drive_if_changed(before);
        len
    }

    #[inline(always)]
    pub(crate) fn full_machine_cycle_timing(
        &self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        let mut refresh = self.refresh.get();
        let waits = refresh.full_machine_cycle_timing(
            self.config,
            address,
            memory_access,
            m1,
            base_t_states,
        );
        self.refresh.set(refresh);
        waits
    }

    #[inline(always)]
    pub(crate) fn full_internal_t_states(&self, t_states: u32) {
        let mut refresh = self.refresh.get();
        refresh.full_internal_t_states(self.config, t_states);
        self.refresh.set(refresh);
    }
}

pub struct RuntimeRamCard {
    state: Rc<RefCell<RuntimeRamState>>,
    refresh: Rc<Cell<RuntimeRamRefreshState>>,
    config: RuntimeRamConfig,
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
        let refresh = Rc::new(Cell::new(RuntimeRamRefreshState::default()));
        (
            Self {
                state: Rc::clone(&state),
                refresh: Rc::clone(&refresh),
                config,
            },
            RuntimeRamHandle {
                state,
                refresh,
                config,
            },
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
        Self::descriptor_for(self.config)
    }
}

impl S100ElectricalCard for RuntimeRamCard {
    fn observe_s100(&mut self, sample: &S100BusSample) {
        let mut state = self.state.borrow_mut();
        let mut refresh = self.refresh.get();
        let before = state.drive_signature();
        let sync = sample.signal_level(S100Signal::Sync) == Some(true);
        let clock = sample.signal_level(S100Signal::Clock) == Some(true);
        let phi2 = sample.signal_level(S100Signal::Phi2) == Some(true);
        let m1 = sample.signal_level(S100Signal::M1) == Some(true);
        let data_bus_in = sample.signal_level(S100Signal::DataBusIn) == Some(true);
        let run = sample.signal_level(S100Signal::Run) == Some(true);
        let halt_ack = sample.signal_level(S100Signal::HaltAcknowledge) == Some(true);
        let protect = sample.signal_level(S100Signal::Protect) == Some(true);
        let unprotect = sample.signal_level(S100Signal::Unprotect) == Some(true);
        let sync_rising = sync && !refresh.previous_sync;
        let clock_rising = clock && !refresh.previous_clock;
        let phi2_rising = phi2 && !refresh.previous_phi2;
        let data_bus_in_falling = !data_bus_in && refresh.previous_data_bus_in;

        state.selected_offset = sample
            .address()
            .and_then(|address| state.offset_for(address));
        let memory_read = state.selected_offset.is_some()
            && sample.signal_level(S100Signal::MemoryRead) == Some(true);
        let memory_write = state.selected_offset.is_some()
            && sample.signal_level(S100Signal::MemoryWrite) == Some(true);
        let memory_access = memory_read || memory_write;

        refresh.advance_refresh_timing(
            state.config.historical_model(),
            sync_rising,
            clock_rising,
            phi2_rising,
            m1,
            data_bus_in_falling,
            run,
            halt_ack,
        );

        if let (Some(address), Some(value)) = (sample.address(), sample.data_out()) {
            if memory_write {
                let _ = state.write_byte_raw(address, value, true);
            }
        }

        if state.selected_offset.is_some() && state.config.supports_front_panel_protect() {
            if let Some(address) = sample.address() {
                if protect && !state.previous_protect {
                    let _ = state.set_protected_raw(address, true);
                }
                if unprotect && !state.previous_unprotect {
                    let _ = state.set_protected_raw(address, false);
                }
            }
        }

        match state
            .config
            .historical_model()
            .map(S100RamBoardModel::timing_model)
            .unwrap_or(S100RamTimingModel::FixedReadWaits(state.config.read_wait_states()))
        {
            S100RamTimingModel::FixedReadWaits(fixed_waits) => {
                if !memory_read || fixed_waits == 0 {
                    state.wait_clocks_remaining = 0;
                } else if sync_rising || (!state.previous_memory_read && sync) {
                    state.wait_clocks_remaining = fixed_waits;
                } else if clock_rising && !sync && state.wait_clocks_remaining != 0 {
                    state.wait_clocks_remaining -= 1;
                }
            }
            S100RamTimingModel::RefreshCollision { .. } => {
                if refresh.refresh_active && memory_access && state.wait_clocks_remaining == 0 {
                    state.wait_clocks_remaining = refresh.refresh_collision_waits;
                } else if clock_rising && !sync && state.wait_clocks_remaining != 0 {
                    state.wait_clocks_remaining -= 1;
                    if state.wait_clocks_remaining == 0 {
                        refresh.refresh_active = false;
                        refresh.refresh_collision_waits = 0;
                    }
                } else if state.selected_offset.is_none() {
                    state.wait_clocks_remaining = 0;
                    refresh.refresh_active = false;
                    refresh.refresh_collision_waits = 0;
                }
            }
            S100RamTimingModel::NoWait => {
                state.wait_clocks_remaining = 0;
            }
        }

        state.memory_read = memory_read;
        state.previous_memory_read = memory_read;
        refresh.previous_sync = sync;
        refresh.previous_clock = clock;
        refresh.previous_phi2 = phi2;
        refresh.previous_m1 = m1;
        refresh.previous_data_bus_in = data_bus_in;
        state.previous_protect = protect;
        state.previous_unprotect = unprotect;
        self.refresh.set(refresh);
        state.refresh_cached_drive_if_changed(before);
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

    fn read_drive(address: u16, sync: bool, clock: bool) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.drive_address(address);
        drive.drive_signal(S100Signal::MemoryRead, true);
        drive.drive_signal(S100Signal::MemoryWrite, false);
        drive.drive_signal(S100Signal::Sync, sync);
        drive.drive_signal(S100Signal::Clock, clock);
        drive
    }

    fn write_drive(address: u16, sync: bool, clock: bool, memory_write: bool) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.drive_address(address);
        drive.drive_data_out(0x5a);
        drive.drive_signal(S100Signal::MemoryRead, false);
        drive.drive_signal(S100Signal::MemoryWrite, memory_write);
        drive.drive_signal(S100Signal::Sync, sync);
        drive.drive_signal(S100Signal::Clock, clock);
        drive
    }

    fn observe(card: &mut RuntimeRamCard, drive: S100CardDrive) -> S100BusSample {
        let backplane = S100Backplane::new(0);
        let observed = backplane.resolve_drive_sets(&[drive]);
        card.observe_s100(&observed);
        let card_drive = card.drive_s100();
        backplane.resolve_drive_sets(&[drive, card_drive])
    }

    fn clock_pulse(card: &mut RuntimeRamCard, address: u16) {
        let _ = observe(card, read_drive(address, false, false));
        let _ = observe(card, read_drive(address, false, true));
        let _ = observe(card, read_drive(address, false, false));
    }

    fn s4k_drive(
        address: u16,
        phi2: bool,
        m1: bool,
        data_bus_in: bool,
        run: bool,
        halt_ack: bool,
    ) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.drive_address(address);
        drive.drive_signal(S100Signal::MemoryRead, true);
        drive.drive_signal(S100Signal::MemoryWrite, false);
        drive.drive_signal(S100Signal::Phi2, phi2);
        drive.drive_signal(S100Signal::M1, m1);
        drive.drive_signal(S100Signal::DataBusIn, data_bus_in);
        drive.drive_signal(S100Signal::Run, run);
        drive.drive_signal(S100Signal::HaltAcknowledge, halt_ack);
        drive
    }

    fn s4k_phi2_pulse(
        card: &mut RuntimeRamCard,
        address: u16,
        m1: bool,
        data_bus_in: bool,
        run: bool,
        halt_ack: bool,
    ) {
        let _ = observe(card, s4k_drive(address, false, m1, data_bus_in, run, halt_ack));
        let _ = observe(card, s4k_drive(address, true, m1, data_bus_in, run, halt_ack));
        let _ = observe(card, s4k_drive(address, false, m1, data_bus_in, run, halt_ack));
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
        let observed = backplane.resolve_drive_sets(&[master]);
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
        let observed = backplane.resolve_drive_sets(&[master]);
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
        let observed = backplane.resolve_drive_sets(&[master]);
        card.observe_s100(&observed);
        let resolved = backplane.resolve_drive_sets(&[master, card.drive_s100()]);
        assert_eq!(resolved.data_in(), Some(0xa5));
        assert!(matches!(handle.config(), RuntimeRamConfig::Compatibility(_)));
    }

    #[test]
    fn historical_protection_lives_on_the_shared_physical_card() {
        let (card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits1KStatic88Mcs, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        drop(card);
        assert!(handle.set_protected(0x0010, true));
        assert!(handle.is_protected(0x03ff));
        assert!(!handle.write_byte(0x0010, 0x55, true));
        assert!(handle.write_byte(0x0010, 0x55, false));
        assert_eq!(handle.read_byte(0x0010), Some(0x55));
    }

    #[test]
    fn compatibility_protection_preserves_legacy_one_kilobyte_blocks() {
        let (_card, handle) = RuntimeRamCard::compatibility(
            FastRamCompatibilityConfig::no_wait(0, 8 * 1024),
            RamInit::Zeroed,
        )
        .unwrap();
        assert!(handle.set_protected(0x0410, true));
        assert!(!handle.is_protected(0x03ff));
        assert!(handle.is_protected(0x0400));
        assert!(handle.is_protected(0x07ff));
        assert!(!handle.is_protected(0x0800));
    }

    #[test]
    fn cached_drive_tracks_host_mutation_when_selected() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        let backplane = S100Backplane::new(0);
        let master = read_drive(0x0010, false, false);
        let observed = backplane.resolve_drive_sets(&[master]);
        card.observe_s100(&observed);
        assert_eq!(
            backplane.resolve_drive_sets(&[master, card.drive_s100()]).data_in(),
            Some(0)
        );

        assert!(handle.write_byte(0x0010, 0x5a, false));
        assert_eq!(
            backplane.resolve_drive_sets(&[master, card.drive_s100()]).data_in(),
            Some(0x5a)
        );
    }

    #[test]
    fn four_mcd_refresh_is_invisible_until_a_selected_access_collides() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KDynamic88_4Mcd, 0),
            RamInit::Zeroed,
        )
        .unwrap();

        for _ in 0..32 {
            clock_pulse(&mut card, 0x5000);
        }
        assert_eq!(handle.refresh.get().refresh_cycles, 0);

        let resolved = observe(&mut card, read_drive(0x5000, true, false));
        assert_eq!(resolved.signal_level(S100Signal::Ready), Some(true));
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
        assert!(!handle.refresh.get().refresh_active);

        for _ in 0..32 {
            clock_pulse(&mut card, 0x0010);
        }
        let resolved = observe(&mut card, read_drive(0x0010, true, false));
        assert_eq!(resolved.signal_level(S100Signal::Ready), Some(false));
        assert_eq!(handle.refresh.get().refresh_cycles, 2);
    }

    #[test]
    fn four_mcd_pending_before_sync_inserts_one_wait() {
        let (mut card, _handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KDynamic88_4Mcd, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        for _ in 0..32 {
            clock_pulse(&mut card, 0x0010);
        }
        // One later CLOC with no SYNC proves the request is already pending rather
        // than becoming due on the CLOC immediately preceding this SYNC.
        clock_pulse(&mut card, 0x0010);
        let resolved = observe(&mut card, read_drive(0x0010, true, false));
        assert_eq!(resolved.signal_level(S100Signal::Ready), Some(false));
        let _ = observe(&mut card, read_drive(0x0010, false, false));
        let after_one_wait = observe(&mut card, read_drive(0x0010, false, true));
        assert_eq!(after_one_wait.signal_level(S100Signal::Ready), Some(true));
    }

    #[test]
    fn four_mcd_refresh_due_on_sync_inserts_two_waits() {
        let (mut card, _handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KDynamic88_4Mcd, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        for _ in 0..31 {
            clock_pulse(&mut card, 0x0010);
        }

        // Real 8080/MITS ordering: T1 PHI1 raises CLOC first, then SYNC appears
        // at T1 PHI2. The card must carry the just-due phase across those two
        // separate electrical observations and retain the two-WAIT collision.
        let _ = observe(&mut card, read_drive(0x0010, false, true));
        let resolved = observe(&mut card, read_drive(0x0010, true, false));
        assert_eq!(resolved.signal_level(S100Signal::Ready), Some(false));

        // T2 PHI1 still sees SYNC high, so it must not consume either wait.
        let t2_phi1 = observe(&mut card, read_drive(0x0010, true, true));
        assert_eq!(t2_phi1.signal_level(S100Signal::Ready), Some(false));
        let _ = observe(&mut card, read_drive(0x0010, false, false));

        let first_wait = observe(&mut card, read_drive(0x0010, false, true));
        assert_eq!(first_wait.signal_level(S100Signal::Ready), Some(false));
        let _ = observe(&mut card, read_drive(0x0010, false, false));
        let second_wait = observe(&mut card, read_drive(0x0010, false, true));
        assert_eq!(second_wait.signal_level(S100Signal::Ready), Some(true));
    }

    #[test]
    fn four_mcd_write_collision_keeps_refresh_active_until_mwrt() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KDynamic88_4Mcd, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        for _ in 0..32 {
            clock_pulse(&mut card, 0x0010);
        }
        // Keep this test on the one-WAIT phase; the two-WAIT phase is covered
        // independently above.
        clock_pulse(&mut card, 0x0010);
        let at_sync = observe(&mut card, write_drive(0x0010, true, false, false));
        assert_eq!(at_sync.signal_level(S100Signal::Ready), Some(true));
        let at_mwrt = observe(&mut card, write_drive(0x0010, false, false, true));
        assert_eq!(at_mwrt.signal_level(S100Signal::Ready), Some(false));
        assert_eq!(handle.read_byte(0x0010), Some(0x5a));
        let released = observe(&mut card, write_drive(0x0010, false, true, true));
        assert_eq!(released.signal_level(S100Signal::Ready), Some(true));
    }

    #[test]
    fn full_four_mcd_timing_preserves_both_collision_phases() {
        let (_card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KDynamic88_4Mcd, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        handle.full_internal_t_states(31);
        assert_eq!(handle.full_machine_cycle_timing(0x0010, true, true, 4), 2);
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
        handle.initialize(RamInit::Zeroed);
        handle.full_internal_t_states(32);
        assert_eq!(handle.full_machine_cycle_timing(0x0010, true, true, 4), 1);
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
    }

    #[test]
    fn s4k_refresh_uses_pdbin_falling_edge_as_hidden_t4() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KSynchronous88S4K, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        for _ in 0..S4K_REFRESH_PHI2_EDGES {
            s4k_phi2_pulse(&mut card, 0x0010, false, false, true, false);
        }
        assert!(handle.refresh.get().refresh_pending);
        assert_eq!(handle.refresh.get().refresh_cycles, 0);

        let _ = observe(&mut card, s4k_drive(0x0010, false, true, true, true, false));
        let resolved = observe(&mut card, s4k_drive(0x0010, false, true, false, true, false));
        assert_eq!(resolved.signal_level(S100Signal::Ready), Some(true));
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
        assert!(!handle.refresh.get().refresh_pending);
    }

    #[test]
    fn s4k_request_after_pdbin_fall_waits_for_next_m1_t4() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KSynchronous88S4K, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        for _ in 0..63 {
            s4k_phi2_pulse(&mut card, 0x0010, false, false, true, false);
        }
        let _ = observe(&mut card, s4k_drive(0x0010, false, true, true, true, false));
        let _ = observe(&mut card, s4k_drive(0x0010, false, true, false, true, false));
        assert_eq!(handle.refresh.get().refresh_cycles, 0);
        assert!(!handle.refresh.get().refresh_pending);

        s4k_phi2_pulse(&mut card, 0x0010, true, false, true, false);
        assert!(handle.refresh.get().refresh_pending);
        assert_eq!(handle.refresh.get().refresh_cycles, 0);

        let _ = observe(&mut card, s4k_drive(0x0010, false, true, true, true, false));
        let _ = observe(&mut card, s4k_drive(0x0010, false, true, false, true, false));
        assert!(!handle.refresh.get().refresh_pending);
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
    }

    #[test]
    fn full_s4k_timing_consumes_pending_refresh_without_waiting() {
        let (_card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KSynchronous88S4K, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        handle.full_internal_t_states(64);
        assert!(handle.refresh.get().refresh_pending);
        assert_eq!(handle.full_machine_cycle_timing(0x0010, true, true, 4), 0);
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
        assert!(!handle.refresh.get().refresh_pending);
    }

    #[test]
    fn full_s4k_request_born_on_m1_t5_remains_pending_until_next_m1() {
        let (_card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KSynchronous88S4K, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        handle.full_internal_t_states(59);
        assert_eq!(handle.full_machine_cycle_timing(0x0010, true, true, 4), 0);
        assert!(!handle.refresh.get().refresh_pending);
        handle.full_internal_t_states(1);
        assert!(handle.refresh.get().refresh_pending);
        assert_eq!(handle.refresh.get().refresh_cycles, 0);
        assert_eq!(handle.full_machine_cycle_timing(0x0010, true, false, 3), 0);
        assert!(handle.refresh.get().refresh_pending);
        assert_eq!(handle.full_machine_cycle_timing(0x0010, true, true, 4), 0);
        assert!(!handle.refresh.get().refresh_pending);
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
    }

    #[test]
    fn s4k_pending_refresh_completes_while_cpu_is_stopped() {
        let (mut card, handle) = RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KSynchronous88S4K, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        for _ in 0..S4K_REFRESH_PHI2_EDGES {
            s4k_phi2_pulse(&mut card, 0x0010, false, false, true, false);
        }
        assert!(handle.refresh.get().refresh_pending);
        s4k_phi2_pulse(&mut card, 0x0010, false, false, false, false);
        assert_eq!(handle.refresh.get().refresh_cycles, 1);
        assert!(!handle.refresh.get().refresh_pending);
    }
}
