use std::sync::OnceLock;

use crate::adaptive_metrics::{self, AdaptiveFallbackReason};
use crate::config::S100InstalledCardConfig;
use crate::cpu8080::Bus;
use crate::cpu8080_cycle::{Cpu8080Cycle, Cpu8080Pins};
use crate::machine::AltairBus;
use crate::s100_memory::S100RamBoardModel;

use super::CycleAccurateMachineBackend;
use super::super::BackendResult;

/// No supported Intel 8080 instruction exceeds 18 T-states without external
/// wait states (XTHL is the longest). The compiled chassis below admits only
/// no-wait static RAM for memory traffic. Serial cards may be present only while
/// their UART timing is electrically quiet, and the full opcode set deliberately
/// excludes IN/OUT, so reserving 18 T-states still guarantees we never overshoot
/// the caller's exact budget.
const FULL_EXECUTION_MAX_T_STATES: u32 = 18;
const FULL_READ_CACHE_ENTRIES: usize = 64;
const FULL_PROTECTION_CACHE_ENTRIES: usize = 64;
const FULL_PANEL_HISTOGRAM_ENTRIES: usize = 256;

#[derive(Clone, Copy)]
struct FullReadCacheEntry {
    address: u16,
    value: u8,
    valid: bool,
}

const EMPTY_FULL_READ_CACHE_ENTRY: FullReadCacheEntry = FullReadCacheEntry {
    address: 0,
    value: 0,
    valid: false,
};

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
struct FullPanelHistogramEntry {
    key: u64,
    weight: u32,
}

const EMPTY_FULL_PANEL_HISTOGRAM_ENTRY: FullPanelHistogramEntry = FullPanelHistogramEntry {
    key: 0,
    weight: 0,
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
    first_key: u64,
    later_key: u64,
    internal_tail: u32,
}

/// Window-local exact front-panel accumulator. Full execution has already proven
/// READY high, no waits/HOLD and no asynchronous hardware transition inside the
/// block. The physical front panel is therefore a passive observer of ADDRESS,
/// DI and the 8212 status latch. Raw duty depends only on the weighted population
/// of those visible states, not on their host-side update order.
///
/// We retain the newest machine cycle separately and replay it in chronological
/// order at the final boundary. That preserves both its one-T-state 8212 latch
/// delay and the exact final S-100 presentation state, while all older states are
/// coalesced by key and materialized only once per distinct state.
struct FullPanelActivity {
    entries: [FullPanelHistogramEntry; FULL_PANEL_HISTOGRAM_ENTRIES],
    used: usize,
    latched_status: u8,
    panel_data: u8,
    latest_committed_key: Option<u64>,
    pending: Option<PendingPanelCycle>,
}

impl FullPanelActivity {
    fn new(bus: &AltairBus) -> Self {
        Self {
            entries: [EMPTY_FULL_PANEL_HISTOGRAM_ENTRY; FULL_PANEL_HISTOGRAM_ENTRIES],
            used: 0,
            latched_status: bus.raw_s100_status_word(),
            panel_data: bus.raw_panel_data(),
            latest_committed_key: None,
            pending: None,
        }
    }

    #[inline]
    fn state_key(
        address: u16,
        panel_data: u8,
        status_word: u8,
        protected: bool,
        inte: bool,
    ) -> u64 {
        u64::from(address)
            | (u64::from(panel_data) << 16)
            | (u64::from(status_word) << 24)
            | (u64::from(protected) << 32)
            | (u64::from(inte) << 33)
    }

    #[inline]
    fn key_index(key: u64) -> usize {
        let mixed = key ^ (key >> 17) ^ (key >> 37);
        (mixed as usize).wrapping_mul(0x9e37_79b1) & (FULL_PANEL_HISTOGRAM_ENTRIES - 1)
    }

    #[inline]
    fn previous_status_reads_memory(&self) -> bool {
        self.latched_status & 0x80 != 0
    }

