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

/// One complete 8080 T-state.
///
/// RusTair's cycle core starts at T-state granularity. Pin transitions within a
/// T-state are derived from the Phi1/Phi2 edge-level reference and can be made
/// explicit later without changing the public machine-cycle model.
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
}
