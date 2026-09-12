use super::{RuntimeRamHandle, RuntimeRamState, S4K_REFRESH_PHI2_EDGES};
use crate::s100_memory::{S100RamBoardModel, S100RamTimingModel};

struct FourMcdFullTiming {
    base_address: u16,
    populated_bytes: usize,
    interval_clocks: u16,
    min_waits: u8,
    max_waits: u8,
    wait_clocks_remaining: u8,
    previous_sync: bool,
    previous_clock: bool,
    previous_m1: bool,
    previous_phi2: bool,
    refresh_clock_count: u16,
    refresh_pending: bool,
    refresh_active: bool,
    refresh_collision_waits: u8,
    #[cfg(test)]
    refresh_cycles: u64,
}

impl FourMcdFullTiming {
    fn from_state(state: &RuntimeRamState) -> Self {
        let S100RamTimingModel::RefreshCollision {
            interval_clocks,
            min_waits,
            max_waits,
        } = S100RamBoardModel::Mits4KDynamic88_4Mcd.timing_model()
        else {
            unreachable!("88-4MCD must retain refresh-collision timing")
        };
        Self {
            base_address: state.config.base_address(),
            populated_bytes: state.config.populated_bytes(),
            interval_clocks,
            min_waits,
            max_waits,
            wait_clocks_remaining: state.wait_clocks_remaining,
            previous_sync: state.previous_sync,
            previous_clock: state.previous_clock,
            previous_m1: state.previous_m1,
            previous_phi2: state.previous_phi2,
            refresh_clock_count: state.refresh_clock_count,
            refresh_pending: state.refresh_pending,
            refresh_active: state.refresh_active,
            refresh_collision_waits: state.refresh_collision_waits,
            #[cfg(test)]
            refresh_cycles: state.refresh_cycles,
        }
    }

    #[inline(always)]
    fn contains(&self, address: u16) -> bool {
        let offset = address.wrapping_sub(self.base_address) as usize;
        address >= self.base_address && offset < self.populated_bytes
    }

    #[inline(always)]
    fn complete_refresh_cycle(&mut self) {
        self.refresh_pending = false;
        #[cfg(test)]
        {
            self.refresh_cycles = self.refresh_cycles.saturating_add(1);
        }
    }

    #[inline(always)]
    fn advance_clocks(&mut self, clocks: u32) {
        if clocks == 0 {
            return;
        }
        let interval = u32::from(self.interval_clocks);
        let total = u32::from(self.refresh_clock_count).saturating_add(clocks);
        let crossed_refresh = total >= interval;
        if crossed_refresh {
            self.refresh_pending = true;
        }
        self.refresh_clock_count = (total % interval) as u16;
        if self.refresh_pending {
            self.refresh_collision_waits = if crossed_refresh && total % interval == 0 {
                self.max_waits
            } else {
                self.min_waits
            };
        } else {
            self.refresh_collision_waits = 0;
        }
    }

    #[inline(always)]
    fn machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        debug_assert!(base_t_states != 0);
        self.advance_clocks(1);
        let refresh_at_sync = self.refresh_pending;
        let waits = if refresh_at_sync && memory_access && self.contains(address) {
            match self.refresh_collision_waits {
                waits if waits == self.max_waits => self.max_waits,
                waits if waits == self.min_waits => self.min_waits,
                _ => self.min_waits,
            }
        } else {
            0
        };

        if refresh_at_sync {
            self.complete_refresh_cycle();
        }
        self.refresh_active = false;
        self.wait_clocks_remaining = 0;
        self.refresh_collision_waits = 0;
        self.advance_clocks(
            base_t_states
                .saturating_sub(1)
                .saturating_add(u32::from(waits)),
        );
        self.previous_sync = false;
        self.previous_clock = false;
        self.previous_m1 = m1;
        self.previous_phi2 = false;
        u32::from(waits)
    }

    #[inline(always)]
    fn internal_t_states(&mut self, t_states: u32) {
        self.advance_clocks(t_states);
    }

    fn commit_into(self, state: &mut RuntimeRamState) {
        debug_assert_eq!(
            state.historical_model(),
            Some(S100RamBoardModel::Mits4KDynamic88_4Mcd)
        );
        state.wait_clocks_remaining = self.wait_clocks_remaining;
        state.previous_sync = self.previous_sync;
        state.previous_clock = self.previous_clock;
        state.previous_m1 = self.previous_m1;
        state.previous_phi2 = self.previous_phi2;
        state.refresh_clock_count = self.refresh_clock_count;
        state.refresh_pending = self.refresh_pending;
        state.refresh_active = self.refresh_active;
        state.refresh_collision_waits = self.refresh_collision_waits;
        #[cfg(test)]
        {
            state.refresh_cycles = self.refresh_cycles;
        }
    }
}