    fn replay_entry(bus: &mut AltairBus, entry: FullPanelHistogramEntry) {
        if entry.weight == 0 {
            return;
        }
        let address = entry.key as u16;
        let panel_data = (entry.key >> 16) as u8;
        let status_word = (entry.key >> 24) as u8;
        let expected_protected = entry.key & (1u64 << 32) != 0;
        let inte = entry.key & (1u64 << 33) != 0;

        // This is presentation-only replay into the canonical duty integrator.
        // It does not enter the physical connector resolver or clock a card.
        bus.cycle_drive_s100_t_state(
            Some(address),
            Some(panel_data),
            Some(panel_data),
            None,
            Some(status_word),
            inte,
            true,
            false,
            false,
        );
        debug_assert_eq!(bus.raw_s100_prot(), expected_protected);
        if entry.weight > 1 {
            bus.cycle_full_project_internal_t_states(entry.weight - 1, inte);
        }
    }

    fn flush_histogram(&mut self, bus: &mut AltairBus, preferred_last: Option<u64>) {
        if self.used == 0 {
            return;
        }

        let mut preferred = None;
        for entry in &mut self.entries {
            if entry.weight == 0 {
                continue;
            }
            let current = *entry;
            *entry = EMPTY_FULL_PANEL_HISTOGRAM_ENTRY;
            if Some(current.key) == preferred_last {
                preferred = Some(current);
            } else {
                Self::replay_entry(bus, current);
            }
        }
        if let Some(entry) = preferred {
            Self::replay_entry(bus, entry);
        }
        self.used = 0;
    }

    fn add_histogram(&mut self, bus: &mut AltairBus, key: u64, weight: u32) {
        if weight == 0 {
            return;
        }

        loop {
            let start = Self::key_index(key);
            for probe in 0..FULL_PANEL_HISTOGRAM_ENTRIES {
                let index = (start + probe) & (FULL_PANEL_HISTOGRAM_ENTRIES - 1);
                let entry = &mut self.entries[index];
                if entry.weight == 0 {
                    *entry = FullPanelHistogramEntry { key, weight };
                    self.used += 1;
                    self.latest_committed_key = Some(key);
                    return;
                }
                if entry.key == key {
                    entry.weight = entry.weight.saturating_add(weight);
                    self.latest_committed_key = Some(key);
                    return;
                }
            }

            // The table contains only completed chronological activity. Flush it
            // with the latest state last so the canonical S-100 presentation is
            // still a valid predecessor for whatever Full records next.
            self.flush_histogram(bus, self.latest_committed_key);
        }
    }

