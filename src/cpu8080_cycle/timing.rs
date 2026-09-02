/// Intel 8080 machine-cycle classes.
///
/// The externally visible status words match the processor status byte driven
/// on D0-D7 while SYNC is asserted in T1. The values follow Intel's 8080 bus
/// definition and the same table used by Jim Drygiannakis' MIT-licensed
/// `8080Emu` reference implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MachineCycle {
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
}

impl MachineCycle {
    pub const fn status_word(self) -> Option<u8> {
        match self {
            Self::InstructionFetch => Some(0xA2),
            Self::MemoryRead => Some(0x82),
            Self::MemoryWrite => Some(0x00),
            Self::StackRead => Some(0x86),
            Self::StackWrite => Some(0x04),
            Self::InputRead => Some(0x42),
            Self::OutputWrite => Some(0x10),
            Self::InterruptAck => Some(0x23),
            Self::HaltAck => Some(0x8A),
            Self::InterruptAckWhileHalt => Some(0x2B),
            // Internal cycles do not expose a processor status word.
            Self::Internal => None,
        }
    }
}

/// One of the four authoritative digital clock edges inside an Intel 8080
/// T-state. The real part requires two non-overlapping clock phases. RusTair
/// models the digital ordering/ownership of those edges; analog pulse width,
/// slew and propagation-delay spread remain explicit non-claims.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockEdge {
    Phi1Rising,
    Phi1Falling,
    Phi2Rising,
    Phi2Falling,
}

impl ClockEdge {
    pub const ALL: [Self; 4] = [
        Self::Phi1Rising,
        Self::Phi1Falling,
        Self::Phi2Rising,
        Self::Phi2Falling,
    ];

    /// Clock-pin levels immediately after this edge. There is deliberately no
    /// overlap: both phases are low between PHI1 falling and PHI2 rising and
    /// again between PHI2 falling and the next PHI1 rising.
    pub const fn clock_levels_after(self) -> (bool, bool) {
        match self {
            Self::Phi1Rising => (true, false),
            Self::Phi1Falling => (false, false),
            Self::Phi2Rising => (false, true),
            Self::Phi2Falling => (false, false),
        }
    }
}

/// One complete 8080 T-state. Each state is now decomposable into the four
/// authoritative `ClockEdge` events above; the compatibility `tick()` API may
/// still expose a complete T-state to callers that do not need edge stepping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TState {
    T1,
    T2,
    Tw,
    T3,
    T4,
    T5,
    /// Indefinite halt dwell state entered after the HLT acknowledge cycle.
    Thalt,
    /// Indefinite bus-release dwell while HOLD remains asserted.
    Thold,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn externally_visible_status_words_match_the_8080_bus_definition() {
        assert_eq!(MachineCycle::InstructionFetch.status_word(), Some(0xA2));
        assert_eq!(MachineCycle::MemoryRead.status_word(), Some(0x82));
        assert_eq!(MachineCycle::MemoryWrite.status_word(), Some(0x00));
        assert_eq!(MachineCycle::StackRead.status_word(), Some(0x86));
        assert_eq!(MachineCycle::StackWrite.status_word(), Some(0x04));
        assert_eq!(MachineCycle::InputRead.status_word(), Some(0x42));
        assert_eq!(MachineCycle::OutputWrite.status_word(), Some(0x10));
        assert_eq!(MachineCycle::InterruptAck.status_word(), Some(0x23));
        assert_eq!(MachineCycle::HaltAck.status_word(), Some(0x8A));
        assert_eq!(
            MachineCycle::InterruptAckWhileHalt.status_word(),
            Some(0x2B)
        );
        assert_eq!(MachineCycle::Internal.status_word(), None);
    }

    #[test]
    fn clock_edges_are_non_overlapping_and_repeat_in_historical_order() {
        assert_eq!(
            ClockEdge::ALL.map(ClockEdge::clock_levels_after),
            [(true, false), (false, false), (false, true), (false, false)]
        );
    }
}
