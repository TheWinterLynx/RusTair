mod cpu_model_compat;
mod external_com;
mod external_serial;
mod machine;
mod terminal;

pub use external_com::{
    ComDataBits, ComFlowControl, ComParity, ComStopBits, ExternalComConfig,
};
pub use external_serial::{
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope,
};
pub use machine::{
    AppConfig, Asr33Speed, CompatibilityConfig, CpuBoard, CpuModel, EmulationSpeed, MachineConfig,
    PeripheralConfig, PreferencesConfig, RamBoardProfile, RamInit, RamSize, SerialBoard,
    TerminalSpeed,
};
pub use terminal::TerminalDuplex;
