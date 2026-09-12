mod external_com;
mod external_serial;
mod machine;
mod s100_codec;
mod s100_hardware;
mod sio;
mod sio_electrical;
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
pub use s100_hardware::{
    FastRamCompatibilityConfig, MAX_S100_SLOTS, S100HardwareConfig, S100HardwareConfigError,
    S100InstalledCardConfig, S100InstalledCardKind, fitted_connector_choices,
};
pub use sio::{
    SioAddressPair, SioBaudRate, SioDataBits, SioHardwareConfig, SioInterface, SioInterruptTarget,
    SioInterruptWiring, SioParity, SioRevision, SioStopBits, SioWordFormat,
};
pub use sio_electrical::{SioConnectorOutputs, SioElectricalLevel};
pub use terminal::TerminalDuplex;
pub use two_sio::{
    TwoSioAddressBlock, TwoSioBaudTap, TwoSioInterruptTarget, TwoSioInterruptWiring,
    TwoSioSignalInterface, TwoSioStraps,
};
