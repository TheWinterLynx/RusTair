mod external_serial;
mod machine;
mod terminal;

pub use external_serial::{
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope,
};
pub use machine::{
    AppConfig, Asr33Speed, CompatibilityConfig, CpuModel, EmulationSpeed, MachineConfig,
    PeripheralConfig, PreferencesConfig, RamInit, RamSize, SerialBoard, TerminalSpeed,
};
pub use terminal::TerminalDuplex;
