use std::time::Duration;

/// Processor model carried by an installed S-100 CPU board.
///
/// Do not add a processor here until RusTair has a real core for it. The
/// physical machine configuration stores a `CpuBoard`, not a bare processor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuModel {
    Intel8080,
}

impl CpuModel {
    pub const ALL: [Self; 1] = [Self::Intel8080];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Intel8080 => "Intel 8080",
        }
    }
}

impl Default for CpuModel {
    fn default() -> Self {
        Self::Intel8080
    }
}

/// Physical CPU board installed in the S-100 chassis.
///
/// This is deliberately separate from the emulator engine. Fast and Cycle
/// Accurate are two implementations of the same currently installed MITS 8080
/// board. A future Z80 implementation should enter the machine as a documented
/// historical S-100 CPU board, not as a synthetic backend-only CPU choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuBoard {
    Mits8080,
}

impl CpuBoard {
    pub const ALL: [Self; 1] = [Self::Mits8080];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mits8080 => "MITS 8080 CPU Board",
        }
    }

    pub const fn cpu_model(self) -> CpuModel {
        match self {
            Self::Mits8080 => CpuModel::Intel8080,
        }
    }

    /// Oscillator/CPU clock produced by the installed board.
    pub const fn clock_hz(self) -> u32 {
        match self {
            Self::Mits8080 => 2_000_000,
        }
    }
}

impl Default for CpuBoard {
    fn default() -> Self {
        Self::Mits8080
    }
}

/// Host-side execution speed. This never changes the installed CPU board or its
/// authentic hardware clock; it only changes how much virtual CPU time RusTair
/// advances per unit of host time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmulationSpeed {
    Authentic,
    X2,
    X5,
    X10,
    Unlimited,
}

impl EmulationSpeed {
    pub const ALL: [Self; 5] = [
        Self::Authentic,
        Self::X2,
        Self::X5,
        Self::X10,
        Self::Unlimited,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Authentic => "Authentic hardware clock",
            Self::X2 => "2x",
            Self::X5 => "5x",
            Self::X10 => "10x",
            Self::Unlimited => "Unlimited",
        }
    }

    pub fn cycle_budget(self, authentic_cycles: u32) -> u32 {
        match self {
            Self::Authentic => authentic_cycles,
            Self::X2 => authentic_cycles.saturating_mul(2),
            Self::X5 => authentic_cycles.saturating_mul(5),
            Self::X10 => authentic_cycles.saturating_mul(10),
            // No wall-clock throttle. Keep one update bounded so egui remains
            // responsive; the next repaint immediately supplies another chunk.
            Self::Unlimited => 1_000_000,
        }
    }
}

impl Default for EmulationSpeed {
    fn default() -> Self {
        Self::Authentic
    }
}

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

/// Electrical/timing profile of the installed S-100 RAM cards.
///
/// Capacity and initial contents are deliberately separate from card timing: an
/// Altair can have the same number of bytes implemented by very different boards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RamBoardProfile {
    /// Compatibility profile for later/fast memory: no PRDY stretching.
    FastNoWait,
    /// Original MITS 1K Static Memory Board using Intel 8101 RAMs. The 1975
    /// Theory of Operation specifies two wait cycles (1 us at 2 MHz) on reads.
    Mits1KStatic1975,
}

impl RamBoardProfile {
    pub const ALL: [Self; 2] = [Self::FastNoWait, Self::Mits1KStatic1975];

    pub const fn label(self) -> &'static str {
        match self {
            Self::FastNoWait => "Fast / no wait states",
            Self::Mits1KStatic1975 => "MITS 1K Static RAM (1975, 2 read waits)",
        }
    }

    pub const fn read_wait_states(self) -> u8 {
        match self {
            Self::FastNoWait => 0,
            Self::Mits1KStatic1975 => 2,
        }
    }
}

impl Default for RamBoardProfile {
    fn default() -> Self {
        Self::FastNoWait
    }
}

/// MITS serial interface installed in the emulated Altair.
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

    pub const fn status_port(self) -> u8 {
        match self {
            Self::Sio88 => 0x00,
            Self::TwoSio88 => 0x10,
        }
    }

    pub const fn data_port(self) -> u8 {
        match self {
            Self::Sio88 => 0x01,
            Self::TwoSio88 => 0x11,
        }
    }

    pub const fn port1_status_port(self) -> Option<u8> {
        match self {
            Self::Sio88 => None,
            Self::TwoSio88 => Some(0x12),
        }
    }

    pub const fn port1_data_port(self) -> Option<u8> {
        match self {
            Self::Sio88 => None,
            Self::TwoSio88 => Some(0x13),
        }
    }
}

impl Default for SerialBoard {
    fn default() -> Self {
        Self::Sio88
    }
}

/// Mechanical throughput of the Model 33. Authentic mode is 110 baud using an
/// 11-unit frame, i.e. 10 characters per second. Faster choices are explicit
/// emulator conveniences and remain independent of CPU emulation speed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Asr33Speed {
    Authentic110,
    Accelerated2x,
    Accelerated4x,
    Instant,
}

