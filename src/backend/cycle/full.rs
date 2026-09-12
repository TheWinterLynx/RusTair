use std::sync::OnceLock;

use crate::adaptive_metrics::{self, AdaptiveFallbackReason};
use crate::config::S100InstalledCardConfig;
use crate::cpu8080::Bus;
use crate::cpu8080_cycle::{Cpu8080Cycle, Cpu8080Pins};
use crate::machine::{AltairBus, FullPanelDuty};

#[cfg(test)]
#[path = "full/panel_histogram_reference.rs"]
mod panel_histogram_reference;
#[cfg(test)]
#[path = "full/control_flow_tests.rs"]
mod control_flow_tests;
#[cfg(test)]
#[path = "full/ei_tests.rs"]
mod ei_tests;
use crate::s100_memory::S100RamBoardModel;

use super::super::BackendResult;
use super::CycleAccurateMachineBackend;

/// No supported Intel 8080 instruction exceeds 18 T-states without external
/// waits (XTHL is the longest). A selected 88-4MCD can add at most one refresh
/// collision per instruction because its 32-CLOC period is longer than any 8080
/// instruction; that collision contributes at most two TW states. Other RAM
/// admitted to Full is processor-transparent at our digital claim boundary.
const FULL_EXECUTION_MAX_T_STATES: u32 = 18;
const FULL_EXECUTION_REFRESH_MAX_T_STATES: u32 = 20;
const EI_OPCODE: u8 = 0xfb;
const LHLD_OPCODE: u8 = 0x2a;
const EI_LHLD_T_STATES: u32 = 20;
const EI_LHLD_MAX_REFRESH_WAITS: u32 = 2;
const FULL_READ_CACHE_ENTRIES: usize = 64;
const FULL_PROTECTION_CACHE_ENTRIES: usize = 64;

#[derive(Clone, Copy)]
struct FullReadCacheEntry(u32);

// Address occupies bits 8..23, data bits 0..7. The invalid tag is outside
// the entire 16-bit address space, including FFFFh; entries remain four bytes.
const EMPTY_FULL_READ_CACHE_ENTRY: FullReadCacheEntry = FullReadCacheEntry(u32::MAX);

#[derive(Clone, Copy)]
struct FullProtectionCacheEntry {
    address: u16,
    protected: bool,
    valid: bool,
}

const EMPTY_FULL_PROTECTION_CACHE_ENTRY: FullProtectionCacheEntry = FullProtectionCacheEntry {
    address: 0,
    protected: false,
    valid: false,
};

#[derive(Clone, Copy)]
struct PendingPanelCycle {
    address: u16,
    data: u8,
    status_word: u8,
    t_states: u32,
    reads_data: bool,
    writes_data: bool,
    inte: bool,
    first_data: u8,
    first_status: u8,
    protected: bool,
    internal_tail: u32,
    internal_tail_inte: bool,
}

#[derive(Clone, Copy)]
struct FullWaitSample {
    address: u16,
    data: u8,
    status_word: u8,
    panel_data: u8,
    reads_data: bool,
    writes_data: bool,
    inte: bool,
    t_states: u32,
}

/// Window-local exact front-panel accumulator. Full execution has already proven
/// HOLD/interrupt/UART quiescence. Ordinary machine-cycle duty is folded into
/// weighted counts; the rare 88-4MCD TW states are retained separately and
/// replayed as WAIT-high samples before the final physical boundary is restored.
///
/// We retain the newest machine cycle separately and replay it in chronological
/// order at the final boundary. That preserves both its one-T-state 8212 latch
/// delay and the exact final S-100 presentation state, while all older states are
/// accumulated as independent byte-duty counts in the canonical integrator.
struct FullPanelActivity {
    duty: FullPanelDuty,
    latched_status: u8,
    panel_data: u8,
    pending: Option<PendingPanelCycle>,
    wait_samples: Vec<FullWaitSample>,
}

impl FullPanelActivity {
    fn new(bus: &AltairBus) -> Self {
        Self {
            duty: FullPanelDuty::new(),
            latched_status: bus.raw_s100_status_word(),
            panel_data: bus.raw_panel_data(),
            pending: None,
            wait_samples: Vec::new(),
        }
    }

    #[inline]
    fn project_machine_cycle(
        &mut self,
        address: u16,
        data: u8,
        status_word: u8,
        t_states: u32,
        wait_t_states: u32,
        reads_data: bool,
        writes_data: bool,
        protected: bool,
        inte: bool,
    ) {
        debug_assert!(t_states >= 1);
        debug_assert!(!(reads_data && writes_data));

        let first_data = self.panel_data;
        let first_status = self.latched_status;
        self.latched_status = status_word;
        if reads_data {
            self.panel_data = data;
        }
        self.duty.record_cycle(
            address,
            first_data,
            self.panel_data,
            first_status,
            status_word,
            protected,
            inte,
            t_states,
        );
        if wait_t_states != 0 {
            self.wait_samples.push(FullWaitSample {
                address,
                data,
                status_word,
                panel_data: self.panel_data,
                reads_data,
                writes_data,
                inte,
                t_states: wait_t_states,
            });
        }
        self.pending = Some(PendingPanelCycle {
            address,
            data,
            status_word,
            t_states,
            reads_data,
            writes_data,
            inte,
            first_data,
            first_status,
            protected,
            internal_tail: 0,
            internal_tail_inte: inte,
        });
    }

    /// EI makes INTE effective on the final T-state of the following
    /// instruction. Full has already projected that successor's final external
    /// cycle with the old INTE level when the semantic core calls `set_inte`.
    /// Shorten that retained cycle by one T-state; `instruction_complete` then
    /// projects the exact same ADDRESS/DATA/status lamp state as a one-T tail
    /// using the new INTE level. Any earlier TW sample keeps the old INTE level,
    /// exactly as the physical processor does before the successor's final T.
    fn reserve_final_external_t_state_for_inte_transition(&mut self) {
        let panel_data = self.panel_data;
        let mut pending = self
            .pending
            .expect("delayed EI transition requires a preceding external cycle");
        debug_assert_eq!(pending.internal_tail, 0);
        debug_assert!(pending.t_states >= 2);

        self.duty.remove_cycle(
            pending.address,
            pending.first_data,
            panel_data,
            pending.first_status,
            pending.status_word,
            pending.protected,
            pending.inte,
            pending.t_states,
        );
        pending.t_states -= 1;
        self.duty.record_cycle(
            pending.address,
            pending.first_data,
            panel_data,
            pending.first_status,
            pending.status_word,
            pending.protected,
            pending.inte,
            pending.t_states,
        );
        self.pending = Some(pending);
    }