    fn commit_pending(&mut self, bus: &mut AltairBus) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.add_histogram(bus, pending.first_key, 1);
        if pending.t_states > 1 {
            self.add_histogram(bus, pending.later_key, pending.t_states - 1);
        }
        if pending.internal_tail != 0 {
            self.add_histogram(bus, pending.later_key, pending.internal_tail);
        }
    }

    fn project_machine_cycle(
        &mut self,
        bus: &mut AltairBus,
        address: u16,
        data: u8,
        status_word: u8,
        t_states: u32,
        reads_data: bool,
        writes_data: bool,
        protected: bool,
        inte: bool,
        write_t1_data_from_s100: Option<u8>,
    ) {
        debug_assert!(t_states >= 1);
        debug_assert!(!(reads_data && writes_data));
        debug_assert!(!writes_data || write_t1_data_from_s100.is_none() || self.previous_status_reads_memory());

        // Once another external machine cycle starts, the previous one can no
        // longer be the final presentation boundary and is safe to coalesce.
        self.commit_pending(bus);

        // A write can begin while the 8212 still exposes the preceding read
        // status. At that T1 address the selected RAM may therefore drive DI
        // before PHI1 latches the write status. Once the write status takes over,
        // DI floats and the front-panel DATA lamps retain that just-seen byte for
        // the rest of the write cycle. Ordinary reads keep their established
        // T1/T2 timing and are intentionally not changed here.
        if writes_data {
            if let Some(t1_data) = write_t1_data_from_s100 {
                self.panel_data = t1_data;
            }
        }

        let first_key = Self::state_key(
            address,
            self.panel_data,
            self.latched_status,
            protected,
            inte,
        );
        self.latched_status = status_word;
        if reads_data {
            self.panel_data = data;
        }
        let later_key = Self::state_key(
            address,
            self.panel_data,
            self.latched_status,
            protected,
            inte,
        );
        self.pending = Some(PendingPanelCycle {
            address,
            data,
            status_word,
            t_states,
            reads_data,
            writes_data,
            inte,
            first_key,
            later_key,
            internal_tail: 0,
        });
    }

    fn project_internal_tail(&mut self, t_states: u32) {
        if t_states == 0 {
            return;
        }
        let pending = self
            .pending
            .as_mut()
            .expect("internal Full T-states require a preceding machine cycle");
        pending.internal_tail = pending.internal_tail.saturating_add(t_states);
    }

    fn replay_final_write_cycle(bus: &mut AltairBus, pending: PendingPanelCycle) {
        debug_assert!(pending.writes_data);
        let first_panel_data = (pending.first_key >> 16) as u8;
        let first_status = (pending.first_key >> 24) as u8;

        // T1: CPU presents the new write status on DO while the 8212 still shows
        // the previous status. If that previous status was sMEMR, DI already
        // contains the pre-write RAM byte captured by the local recorder.
        bus.cycle_drive_s100_t_state(
            Some(pending.address),
            Some(pending.status_word),
            Some(first_panel_data),
            Some(pending.status_word),
            Some(first_status),
            pending.inte,
            true,
            false,
            false,
        );

        // T2/T3: the 8212 has latched the write status, DI is released, and the
        // write byte is on CPU D/DO. DATA lamps retain the T1 DI byte.
        for index in 1..pending.t_states {
            bus.cycle_drive_s100_t_state(
                Some(pending.address),
                Some(pending.data),
                None,
                Some(pending.data),
                (index == 1).then_some(pending.status_word),
                pending.inte,
                true,
                false,
                false,
            );
        }
    }

    fn finish(&mut self, bus: &mut AltairBus) {
        let Some(pending) = self.pending.take() else {
            self.flush_histogram(bus, self.latest_committed_key);
            return;
        };

        // Every state before the final machine cycle may be replayed in any order
        // for raw duty, but end with the true predecessor so the canonical helper
        // sees exactly the 8212/panel DATA state that existed before final T1.
        self.flush_histogram(bus, self.latest_committed_key);
        if pending.writes_data {
            // Final writes need the same pre-PHI1 DI retention as completed writes.
            // Materialize their three exact visible T-states directly so the bus
            // also rejoins Partial with the real write direction still presented.
            Self::replay_final_write_cycle(bus, pending);
        } else {
            bus.cycle_full_project_panel_cycle(
                pending.address,
                pending.data,
                pending.status_word,
                pending.t_states,
                pending.reads_data,
                pending.writes_data,
                pending.inte,
            );
        }
        if pending.internal_tail != 0 {
            bus.cycle_full_project_internal_t_states(pending.internal_tail, pending.inte);
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

/// Prepared guest-bus recorder for Cycle Full. Guest memory traffic reaches the
/// same bus-owned S-100 decoder and RuntimeRamCard storage as Partial, while the
/// expensive connector graph remains lazy until an actual synchronization
/// boundary. Front-panel duty is accumulated locally across the semantic window
/// and folded into the canonical integrator only at synchronization boundaries.
struct FullInstructionBus<'a> {
    bus: &'a mut AltairBus,
    inte: bool,
    boundary_pins: Cpu8080Pins,
    projected_t_states: u32,
    last_projected_address: Option<u16>,
    panel: FullPanelActivity,
    read_cache: [FullReadCacheEntry; FULL_READ_CACHE_ENTRIES],
    protection_cache: [FullProtectionCacheEntry; FULL_PROTECTION_CACHE_ENTRIES],
    prefetched_opcode: Option<(u16, u8)>,
}

impl<'a> FullInstructionBus<'a> {
    fn new(bus: &'a mut AltairBus, inte: bool) -> Self {
        let mut boundary_pins = Cpu8080Pins::default();
        boundary_pins.inte = inte;
        let panel = FullPanelActivity::new(bus);
        Self {
            bus,
            inte,
            boundary_pins,
            projected_t_states: 0,
            last_projected_address: None,
            panel,
            read_cache: [EMPTY_FULL_READ_CACHE_ENTRY; FULL_READ_CACHE_ENTRIES],
            protection_cache: [EMPTY_FULL_PROTECTION_CACHE_ENTRY; FULL_PROTECTION_CACHE_ENTRIES],
            prefetched_opcode: None,
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
        if cached.valid && cached.address == address {
            return cached.value;
        }
        let value = self.bus.cycle_full_guest_read(address);
        self.read_cache[index] = FullReadCacheEntry {
            address,
            value,
            valid: true,
        };
        value
    }

    #[inline]
    fn invalidate_guest_read(&mut self, address: u16) {
        let index = Self::read_cache_index(address);
        if self.read_cache[index].valid && self.read_cache[index].address == address {
            self.read_cache[index].valid = false;
        }
    }

    #[inline]
    fn protection_cache_index(address: u16) -> usize {
        let address = address as usize;
        (address ^ (address >> 6)) & (FULL_PROTECTION_CACHE_ENTRIES - 1)
    }

    #[inline]
    fn protected(&mut self, address: u16) -> bool {
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
        let write_t1_data_from_s100 = if writes_data && self.panel.previous_status_reads_memory() {
            self.bus.peek_memory(address)
        } else {
            None
        };
        self.panel.project_machine_cycle(
            self.bus,
            address,
            data,
            status_word,
            t_states,
            reads_data,
            writes_data,
            protected,
            self.inte,
            write_t1_data_from_s100,
        );
        self.projected_t_states = self.projected_t_states.saturating_add(t_states);
        self.last_projected_address = Some(address);
    }

    #[inline]
    fn project_internal_tail(&mut self, t_states: u32) {
        let Some(address) = self.last_projected_address else {
            debug_assert_eq!(t_states, 0);
            return;
        };
        if t_states == 0 {
            return;
        }
        self.panel.project_internal_tail(t_states);
        // T4/T5/internal states release package D and write strobes while ADDRESS
        // and the latched 8212 status remain at the preceding external cycle.
        // This matters for XTHL, whose final StackWrite continues through T4/T5.
        self.remember_internal_boundary(address);
    }

    #[inline]
    fn project_internal_gap(&mut self, t_states: u32) {
        if t_states == 0 {
            return;
        }
        self.project_internal_tail(t_states);
        self.projected_t_states = self.projected_t_states.saturating_add(t_states);
    }

    #[inline]
    fn remember_read_boundary(&mut self, address: u16) {
        self.boundary_pins = Cpu8080Pins {
            phi1: false,
            phi2: false,
            address: Some(address),
            data_out: None,
            sync: false,
            dbin: false,
            wr_n: true,
            inte: self.inte,
            wait: false,
            hlda: false,
        };
    }

    #[inline]
    fn remember_write_boundary(&mut self, address: u16, value: u8) {
        self.boundary_pins = Cpu8080Pins {
            phi1: false,
            phi2: false,
            address: Some(address),
            data_out: Some(value),
            sync: false,
            dbin: false,
            wr_n: false,
            inte: self.inte,
            wait: false,
            hlda: false,
        };
    }

    #[inline]
    fn remember_internal_boundary(&mut self, address: u16) {
        self.boundary_pins = Cpu8080Pins {
            phi1: false,
            phi2: false,
            address: Some(address),
            data_out: None,
            sync: false,
            dbin: false,
            wr_n: true,
            inte: self.inte,
            wait: false,
            hlda: false,
        };
    }

    #[inline]
    fn prime_opcode_fetch(&mut self, address: u16, opcode: u8) {
        debug_assert!(self.prefetched_opcode.is_none());
        self.prefetched_opcode = Some((address, opcode));
    }

    #[inline]
    fn finish_opcode_fetch(&mut self, address: u16, opcode: u8) -> u8 {
        self.project_machine_cycle(address, opcode, 0xa2, 4, true, false);
        self.remember_read_boundary(address);
        self.project_internal_gap(Cpu8080Cycle::full_post_fetch_internal_t_states(opcode));
        opcode
    }

    fn finish(mut self) -> Cpu8080Pins {
        self.panel.finish(self.bus);
        self.boundary_pins
    }
}

impl Bus for FullInstructionBus<'_> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        let value = self.guest_read(address);
        self.project_machine_cycle(address, value, 0x82, 3, true, false);
        self.remember_read_boundary(address);
        value
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        // Capture the pre-write T1 DI level before the guest RAM byte changes.
        self.project_machine_cycle(address, value, 0x00, 3, false, true);
        self.bus.cycle_full_guest_write(address, value);
        self.invalidate_guest_read(address);
        self.remember_write_boundary(address, value);
    }

    fn input(&mut self, _port: u8) -> u8 {
        unreachable!("IN is classified as a Full/Partial synchronization barrier")
    }

    fn output(&mut self, _port: u8, _value: u8) {
        unreachable!("OUT is classified as a Full/Partial synchronization barrier")
    }

    fn set_inte(&mut self, enabled: bool) {
        self.inte = enabled;
        self.bus.cycle_full_set_inte(enabled);
        self.boundary_pins.inte = enabled;
    }

    #[inline]
    fn opcode_fetch(&mut self, address: u16) -> u8 {
        if let Some((cached_address, opcode)) = self.prefetched_opcode.take() {
            debug_assert_eq!(cached_address, address);
            if cached_address == address {
                return self.finish_opcode_fetch(address, opcode);
            }
        }
        let value = self.guest_read(address);
        self.finish_opcode_fetch(address, value)
    }

    #[inline]
    fn stack_read(&mut self, address: u16) -> u8 {
        let value = self.guest_read(address);
        self.project_machine_cycle(address, value, 0x86, 3, true, false);
        self.remember_read_boundary(address);
        value
    }

    #[inline]
    fn stack_write(&mut self, address: u16, value: u8) {
        // Stack writes have the same stale-sMEMR T1 behavior as ordinary writes.
        self.project_machine_cycle(address, value, 0x04, 3, false, true);
        self.bus.cycle_full_guest_write(address, value);
        self.invalidate_guest_read(address);
        self.remember_write_boundary(address, value);
    }

    fn halt_ack(&mut self, _address: u16, _opcode: u8) {
        unreachable!("HLT is classified as a Full/Partial synchronization barrier")
    }

    fn interrupt_ack(&mut self, _address: u16, _opcode: u8, _while_halted: bool) {
        unreachable!("interrupt acknowledge is a Full/Partial synchronization barrier")
    }

    #[inline]
    fn take_wait_states(&mut self) -> u32 { 0 }

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
            match card {
                S100InstalledCardConfig::Mits8080Cpu => {}
                S100InstalledCardConfig::Ram(config)
                    if matches!(
                        config.model,
                        S100RamBoardModel::Mits4KStatic88_4Mcs
                            | S100RamBoardModel::Mits16KStatic88_16Mcs
                    ) =>
                {
                    let start = u32::from(config.base_address);
                    let end = start + config.populated_bytes as u32;
                    if ranges
                        .iter()
                        .any(|&(other_start, other_end)| start < other_end && other_start < end)
                    {
                        return false;
                    }
                    ranges.push((start, end));
                    saw_ram = true;
                }
                S100InstalledCardConfig::Mits88Sio(_)
                | S100InstalledCardConfig::Mits88TwoSio { .. } => {}
                _ => return false,
            }
        }
        saw_ram
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
        self.machine.bus.serial_rx_line_idle()
            && !self.machine.bus.tx_busy()
            && self.machine.bus.serial_port1_rx_line_idle()
            && !self.machine.bus.serial_port1_tx_busy()
    }

    #[cfg(test)]
    #[inline]
    fn compiled_full_opcode(&mut self, remaining: u32, full_window: bool) -> Option<u8> {
        if !full_window
            || remaining < FULL_EXECUTION_MAX_T_STATES
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

        debug_assert!(elapsed <= FULL_EXECUTION_MAX_T_STATES);
        self.cpu.set_full_boundary_pins(boundary_pins);
        self.machine.bus.cycle_mark_full_execution_desynced();
        self.last_teaching_tick = None;
        Some(elapsed)
    }

    fn execute_compiled_full_window(
        &mut self,
        remaining: &mut u32,
        full_window: bool,
    ) -> Option<u64> {
        if !full_window
            || *remaining < FULL_EXECUTION_MAX_T_STATES
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
        let first_opcode = self
            .machine
            .bus
            .peek_memory(self.cpu.registers().pc)
            .unwrap_or(0xff);
        if !opcode_table[first_opcode as usize] {
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

            while *remaining >= FULL_EXECUTION_MAX_T_STATES {
                let opcode_address = full.pc;
                let opcode = full_bus.guest_read(opcode_address);
                if !opcode_table[opcode as usize] {
                    break;
                }

                full_bus.prime_opcode_fetch(opcode_address, opcode);
                last_address = opcode_address;
                let elapsed = full.step(&mut full_bus);
                debug_assert!(full_bus.prefetched_opcode.is_none());
                debug_assert!(elapsed <= FULL_EXECUTION_MAX_T_STATES);
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
        full_window_blocker: Option<AdaptiveFallbackReason>,
    ) -> AdaptiveFallbackReason {
        if let Some(reason) = full_window_blocker { return reason; }
        if remaining < FULL_EXECUTION_MAX_T_STATES { return AdaptiveFallbackReason::BudgetTail; }
        if !self.at_instruction_boundary() { return AdaptiveFallbackReason::NotInstructionBoundary; }
        if self.stop_wait_park_pending { return AdaptiveFallbackReason::StopWaitPending; }
        if self.cpu_fault.is_some() { return AdaptiveFallbackReason::CpuFault; }
        if self.machine.bus.cpu_control_lines().reset { return AdaptiveFallbackReason::Reset; }

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
        let Some(start_t) = partial_start_t.take() else { return; };
        let elapsed = end_t.saturating_sub(start_t);
        adaptive_metrics::record_partial_span(
            elapsed,
            partial_reason.take().unwrap_or(AdaptiveFallbackReason::FullWindowUnavailable),
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

    pub(super) fn service_execution_compiled(
        &mut self,
        t_state_budget: u32,
    ) -> BackendResult<()> {
        self.machine.bus.refresh_interrupt_request_line();
        let lines = self.machine.bus.cpu_control_lines();
        if t_state_budget == 0
            || !self.machine.powered
            || !self.machine.running
            || lines.reset
        {
            return self.fail_if_cpu_fault("service execution");
        }

        let serial_clocked = self.compiled_full_chassis_has_serial();
        let chassis_available = self.compiled_full_chassis_available();
        let mut remaining = t_state_budget;
        let mut deferred_serial_t_states = 0u64;
        let mut partial_start_t = None;
        let mut partial_reason = None;
        while remaining != 0 && self.machine.running {
            // Full may only start at an instruction boundary, and all dynamic
            // electrical blockers are re-evaluated at that boundary. In
            // particular an exact Partial OUT can make a UART active inside this
            // same service call; a stale entry-time `serial_quiet` decision must
            // never let Full skip over that card timing.
            let at_boundary = self.at_instruction_boundary();
            let full_window_blocker = if at_boundary {
                self.compiled_full_window_blocker(serial_clocked, chassis_available)
            } else {
                None
            };
            let full_window = at_boundary && full_window_blocker.is_none();

            let before_completed = self.cpu.completed_instructions();
            let before_full_t = self.cpu.total_t_states();
            if let Some(elapsed) = self.execute_compiled_full_window(&mut remaining, full_window) {
                // execute_compiled_full_window commits its T-states before returning.
                // Close any preceding Partial span at the exact pre-Full boundary
                // so those T-states are not counted once as Partial and again as Full.
                self.record_partial_metrics_span_until(
                    &mut partial_start_t,
                    &mut partial_reason,
                    before_full_t,
                );
                let completed = self.cpu.completed_instructions().saturating_sub(before_completed);
                adaptive_metrics::record_full_window(completed, elapsed);
                if serial_clocked {
                    deferred_serial_t_states = deferred_serial_t_states.saturating_add(elapsed);
                }
                continue;
            }

            if partial_start_t.is_none() {
                partial_start_t = Some(self.cpu.total_t_states());
                partial_reason = Some(self.compiled_full_fallback_reason(remaining, full_window_blocker));
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
        self.machine.bus.refresh_interrupt_request_line();
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
    use crate::s100_chassis::S100ChassisConfig;
    use crate::s100_memory::S100RamCardConfig;

    fn static_4k_hardware() -> S100HardwareConfig {
        let mut hardware =
            S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits4KStatic88_4Mcs,
                        0,
                    ),
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
        backend
            .load_bytes(0, &[0x2a, 0x10, 0x00])
            .unwrap();
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
        assert_eq!(backend.cpu.machine_cycle(), crate::cpu8080_cycle::MachineCycle::InstructionFetch);
        assert_eq!(backend.cpu.t_state(), crate::cpu8080_cycle::TState::T1);
        assert!(backend.last_teaching_tick.is_none());

        backend.service_execution_compiled(1).unwrap();
        assert_eq!(backend.cpu.total_t_states(), 17);
    }

    #[test]
    fn compiled_full_read_cache_invalidates_on_guest_write() {
        let mut backend = prepare_static_backend(&[0x00]);
        backend.machine.bus.debugger_write_memory(0x0020, 0x11, false);
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
        assert_eq!(backend.cpu.machine_cycle(), crate::cpu8080_cycle::MachineCycle::InstructionFetch);
    }

    #[test]
    fn compiled_full_cpu_only_stack_control_flow_matches_forced_partial() {
        const BUDGET: u32 = 40_000;
        let mut program = [0u8; 0x40];
        // Main setup then a loop containing every newly admitted family:
        // DI, PUSH, DAD, XTHL, CALL, conditional RET and RST. The two XTHLs
        // restore both HL and the stack so the loop remains deterministic.
        program[..22].copy_from_slice(&[
            0x31, 0xf0, 0x03, // LXI SP,03F0h
            0x21, 0x01, 0x00, // LXI H,0001h
            0x01, 0x02, 0x00, // LXI B,0002h
            0xf3,             // DI
            0xc5,             // PUSH B
            0x09,             // DAD B
            0xe3,             // XTHL
            0xe3,             // XTHL
            0xc1,             // POP B
            0xcd, 0x20, 0x00, // CALL 0020h
            0xff,             // RST 7 -> 0038h
            0xc3, 0x0a, 0x00, // JMP PUSH B
        ]);
        program[0x20..0x23].copy_from_slice(&[
            0xaf, // XRA A: set Z
            0xc0, // RNZ: not taken (5 T)
            0xc8, // RZ: taken (11 T), returns to caller
        ]);
        program[0x38] = 0xc9; // RET from RST 7

        let mut compiled = prepare_static_backend(&program);
        let mut partial = prepare_static_backend(&program);
        // POWER ON intentionally leaves programmer-visible 8080 registers
        // undefined. Differential execution must therefore start both engines
        // from the same sampled physical CPU state instead of comparing two
        // independent random power-on samples. RESET already aligned PC/INTE.
        partial.cpu.set_registers(compiled.cpu.registers());

        compiled.service_execution_compiled(BUDGET).unwrap();
        for _ in 0..BUDGET {
            let ready = partial.machine.bus.cycle_front_panel_ready_input();
            let trace = partial.tick_once(ready);
            assert!(trace.fault.is_none());
        }

        assert_eq!(compiled.cpu.total_t_states(), partial.cpu.total_t_states());
        assert_eq!(compiled.cpu.registers(), partial.cpu.registers());
        assert_eq!(compiled.cpu.interrupts_enabled(), partial.cpu.interrupts_enabled());
        for address in 0x03d0..=0x03ff {
            assert_eq!(
                compiled.machine.bus.peek_memory(address),
                partial.machine.bus.peek_memory(address),
                "stack/RAM differs at {address:04x}"
            );
        }
        assert_eq!(
            compiled.machine.bus.raw_panel_lamp_duty(),
            partial.machine.bus.raw_panel_lamp_duty(),
            "new Full CPU-only families must preserve exact front-panel duty"
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
        // MVI A,'X' is Full-capable. OUT 11h must execute through exact Partial
        // and makes the 110-baud 88-2SIO transmitter active. The following
        // NOP/JMP loop must therefore stay Partial for the rest of this call.
        backend
            .load_bytes(0, &[0x3e, b'X', 0xd3, 0x11, 0x00, 0xc3, 0x04, 0x00])
            .unwrap();
        backend.run().unwrap();

        adaptive_metrics::begin_measurement();
        backend.service_execution_compiled(BUDGET).unwrap();
        let stats = adaptive_metrics::end_measurement();

        assert_eq!(stats.total_t_states(), u64::from(BUDGET));
        assert_eq!(stats.full_t_states, 7, "only MVI may execute in Full before OUT activates the UART");
        assert_eq!(stats.partial_t_states, u64::from(BUDGET - 7));
        assert!(backend.machine.bus.tx_busy(), "110-baud transmitter must still be active after only 1000 T-states");
    }

    #[test]
    fn compiled_full_allows_idle_serial_but_rejects_wait_ram_and_overlap() {
        let mut wait_hardware =
            S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        wait_hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        wait_hardware
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits1KStatic88Mcs,
                        0,
                    ),
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
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits4KStatic88_4Mcs,
                        0,
                    ),
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
}