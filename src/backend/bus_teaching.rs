use crate::cpu8080_cycle::{MachineCycle, TState};
use crate::machine::{AltairBus, AltairChassis, AltairMachine, PanelLampSnapshot};

use super::{CpuState, EmulationEngine, FrontPanelState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusTeachingAccuracy {
    Exact,
    ControlState,
    Reconstructed,
}

impl BusTeachingAccuracy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Exact => "EXACT T-STATE SAMPLE",
            Self::ControlState => "CONTROL STATE / NO T-STATE SAMPLE",
            Self::Reconstructed => "RECONSTRUCTED / APPROXIMATE",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusMachineCycle {
    PowerOff,
    PowerOnUndefined,
    ResetAsserted,
    ResetReleasedStopped,
    ResetReleasedRunning,
    InstructionFetch,
    MemoryRead,
    MemoryWrite,
    StackRead,
    StackWrite,
    InputRead,
    OutputWrite,
    InterruptAck,
    HaltAck,
    InterruptAckWhileHalt,
    Internal,
    Unknown,
}

impl BusMachineCycle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PowerOff => "POWER OFF",
            Self::PowerOnUndefined => "POWER ON / CPU STATE UNDEFINED",
            Self::ResetAsserted => "RESET ASSERTED",
            Self::ResetReleasedStopped => "RESET RELEASED / STOP-WAIT",
            Self::ResetReleasedRunning => "RESET RELEASED / RUN",
            Self::InstructionFetch => "INSTRUCTION FETCH",
            Self::MemoryRead => "MEMORY READ",
            Self::MemoryWrite => "MEMORY WRITE",
            Self::StackRead => "STACK READ",
            Self::StackWrite => "STACK WRITE",
            Self::InputRead => "INPUT READ",
            Self::OutputWrite => "OUTPUT WRITE",
            Self::InterruptAck => "INTERRUPT ACK",
            Self::HaltAck => "HALT ACK",
            Self::InterruptAckWhileHalt => "INTERRUPT ACK WHILE HALTED",
            Self::Internal => "INTERNAL",
            Self::Unknown => "UNSAMPLED / UNKNOWN",
        }
    }
}

impl From<MachineCycle> for BusMachineCycle {
    fn from(value: MachineCycle) -> Self {
        match value {
            MachineCycle::InstructionFetch => Self::InstructionFetch,
            MachineCycle::MemoryRead => Self::MemoryRead,
            MachineCycle::MemoryWrite => Self::MemoryWrite,
            MachineCycle::StackRead => Self::StackRead,
            MachineCycle::StackWrite => Self::StackWrite,
            MachineCycle::InputRead => Self::InputRead,
            MachineCycle::OutputWrite => Self::OutputWrite,
            MachineCycle::InterruptAck => Self::InterruptAck,
            MachineCycle::HaltAck => Self::HaltAck,
            MachineCycle::InterruptAckWhileHalt => Self::InterruptAckWhileHalt,
            MachineCycle::Internal => Self::Internal,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusTState {
    T1,
    T2,
    Tw,
    T3,
    T4,
    T5,
    Halt,
    Hold,
    Unknown,
}

impl BusTState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T2 => "T2",
            Self::Tw => "TW",
            Self::T3 => "T3",
            Self::T4 => "T4",
            Self::T5 => "T5",
            Self::Halt => "THALT",
            Self::Hold => "THOLD",
            Self::Unknown => "NO T-STATE",
        }
    }
}

