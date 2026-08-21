#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RamSize {
    Bytes256,
    K1,
    K4,
    K8,
    K16,
    K32,
    K48,
    K64,
}

impl RamSize {
    pub const ALL: [Self; 8] = [
        Self::Bytes256,
        Self::K1,
        Self::K4,
        Self::K8,
        Self::K16,
        Self::K32,
        Self::K48,
        Self::K64,
    ];

    pub const fn bytes(self) -> usize {
        match self {
            Self::Bytes256 => 256,
            Self::K1 => 1024,
            Self::K4 => 4 * 1024,
            Self::K8 => 8 * 1024,
            Self::K16 => 16 * 1024,
            Self::K32 => 32 * 1024,
            Self::K48 => 48 * 1024,
            Self::K64 => 64 * 1024,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bytes256 => "256 bytes",
            Self::K1 => "1 KiB",
            Self::K4 => "4 KiB",
            Self::K8 => "8 KiB",
            Self::K16 => "16 KiB",
            Self::K32 => "32 KiB",
            Self::K48 => "48 KiB",
            Self::K64 => "64 KiB",
        }
    }
}

impl Default for RamSize {
    fn default() -> Self {
        Self::K8
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RamInit {
    Random,
    Zeroed,
}

impl RamInit {
    pub const ALL: [Self; 2] = [Self::Random, Self::Zeroed];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Random => "Random power-on contents",
            Self::Zeroed => "Zero-filled",
        }
    }
}

impl Default for RamInit {
    fn default() -> Self {
        Self::Random
    }
}

/// MITS serial interface installed in the emulated Altair.
///
/// The standard MITS console port assignments are used. Only the selected
/// board decodes its ports; this is hardware configuration, not a compatibility
/// alias that exposes both interfaces at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialBoard {
    Sio88,
    TwoSio88,
}

impl SerialBoard {
    pub const ALL: [Self; 2] = [Self::Sio88, Self::TwoSio88];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sio88 => "MITS 88-SIO",
            Self::TwoSio88 => "MITS 88-2SIO",
        }
    }

    /// Status/control address of the board's first serial port.
    pub const fn status_port(self) -> u8 {
        match self {
            Self::Sio88 => 0x00,
            Self::TwoSio88 => 0x10,
        }
    }

    /// Data address of the board's first serial port.
    pub const fn data_port(self) -> u8 {
        match self {
            Self::Sio88 => 0x01,
            Self::TwoSio88 => 0x11,
        }
    }

    /// Status/control address of Port 1 on a fully populated 88-2SIO.
    pub const fn port1_status_port(self) -> Option<u8> {
        match self {
            Self::Sio88 => None,
            Self::TwoSio88 => Some(0x12),
        }
    }

    /// Data address of Port 1 on a fully populated 88-2SIO.
    pub const fn port1_data_port(self) -> Option<u8> {
        match self {
            Self::Sio88 => None,
            Self::TwoSio88 => Some(0x13),
        }
    }
}

impl Default for SerialBoard {
    fn default() -> Self {
        // Preserve the current bundled BASIC 3.2 startup with the front-panel
        // sense switches left at zero.
        Self::Sio88
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct MachineConfig {
    pub ram_size: RamSize,
    pub ram_init: RamInit,
    pub serial_board: SerialBoard,
}

/// Optional software compatibility workarounds.
///
/// These default to disabled so the emulator reproduces original hardware and
/// software behaviour unless the user explicitly opts into a workaround.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct CompatibilityConfig {
    pub basic32_64k_probe_workaround: bool,
}

/// Application convenience behaviour that does not change emulated hardware.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreferencesConfig {
    /// Reveal the device physically connected to the bundled BASIC console port
    /// after loading BASIC. This never changes serial cable assignments.
    pub auto_open_basic_console: bool,
}

impl Default for PreferencesConfig {
    fn default() -> Self {
        Self {
            // Preserve the historical RusTair UI behaviour unless the user opts
            // out; the preference only controls window visibility.
            auto_open_basic_console: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct AppConfig {
    pub machine: MachineConfig,
    pub compatibility: CompatibilityConfig,
    pub preferences: PreferencesConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_workarounds_are_opt_in() {
        let config = AppConfig::default();
        assert!(!config.compatibility.basic32_64k_probe_workaround);
    }

    #[test]
    fn basic_console_auto_open_preserves_previous_default() {
        assert!(AppConfig::default().preferences.auto_open_basic_console);
    }

    #[test]
    fn default_serial_board_is_88_sio() {
        assert_eq!(AppConfig::default().machine.serial_board, SerialBoard::Sio88);
    }

    #[test]
    fn two_sio_exposes_both_standard_port_pairs() {
        let board = SerialBoard::TwoSio88;
        assert_eq!((board.status_port(), board.data_port()), (0x10, 0x11));
        assert_eq!(board.port1_status_port(), Some(0x12));
        assert_eq!(board.port1_data_port(), Some(0x13));
    }
}
