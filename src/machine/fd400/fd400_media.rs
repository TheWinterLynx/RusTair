//! Read-only removable 8-inch hard-sectored media for the Pertec FD-400.
//!
//! This is a physical-byte surface below the drive electronics, not a guest
//! filesystem-sector API. The first supported representation is deliberately
//! exact and simple: 77 tracks × 32 hard sectors × 137 physical bytes per
//! sector, stored track-major then sector-major. No host pathname or image
//! format metadata is retained in the emulated medium.

pub(super) const HARD_SECTORED_TRACKS: u8 = 77;
pub(super) const HARD_SECTORS_PER_TRACK: u8 = 32;
pub(super) const PHYSICAL_BYTES_PER_SECTOR: usize = 137;
pub(super) const PHYSICAL_MEDIA_BYTES: usize = HARD_SECTORED_TRACKS as usize
    * HARD_SECTORS_PER_TRACK as usize
    * PHYSICAL_BYTES_PER_SECTOR;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HardSectored8InchGeometry {
    pub(super) tracks: u8,
    pub(super) sectors_per_track: u8,
    pub(super) physical_bytes_per_sector: usize,
}

impl HardSectored8InchGeometry {
    pub(super) const MITS_FD400: Self = Self {
        tracks: HARD_SECTORED_TRACKS,
        sectors_per_track: HARD_SECTORS_PER_TRACK,
        physical_bytes_per_sector: PHYSICAL_BYTES_PER_SECTOR,
    };

    pub(super) const fn total_bytes(self) -> usize {
        self.tracks as usize
            * self.sectors_per_track as usize
            * self.physical_bytes_per_sector
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct HardSectoredMediaSizeError {
    pub(super) expected: usize,
    pub(super) actual: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct HardSectored8InchMedia {
    bytes: Box<[u8]>,
    write_protected: bool,
}

impl HardSectored8InchMedia {
    pub(super) fn from_physical_bytes(
        bytes: Vec<u8>,
        write_protected: bool,
    ) -> Result<Self, HardSectoredMediaSizeError> {
        if bytes.len() != PHYSICAL_MEDIA_BYTES {
            return Err(HardSectoredMediaSizeError {
                expected: PHYSICAL_MEDIA_BYTES,
                actual: bytes.len(),
            });
        }
        Ok(Self {
            bytes: bytes.into_boxed_slice(),
            write_protected,
        })
    }

    pub(super) const fn geometry(&self) -> HardSectored8InchGeometry {
        HardSectored8InchGeometry::MITS_FD400
    }

    pub(super) const fn write_protected(&self) -> bool {
        self.write_protected
    }

    pub(super) fn physical_sector(&self, track: u8, sector: u8) -> Option<&[u8]> {
        let start = Self::physical_sector_offset(track, sector)?;
        self.bytes
            .get(start..start.saturating_add(PHYSICAL_BYTES_PER_SECTOR))
    }

    pub(super) fn physical_byte(&self, track: u8, sector: u8, offset: usize) -> Option<u8> {
        self.physical_sector(track, sector)?.get(offset).copied()
    }

    pub(super) fn into_physical_bytes(self) -> Vec<u8> {
        self.bytes.into_vec()
    }

    fn physical_sector_offset(track: u8, sector: u8) -> Option<usize> {
        if track >= HARD_SECTORED_TRACKS || sector >= HARD_SECTORS_PER_TRACK {
            return None;
        }
        let logical_sector = track as usize * HARD_SECTORS_PER_TRACK as usize + sector as usize;
        Some(logical_sector * PHYSICAL_BYTES_PER_SECTOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterned_media(write_protected: bool) -> HardSectored8InchMedia {
        let bytes = (0..PHYSICAL_MEDIA_BYTES)
            .map(|index| (index & 0xff) as u8)
            .collect();
        HardSectored8InchMedia::from_physical_bytes(bytes, write_protected).unwrap()
    }

    #[test]
    fn fd400_physical_geometry_is_exact_and_self_consistent() {
        let geometry = HardSectored8InchGeometry::MITS_FD400;
        assert_eq!(geometry.tracks, 77);
        assert_eq!(geometry.sectors_per_track, 32);
        assert_eq!(geometry.physical_bytes_per_sector, 137);
        assert_eq!(geometry.total_bytes(), 337_568);
        assert_eq!(geometry.total_bytes(), PHYSICAL_MEDIA_BYTES);
    }

    #[test]
    fn raw_physical_media_requires_the_exact_geometry_size() {
        for actual in [PHYSICAL_MEDIA_BYTES - 1, PHYSICAL_MEDIA_BYTES + 1] {
            let error = HardSectored8InchMedia::from_physical_bytes(vec![0; actual], false)
                .unwrap_err();
            assert_eq!(
                error,
                HardSectoredMediaSizeError {
                    expected: PHYSICAL_MEDIA_BYTES,
                    actual,
                }
            );
        }
        assert!(
            HardSectored8InchMedia::from_physical_bytes(vec![0; PHYSICAL_MEDIA_BYTES], false)
                .is_ok()
        );
    }

    #[test]
    fn physical_byte_addressing_is_track_sector_and_byte_exact() {
        let media = patterned_media(true);
        for (track, sector, byte) in [(0, 0, 0), (0, 31, 136), (1, 0, 0), (76, 31, 136)] {
            let absolute = ((track as usize * 32 + sector as usize) * 137) + byte;
            assert_eq!(
                media.physical_byte(track, sector, byte),
                Some((absolute & 0xff) as u8)
            );
        }
        assert_eq!(media.physical_byte(77, 0, 0), None);
        assert_eq!(media.physical_byte(0, 32, 0), None);
        assert_eq!(media.physical_byte(0, 0, 137), None);
    }

    #[test]
    fn write_protect_is_medium_metadata_not_encoded_into_physical_bytes() {
        let writable = patterned_media(false);
        let protected = patterned_media(true);
        assert!(!writable.write_protected());
        assert!(protected.write_protected());
        assert_eq!(
            writable.physical_sector(12, 7),
            protected.physical_sector(12, 7)
        );
    }

    #[test]
    fn ejectable_medium_round_trips_all_physical_bytes_without_host_metadata() {
        let bytes = (0..PHYSICAL_MEDIA_BYTES)
            .map(|index| ((index * 17) & 0xff) as u8)
            .collect::<Vec<_>>();
        let media = HardSectored8InchMedia::from_physical_bytes(bytes.clone(), false).unwrap();
        assert_eq!(media.geometry(), HardSectored8InchGeometry::MITS_FD400);
        assert_eq!(media.into_physical_bytes(), bytes);
    }
}