    fn project_internal_tail(&mut self, t_states: u32, inte: bool) {
        if t_states == 0 {
            return;
        }
        let pending = self
            .pending
            .as_mut()
            .expect("internal Full T-states require a preceding machine cycle");
        if pending.internal_tail != 0 {
            debug_assert_eq!(
                pending.internal_tail_inte, inte,
                "one retained internal tail cannot cross an INTE transition"
            );
        }
        pending.internal_tail += t_states;
        pending.internal_tail_inte = inte;
        self.duty.record_tail(
            pending.address,
            self.panel_data,
            pending.status_word,
            pending.protected,
            inte,
            t_states,
        );
    }

    fn finish(&mut self, bus: &mut AltairBus) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.duty.remove_cycle(
            pending.address,
            pending.first_data,
            self.panel_data,
            pending.first_status,
            pending.status_word,
            pending.protected,
            pending.inte,
            pending.t_states,
        );
        if pending.internal_tail != 0 {
            self.duty.remove_cycle(
                pending.address,
                self.panel_data,
                self.panel_data,
                pending.status_word,
                pending.status_word,
                pending.protected,
                pending.internal_tail_inte,
                pending.internal_tail,
            );
        }
        bus.cycle_full_merge_panel_duty(&self.duty);

        // TWs are rare and cannot affect semantic state inside Full, but WAIT is
        // a physical front-panel lamp. Replay only those retained states through
        // the canonical panel sampler; no RAM/device connector is touched here.
        for sample in self.wait_samples.drain(..) {
            bus.cycle_full_prepare_panel_latch(sample.panel_data, sample.status_word);
            let cpu_data = (sample.reads_data || sample.writes_data).then_some(sample.data);
            let data_in = sample.reads_data.then_some(sample.data);
            let data_out = sample.writes_data.then_some(sample.data);
            for _ in 0..sample.t_states {
                bus.cycle_drive_s100_t_state(
                    Some(sample.address),
                    cpu_data,
                    data_in,
                    data_out,
                    Some(sample.status_word),
                    sample.inte,
                    false,
                    true,
                    false,
                );
            }
        }

        // Prior time is already integrated. Restore the retained latch inputs
        // without inventing a sample, then replay the final non-WAIT cycle so the
        // externally visible boundary is READY high / WAIT low.
        bus.cycle_full_prepare_panel_latch(pending.first_data, pending.first_status);
        bus.cycle_full_project_panel_cycle(
            pending.address,
            pending.data,
            pending.status_word,
            pending.t_states,
            pending.reads_data,
            pending.writes_data,
            pending.inte,
        );
        if pending.internal_tail != 0 {
            bus.cycle_full_project_internal_t_states(
                pending.internal_tail,
                pending.internal_tail_inte,
            );
        }
    }
}

/// Compile the Cycle core's authoritative Full/Partial opcode classifier once.
/// The exact same predicate still defines eligibility; hot execution only turns
/// the repeated decoder walk into one indexed byte lookup.
fn full_opcode_table() -> &'static [bool; 256] {
    static TABLE: OnceLock<[bool; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [false; 256];
        let mut opcode = 0usize;
        while opcode < table.len() {
            table[opcode] = Cpu8080Cycle::full_opcode_class_supported(opcode as u8);
            opcode += 1;
        }
        table
    })
}

#[inline]
fn full_ei_lhld_pair_is_safe(
    bus: &AltairBus,
    address: u16,
    remaining: u32,
    instruction_limit: u32,
    opcode_table: &[bool; 256],
) -> bool {
    let guarded_t_states = EI_LHLD_T_STATES
        .saturating_add(EI_LHLD_MAX_REFRESH_WAITS)
        .saturating_add(instruction_limit);
    if remaining < guarded_t_states || bus.cpu_control_lines().interrupt {
        return false;
    }
    if bus.peek_memory(address.wrapping_add(1)) != Some(LHLD_OPCODE) {
        return false;
    }
    let guard_opcode = bus.peek_memory(address.wrapping_add(4)).unwrap_or(0xff);
    opcode_table[guard_opcode as usize]
}

/// Prepared guest-bus recorder for Cycle Full. Guest memory traffic reaches the
/// same bus-owned S-100 decoder and RuntimeRamCard storage as Partial, while the
/// expensive connector graph remains lazy until an actual synchronization
/// boundary. Front-panel duty is accumulated locally across the semantic window
/// and folded into the canonical integrator only at synchronization boundaries.
struct FullInstructionBus<'a> {
    bus: &'a mut AltairBus,
    inte: bool,
    projected_t_states: u32,
    pending_wait_states: u32,
    dynamic_ram_timing: bool,
    last_projected_address: Option<u16>,
    panel: FullPanelActivity,
    read_cache: [FullReadCacheEntry; FULL_READ_CACHE_ENTRIES],
    protection_possible: bool,
    protection_cache: [FullProtectionCacheEntry; FULL_PROTECTION_CACHE_ENTRIES],
    prefetched_opcode: Option<(u16, u8)>,
    delayed_ei_transition_armed: bool,
}

impl<'a> FullInstructionBus<'a> {
    fn new(bus: &'a mut AltairBus, inte: bool) -> Self {
        let panel = FullPanelActivity::new(bus);
        let hardware = bus.s100_hardware_memory();
        let mut protection_possible = false;
        let mut dynamic_ram_timing = false;
        for (_, card) in hardware.installed_cards() {
            match card {
                S100InstalledCardConfig::Ram(config) => {
                    protection_possible |= config.model.supports_front_panel_protect();
                    dynamic_ram_timing |= matches!(
                        config.model,
                        S100RamBoardModel::Mits4KDynamic88_4Mcd
                            | S100RamBoardModel::Mits4KSynchronous88S4K
                    );
                }
                S100InstalledCardConfig::FastRamCompatibility(_) => {
                    protection_possible = true;
                }
                _ => {}
            }
        }
        Self {
            bus,
            inte,
            projected_t_states: 0,
            pending_wait_states: 0,
            dynamic_ram_timing,
            last_projected_address: None,
            panel,
            read_cache: [EMPTY_FULL_READ_CACHE_ENTRY; FULL_READ_CACHE_ENTRIES],
            protection_possible,
            protection_cache: [EMPTY_FULL_PROTECTION_CACHE_ENTRY; FULL_PROTECTION_CACHE_ENTRIES],
            prefetched_opcode: None,
            delayed_ei_transition_armed: false,
        }
    }

