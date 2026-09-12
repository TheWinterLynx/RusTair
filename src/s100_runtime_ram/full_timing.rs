use super::{RuntimeRamHandle, RuntimeRamState};
use crate::s100_backplane::S100CardDrive;
use crate::s100_memory::S100RamBoardModel;

/// Window-local copy of only the digital RAM timing state that Full can advance.
///
/// Guest bytes and protection latches deliberately do not live here: Full keeps
/// reading and writing the installed RuntimeRamCard storage through the normal
/// bus-owned decoder. The empty vectors below therefore carry no guest-memory
/// allocation and exist only so the exact, already-tested RuntimeRamState timing
/// implementation can be reused rather than duplicated.
pub(crate) struct RuntimeRamTimingSnapshot {
    state: RuntimeRamState,
}

impl RuntimeRamTimingSnapshot {
    fn from_state(state: &RuntimeRamState) -> Option<Self> {
        if !matches!(
            state.historical_model(),
            Some(
                S100RamBoardModel::Mits4KDynamic88_4Mcd
                    | S100RamBoardModel::Mits4KSynchronous88S4K
            )
        ) {
            return None;
        }

        Some(Self {
            state: RuntimeRamState {
                config: state.config,
                bytes: Vec::new(),
                protected: Vec::new(),
                selected_offset: None,
                memory_read: false,
                wait_clocks_remaining: state.wait_clocks_remaining,
                previous_sync: state.previous_sync,
                previous_clock: state.previous_clock,
                previous_phi2: state.previous_phi2,
                previous_m1: state.previous_m1,
                previous_data_bus_in: false,
                previous_memory_read: false,
                previous_protect: false,
                previous_unprotect: false,
                refresh_clock_count: state.refresh_clock_count,
                refresh_pending: state.refresh_pending,
                refresh_active: state.refresh_active,
                refresh_collision_waits: state.refresh_collision_waits,
                #[cfg(test)]
                refresh_cycles: state.refresh_cycles,
                m1_phi2_count: state.m1_phi2_count,
                cached_drive: S100CardDrive::new(),
            },
        })
    }

    fn commit_into(self, state: &mut RuntimeRamState) {
        state.wait_clocks_remaining = self.state.wait_clocks_remaining;
        state.previous_sync = self.state.previous_sync;
        state.previous_clock = self.state.previous_clock;
        state.previous_phi2 = self.state.previous_phi2;
        state.previous_m1 = self.state.previous_m1;
        state.refresh_clock_count = self.state.refresh_clock_count;
        state.refresh_pending = self.state.refresh_pending;
        state.refresh_active = self.state.refresh_active;
        state.refresh_collision_waits = self.state.refresh_collision_waits;
        #[cfg(test)]
        {
            state.refresh_cycles = self.state.refresh_cycles;
        }
        state.m1_phi2_count = self.state.m1_phi2_count;
    }

    #[inline(always)]
    fn machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        self.state
            .full_machine_cycle_timing(address, memory_access, m1, base_t_states)
    }

    #[inline(always)]
    fn internal_t_states(&mut self, t_states: u32) {
        self.state.full_internal_t_states(t_states);
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

/// Transactional timing state for one Adaptive Full window.
///
/// The common historical configurations contain exactly one dynamic RAM card.
/// That one-entry case is kept branch-direct in the hot path; the generic loop
/// remains only for valid multi-board chassis configurations.
pub(crate) struct RuntimeRamTimingWindow {
    entries: Vec<RuntimeRamTimingEntry>,
}

impl RuntimeRamTimingWindow {
    pub(crate) fn from_handles(handles: impl IntoIterator<Item = RuntimeRamHandle>) -> Self {
        let entries = handles
            .into_iter()
            .filter_map(|handle| {
                handle
                    .full_timing_snapshot()
                    .map(|timing| RuntimeRamTimingEntry { handle, timing })
            })
            .collect();
        Self { entries }
    }

    #[inline(always)]
    pub(crate) fn machine_cycle_timing(
        &mut self,
        address: u16,
        memory_access: bool,
        m1: bool,
        base_t_states: u32,
    ) -> u32 {
        match self.entries.as_mut_slice() {
            [] => 0,
            [entry] => entry
                .timing
                .machine_cycle_timing(address, memory_access, m1, base_t_states),
            entries => {
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
        match self.entries.as_mut_slice() {
            [] => {}
            [entry] => entry.timing.internal_t_states(t_states),
            entries => {
                for entry in entries {
                    entry.timing.internal_t_states(t_states);
                }
            }
        }
    }

    pub(crate) fn commit(self) {
        for entry in self.entries {
            entry.handle.commit_full_timing_snapshot(entry.timing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RamInit;
    use crate::s100_memory::S100RamCardConfig;

    #[test]
    fn full_timing_window_snapshots_once_and_commits_without_shadowing_guest_bytes() {
        let (_card, handle) = super::super::RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KDynamic88_4Mcd, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        assert!(handle.write_byte(0x0010, 0x5a, false));

        let mut window = RuntimeRamTimingWindow::from_handles([handle.clone()]);
        assert_eq!(window.entries.len(), 1);
        assert!(window.entries[0].timing.state.bytes.is_empty());
        assert!(window.entries[0].timing.state.protected.is_empty());

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
        let (_card, handle) = super::super::RuntimeRamCard::historical(
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KSynchronous88S4K, 0),
            RamInit::Zeroed,
        )
        .unwrap();
        assert!(handle.write_byte(0x0020, 0xa5, false));

        let mut window = RuntimeRamTimingWindow::from_handles([handle.clone()]);
        window.internal_t_states(64);
        assert_eq!(window.machine_cycle_timing(0x0020, true, true, 4), 0);

        assert_eq!(handle.read_byte(0x0020), Some(0xa5));
        assert_eq!(handle.state.borrow().refresh_cycles, 0);
        window.commit();
        assert_eq!(handle.read_byte(0x0020), Some(0xa5));
        assert_eq!(handle.state.borrow().refresh_cycles, 1);
        assert!(!handle.state.borrow().refresh_pending);
    }
}
