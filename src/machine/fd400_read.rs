//! Source-backed FD-400 physical read-stream timing.
//!
//! The drive exposes only the byte that has physically passed under the head at
//! a supplied virtual timestamp. Controller-side synchronization, the read-data
//! latch and NRDA remain owned by the MITS 88-DCDD Board #1 read circuitry.
//! No guest I/O semantics or filesystem-sector API exists here.

use super::super::{
    Fd400Time, HARD_SECTORS_PER_TRACK, PertecFd400, SECTOR_TIME_UNITS, TIME_UNITS_PER_MICROSECOND,
};
use super::PHYSICAL_BYTES_PER_SECTOR;

const READ_DATA_START_UNITS: u64 = 140 * TIME_UNITS_PER_MICROSECOND;
const READ_BYTE_INTERVAL_UNITS: u64 = 32 * TIME_UNITS_PER_MICROSECOND;
const LAST_PHYSICAL_BYTE: usize = PHYSICAL_BYTES_PER_SECTOR - 1;
const LAST_PHYSICAL_BYTE_DUE_UNITS: u64 =
    READ_DATA_START_UNITS + LAST_PHYSICAL_BYTE as u64 * READ_BYTE_INTERVAL_UNITS;

impl PertecFd400 {
    /// Return the newest physical byte event that could have reached the
    /// controller read electronics by `now`.
    ///
    /// The first byte of a normal MITS physical record carries D7 as the SYNC
    /// bit. Read data begins 140 us after Sector True and then advances every
    /// 32 us. A large time jump resolves directly to the latest byte event; no
    /// intermediate bit/byte callbacks are required.
    pub(in crate::machine) fn latest_read_byte_event(
        &mut self,
        now: Fd400Time,
    ) -> Option<(u64, u8)> {
        self.settle_step(now);
        if !self.spindle_rotating() || !self.head_commanded_loaded {
            return None;
        }
        let head_ready_at = self.head_ready_at?;
        let rotation = self.rotation_snapshot_without_settle(now);

        let (revolution, sector, byte_offset, age_units) = if rotation.sector_offset_units
            >= READ_DATA_START_UNITS
        {
            let elapsed = rotation.sector_offset_units - READ_DATA_START_UNITS;
            let byte_offset =
                (elapsed / READ_BYTE_INTERVAL_UNITS).min(LAST_PHYSICAL_BYTE as u64) as usize;
            let due_offset = READ_DATA_START_UNITS + byte_offset as u64 * READ_BYTE_INTERVAL_UNITS;
            (
                rotation.revolution,
                rotation.sector,
                byte_offset,
                rotation.sector_offset_units - due_offset,
            )
        } else {
            if rotation.revolution == 0 && rotation.sector == 0 {
                return None;
            }
            let (revolution, sector) = if rotation.sector == 0 {
                (
                    rotation.revolution.checked_sub(1)?,
                    HARD_SECTORS_PER_TRACK - 1,
                )
            } else {
                (rotation.revolution, rotation.sector - 1)
            };
            (
                revolution,
                sector,
                LAST_PHYSICAL_BYTE,
                rotation.sector_offset_units + SECTOR_TIME_UNITS - LAST_PHYSICAL_BYTE_DUE_UNITS,
            )
        };

        let due_at = Fd400Time::from_units(now.units().saturating_sub(age_units));
        if due_at < head_ready_at {
            return None;
        }

        // The physical record does not synchronize the controller unless its
        // first byte carries the documented SYNC bit in D7.
        if self.physical_media_byte(self.track, sector, 0)? & 0x80 == 0 {
            return None;
        }
        let byte = self.physical_media_byte(self.track, sector, byte_offset)?;
        let sector_ordinal = revolution
            .saturating_mul(HARD_SECTORS_PER_TRACK as u64)
            .saturating_add(u64::from(sector));
        let generation = sector_ordinal
            .saturating_mul(PHYSICAL_BYTES_PER_SECTOR as u64)
            .saturating_add(byte_offset as u64);
        Some((generation, byte))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{HardSectored8InchMedia, PHYSICAL_MEDIA_BYTES};
    use super::*;

    fn readable_media() -> HardSectored8InchMedia {
        let mut bytes = vec![0u8; PHYSICAL_MEDIA_BYTES];
        for track in 0..super::super::super::HARD_SECTORED_TRACKS {
            for sector in 0..HARD_SECTORS_PER_TRACK {
                let base = (track as usize * HARD_SECTORS_PER_TRACK as usize + sector as usize)
                    * PHYSICAL_BYTES_PER_SECTOR;
                bytes[base] = 0x80 | ((track.wrapping_add(sector)) & 0x7f);
                for offset in 1..PHYSICAL_BYTES_PER_SECTOR {
                    bytes[base + offset] = track
                        .wrapping_mul(17)
                        .wrapping_add(sector.wrapping_mul(5))
                        .wrapping_add(offset as u8);
                }
            }
        }
        HardSectored8InchMedia::from_physical_bytes(bytes, true).unwrap()
    }

    fn readable_drive() -> PertecFd400 {
        let mut drive = PertecFd400::new();
        drive.insert_media(readable_media()).unwrap();
        drive.set_power(true, Fd400Time::ZERO);
        drive.set_motor_on(true, Fd400Time::ZERO);
        drive.set_head_loaded(true, Fd400Time::ZERO);
        drive.set_door_open(false, Fd400Time::ZERO);
        drive
    }

    fn expected_byte(track: u8, sector: u8, offset: usize) -> u8 {
        if offset == 0 {
            0x80 | ((track.wrapping_add(sector)) & 0x7f)
        } else {
            track
                .wrapping_mul(17)
                .wrapping_add(sector.wrapping_mul(5))
                .wrapping_add(offset as u8)
        }
    }

    #[test]
    fn read_stream_waits_for_head_ready_then_catches_the_next_physical_byte() {
        let mut drive = readable_drive();
        let just_before_next_byte = Fd400Time::from_units(240_133);
        assert_eq!(drive.latest_read_byte_event(just_before_next_byte), None);

        let next_byte = Fd400Time::from_units(240_134);
        let event = drive.latest_read_byte_event(next_byte).unwrap();
        assert_eq!(event.1, expected_byte(0, 7, 107));
    }

    #[test]
    fn read_stream_switches_at_140_us_then_every_32_us() {
        let mut drive = readable_drive();
        let sector_8 = 8 * SECTOR_TIME_UNITS;

        let before_first = drive
            .latest_read_byte_event(Fd400Time::from_units(sector_8 + READ_DATA_START_UNITS - 1))
            .unwrap();
        assert_eq!(before_first.1, expected_byte(0, 7, LAST_PHYSICAL_BYTE));

        let first = drive
            .latest_read_byte_event(Fd400Time::from_units(sector_8 + READ_DATA_START_UNITS))
            .unwrap();
        assert_eq!(first.1, expected_byte(0, 8, 0));
        assert!(first.0 > before_first.0);

        let before_second = drive
            .latest_read_byte_event(Fd400Time::from_units(
                sector_8 + READ_DATA_START_UNITS + READ_BYTE_INTERVAL_UNITS - 1,
            ))
            .unwrap();
        assert_eq!(before_second, first);

        let second = drive
            .latest_read_byte_event(Fd400Time::from_units(
                sector_8 + READ_DATA_START_UNITS + READ_BYTE_INTERVAL_UNITS,
            ))
            .unwrap();
        assert_eq!(second.1, expected_byte(0, 8, 1));
        assert_eq!(second.0, first.0 + 1);
    }

    #[test]
    fn read_stream_large_jump_resolves_to_last_byte_before_next_sector_sync() {
        let mut drive = readable_drive();
        let sector_9 = 9 * SECTOR_TIME_UNITS;
        let event = drive
            .latest_read_byte_event(Fd400Time::from_units(sector_9 + READ_DATA_START_UNITS - 1))
            .unwrap();
        assert_eq!(event.1, expected_byte(0, 8, LAST_PHYSICAL_BYTE));

        let next = drive
            .latest_read_byte_event(Fd400Time::from_units(sector_9 + READ_DATA_START_UNITS))
            .unwrap();
        assert_eq!(next.1, expected_byte(0, 9, 0));
        assert_eq!(next.0, event.0 + 1);
    }

    #[test]
    fn a_record_without_the_sync_bit_does_not_create_a_read_event() {
        let mut media = readable_media().into_physical_bytes();
        let sector_8 = 8usize * PHYSICAL_BYTES_PER_SECTOR;
        media[sector_8] &= 0x7f;
        let media = HardSectored8InchMedia::from_physical_bytes(media, true).unwrap();

        let mut drive = PertecFd400::new();
        drive.insert_media(media).unwrap();
        drive.set_power(true, Fd400Time::ZERO);
        drive.set_motor_on(true, Fd400Time::ZERO);
        drive.set_head_loaded(true, Fd400Time::ZERO);
        drive.set_door_open(false, Fd400Time::ZERO);

        let now = Fd400Time::from_units(8 * SECTOR_TIME_UNITS + READ_DATA_START_UNITS + 500);
        assert_eq!(drive.latest_read_byte_event(now), None);
    }
}