impl From<TState> for BusTState {
    fn from(value: TState) -> Self {
        match value {
            TState::T1 => Self::T1,
            TState::T2 => Self::T2,
            TState::Tw => Self::Tw,
            TState::T3 => Self::T3,
            TState::T4 => Self::T4,
            TState::T5 => Self::T5,
            TState::Thalt => Self::Halt,
            TState::Thold => Self::Hold,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BusCpuPins {
    /// Exact Intel 8080 PHI1 package input. Fast/reconstructed snapshots leave
    /// this unknown rather than inventing a phase from elapsed instruction time.
    pub phi1: Option<bool>,
    /// Exact Intel 8080 PHI2 package input.
    pub phi2: Option<bool>,
    pub sync: Option<bool>,
    pub dbin: Option<bool>,
    pub wr_n: Option<bool>,
    pub inte: Option<bool>,
    pub wait: Option<bool>,
    pub hlda: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BusStatusLines {
    pub memr: Option<bool>,
    pub inp: Option<bool>,
    pub m1: Option<bool>,
    pub out: Option<bool>,
    pub hlta: Option<bool>,
    pub stack: Option<bool>,
    pub wo: Option<bool>,
    pub int_ack: Option<bool>,
    pub inte: Option<bool>,
    pub prot: Option<bool>,
    pub wait: Option<bool>,
    pub hlda: Option<bool>,
}

impl BusStatusLines {
    pub fn from_status_word(word: Option<u8>) -> Self {
        let Some(word) = word else { return Self::default(); };
        Self {
            memr: Some(word & 0x80 != 0),
            inp: Some(word & 0x40 != 0),
            m1: Some(word & 0x20 != 0),
            out: Some(word & 0x10 != 0),
            hlta: Some(word & 0x08 != 0),
            stack: Some(word & 0x04 != 0),
            wo: Some(word & 0x02 != 0),
            int_ack: Some(word & 0x01 != 0),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BusChassisSnapshot {
    pub accuracy: BusTeachingAccuracy,
    pub engine: EmulationEngine,
    pub powered: bool,
    pub running: bool,
    pub ext_clear: Option<bool>,
    pub address: Option<u16>,
    pub cpu_data: Option<u8>,
    pub s100_di: Option<u8>,
    pub s100_do: Option<u8>,
    pub panel_data: Option<u8>,
    pub status_word: Option<u8>,
    pub status: BusStatusLines,
    pub ready: Option<bool>,
    pub interrupt: Option<bool>,
    pub hold: Option<bool>,
    pub reset: Option<bool>,
    pub visible_lamps: PanelLampSnapshot,
}

pub(crate) trait BusChassisSource {
    fn powered(&self) -> bool;
    fn running(&self) -> bool;
    fn bus(&self) -> &AltairBus;
    fn ext_clear_asserted(&self) -> bool;
    fn address_leds(&self) -> u16;
    fn panel_lamps(&self) -> PanelLampSnapshot;
}

impl BusChassisSource for AltairMachine {
    fn powered(&self) -> bool { self.powered }
    fn running(&self) -> bool { self.running }
    fn bus(&self) -> &AltairBus { &self.bus }
    fn ext_clear_asserted(&self) -> bool { self.ext_clear_asserted() }
    fn address_leds(&self) -> u16 { self.address_leds() }
    fn panel_lamps(&self) -> PanelLampSnapshot { self.panel_lamps() }
}

impl BusChassisSource for AltairChassis {
    fn powered(&self) -> bool { self.powered }
    fn running(&self) -> bool { self.running }
    fn bus(&self) -> &AltairBus { &self.bus }
    fn ext_clear_asserted(&self) -> bool { self.ext_clear_asserted() }
    fn address_leds(&self) -> u16 { self.address_leds() }
    fn panel_lamps(&self) -> PanelLampSnapshot { self.panel_lamps() }
}

impl BusChassisSnapshot {
    pub(crate) fn from_altair_machine<M: BusChassisSource>(
        engine: EmulationEngine,
        machine: &M,
    ) -> Self {
        let powered = machine.powered();
        let bus = machine.bus();
        let lines = bus.cpu_control_lines();
        let status_word = powered.then(|| bus.raw_s100_status_word());
        let mut status = BusStatusLines::from_status_word(status_word);
        if powered {
            status.inte = Some(bus.raw_s100_inte());
            status.prot = Some(bus.raw_s100_prot());
            status.wait = Some(bus.raw_s100_wait());
            status.hlda = Some(bus.raw_s100_hlda());
        }
        Self {
            accuracy: BusTeachingAccuracy::ControlState,
            engine,
            powered,
            running: machine.running(),
            ext_clear: powered.then(|| machine.ext_clear_asserted()),
            address: powered.then(|| machine.address_leds()),
            cpu_data: powered.then(|| bus.raw_cpu_data()).flatten(),
            s100_di: powered.then(|| bus.raw_s100_data_in()).flatten(),
            s100_do: powered.then(|| bus.raw_s100_data_out()).flatten(),
            panel_data: powered.then(|| bus.raw_panel_data()),
            status_word,
            status,
            ready: powered.then_some(lines.ready),
            interrupt: powered.then_some(lines.interrupt),
            hold: powered.then_some(lines.hold),
            reset: powered.then_some(lines.reset),
            visible_lamps: machine.panel_lamps(),
        }
    }

    pub fn reconstructed(engine: EmulationEngine, panel: FrontPanelState) -> Self {
        let panel_data = panel.powered.then_some(panel.data);
        Self {
            accuracy: BusTeachingAccuracy::Reconstructed,
            engine,
            powered: panel.powered,
            running: panel.running,
            ext_clear: panel.powered.then_some(panel.ext_clear_asserted),
            address: panel.powered.then_some(panel.address),
            cpu_data: None,
            s100_di: None,
            s100_do: None,
            panel_data,
            status_word: None,
            status: BusStatusLines::default(),
            ready: None,
            interrupt: None,
            hold: None,
            reset: None,
            visible_lamps: panel.lamps,
        }
    }
}

/// Last teaching observation. `Exact` snapshots deliberately retain the CPU
/// inputs and outputs of the displayed T-state even when current chassis state
/// subsequently changes without another CPU clock. `current_chassis` is a
/// separate plane and is refreshed by the backend whenever the snapshot is read.
#[derive(Clone, Copy, Debug)]
pub struct BusTeachingSnapshot {
    pub accuracy: BusTeachingAccuracy,
    pub engine: EmulationEngine,
    pub instruction_address: Option<u16>,
    pub opcode: Option<u8>,
    pub machine_cycle: BusMachineCycle,
    pub machine_cycle_index: Option<u8>,
    pub t_state: BusTState,
    pub address: Option<u16>,
    pub data: Option<u8>,
    pub cpu_data: Option<u8>,
    pub s100_di: Option<u8>,
    pub s100_do: Option<u8>,
    pub panel_data: Option<u8>,
    pub status_word: Option<u8>,
    pub pins: BusCpuPins,
    pub status: BusStatusLines,
    pub ready: Option<bool>,
    pub interrupt: Option<bool>,
    pub hold: Option<bool>,
    pub reset: Option<bool>,
    pub total_t_states: Option<u64>,
    pub instruction_t_states: Option<u32>,
    pub instruction_complete: Option<bool>,
    pub visible_lamps: PanelLampSnapshot,
    /// Present-time chassis/backplane state. This is deliberately not part of
    /// the exact T-state sample above and must not be used as CPU pin truth for
    /// that historical tick.
    pub current_chassis: Option<BusChassisSnapshot>,
}

impl BusTeachingSnapshot {
    pub fn reconstructed(engine: EmulationEngine, panel: FrontPanelState, cpu: CpuState) -> Self {
        let CpuState::Intel8080(cpu) = cpu;
        let instruction_address = Some(cpu.pc);
        let total_t_states = cpu.total_t_states;
        let panel_data = if panel.powered { Some(panel.data) } else { None };
        let current_chassis = Some(BusChassisSnapshot::reconstructed(engine, panel));
        Self {
            accuracy: BusTeachingAccuracy::Reconstructed,
            engine,
            instruction_address,
            opcode: None,
            machine_cycle: if panel.powered { BusMachineCycle::Unknown } else { BusMachineCycle::PowerOff },
            machine_cycle_index: None,
            t_state: BusTState::Unknown,
            address: if panel.powered { Some(panel.address) } else { None },
            data: panel_data,
            cpu_data: None,
            s100_di: None,
            s100_do: None,
            panel_data,
            status_word: None,
            pins: BusCpuPins::default(),
            status: BusStatusLines::default(),
            ready: None,
            interrupt: None,
            hold: None,
            reset: None,
            total_t_states,
            instruction_t_states: None,
            instruction_complete: None,
            visible_lamps: panel.lamps,
            current_chassis,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_word_decodes_front_panel_lines() {
        let lines = BusStatusLines::from_status_word(Some(0xA2));
        assert_eq!(lines.memr, Some(true));
        assert_eq!(lines.m1, Some(true));
        assert_eq!(lines.wo, Some(true));
        assert_eq!(lines.inp, Some(false));
        assert_eq!(lines.out, Some(false));
    }

    #[test]
    fn unknown_cycle_label_does_not_claim_reconstruction_for_exact_backend_control_states() {
        assert_eq!(BusMachineCycle::Unknown.label(), "UNSAMPLED / UNKNOWN");
    }

    #[test]
    fn exact_accuracy_label_is_explicitly_a_sample() {
        assert_eq!(BusTeachingAccuracy::Exact.label(), "EXACT T-STATE SAMPLE");
    }
}