impl Asr33Speed {
    pub const ALL: [Self; 4] = [
        Self::Authentic110,
        Self::Accelerated2x,
        Self::Accelerated4x,
        Self::Instant,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Authentic110 => "110 baud / 10 cps (authentic)",
            Self::Accelerated2x => "20 cps (2x emulation)",
            Self::Accelerated4x => "40 cps (4x emulation)",
            Self::Instant => "Instant (emulation)",
        }
    }

    pub fn char_time(self) -> Duration {
        match self {
            Self::Authentic110 => Duration::from_millis(100),
            Self::Accelerated2x => Duration::from_millis(50),
            Self::Accelerated4x => Duration::from_millis(25),
            Self::Instant => Duration::ZERO,
        }
    }
}

impl Default for Asr33Speed {
    fn default() -> Self {
        Self::Authentic110
    }
}

/// Text-terminal line rate. Timing uses a conventional ten-bit asynchronous
/// frame (start + 8 data + stop). It is independent of CPU and ASR-33 speed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalSpeed {
    Instant,
    Baud300,
    Baud1200,
    Baud2400,
    Baud9600,
}

impl TerminalSpeed {
    pub const ALL: [Self; 5] = [
        Self::Instant,
        Self::Baud300,
        Self::Baud1200,
        Self::Baud2400,
        Self::Baud9600,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Instant => "Instant",
            Self::Baud300 => "300 baud",
            Self::Baud1200 => "1200 baud",
            Self::Baud2400 => "2400 baud",
            Self::Baud9600 => "9600 baud",
        }
    }

    pub const fn baud(self) -> Option<u32> {
        match self {
            Self::Instant => None,
            Self::Baud300 => Some(300),
            Self::Baud1200 => Some(1_200),
            Self::Baud2400 => Some(2_400),
            Self::Baud9600 => Some(9_600),
        }
    }

    pub fn char_time(self) -> Duration {
        match self.baud() {
            Some(baud) => Duration::from_secs_f64(10.0 / baud as f64),
            None => Duration::ZERO,
        }
    }
}

impl Default for TerminalSpeed {
    fn default() -> Self {
        Self::Baud9600
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct MachineConfig {
    pub cpu_board: CpuBoard,
    pub ram_size: RamSize,
    pub ram_init: RamInit,
    pub ram_board_profile: RamBoardProfile,
    pub serial_board: SerialBoard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct PeripheralConfig {
    pub asr33_speed: Asr33Speed,
    pub terminal_speed: TerminalSpeed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct CompatibilityConfig {
    pub basic32_64k_probe_workaround: bool,
    /// Reproduce the original 8800 RUN/STOP R-S latch having no defined
    /// power-on state. Disabled by default so powering on remains safely STOPped.
    pub historical_undefined_run_latch_power_on: bool,
}

/// Application/emulator behaviour that does not alter the physical Altair
/// configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreferencesConfig {
    pub auto_open_basic_console: bool,
    pub emulation_speed: EmulationSpeed,
}

impl Default for PreferencesConfig {
    fn default() -> Self {
        Self {
            auto_open_basic_console: true,
            emulation_speed: EmulationSpeed::Authentic,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct AppConfig {
    pub machine: MachineConfig,
    pub peripherals: PeripheralConfig,
    pub compatibility: CompatibilityConfig,
    pub preferences: PreferencesConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_altair_installs_mits_8080_board_at_two_megahertz() {
        let board = AppConfig::default().machine.cpu_board;
        assert_eq!(board, CpuBoard::Mits8080);
        assert_eq!(board.cpu_model(), CpuModel::Intel8080);
        assert_eq!(board.clock_hz(), 2_000_000);
    }

    #[test]
    fn emulation_speed_defaults_to_authentic() {
        assert_eq!(
            AppConfig::default().preferences.emulation_speed,
            EmulationSpeed::Authentic
        );
        assert_eq!(EmulationSpeed::X10.cycle_budget(40_000), 400_000);
    }

    #[test]
    fn peripheral_timing_defaults_are_independent() {
        let config = AppConfig::default();
        assert_eq!(config.peripherals.asr33_speed, Asr33Speed::Authentic110);
        assert_eq!(config.peripherals.asr33_speed.char_time(), Duration::from_millis(100));
        assert_eq!(config.peripherals.terminal_speed, TerminalSpeed::Baud9600);
    }

    #[test]
    fn terminal_baud_timing_uses_ten_bit_frames() {
        assert_eq!(
            TerminalSpeed::Baud300.char_time(),
            Duration::from_secs_f64(10.0 / 300.0)
        );
    }

    #[test]
    fn compatibility_workarounds_are_opt_in() {
        let compatibility = AppConfig::default().compatibility;
        assert!(!compatibility.basic32_64k_probe_workaround);
        assert!(!compatibility.historical_undefined_run_latch_power_on);
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

#[cfg(test)]
mod memory_board_profile_tests {
    use super::*;

    #[test]
    fn original_mits_1k_profile_has_two_read_wait_states() {
        assert_eq!(RamBoardProfile::Mits1KStatic1975.read_wait_states(), 2);
        assert_eq!(RamBoardProfile::FastNoWait.read_wait_states(), 0);
        assert_eq!(AppConfig::default().machine.ram_board_profile, RamBoardProfile::FastNoWait);
    }
}