    #[inline]
    fn read_cache_index(address: u16) -> usize {
        let address = address as usize;
        (address ^ (address >> 6)) & (FULL_READ_CACHE_ENTRIES - 1)
    }

    #[inline]
    fn guest_read(&mut self, address: u16) -> u8 {
        let index = Self::read_cache_index(address);
        let cached = self.read_cache[index];
        if cached.0 >> 8 == u32::from(address) {
            return cached.0 as u8;
        }
        let value = self.bus.cycle_full_guest_read(address);
        self.read_cache[index] = FullReadCacheEntry((u32::from(address) << 8) | u32::from(value));
        value
    }

    #[inline]
    fn invalidate_guest_read(&mut self, address: u16) {
        let index = Self::read_cache_index(address);
        if self.read_cache[index].0 >> 8 == u32::from(address) {
            self.read_cache[index] = EMPTY_FULL_READ_CACHE_ENTRY;
        }
    }

    #[inline]
    fn protection_cache_index(address: u16) -> usize {
        let address = address as usize;
        (address ^ (address >> 6)) & (FULL_PROTECTION_CACHE_ENTRIES - 1)
    }

    #[inline]
    fn protected(&mut self, address: u16) -> bool {
        if !self.protection_possible {
            return false;
        }
        let index = Self::protection_cache_index(address);
        let cached = self.protection_cache[index];
        if cached.valid && cached.address == address {
            return cached.protected;
        }
        let protected = self.bus.is_protected(address);
        self.protection_cache[index] = FullProtectionCacheEntry {
            address,
            protected,
            valid: true,
        };
        protected
    }

    #[inline]
    fn prime_t1_read_data_if_stale_memr(&mut self, address: u16, guest_value: u8) {
        if self.panel.latched_status & 0x80 == 0 {
            return;
        }
        if address == u16::MAX {
            if let Some(physical) = self.bus.peek_memory(address) {
                self.panel.panel_data = physical;
            }
            return;
        }
        if guest_value != 0xff || self.bus.peek_memory(address).is_some() {
            self.panel.panel_data = guest_value;
        }
    }

    #[inline]
    fn prime_t1_prewrite_data_if_stale_memr(&mut self, address: u16) {
        if self.panel.latched_status & 0x80 != 0 {
            if let Some(previous) = self.bus.peek_memory(address) {
                self.panel.panel_data = previous;
            }
        }
    }

    #[inline(always)]
    fn project_machine_cycle(
        &mut self,
        address: u16,
        data: u8,
        status_word: u8,
        t_states: u32,
        reads_data: bool,
        writes_data: bool,
    ) {
        let protected = self.protected(address);
        let wait_t_states = if self.dynamic_ram_timing {
            self.bus.cycle_full_ram_machine_cycle_timing(
                address,
                reads_data || writes_data,
                status_word & 0x20 != 0,
                t_states,
            )
        } else {
            0
        };
        self.panel.project_machine_cycle(
            address,
            data,
            status_word,
            t_states,
            wait_t_states,
            reads_data,
            writes_data,
            protected,
            self.inte,
        );
        self.projected_t_states = self
            .projected_t_states
            .saturating_add(t_states)
            .saturating_add(wait_t_states);
        self.pending_wait_states = self.pending_wait_states.saturating_add(wait_t_states);
        self.last_projected_address = Some(address);
    }

    #[inline]
    fn project_internal_tail(&mut self, t_states: u32) {
        if t_states == 0 {
            return;
        }
        if self.last_projected_address.is_none() {
            debug_assert_eq!(t_states, 0);
            return;
        }
        if self.dynamic_ram_timing {
            self.bus.cycle_full_ram_internal_t_states(t_states);
        }
        self.panel.project_internal_tail(t_states, self.inte);
    }

    #[inline]
    fn project_interleaved_internal_t_states(&mut self, t_states: u32) {
        if t_states == 0 {
            return;
        }
        self.project_internal_tail(t_states);
        self.projected_t_states = self.projected_t_states.saturating_add(t_states);
    }

    #[inline]
    fn prime_opcode_fetch(&mut self, address: u16, opcode: u8) {
        debug_assert!(self.prefetched_opcode.is_none());
        self.prefetched_opcode = Some((address, opcode));
    }

    #[inline]
    fn arm_delayed_ei_transition(&mut self) {
        debug_assert!(!self.delayed_ei_transition_armed);
        self.delayed_ei_transition_armed = true;
    }

    #[inline]
    fn project_opcode_fetch(&mut self, address: u16, opcode: u8) {
        self.prime_t1_read_data_if_stale_memr(address, opcode);
        let external_fetch_t_states = if opcode == 0xf3 { 3 } else { 4 };
        self.project_machine_cycle(
            address,
            opcode,
            0xa2,
            external_fetch_t_states,
            true,
            false,
        );
        let post_m1_t5 = opcode & 0xcf == 0xc5
            || opcode & 0xcf == 0xcd
            || opcode & 0xc7 == 0xc4
            || opcode & 0xc7 == 0xc0
            || opcode & 0xc7 == 0xc7;
        if post_m1_t5 {
            self.project_interleaved_internal_t_states(1);
        }
    }

    fn finish(mut self) -> Cpu8080Pins {
        debug_assert!(!self.delayed_ei_transition_armed);
        debug_assert_eq!(self.pending_wait_states, 0);
        let mut pins = Cpu8080Pins {
            inte: self.inte,
            ..Cpu8080Pins::default()
        };
        if let Some(pending) = self.panel.pending.as_ref() {
            pins.address = Some(pending.address);
            pins.data_out = pending.writes_data.then_some(pending.data);
            pins.wr_n = !pending.writes_data;
        }
        self.panel.finish(self.bus);
        pins
    }
}

