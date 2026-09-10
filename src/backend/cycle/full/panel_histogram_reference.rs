// Original Full observer, retained only as an independent equivalence oracle.
use crate::machine::AltairBus;
const FULL_PANEL_HISTOGRAM_ENTRIES: usize = 256;

#[derive(Clone, Copy)]
struct FullPanelHistogramEntry {
    key: u64,
    weight: u32,
}

const EMPTY_FULL_PANEL_HISTOGRAM_ENTRY: FullPanelHistogramEntry =
    FullPanelHistogramEntry { key: 0, weight: 0 };

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
pub(super) struct FullPanelActivity {
    entries: [FullPanelHistogramEntry; FULL_PANEL_HISTOGRAM_ENTRIES],
    used: usize,
    latched_status: u8,
    panel_data: u8,
    latest_committed_key: Option<u64>,
    pending: Option<PendingPanelCycle>,
}

impl FullPanelActivity {
    pub(super) fn new(bus: &AltairBus) -> Self {
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

    pub(super) fn project_machine_cycle(
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
    ) {
        debug_assert!(t_states >= 1);
        debug_assert!(!(reads_data && writes_data));

        // Once another external machine cycle starts, the previous one can no
        // longer be the final presentation boundary and is safe to coalesce.
        self.commit_pending(bus);

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

    pub(super) fn project_internal_tail(&mut self, t_states: u32) {
        if t_states == 0 {
            return;
        }
        let pending = self
            .pending
            .as_mut()
            .expect("internal Full T-states require a preceding machine cycle");
        pending.internal_tail = pending.internal_tail.saturating_add(t_states);
    }

    pub(super) fn finish(&mut self, bus: &mut AltairBus) {
        let Some(pending) = self.pending.take() else {
            self.flush_histogram(bus, self.latest_committed_key);
            return;
        };

        // Every state before the final machine cycle may be replayed in any order
        // for raw duty, but end with the true predecessor so the canonical helper
        // sees exactly the 8212/panel DATA state that existed before final T1.
        self.flush_histogram(bus, self.latest_committed_key);
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
            bus.cycle_full_project_internal_t_states(pending.internal_tail, pending.inte);
        }
    }
}

