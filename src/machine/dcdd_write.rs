//! MITS 88-DCDD Board #2 authentic write-sequencer timing.
//!
//! Board #2 owns WRITE ENABLE, ENWD generation and the retained write-data
//! latch. The FD-400/media layer only receives accepted physical-byte effects;
//! it never decides guest I/O timing or exposes a sector API.

use super::fd400::Fd400Time;

const TRIM_ERASE_START_DELAY_US: u64 = 200;
const FIRST_ENWD_FROM_SECTOR_START_US: u64 = 280;
const WRITE_BYTE_INTERVAL_US: u64 = 32;
const TRIM_ERASE_TAIL_US: u64 = 475;
const DATA_BYTES_PER_SECTOR: u64 = 137;
const FILL_BYTE_GENERATION: u64 = DATA_BYTES_PER_SECTOR;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WriteByteAcceptance {
    /// Zero-based 32-us write opportunity. Generations 0..=136 are the 137
    /// physical data bytes; generation 137 is the documented last/fill byte.
    pub(super) generation: u64,
    pub(super) value: u8,
    pub(super) physical_data_offset: Option<usize>,
    pub(super) fill_to_sector_end: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WriteWindow {
    write_enable_at: Fd400Time,
    trim_erase_on_at: Fd400Time,
    first_enwd_at: Fd400Time,
    sector_end_at: Fd400Time,
    trim_erase_off_at: Fd400Time,
}

#[derive(Debug, Default)]
pub(super) struct Board2WriteElectronics {
    window: Option<WriteWindow>,
    consumed_generation: Option<u64>,
    fill_byte_latched: bool,
}

impl Board2WriteElectronics {
    pub(super) const fn new() -> Self {
        Self {
            window: None,
            consumed_generation: None,
            fill_byte_latched: false,
        }
    }

    /// WRITE ENABLE is asserted after software observes Sector True. The MITS
    /// guide defines trim erase relative to WRITE ENABLE, but ENWD relative to
    /// the beginning of the selected sector.
    pub(super) fn begin(
        &mut self,
        write_enable_at: Fd400Time,
        sector_start_at: Fd400Time,
        sector_end_at: Fd400Time,
    ) {
        self.window = Some(WriteWindow {
            write_enable_at,
            trim_erase_on_at: write_enable_at.saturating_add(
                Fd400Time::from_microseconds(TRIM_ERASE_START_DELAY_US).units(),
            ),
            first_enwd_at: sector_start_at.saturating_add(
                Fd400Time::from_microseconds(FIRST_ENWD_FROM_SECTOR_START_US).units(),
            ),
            sector_end_at,
            trim_erase_off_at: sector_end_at
                .saturating_add(Fd400Time::from_microseconds(TRIM_ERASE_TAIL_US).units()),
        });
        self.consumed_generation = None;
        self.fill_byte_latched = false;
    }

    pub(super) fn write_active(&self, now: Fd400Time) -> bool {
        self.window
            .is_some_and(|window| now >= window.write_enable_at && now < window.sector_end_at)
    }

    pub(super) fn trim_erase_active(&self, now: Fd400Time) -> bool {
        self.window.is_some_and(|window| {
            now >= window.trim_erase_on_at && now < window.trim_erase_off_at
        })
    }

    /// Move Head is inhibited from WRITE ENABLE until trim erase has completed.
    pub(super) fn move_head_inhibited(&self, now: Fd400Time) -> bool {
        self.window
            .is_some_and(|window| now >= window.write_enable_at && now < window.trim_erase_off_at)
    }

    fn current_generation(&self, now: Fd400Time) -> Option<u64> {
        let window = self.window?;
        if now < window.first_enwd_at || now >= window.sector_end_at || self.fill_byte_latched {
            return None;
        }
        let elapsed = now.saturating_duration_since(window.first_enwd_at);
        let interval = Fd400Time::from_microseconds(WRITE_BYTE_INTERVAL_US).units();
        Some((elapsed / interval).min(FILL_BYTE_GENERATION))
    }

    /// ENWD is active-low at the guest status port, but represented here as the
    /// positive physical condition: Board #2 is requesting a new output byte.
    pub(super) fn enwd(&self, now: Fd400Time) -> bool {
        self.current_generation(now)
            .is_some_and(|generation| self.consumed_generation != Some(generation))
    }

    /// OUT 0Ah resets ENWD for the current 32-us opportunity and updates the
    /// retained write-data latch. After the documented 138th byte, the write
    /// circuit repeats that value to the end of sector and ignores later ENWD.
    pub(super) fn consume_data(
        &mut self,
        now: Fd400Time,
        value: u8,
    ) -> Option<WriteByteAcceptance> {
        let generation = self.current_generation(now)?;
        if self.consumed_generation == Some(generation) {
            return None;
        }
        self.consumed_generation = Some(generation);
        let fill_to_sector_end = generation == FILL_BYTE_GENERATION;
        if fill_to_sector_end {
            self.fill_byte_latched = true;
        }
        Some(WriteByteAcceptance {
            generation,
            value,
            physical_data_offset: (generation < DATA_BYTES_PER_SECTOR)
                .then_some(generation as usize),
            fill_to_sector_end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times() -> (Fd400Time, Fd400Time, Fd400Time) {
        let sector_start = Fd400Time::from_microseconds(1_000);
        let write_enable = Fd400Time::from_microseconds(1_020);
        let sector_end = Fd400Time::from_microseconds(6_208);
        (sector_start, write_enable, sector_end)
    }

    #[test]
    fn trim_erase_and_first_enwd_follow_their_distinct_documented_origins() {
        let (sector_start, write_enable, sector_end) = times();
        let mut write = Board2WriteElectronics::new();
        write.begin(write_enable, sector_start, sector_end);

        assert!(!write.trim_erase_active(Fd400Time::from_microseconds(1_219)));
        assert!(write.trim_erase_active(Fd400Time::from_microseconds(1_220)));
        assert!(!write.enwd(Fd400Time::from_microseconds(1_279)));
        assert!(write.enwd(Fd400Time::from_microseconds(1_280)));
    }

    #[test]
    fn output_clears_enwd_only_until_the_next_32_us_opportunity() {
        let (sector_start, write_enable, sector_end) = times();
        let mut write = Board2WriteElectronics::new();
        write.begin(write_enable, sector_start, sector_end);

        let first_at = Fd400Time::from_microseconds(1_280);
        let first = write.consume_data(first_at, 0x81).unwrap();
        assert_eq!(first.generation, 0);
        assert_eq!(first.physical_data_offset, Some(0));
        assert!(!write.enwd(first_at));
        assert!(!write.enwd(Fd400Time::from_microseconds(1_311)));
        assert!(write.enwd(Fd400Time::from_microseconds(1_312)));
    }

    #[test]
    fn large_time_jump_resolves_directly_to_the_current_write_generation() {
        let (sector_start, write_enable, sector_end) = times();
        let mut write = Board2WriteElectronics::new();
        write.begin(write_enable, sector_start, sector_end);

        let now = Fd400Time::from_microseconds(1_280 + 73 * 32);
        let accepted = write.consume_data(now, 0x5a).unwrap();
        assert_eq!(accepted.generation, 73);
        assert_eq!(accepted.physical_data_offset, Some(73));
    }

    #[test]
    fn documented_138th_byte_switches_to_fill_and_suppresses_further_enwd() {
        let (sector_start, write_enable, sector_end) = times();
        let mut write = Board2WriteElectronics::new();
        write.begin(write_enable, sector_start, sector_end);

        let fill_at = Fd400Time::from_microseconds(1_280 + 137 * 32);
        assert!(write.enwd(fill_at));
        let fill = write.consume_data(fill_at, 0x00).unwrap();
        assert_eq!(fill.generation, 137);
        assert_eq!(fill.physical_data_offset, None);
        assert!(fill.fill_to_sector_end);
        assert!(!write.enwd(Fd400Time::from_microseconds(5_900)));
    }

    #[test]
    fn write_disables_at_sector_end_but_trim_erase_holds_move_head_475_us_longer() {
        let (sector_start, write_enable, sector_end) = times();
        let mut write = Board2WriteElectronics::new();
        write.begin(write_enable, sector_start, sector_end);

        assert!(write.write_active(Fd400Time::from_microseconds(6_207)));
        assert!(!write.write_active(sector_end));
        assert!(write.move_head_inhibited(sector_end));
        assert!(write.trim_erase_active(Fd400Time::from_microseconds(6_682)));
        assert!(!write.trim_erase_active(Fd400Time::from_microseconds(6_683)));
        assert!(!write.move_head_inhibited(Fd400Time::from_microseconds(6_683)));
    }
}