struct S4kFullTiming {
    previous_m1: bool,
    previous_phi2: bool,
    refresh_clock_count: u16,
    refresh_pending: bool,
    m1_phi2_count: u8,
    #[cfg(test)]
    refresh_cycles: u64,
}

impl S4kFullTiming {
    fn from_state(state: &RuntimeRamState) -> Self {
        Self {
            previous_m1: state.previous_m1,
            previous_phi2: state.previous_phi2,
            refresh_clock_count: state.refresh_clock_count,
            refresh_pending: state.refresh_pending,
            m1_phi2_count: state.m1_phi2_count,
            #[cfg(test)]
            refresh_cycles: state.refresh_cycles,
        }
    }

    #[inline(always)]
    fn complete_refresh_cycle(&mut self) {
        self.refresh_pending = false;
        #[cfg(test)]
        {
            self.refresh_cycles = self.refresh_cycles.saturating_add(1);
        }
    }

    #[inline(always)]
    fn advance_phi2(&mut self, edges: u32, m1: bool, new_machine_cycle: bool) {
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
    fn machine_cycle_timing(&mut self, m1: bool, base_t_states: u32) -> u32 {
        self.advance_phi2(base_t_states, m1, true);
        0
    }

    #[inline(always)]
    fn internal_t_states(&mut self, t_states: u32) {
        if t_states != 0 {
            self.advance_phi2(t_states, self.previous_m1, false);
        }
    }

    fn commit_into(self, state: &mut RuntimeRamState) {
        debug_assert_eq!(
            state.historical_model(),
            Some(S100RamBoardModel::Mits4KSynchronous88S4K)
        );
        state.previous_m1 = self.previous_m1;
        state.previous_phi2 = self.previous_phi2;
        state.refresh_clock_count = self.refresh_clock_count;
        state.refresh_pending = self.refresh_pending;
        state.m1_phi2_count = self.m1_phi2_count;
        #[cfg(test)]
        {
            state.refresh_cycles = self.refresh_cycles;
        }
    }
}

enum RuntimeRamTimingSnapshot {
    FourMcd(FourMcdFullTiming),
    S4k(S4kFullTiming),
}

impl RuntimeRamTimingSnapshot {
    fn from_state(state: &RuntimeRamState) -> Option<Self> {
        match state.historical_model()? {
            S100RamBoardModel::Mits4KDynamic88_4Mcd => {
                Some(Self::FourMcd(FourMcdFullTiming::from_state(state)))
            }
            S100RamBoardModel::Mits4KSynchronous88S4K => {
                Some(Self::S4k(S4kFullTiming::from_state(state)))
            }
            _ => None,
        }
    }

    #[inline(always)]
    fn machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        match self {
            Self::FourMcd(timing) => {
                timing.machine_cycle_timing(address, memory_access, m1, base_t_states)
            }
            Self::S4k(timing) => timing.machine_cycle_timing(m1, base_t_states),
        }
    }

    #[inline(always)]
    fn internal_t_states(&mut self, t_states: u32) {
        match self {
            Self::FourMcd(timing) => timing.internal_t_states(t_states),
            Self::S4k(timing) => timing.internal_t_states(t_states),
        }
    }

    fn commit_into(self, state: &mut RuntimeRamState) {
        match self {
            Self::FourMcd(timing) => timing.commit_into(state),
            Self::S4k(timing) => timing.commit_into(state),
        }
    }
}

impl RuntimeRamHandle {
    fn full_timing_snapshot(&self) -> Option<RuntimeRamTimingSnapshot> {
        let state = self.state.borrow();
        RuntimeRamTimingSnapshot::from_state(&state)
    }

    fn commit_full_timing_snapshot(&self, snapshot: RuntimeRamTimingSnapshot) {
        let mut state = self.state.borrow_mut();
        let before = state.drive_signature();
        snapshot.commit_into(&mut state);
        state.refresh_cached_drive_if_changed(before);
    }
}

struct RuntimeRamTimingEntry {
    handle: RuntimeRamHandle,
    timing: RuntimeRamTimingSnapshot,
}

enum RuntimeRamTimingWindowKind {
    None,
    FourMcd {
        handle: RuntimeRamHandle,
        timing: FourMcdFullTiming,
    },
    S4k {
        handle: RuntimeRamHandle,
        timing: S4kFullTiming,
    },
    Many(Vec<RuntimeRamTimingEntry>),
}