impl Bus for FullInstructionBus<'_> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        let value = self.guest_read(address);
        self.prime_t1_read_data_if_stale_memr(address, value);
        self.project_machine_cycle(address, value, 0x82, 3, true, false);
        value
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        self.prime_t1_prewrite_data_if_stale_memr(address);
        self.bus.cycle_full_guest_write(address, value);
        self.invalidate_guest_read(address);
        self.project_machine_cycle(address, value, 0x00, 3, false, true);
    }

    fn input(&mut self, _port: u8) -> u8 {
        unreachable!("IN is classified as a Full/Partial synchronization barrier")
    }

    fn output(&mut self, _port: u8, _value: u8) {
        unreachable!("OUT is classified as a Full/Partial synchronization barrier")
    }

    fn set_inte(&mut self, enabled: bool) {
        if enabled && self.delayed_ei_transition_armed {
            if !self.inte {
                self.panel.reserve_final_external_t_state_for_inte_transition();
                debug_assert!(self.projected_t_states != 0);
                self.projected_t_states = self.projected_t_states.saturating_sub(1);
            }
            self.delayed_ei_transition_armed = false;
        }
        self.inte = enabled;
        self.bus.cycle_full_set_inte(enabled);
    }

    #[inline]
    fn opcode_fetch(&mut self, address: u16) -> u8 {
        if let Some((cached_address, opcode)) = self.prefetched_opcode.take() {
            debug_assert_eq!(cached_address, address);
            if cached_address == address {
                self.project_opcode_fetch(address, opcode);
                return opcode;
            }
        }
        let value = self.guest_read(address);
        self.project_opcode_fetch(address, value);
        value
    }

    #[inline]
    fn stack_read(&mut self, address: u16) -> u8 {
        let value = self.guest_read(address);
        self.prime_t1_read_data_if_stale_memr(address, value);
        self.project_machine_cycle(address, value, 0x86, 3, true, false);
        value
    }

    #[inline]
    fn stack_write(&mut self, address: u16, value: u8) {
        self.prime_t1_prewrite_data_if_stale_memr(address);
        self.bus.cycle_full_guest_write(address, value);
        self.invalidate_guest_read(address);
        self.project_machine_cycle(address, value, 0x04, 3, false, true);
    }

    fn halt_ack(&mut self, _address: u16, _opcode: u8) {
        unreachable!("HLT is classified as a Full/Partial synchronization barrier")
    }

    fn interrupt_ack(&mut self, _address: u16, _opcode: u8, _while_halted: bool) {
        unreachable!("interrupt acknowledge is a Full/Partial synchronization barrier")
    }

    #[inline]
    fn take_wait_states(&mut self) -> u32 {
        let waits = self.pending_wait_states;
        self.pending_wait_states = 0;
        waits
    }

    #[inline]
    fn instruction_complete(&mut self, address: u16, _opcode: u8, t_states: u32) {
        debug_assert!(self.projected_t_states <= t_states);
        let residual = t_states.saturating_sub(self.projected_t_states);
        self.project_internal_tail(residual);
        self.projected_t_states = 0;
        self.last_projected_address = None;
        self.bus.cycle_full_instruction_complete(address, t_states);
    }
}

impl CycleAccurateMachineBackend {
    fn compiled_full_chassis_available(&self) -> bool {
        let hardware = self.machine.bus.s100_hardware_memory();
        let mut saw_ram = false;
        let mut ranges: Vec<(u32, u32)> = Vec::new();

        for (_, card) in hardware.installed_cards() {
            let (start, end) = match card {
                S100InstalledCardConfig::Mits8080Cpu => continue,
                S100InstalledCardConfig::Ram(config)
                    if matches!(
                        config.model,
                        S100RamBoardModel::Mits4KDynamic88_4Mcd
                            | S100RamBoardModel::Mits4KSynchronous88S4K
                            | S100RamBoardModel::Mits4KStatic88_4Mcs
                            | S100RamBoardModel::Mits16KStatic88_16Mcs
                            | S100RamBoardModel::Mits16KDynamic88_16Mcd
                    ) =>
                {
                    let start = u32::from(config.base_address);
                    let end = start + config.populated_bytes as u32;
                    (start, end)
                }
                S100InstalledCardConfig::FastRamCompatibility(config)
                    if config.read_wait_states == 0 =>
                {
                    let start = u32::from(config.base_address);
                    (start, start + config.populated_bytes as u32)
                }
                S100InstalledCardConfig::Mits88Sio(_)
                | S100InstalledCardConfig::Mits88TwoSio { .. } => continue,
                _ => return false,
            };
            if ranges
                .iter()
                .any(|&(other_start, other_end)| start < other_end && other_start < end)
            {
                return false;
            }
            ranges.push((start, end));
            saw_ram = true;
        }
        saw_ram
    }

    fn compiled_full_instruction_limit(&self) -> u32 {
        let has_refresh_wait_ram = self
            .machine
            .bus
            .s100_hardware_memory()
            .installed_cards()
            .any(|(_, card)| {
                matches!(
                    card,
                    S100InstalledCardConfig::Ram(config)
                        if config.model == S100RamBoardModel::Mits4KDynamic88_4Mcd
                )
            });
        if has_refresh_wait_ram {
            FULL_EXECUTION_REFRESH_MAX_T_STATES
        } else {
            FULL_EXECUTION_MAX_T_STATES
        }
    }

    fn compiled_full_chassis_has_serial(&self) -> bool {
        self.machine
            .bus
            .s100_hardware_memory()
            .installed_cards()
            .any(|(_, card)| {
                matches!(
                    card,
                    S100InstalledCardConfig::Mits88Sio(_)
                        | S100InstalledCardConfig::Mits88TwoSio { .. }
                )
            })
    }

    fn compiled_serial_timing_is_quiet(&self) -> bool {
        self.machine.bus.serial_timing_is_quiet()
    }

    #[cfg(test)]
    #[inline]
    fn compiled_full_opcode(&mut self, remaining: u32, full_window: bool) -> Option<u8> {
        if !full_window
            || remaining < self.compiled_full_instruction_limit()
            || !self.at_instruction_boundary()
            || self.stop_wait_park_pending
            || self.cpu_fault.is_some()
            || self.machine.bus.cpu_control_lines().reset
        {
            return None;
        }

        if !self.cpu.prepare_full_boundary_after_reset_release() {
            return None;
        }

        let opcode = self
            .machine
            .bus
            .peek_memory(self.cpu.registers().pc)
            .unwrap_or(0xff);
        self.cpu
            .full_execution_opcode_supported(opcode)
            .then_some(opcode)
    }

    #[cfg(test)]
    fn execute_compiled_full_instruction(&mut self, opcode: u8) -> Option<u32> {
        self.instruction_address = self.cpu.registers().pc;
        let inte = self.cpu.interrupts_enabled();

        let (elapsed, boundary_pins) = {
            let cpu = &mut self.cpu;
            let bus = &mut self.machine.bus;
            let mut full_bus = FullInstructionBus::new(bus, inte);
            let elapsed = cpu.execute_full_instruction(&mut full_bus, opcode)?;
            let boundary_pins = full_bus.finish();
            (elapsed, boundary_pins)
        };

        debug_assert!(elapsed <= self.compiled_full_instruction_limit());
        self.cpu.set_full_boundary_pins(boundary_pins);
        self.machine.bus.cycle_mark_full_execution_desynced();
        self.last_teaching_tick = None;
        Some(elapsed)
    }

