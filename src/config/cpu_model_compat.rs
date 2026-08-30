use super::machine::CpuModel;

impl CpuModel {
    /// Legacy compatibility helper for existing UI/runtime code.
    ///
    /// New physical-machine code should obtain the authentic clock from
    /// `MachineConfig::cpu_board().clock_hz()`. This method can be removed when
    /// the runtime is migrated before a second CPU board becomes selectable.
    pub const fn clock_hz(self) -> u32 {
        match self {
            Self::Intel8080 => 2_000_000,
        }
    }
}
