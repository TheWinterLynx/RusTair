use crate::config::{RamSize, SerialBoard};

/// Hardware/configuration limits of the classic Open SIMH `ALTAIR` target.
///
/// These are intentionally narrower than RusTair's native machine. The classic
/// simulator exposes a MITS 2SIO model and CPU memory-size modifiers beginning
/// at 4 KiB, so selecting it may require an explicit configuration change.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClassicAltairProfile;

impl ClassicAltairProfile {
    pub const fn supports_ram_size(size: RamSize) -> bool {
        matches!(
            size,
            RamSize::K4 | RamSize::K8 | RamSize::K16 | RamSize::K32 | RamSize::K48 | RamSize::K64
        )
    }

    pub const fn supports_serial_board(board: SerialBoard) -> bool {
        matches!(board, SerialBoard::TwoSio88)
    }

    /// SIMH monitor token used by `SET CPU <size>` for the RusTair sizes that
    /// have a direct representation in the classic target.
    pub const fn ram_modifier(size: RamSize) -> Option<&'static str> {
        match size {
            RamSize::K4 => Some("4K"),
            RamSize::K8 => Some("8K"),
            RamSize::K16 => Some("16K"),
            RamSize::K32 => Some("32K"),
            RamSize::K48 => Some("48K"),
            RamSize::K64 => Some("64K"),
            RamSize::Bytes256 | RamSize::K1 => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_simh_altair_rejects_sub_4k_rustair_memory_configs() {
        assert!(!ClassicAltairProfile::supports_ram_size(RamSize::Bytes256));
        assert!(!ClassicAltairProfile::supports_ram_size(RamSize::K1));
        assert!(ClassicAltairProfile::supports_ram_size(RamSize::K4));
    }

    #[test]
    fn classic_simh_altair_maps_our_supported_memory_sizes() {
        assert_eq!(ClassicAltairProfile::ram_modifier(RamSize::K8), Some("8K"));
        assert_eq!(ClassicAltairProfile::ram_modifier(RamSize::K64), Some("64K"));
    }

    #[test]
    fn classic_simh_altair_is_2sio_only() {
        assert!(!ClassicAltairProfile::supports_serial_board(SerialBoard::Sio88));
        assert!(ClassicAltairProfile::supports_serial_board(SerialBoard::TwoSio88));
    }
}