/// Transactional timing state for one Adaptive Full window.
///
/// The common one-dynamic-board chassis is represented without a Vec allocation
/// and without a generic RuntimeRamState in the hot path. Multi-board machines
/// retain exact parallel clock advancement through the generic `Many` fallback.
pub(crate) struct RuntimeRamTimingWindow {
    kind: RuntimeRamTimingWindowKind,
}

impl RuntimeRamTimingWindow {
    pub(crate) fn from_handles(handles: impl IntoIterator<Item = RuntimeRamHandle>) -> Self {
        let mut entries = handles.into_iter().filter_map(|handle| {
            handle
                .full_timing_snapshot()
                .map(|timing| RuntimeRamTimingEntry { handle, timing })
        });
        let Some(first) = entries.next() else {
            return Self {
                kind: RuntimeRamTimingWindowKind::None,
            };
        };
        if let Some(second) = entries.next() {
            let mut many = vec![first, second];
            many.extend(entries);
            return Self {
                kind: RuntimeRamTimingWindowKind::Many(many),
            };
        }

        let RuntimeRamTimingEntry { handle, timing } = first;
        let kind = match timing {
            RuntimeRamTimingSnapshot::FourMcd(timing) => {
                RuntimeRamTimingWindowKind::FourMcd { handle, timing }
            }
            RuntimeRamTimingSnapshot::S4k(timing) => {
                RuntimeRamTimingWindowKind::S4k { handle, timing }
            }
        };
        Self { kind }
    }

