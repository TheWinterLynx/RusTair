mod external_serial;
mod machine;

pub use external_serial::{ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope};
pub use machine::{
    AppConfig, Asr33Speed, CompatibilityConfig, CpuModel, EmulationSpeed, MachineConfig,
    PeripheralConfig, PreferencesConfig, RamInit, RamSize, SerialBoard, TerminalSpeed,
};