    fn execute_compiled_full_window(
        &mut self,
        remaining: &mut u32,
        full_window: bool,
        instruction_limit: u32,
    ) -> Option<u64> {
        if !full_window
            || !self.machine.bus.cycle_full_panel_capacity(*remaining)
            || *remaining < instruction_limit
            || !self.at_instruction_boundary()
            || self.stop_wait_park_pending
            || self.cpu_fault.is_some()
            || self.machine.bus.cpu_control_lines().reset
        {
            return None;
        }

        if !self.cpu.prepare_full_boundary_after_reset_release() {
            return None;
        }

        let opcode_table = full_opcode_table();
        let first_address = self.cpu.registers().pc;
        let first_opcode = self
            .machine
            .bus
            .peek_memory(first_address)
            .unwrap_or(0xff);
        let first_is_safe_ei_pair = first_opcode == EI_OPCODE
            && full_ei_lhld_pair_is_safe(
                &self.machine.bus,
                first_address,
                *remaining,
                instruction_limit,
                opcode_table,
            );
        if !opcode_table[first_opcode as usize] && !first_is_safe_ei_pair {
            return None;
        }

        let mut full = self.cpu.begin_full_execution_window()?;
        let inte = full.inte;
        let start_cycles = full.cycles;
        let mut completed = 0u64;
        let mut last_elapsed = 0u32;
        let mut last_address = full.pc;

        let boundary_pins = {
            let bus = &mut self.machine.bus;
            let mut full_bus = FullInstructionBus::new(bus, inte);

            while *remaining >= instruction_limit {
                let opcode_address = full.pc;
                let opcode = full_bus.guest_read(opcode_address);

                if opcode == EI_OPCODE {
                    if !full_ei_lhld_pair_is_safe(
                        &*full_bus.bus,
                        opcode_address,
                        *remaining,
                        instruction_limit,
                        opcode_table,
                    ) {
                        break;
                    }

                    full_bus.prime_opcode_fetch(opcode_address, opcode);
                    let ei_elapsed = full.step(&mut full_bus);
                    debug_assert!(full_bus.prefetched_opcode.is_none());
                    debug_assert!((4..=6).contains(&ei_elapsed));
                    debug_assert!(ei_elapsed <= *remaining);
                    *remaining -= ei_elapsed;
                    completed = completed.saturating_add(1);

                    let successor_address = full.pc;
                    let successor = full_bus.guest_read(successor_address);
                    debug_assert_eq!(successor, LHLD_OPCODE);
                    full_bus.arm_delayed_ei_transition();
                    full_bus.prime_opcode_fetch(successor_address, successor);
                    last_address = successor_address;
                    let successor_elapsed = full.step(&mut full_bus);
                    debug_assert!(full_bus.prefetched_opcode.is_none());
                    debug_assert!(!full_bus.delayed_ei_transition_armed);
                    debug_assert!((16..=18).contains(&successor_elapsed));
                    debug_assert!(successor_elapsed <= *remaining);
                    *remaining -= successor_elapsed;
                    completed = completed.saturating_add(1);
                    last_elapsed = successor_elapsed;

                    debug_assert!(
                        !full_bus.bus.cpu_control_lines().interrupt,
                        "PINT changed inside a supposedly quiescent EI-LHLD Full pair"
                    );
                    continue;
                }

                if !opcode_table[opcode as usize] {
                    break;
                }

                full_bus.prime_opcode_fetch(opcode_address, opcode);
                last_address = opcode_address;
                let elapsed = full.step(&mut full_bus);
                debug_assert!(full_bus.prefetched_opcode.is_none());
                debug_assert!(elapsed <= instruction_limit);
                debug_assert!(elapsed <= *remaining);
                *remaining -= elapsed;
                completed = completed.saturating_add(1);
                last_elapsed = elapsed;
            }

            full_bus.finish()
        };

        if completed == 0 {
            return None;
        }

        let elapsed_total = full.cycles.saturating_sub(start_cycles);
        self.instruction_address = last_address;
        self.cpu
            .commit_full_execution_window(&full, completed, last_elapsed);
        self.cpu.set_full_boundary_pins(boundary_pins);
        self.machine.bus.cycle_mark_full_execution_desynced();
        self.last_teaching_tick = None;
        Some(elapsed_total)
    }

    fn compiled_full_window_blocker(
        &self,
        serial_clocked: bool,
        chassis_available: bool,
    ) -> Option<AdaptiveFallbackReason> {
        if !chassis_available {
            return Some(AdaptiveFallbackReason::ChassisUnsupported);
        }
        if serial_clocked && !self.compiled_serial_timing_is_quiet() {
            return Some(AdaptiveFallbackReason::SerialActive);
        }
        let lines = self.machine.bus.cpu_control_lines();
        if !lines.ready {
            return Some(AdaptiveFallbackReason::ReadyLow);
        }
        if lines.hold {
            return Some(AdaptiveFallbackReason::Hold);
        }
        if lines.interrupt && self.cpu.interrupts_enabled() {
            return Some(AdaptiveFallbackReason::InterruptPending);
        }
        None
    }

    fn compiled_full_fallback_reason(
        &self,
        remaining: u32,
        instruction_limit: u32,
        full_window_blocker: Option<AdaptiveFallbackReason>,
    ) -> AdaptiveFallbackReason {
        if let Some(reason) = full_window_blocker {
            return reason;
        }
        if remaining < instruction_limit {
            return AdaptiveFallbackReason::BudgetTail;
        }
        if !self.at_instruction_boundary() {
            return AdaptiveFallbackReason::NotInstructionBoundary;
        }
        if self.stop_wait_park_pending {
            return AdaptiveFallbackReason::StopWaitPending;
        }
        if self.cpu_fault.is_some() {
            return AdaptiveFallbackReason::CpuFault;
        }
        if self.machine.bus.cpu_control_lines().reset {
            return AdaptiveFallbackReason::Reset;
        }

        let opcode = self
            .machine
            .bus
            .peek_memory(self.cpu.registers().pc)
            .unwrap_or(0xff);
        if !full_opcode_table()[opcode as usize] {
            AdaptiveFallbackReason::OpcodeBarrier
        } else {
            AdaptiveFallbackReason::FullWindowUnavailable
        }
    }

