use crate::cpu8080_cycle::{MachineCycle, TState};
use crate::machine::PanelLampSnapshot;

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
pub struct BusTeachingSnapshot {
    pub accuracy: BusTeachingAccuracy,
    pub engine: EmulationEngine,
    pub instruction_address: Option<u16>,
    pub opcode: Option<u8>,
    pub machine_cycle: BusMachineCycle,
    pub machine_cycle_index: Option<u8>,
    pub t_state: BusTState,
    pub address: Option<u16>,
    /// Transitional compatibility view. The canonical machine model now keeps
    /// 8080 D0-D7, S-100 DI, S-100 DO and the DI-wired front-panel DATA source
    /// distinct. This single field remains until the Teacher migration lands in
    /// the next checkpoint; it must not be treated as new electrical authority.
    pub data: Option<u8>,
    pub status_word: Option<u8>,
    pub pins: BusCpuPins,
    pub status: BusStatusLines,
    /// For `Exact`, these are CPU input levels captured with the exact T-state.
    /// They are intentionally historical if the host/debugger changes chassis
    /// controls after the tick without clocking another CPU T-state.
    pub ready: Option<bool>,
    /// Intel 8080 pin 14 INT input. On the Altair this is sourced from the
    /// canonical S-100 PINT line. It is deliberately distinct from the front-
    /// panel INT lamp / SINTA acknowledgement status bit.
    pub interrupt: Option<bool>,
    pub hold: Option<bool>,
    pub reset: Option<bool>,
    pub total_t_states: Option<u64>,
    pub instruction_t_states: Option<u32>,
    pub instruction_complete: Option<bool>,
    pub visible_lamps: PanelLampSnapshot,
}

impl BusTeachingSnapshot {
    pub fn reconstructed(engine: EmulationEngine, panel: FrontPanelState, cpu: CpuState) -> Self {
        let (instruction_address, total_t_states) = match cpu {
            CpuState::Intel8080(cpu) => (Some(cpu.pc), cpu.total_t_states),
            CpuState::Z80(cpu) => (Some(cpu.pc), cpu.total_t_states),
        };
        Self {
            accuracy: BusTeachingAccuracy::Reconstructed,
            engine,
            instruction_address,
            opcode: None,
            machine_cycle: if panel.powered { BusMachineCycle::Unknown } else { BusMachineCycle::PowerOff },
            machine_cycle_index: None,
            t_state: BusTState::Unknown,
            address: if panel.powered { Some(panel.address) } else { None },
            data: if panel.powered { Some(panel.data) } else { None },
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
