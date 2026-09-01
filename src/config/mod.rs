mod external_com;
mod external_serial;
mod machine;
mod sio;
mod terminal;
mod two_sio;

pub use external_com::{
    ComDataBits, ComFlowControl, ComModemInputMode, ComParity, ComStopBits, ExternalComConfig,
};
pub use external_serial::{
    ExternalSerialCharacterMode, ExternalSerialConfig, ExternalSerialSpeed, TcpListenScope,
};
pub use machine::{
    AppConfig, Asr33Speed, CompatibilityConfig, CpuBoard, CpuModel, EmulationSpeed, MachineConfig,
    PeripheralConfig, PreferencesConfig, RamBoardProfile, RamInit, RamSize, SerialBoard,
    TerminalSpeed,
};
pub use sio::{
    SioAddressPair, SioBaudRate, SioDataBits, SioHardwareConfig, SioInterface,
    SioInterruptTarget, SioInterruptWiring, SioParity, SioRevision, SioStopBits, SioWordFormat,
};
pub use terminal::TerminalDuplex;
pub use two_sio::{
    TwoSioAddressBlock, TwoSioBaudTap, TwoSioInterruptTarget, TwoSioInterruptWiring,
    TwoSioStraps,
};