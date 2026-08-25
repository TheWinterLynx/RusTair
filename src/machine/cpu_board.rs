use crate::cpu8080_cycle::{TState, TickTrace};

/// Machine-cycle classes emitted by the instruction-level 8080 core.
///
/// The fast core cannot expose every physical T-state, so its CPU-board adapter
/// expands one semantic machine cycle into synthetic S-100 samples. The cycle
/// core does not use this enum for timing; it supplies real pin-level samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum S100Cycle {
    InstructionFetch,
    MemoryRead,
    MemoryWrite,
    StackRead,
    StackWrite,
    InputRead,
    OutputWrite,
    InterruptAcknowledge,
    HaltAcknowledge,
    InterruptAcknowledgeWhileHalted,
}

impl S100Cycle {
    pub(super) const fn status_word(self) -> u8 {
        match self {
            Self::InstructionFetch => 0xA2,
            Self::MemoryRead => 0x82,
            Self::MemoryWrite => 0x00,
            Self::StackRead => 0x86,
            Self::StackWrite => 0x04,
            Self::InputRead => 0x42,
            Self::OutputWrite => 0x10,
            Self::InterruptAcknowledge => 0x23,
            Self::HaltAcknowledge => 0x8A,
            Self::InterruptAcknowledgeWhileHalted => 0x2B,
        }
    }

    const fn t_states(self) -> u32 {
        match self {
            Self::InstructionFetch | Self::HaltAcknowledge => 4,
            _ => 3,
        }
    }
}

/// Common electrical contract between a CPU-board adapter and the S-100 bus.
///
/// The front panel consumes only these samples; it never branches on which CPU
/// engine produced them. `None` address/data represents a tri-stated or
/// otherwise undriven bus. `status_word` is present only when the CPU board is
/// updating the S-100 status latch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct S100CpuSample {
    pub address: Option<u16>,
    pub data: Option<u8>,
    pub status_word: Option<u8>,
    pub inte: bool,
    pub ready: bool,
    pub wait: bool,
    pub hlda: bool,
}

/// S-100/front-panel control lines presented to a CPU board.
///
/// Interrupt request is intentionally not included yet: RusTair does not have
/// an S-100 interrupt-controller source wired into the chassis abstraction.
/// Adding it belongs here rather than in a CPU-specific backend.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct S100CpuControlLines {
    pub ready: bool,
    pub hold: bool,
    pub reset: bool,
}

/// Adapter for the validated instruction-level 8080 core.
///
/// It cannot reconstruct true sub-instruction bus activity, so it synthesizes a
/// conservative T-state-shaped stream from the machine-cycle callback. This is
/// why the fast backend continues to advertise `exact_bus_activity = false`.
pub(super) struct Fast8080S100Adapter;

impl Fast8080S100Adapter {
    pub(super) fn for_each_sample(
        address: u16,
        data: u8,
        cycle: S100Cycle,
        inte: bool,
        ready: bool,
        wait: bool,
        mut emit: impl FnMut(S100CpuSample),
    ) {
        let status = cycle.status_word();
        emit(S100CpuSample {
            address: Some(address),
            data: Some(status),
            status_word: Some(status),
            inte,
            ready,
            wait,
            hlda: false,
        });

        for _ in 1..cycle.t_states() {
            emit(S100CpuSample {
                address: Some(address),
                data: Some(data),
                status_word: None,
                inte,
                ready,
                wait,
                hlda: false,
            });
        }
    }
}

/// Adapter for the T-state Intel 8080 core.
///
/// Timing and output-control signals come directly from `TickTrace`. The
/// backend supplies `visible_data` because read data originates on RAM/I/O
/// boards rather than on the CPU's output pins.
pub(crate) struct Cycle8080S100Adapter;

impl Cycle8080S100Adapter {
    pub(crate) fn sample(
        trace: &TickTrace,
        visible_data: Option<u8>,
        ready: bool,
    ) -> S100CpuSample {
        let status_word = if trace.pins.hlda {
            None
        } else if trace.pins.sync {
            trace.pins.data_out
        } else if trace.t_state == TState::Thalt {
            // HALT dwell has no repeated SYNC, but after a HOLD grant the S-100
            // status latch must again represent the still-halted processor.
            trace.machine_cycle.status_word()
        } else {
            None
        };

        S100CpuSample {
            address: trace.pins.address,
            data: trace.pins.data_out.or(visible_data),
            status_word,
            inte: trace.pins.inte,
            ready,
            wait: trace.pins.wait,
            hlda: trace.pins.hlda,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu8080_cycle::{Cpu8080Pins, MachineCycle};

    #[test]
    fn fast_adapter_emits_common_s100_samples_without_claiming_pin_truth() {
        let mut samples = Vec::new();
        Fast8080S100Adapter::for_each_sample(
            0x1234,
            0x56,
            S100Cycle::InstructionFetch,
            true,
            true,
            false,
            |sample| samples.push(sample),
        );

        assert_eq!(samples.len(), 4);
        assert_eq!(samples[0].status_word, Some(0xA2));
        assert_eq!(samples[0].data, Some(0xA2));
        assert_eq!(samples[1].status_word, None);
        assert_eq!(samples[1].data, Some(0x56));
        assert!(samples.iter().all(|sample| sample.address == Some(0x1234)));
    }

    #[test]
    fn cycle_adapter_maps_real_tick_pins_to_the_same_contract() {
        let trace = TickTrace {
            machine_cycle: MachineCycle::MemoryRead,
            machine_cycle_index: 2,
            t_state: TState::T1,
            pins: Cpu8080Pins {
                address: Some(0x2000),
                data_out: Some(0x82),
                sync: true,
                inte: true,
                ..Cpu8080Pins::default()
            },
            opcode: Some(0x3A),
            instruction_complete: false,
            reset: false,
            fault: None,
            total_t_states: 5,
            instruction_t_states: 5,
        };

        let sample = Cycle8080S100Adapter::sample(&trace, None, true);
        assert_eq!(sample.address, Some(0x2000));
        assert_eq!(sample.data, Some(0x82));
        assert_eq!(sample.status_word, Some(0x82));
        assert!(sample.inte);
        assert!(sample.ready);
    }
}