    fn record_partial_metrics_span_until(
        &self,
        partial_start_t: &mut Option<u64>,
        partial_reason: &mut Option<AdaptiveFallbackReason>,
        end_t: u64,
    ) {
        let Some(start_t) = partial_start_t.take() else {
            return;
        };
        let elapsed = end_t.saturating_sub(start_t);
        adaptive_metrics::record_partial_span(
            elapsed,
            partial_reason
                .take()
                .unwrap_or(AdaptiveFallbackReason::FullWindowUnavailable),
        );
    }

    fn record_partial_metrics_span(
        &self,
        partial_start_t: &mut Option<u64>,
        partial_reason: &mut Option<AdaptiveFallbackReason>,
    ) {
        let end_t = self.cpu.total_t_states();
        self.record_partial_metrics_span_until(partial_start_t, partial_reason, end_t);
    }

    pub(super) fn service_execution_compiled(&mut self, t_state_budget: u32) -> BackendResult<()> {
        self.machine.bus.settle_serial_connector_state();
        let lines = self.machine.bus.cpu_control_lines();
        if t_state_budget == 0 || !self.machine.powered || !self.machine.running() || lines.reset {
            return self.fail_if_cpu_fault("service execution");
        }

        let serial_clocked = self.compiled_full_chassis_has_serial();
        let chassis_available = self.compiled_full_chassis_available();
        let instruction_limit = self.compiled_full_instruction_limit();
        let mut remaining = t_state_budget;
        let mut deferred_serial_t_states = 0u64;
        let mut partial_start_t = None;
        let mut partial_reason = None;
        while remaining != 0 && self.machine.running() {
            let at_boundary = self.at_instruction_boundary();
            let full_window_blocker = if at_boundary {
                self.compiled_full_window_blocker(serial_clocked, chassis_available)
            } else {
                None
            };
            let full_window = at_boundary && full_window_blocker.is_none();

            let before_completed = self.cpu.completed_instructions();
            let before_full_t = self.cpu.total_t_states();
            if let Some(elapsed) =
                self.execute_compiled_full_window(&mut remaining, full_window, instruction_limit)
            {
                self.record_partial_metrics_span_until(
                    &mut partial_start_t,
                    &mut partial_reason,
                    before_full_t,
                );
                let completed = self
                    .cpu
                    .completed_instructions()
                    .saturating_sub(before_completed);
                adaptive_metrics::record_full_window(completed, elapsed);
                if serial_clocked {
                    deferred_serial_t_states = deferred_serial_t_states.saturating_add(elapsed);
                }
                continue;
            }

            if partial_start_t.is_none() {
                partial_start_t = Some(self.cpu.total_t_states());
                partial_reason = Some(self.compiled_full_fallback_reason(
                    remaining,
                    instruction_limit,
                    full_window_blocker,
                ));
            }

            if deferred_serial_t_states != 0 {
                self.machine
                    .bus
                    .advance_serial_hardware_time(deferred_serial_t_states);
                deferred_serial_t_states = 0;
            }

            let ready = self.machine.bus.cycle_front_panel_ready_input();
            let trace = self.tick_once(ready);
            remaining -= 1;
            if trace.fault.is_some() {
                self.record_partial_metrics_span(&mut partial_start_t, &mut partial_reason);
                return self.fail_if_cpu_fault("service execution");
            }
            if self.stop_wait_park_pending {
                self.park_physical_stop_at_first_tw();
                break;
            }
        }

        self.record_partial_metrics_span(&mut partial_start_t, &mut partial_reason);
        if deferred_serial_t_states != 0 {
            self.machine
                .bus
                .advance_serial_hardware_time(deferred_serial_t_states);
        }
        self.machine.bus.settle_serial_connector_state();
        self.fail_if_cpu_fault("service execution")
    }

