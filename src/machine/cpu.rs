use crate::config::CpuModel;
use crate::cpu8080::{Bus, Cpu8080};

/// CPU implementation installed in the emulated machine.
///
/// Keep the concrete cores behind this boundary so adding another processor
/// model does not leak core-specific types into the Altair bus, front panel or
/// application UI.
pub(super) enum CpuCore {
    Intel8080(Cpu8080),
}

impl Default for CpuCore {
    fn default() -> Self {
        Self::new(CpuModel::Intel8080)
    }
}

impl CpuCore {
    pub(super) fn new(model: CpuModel) -> Self {
        match model {
            CpuModel::Intel8080 => Self::Intel8080(Cpu8080::new()),
        }
    }

    pub(super) fn model(&self) -> CpuModel {
        match self {
            Self::Intel8080(_) => CpuModel::Intel8080,
        }
    }

    pub(super) fn reset(&mut self) {
        match self {
            Self::Intel8080(cpu) => cpu.reset(),
        }
    }

    pub(super) fn step<B: Bus>(&mut self, bus: &mut B) -> u32 {
        match self {
            Self::Intel8080(cpu) => cpu.step(bus),
        }
    }

    pub(super) fn run_cycles<B: Bus>(&mut self, bus: &mut B, budget: u32) -> u32 {
        match self {
            Self::Intel8080(cpu) => cpu.run_cycles(bus, budget),
        }
    }

    pub(super) fn pc(&self) -> u16 {
        match self {
            Self::Intel8080(cpu) => cpu.pc,
        }
    }

    pub(super) fn set_pc(&mut self, pc: u16) {
        match self {
            Self::Intel8080(cpu) => cpu.pc = pc,
        }
    }

    pub(super) fn sp(&self) -> u16 {
        match self {
            Self::Intel8080(cpu) => cpu.sp,
        }
    }

    pub(super) fn accumulator(&self) -> u8 {
        match self {
            Self::Intel8080(cpu) => cpu.a,
        }
    }

    pub(super) fn flags(&self) -> u8 {
        match self {
            Self::Intel8080(cpu) => cpu.f,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_core_is_the_altair_8080() {
        let cpu = CpuCore::default();
        assert_eq!(cpu.model(), CpuModel::Intel8080);
        assert_eq!(cpu.pc(), 0);
    }
}