    #[inline(always)]
    pub(crate) fn machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        match &mut self.kind {
            RuntimeRamTimingWindowKind::None => 0,
            RuntimeRamTimingWindowKind::FourMcd { timing, .. } => {
                timing.machine_cycle_timing(address, memory_access, m1, base_t_states)
            }
            RuntimeRamTimingWindowKind::S4k { timing, .. } => {
                timing.machine_cycle_timing(m1, base_t_states)
            }
            RuntimeRamTimingWindowKind::Many(entries) => {
                let mut waits = 0;
                for entry in entries {
                    waits = waits.max(entry.timing.machine_cycle_timing(
                        address,
                        memory_access,
                        m1,
                        base_t_states,
                    ));
                }
                waits
            }
        }
    }

    #[inline(always)]
    pub(crate) fn internal_t_states(&mut self, t_states: u32) {
        if t_states == 0 {
            return;
        }
        match &mut self.kind {
            RuntimeRamTimingWindowKind::None => {}
            RuntimeRamTimingWindowKind::FourMcd { timing, .. } => {
                timing.internal_t_states(t_states)
            }
            RuntimeRamTimingWindowKind::S4k { timing, .. } => timing.internal_t_states(t_states),
            RuntimeRamTimingWindowKind::Many(entries) => {
                for entry in entries {
                    entry.timing.internal_t_states(t_states);
                }
            }
        }
    }

    pub(crate) fn commit(self) {
        match self.kind {
            RuntimeRamTimingWindowKind::None => {}
            RuntimeRamTimingWindowKind::FourMcd { handle, timing } => {
                handle.commit_full_timing_snapshot(RuntimeRamTimingSnapshot::FourMcd(timing));
            }
            RuntimeRamTimingWindowKind::S4k { handle, timing } => {
                handle.commit_full_timing_snapshot(RuntimeRamTimingSnapshot::S4k(timing));
            }
            RuntimeRamTimingWindowKind::Many(entries) => {
                for entry in entries {
                    entry.handle.commit_full_timing_snapshot(entry.timing);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RamInit;
    use crate::s100_memory::S100RamCardConfig;

    fn historical_handle(model: S100RamBoardModel) -> RuntimeRamHandle {
        let (_card, handle) = super::super::RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(model, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        handle
    }

    fn assert_four_mcd_state_eq(expected: &RuntimeRamHandle, actual: &RuntimeRamHandle) {
        let expected = expected.state.borrow();
        let actual = actual.state.borrow();
        assert_eq!(actual.wait_clocks_remaining, expected.wait_clocks_remaining);
        assert_eq!(actual.previous_sync, expected.previous_sync);
        assert_eq!(actual.previous_clock, expected.previous_clock);
        assert_eq!(actual.previous_m1, expected.previous_m1);
        assert_eq!(actual.previous_phi2, expected.previous_phi2);
        assert_eq!(actual.refresh_clock_count, expected.refresh_clock_count);
        assert_eq!(actual.refresh_pending, expected.refresh_pending);
        assert_eq!(actual.refresh_active, expected.refresh_active);
        assert_eq!(actual.refresh_collision_waits, expected.refresh_collision_waits);
        assert_eq!(actual.refresh_cycles, expected.refresh_cycles);
    }

    fn assert_s4k_state_eq(expected: &RuntimeRamHandle, actual: &RuntimeRamHandle) {
        let expected = expected.state.borrow();
        let actual = actual.state.borrow();
        assert_eq!(actual.previous_m1, expected.previous_m1);
        assert_eq!(actual.previous_phi2, expected.previous_phi2);
        assert_eq!(actual.refresh_clock_count, expected.refresh_clock_count);
        assert_eq!(actual.refresh_pending, expected.refresh_pending);
        assert_eq!(actual.m1_phi2_count, expected.m1_phi2_count);
        assert_eq!(actual.refresh_cycles, expected.refresh_cycles);
    }

    #[test]
    fn full_timing_window_snapshots_once_and_commits_without_shadowing_guest_bytes() {
        let handle = historical_handle(S100RamBoardModel::Mits4KDynamic88_4Mcd);
        assert!(handle.write_byte(0x0010, 0x5a, false));

        let mut window = RuntimeRamTimingWindow::from_handles([handle.clone()]);
        assert!(matches!(
            &window.kind,
            RuntimeRamTimingWindowKind::FourMcd { .. }
        ));
        window.internal_t_states(31);
        assert_eq!(window.machine_cycle_timing(0x0010, true, true, 4), 2);

        assert_eq!(handle.read_byte(0x0010), Some(0x5a));
        assert_eq!(handle.state.borrow().refresh_cycles, 0);
        window.commit();
        assert_eq!(handle.read_byte(0x0010), Some(0x5a));
        assert_eq!(handle.state.borrow().refresh_cycles, 1);
    }

    #[test]
    fn s4k_full_timing_window_commits_hidden_refresh_without_prdy_waits() {
        let handle = historical_handle(S100RamBoardModel::Mits4KSynchronous88S4K);
        assert!(handle.write_byte(0x0020, 0xa5, false));

        let mut window = RuntimeRamTimingWindow::from_handles([handle.clone()]);
        assert!(matches!(
            &window.kind,
            RuntimeRamTimingWindowKind::S4k { .. }
        ));
        window.internal_t_states(64);
        assert_eq!(window.machine_cycle_timing(0x0020, true, true, 4), 0);

        assert_eq!(handle.read_byte(0x0020), Some(0xa5));
        assert_eq!(handle.state.borrow().refresh_cycles, 0);
        window.commit();
        assert_eq!(handle.read_byte(0x0020), Some(0xa5));
        assert_eq!(handle.state.borrow().refresh_cycles, 1);
        assert!(!handle.state.borrow().refresh_pending);
    }

    #[test]
    fn specialized_four_mcd_window_matches_runtime_state_oracle() {
        let oracle = historical_handle(S100RamBoardModel::Mits4KDynamic88_4Mcd);
        let optimized = historical_handle(S100RamBoardModel::Mits4KDynamic88_4Mcd);
        let mut window = RuntimeRamTimingWindow::from_handles([optimized.clone()]);

        for step in 0..1024u32 {
            let internal = step % 7;
            oracle.full_internal_t_states(internal);
            window.internal_t_states(internal);

            let address = if step % 5 == 0 { 0x5000 } else { 0x0010 };
            let memory_access = step % 4 != 0;
            let m1 = step % 3 == 0;
            let base_t_states = if m1 { 4 } else { 3 };
            let expected = oracle.full_machine_cycle_timing(
                address,
                memory_access,
                m1,
                base_t_states,
            );
            let actual = window.machine_cycle_timing(
                address,
                memory_access,
                m1,
                base_t_states,
            );
            assert_eq!(actual, expected, "4MCD wait mismatch at step {step}");
        }

        window.commit();
        assert_four_mcd_state_eq(&oracle, &optimized);
    }

    #[test]
    fn specialized_s4k_window_matches_runtime_state_oracle() {
        let oracle = historical_handle(S100RamBoardModel::Mits4KSynchronous88S4K);
        let optimized = historical_handle(S100RamBoardModel::Mits4KSynchronous88S4K);
        let mut window = RuntimeRamTimingWindow::from_handles([optimized.clone()]);

        for step in 0..1024u32 {
            let internal = step % 6;
            oracle.full_internal_t_states(internal);
            window.internal_t_states(internal);

            let m1 = step % 3 == 0;
            let base_t_states = if m1 { 4 } else { 3 };
            let expected = oracle.full_machine_cycle_timing(0x0010, true, m1, base_t_states);
            let actual = window.machine_cycle_timing(0x0010, true, m1, base_t_states);
            assert_eq!(actual, expected, "S4K wait mismatch at step {step}");
        }

        window.commit();
        assert_s4k_state_eq(&oracle, &optimized);
    }
}