    pub(crate) fn service_execution(&mut self, t_state_budget: u32) -> BackendResult<()> {
        self.service_execution_compiled(t_state_budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_metrics;
    use crate::backend::MachineBackend;
    use crate::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
    use crate::cpu8080_cycle::Registers;
    use crate::s100_chassis::S100ChassisConfig;
    use crate::s100_memory::S100RamCardConfig;

    fn static_4k_hardware() -> S100HardwareConfig {
        let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
                )),
            )
            .unwrap();
        hardware.validate().unwrap()
    }

    fn prepare_static_backend(program: &[u8]) -> CycleAccurateMachineBackend {
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.load_bytes(0, program).unwrap();
        backend.release_reset().unwrap();
        backend.run().unwrap();
        backend
    }

    #[test]
    fn marginal_panel_duty_matches_original_histogram_and_final_state() {
        for seed in 0..8u32 {
            let mut actual = prepare_static_backend(&[0]);
            let mut reference = prepare_static_backend(&[0]);
            let mut marginal = FullPanelActivity::new(&actual.machine.bus);
            let mut histogram =
                panel_histogram_reference::FullPanelActivity::new(&reference.machine.bus);
            let mut random = seed + 1;
            for cycle in 0..1000 {
                random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                let address = ((random >> 8) as u16) & 0x0fff;
                let data = random as u8;
                let (status, states, reads, writes) = match cycle % 5 {
                    0 => (0xa2, 4, true, false),
                    1 => (0x82, 3, true, false),
                    2 => (0x86, 3, true, false),
                    3 => (0x00, 3, false, true),
                    _ => (0x04, 3, false, true),
                };
                let inte = random & 0x100 != 0;
                marginal.project_machine_cycle(
                    address, data, status, states, 0, reads, writes, false, inte,
                );
                histogram.project_machine_cycle(
                    &mut reference.machine.bus,
                    address,
                    data,
                    status,
                    states,
                    reads,
                    writes,
                    false,
                    inte,
                );
                let tail = if cycle == 500 { 1_000_000 } else { random % 3 };
                marginal.project_internal_tail(tail, inte);
                histogram.project_internal_tail(tail);
            }
            marginal.finish(&mut actual.machine.bus);
            histogram.finish(&mut reference.machine.bus);
            assert_eq!(
                actual.machine.bus.raw_panel_lamp_duty(),
                reference.machine.bus.raw_panel_lamp_duty(),
                "seed={seed}"
            );
            assert_eq!(
                actual.machine.bus.raw_s100_status_word(),
                reference.machine.bus.raw_s100_status_word()
            );
            assert_eq!(
                actual.machine.bus.raw_panel_data(),
                reference.machine.bus.raw_panel_data()
            );
        }
    }

    #[test]
    fn full_final_pins_match_last_transfer_and_current_inte() {
        for transfer in 0..6 {
            for inte in [false, true] {
                let mut backend = prepare_static_backend(&[0]);
                let mut bus = FullInstructionBus::new(&mut backend.machine.bus, !inte);
                let mut expected = Cpu8080Pins {
                    inte,
                    ..Cpu8080Pins::default()
                };
                if transfer != 0 {
                    bus.write(0x0100, 0x81);
                    bus.read(0x0200);
                    match transfer {
                        1 => {
                            bus.read(0x0300);
                        }
                        2 => {
                            bus.stack_read(0x0300);
                        }
                        3 => {
                            bus.opcode_fetch(0x0300);
                        }
                        4 => bus.write(0x0300, 0x5a),
                        5 => bus.stack_write(0x0300, 0x5a),
                        _ => unreachable!(),
                    }
                    bus.project_internal_tail(2);
                    expected.address = Some(0x0300);
                    if transfer >= 4 {
                        expected.data_out = Some(0x5a);
                        expected.wr_n = false;
                    }
                }
                bus.set_inte(inte);
                assert_eq!(bus.finish(), expected, "transfer={transfer}, INTE={inte}");
            }
        }
    }

    #[test]
    fn compiled_full_static_ram_executes_whole_instruction_and_rejoins_partial_boundary() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x2a, 0x10, 0x00]).unwrap();
        backend.load_bytes(0x0010, &[0x5a, 0xa5]).unwrap();
        backend.run().unwrap();
        assert!(backend.compiled_full_chassis_available());
        let opcode = backend
            .compiled_full_opcode(FULL_EXECUTION_MAX_T_STATES, true)
            .expect("static 4K chassis must compile");
        assert_eq!(opcode, 0x2a);
        assert_eq!(backend.execute_compiled_full_instruction(opcode), Some(16));
        let registers = backend.cpu.registers();
        assert_eq!((registers.h, registers.l), (0xa5, 0x5a));
        assert_eq!(registers.pc, 3);
        assert_eq!(backend.cpu.total_t_states(), 16);
        assert_eq!(
            backend.cpu.machine_cycle(),
            crate::cpu8080_cycle::MachineCycle::InstructionFetch
        );
        assert_eq!(backend.cpu.t_state(), crate::cpu8080_cycle::TState::T1);
        assert!(backend.last_teaching_tick.is_none());

        backend.service_execution_compiled(1).unwrap();
        assert_eq!(backend.cpu.total_t_states(), 17);
    }

    #[test]
    fn full_read_cache_keeps_conflicting_addresses_and_open_bus_distinct() {
        let mut backend = prepare_static_backend(&[0x12]);
        backend.load_bytes(0x0041, &[0x34]).unwrap();
        backend.load_bytes(0x0fff, &[0x56]).unwrap();
        let mut bus = FullInstructionBus::new(&mut backend.machine.bus, false);
        assert_eq!(
            FullInstructionBus::read_cache_index(0),
            FullInstructionBus::read_cache_index(0x41)
        );
        for _ in 0..3 {
            for (address, expected) in [
                (0xffff, 0xff),
                (0, 0x12),
                (0x41, 0x34),
                (0x0fff, 0x56),
            ] {
                assert_eq!(bus.guest_read(address), expected);
                assert_eq!(bus.guest_read(address), expected);
            }
        }
        bus.write(0, 0xa5);
        assert_eq!(bus.guest_read(0), 0xa5);
        bus.stack_write(0x41, 0x5a);
        assert_eq!(bus.guest_read(0x41), 0x5a);
        assert_eq!(bus.guest_read(0), 0xa5);
        assert_eq!(bus.guest_read(0xffff), 0xff);
        bus.finish();
    }

    #[test]
    fn compiled_full_read_cache_invalidates_on_guest_write() {
        let mut backend = prepare_static_backend(&[0x00]);
        backend
            .machine
            .bus
            .debugger_write_memory(0x0020, 0x11, false);
        let mut full_bus = FullInstructionBus::new(&mut backend.machine.bus, false);
        assert_eq!(full_bus.guest_read(0x0020), 0x11);
        full_bus.write(0x0020, 0x5a);
        assert_eq!(full_bus.guest_read(0x0020), 0x5a);
        let _ = full_bus.finish();
    }

    #[test]
    fn compiled_full_window_executes_many_instructions_before_rejoining_partial() {
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]).unwrap();
        backend.run().unwrap();

        let before = backend.cpu.completed_instructions();
        backend.service_execution_compiled(14_000).unwrap();
        assert_eq!(backend.cpu.total_t_states(), 14_000);
        assert!(backend.cpu.completed_instructions().saturating_sub(before) > 1_000);
        assert_eq!(
            backend.cpu.machine_cycle(),
            crate::cpu8080_cycle::MachineCycle::InstructionFetch
        );
    }

    #[test]
    fn compiled_full_front_panel_duty_matches_forced_partial_including_internal_t5() {
        const BUDGET: u32 = 1_500;
        let program = [0x41, 0xc3, 0x00, 0x00];
        let mut compiled = prepare_static_backend(&program);
        let mut partial = prepare_static_backend(&program);

        compiled.service_execution_compiled(BUDGET).unwrap();
        for _ in 0..BUDGET {
            let ready = partial.machine.bus.cycle_front_panel_ready_input();
            let trace = partial.tick_once(ready);
            assert!(trace.fault.is_none());
        }

        assert_eq!(compiled.cpu.total_t_states(), partial.cpu.total_t_states());
        assert_eq!(compiled.cpu.registers().pc, partial.cpu.registers().pc);
        assert_eq!(
            compiled.machine.bus.raw_panel_lamp_duty(),
            partial.machine.bus.raw_panel_lamp_duty(),
            "Cycle Full must preserve the exact raw front-panel duty of Partial"
        );
    }

    #[test]
    fn compiled_full_push_family_matches_forced_partial_exactly() {
        const BUDGET: u32 = 18;
        const STACK_LO: u16 = 0x07fe;
        const STACK_HI: u16 = 0x07ff;
        let registers = Registers {
            a: 0xde,
            b: 0x12,
            c: 0x34,
            d: 0x56,
            e: 0x78,
            h: 0x9a,
            l: 0xbc,
            f: 0xd7,
            sp: 0x0800,
            pc: 0,
        };

        for opcode in [0xc5, 0xd5, 0xe5, 0xf5] {
            let program = [opcode, 0x00, 0x00, 0x00];
            let mut compiled = prepare_static_backend(&program);
            let mut partial = prepare_static_backend(&program);
            compiled.cpu.set_registers(registers);
            partial.cpu.set_registers(registers);

            adaptive_metrics::begin_measurement();
            compiled.service_execution_compiled(BUDGET).unwrap();
            let stats = adaptive_metrics::end_measurement();
            assert_eq!(
                stats.full_t_states, 11,
                "PUSH {opcode:02x} must be exactly one 11T Full instruction"
            );
            assert_eq!(
                stats.partial_t_states, 7,
                "remaining budget must rejoin exact Partial"
            );
            assert_eq!(
                stats.fallbacks.opcode_barrier,
                0,
                "PUSH {opcode:02x} must not be a Full barrier"
            );

            for _ in 0..BUDGET {
                let ready = partial.machine.bus.cycle_front_panel_ready_input();
                let trace = partial.tick_once(ready);
                assert!(trace.fault.is_none(), "PUSH {opcode:02x} Partial oracle faulted");
            }

            assert_eq!(compiled.cpu.total_t_states(), partial.cpu.total_t_states());
            assert_eq!(compiled.cpu.registers(), partial.cpu.registers());
            assert_eq!(
                compiled.machine.bus.peek_memory(STACK_LO),
                partial.machine.bus.peek_memory(STACK_LO)
            );
            assert_eq!(
                compiled.machine.bus.peek_memory(STACK_HI),
                partial.machine.bus.peek_memory(STACK_HI)
            );
            assert_eq!(
                compiled.machine.bus.raw_panel_lamp_duty(),
                partial.machine.bus.raw_panel_lamp_duty()
            );
        }
    }

    #[test]
    fn compiled_full_clocks_idle_two_sio_exactly_once() {
        let hardware = S100HardwareConfig::historical_8800b_18_slot_starter();
        let mut compiled = CycleAccurateMachineBackend::default();
        let mut reference = CycleAccurateMachineBackend::default();

        for backend in [&mut compiled, &mut reference] {
            backend
                .machine
                .bus
                .configure_s100_hardware_memory(hardware, RamInit::Zeroed)
                .unwrap();
            backend.power(true).unwrap();
            backend.assert_reset().unwrap();
            backend.release_reset().unwrap();
            backend.load_bytes(0, &[0x00, 0xc3, 0x00, 0x00]).unwrap();
            backend.run().unwrap();
        }

        compiled.service_execution_compiled(14_000).unwrap();
        assert_eq!(compiled.cpu.total_t_states(), 14_000);
        reference.machine.bus.advance_serial_hardware_time(14_000);

        for backend in [&mut compiled, &mut reference] {
            backend.machine.bus.debugger_output_port(0x12, 0x15);
            backend.machine.bus.debugger_output_port(0x13, b'P');
        }

        let mut compiled_done = None;
        let mut reference_done = None;
        for elapsed in 1..=5_000u64 {
            compiled.machine.bus.advance_serial_hardware_time(1);
            reference.machine.bus.advance_serial_hardware_time(1);
            if compiled_done.is_none() && compiled.machine.bus.serial_port1_tx_front().is_some() {
                compiled_done = Some(elapsed);
            }
            if reference_done.is_none() && reference.machine.bus.serial_port1_tx_front().is_some() {
                reference_done = Some(elapsed);
            }
            if compiled_done.is_some() && reference_done.is_some() {
                break;
            }
        }

        assert_eq!(compiled_done, reference_done);
        assert!(compiled_done.is_some());
    }

    #[test]
    fn compiled_rechecks_serial_activity_after_partial_out_inside_same_service_call() {
        const BUDGET: u32 = 1_000;
        let hardware = S100HardwareConfig::historical_8800b_18_slot_starter();
        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(hardware, RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend
            .load_bytes(0, &[0x3e, b'X', 0xd3, 0x11, 0x00, 0xc3, 0x04, 0x00])
            .unwrap();
        backend.run().unwrap();

        adaptive_metrics::begin_measurement();
        backend.service_execution_compiled(BUDGET).unwrap();
        let stats = adaptive_metrics::end_measurement();

        assert_eq!(stats.total_t_states(), u64::from(BUDGET));
        assert_eq!(stats.full_t_states, 7);
        assert_eq!(stats.partial_t_states, u64::from(BUDGET - 7));
        assert!(backend.machine.bus.tx_busy());
    }

    #[test]
    fn compiled_full_allows_idle_serial_but_rejects_fixed_wait_ram_and_overlap() {
        let mut wait_hardware =
            S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        wait_hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        wait_hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(S100RamBoardModel::Mits1KStatic88Mcs, 0),
                )),
            )
            .unwrap();

        let mut backend = CycleAccurateMachineBackend::default();
        backend
            .machine
            .bus
            .configure_s100_hardware_memory(wait_hardware, RamInit::Zeroed)
            .unwrap();
        assert!(!backend.compiled_full_chassis_available());

        let mut serial = CycleAccurateMachineBackend::default();
        serial
            .machine
            .bus
            .configure_s100_hardware_memory(
                S100HardwareConfig::historical_8800b_18_slot_starter(),
                RamInit::Zeroed,
            )
            .unwrap();
        assert!(serial.compiled_full_chassis_available());
        assert!(serial.compiled_full_chassis_has_serial());
        assert!(serial.compiled_serial_timing_is_quiet());

        let mut overlap = static_4k_hardware();
        overlap
            .set_slot(
                3,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0),
                )),
            )
            .unwrap();
        let mut overlapped = CycleAccurateMachineBackend::default();
        overlapped
            .machine
            .bus
            .configure_s100_hardware_memory(overlap, RamInit::Zeroed)
            .unwrap();
        assert!(!overlapped.compiled_full_chassis_available());
    }

    #[test]
    fn migration_ram_full_admission_preserves_waits_and_overlap_barriers() {
        use crate::config::FastRamCompatibilityConfig;
        let mut hardware = static_4k_hardware();
        for (waits, base, admitted) in [(0, 0x4000, true), (1, 0x4000, false), (0, 0, false)] {
            hardware
                .set_slot(
                    3,
                    Some(S100InstalledCardConfig::FastRamCompatibility(
                        FastRamCompatibilityConfig {
                            base_address: base,
                            populated_bytes: 4096,
                            read_wait_states: waits,
                        },
                    )),
                )
                .unwrap();
            let mut backend = CycleAccurateMachineBackend::default();
            backend
                .machine
                .bus
                .configure_s100_hardware_memory(hardware, RamInit::Zeroed)
                .unwrap();
            assert_eq!(backend.compiled_full_chassis_available(), admitted);
        }
    }
}