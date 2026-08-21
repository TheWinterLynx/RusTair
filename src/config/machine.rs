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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct MachineConfig {
    pub ram_size: RamSize,
    pub ram_init: RamInit,
}

/// Optional software compatibility workarounds.
///
/// These default to disabled so the emulator reproduces original hardware and
/// software behaviour unless the user explicitly opts into a workaround.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct CompatibilityConfig {
    pub basic32_64k_probe_workaround: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct AppConfig {
    pub machine: MachineConfig,
    pub compatibility: CompatibilityConfig,
}
