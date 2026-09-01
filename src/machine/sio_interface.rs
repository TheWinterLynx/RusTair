pub(in crate::machine) use crate::config::{SioConnectorOutputs, SioElectricalLevel};
use crate::config::SioInterface;

/// Convert one board-internal TTL output to the selected physical interface.
///
/// MITS' theory of operation specifies:
/// - 88-SIO A: an internal TTL HIGH becomes a negative RS-232 level, while LOW
///   becomes approximately +3 V (logic inversion at the electrical boundary);
/// - 88-SIO B: non-inverting TTL buffers;
/// - 88-SIO C: an internal HIGH causes the output transistor to conduct, while
///   LOW leaves a high-impedance current-loop output.
pub(in crate::machine) const fn encode_output(
    interface: SioInterface,
    ttl_high: bool,
) -> SioElectricalLevel {
    match (interface, ttl_high) {
        (SioInterface::Rs232A, true) => SioElectricalLevel::Rs232Negative,
        (SioInterface::Rs232A, false) => SioElectricalLevel::Rs232Positive,
        (SioInterface::TtlB, true) => SioElectricalLevel::TtlHigh,
        (SioInterface::TtlB, false) => SioElectricalLevel::TtlLow,
        (SioInterface::TtyC, true) => SioElectricalLevel::CurrentLoopConducting,
        (SioInterface::TtyC, false) => SioElectricalLevel::CurrentLoopOpen,
    }
}

/// Convert an external connector input back to the board's TTL logic domain.
/// Returns `None` when a level from the wrong electrical family is presented to
/// the selected interface rather than silently accepting an impossible cable.
///
/// A is electrically inverted by the MITS level shifter. B is non-inverting.
/// C is logically non-inverting at SRSI/SRIN/SROT; current present is the active
/// HIGH/mark condition and an open loop is LOW/space at this digital boundary.
pub(in crate::machine) const fn decode_input(
    interface: SioInterface,
    level: SioElectricalLevel,
) -> Option<bool> {
    match (interface, level) {
        (SioInterface::Rs232A, SioElectricalLevel::Rs232Positive) => Some(false),
        (SioInterface::Rs232A, SioElectricalLevel::Rs232Negative) => Some(true),
        (SioInterface::TtlB, SioElectricalLevel::TtlLow) => Some(false),
        (SioInterface::TtlB, SioElectricalLevel::TtlHigh) => Some(true),
        (SioInterface::TtyC, SioElectricalLevel::CurrentLoopOpen) => Some(false),
        (SioInterface::TtyC, SioElectricalLevel::CurrentLoopConducting) => Some(true),
        _ => None,
    }
}

pub(in crate::machine) const fn connector_outputs(
    interface: SioInterface,
    tso_high: bool,
    bin_high: bool,
    bot_high: bool,
) -> SioConnectorOutputs {
    SioConnectorOutputs {
        stso: encode_output(interface, tso_high),
        sbin: encode_output(interface, bin_high),
        sbot: encode_output(interface, bot_high),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sio_a_inverts_between_ttl_and_rs232_levels() {
        assert_eq!(encode_output(SioInterface::Rs232A, true), SioElectricalLevel::Rs232Negative);
        assert_eq!(encode_output(SioInterface::Rs232A, false), SioElectricalLevel::Rs232Positive);
        assert_eq!(decode_input(SioInterface::Rs232A, SioElectricalLevel::Rs232Negative), Some(true));
        assert_eq!(decode_input(SioInterface::Rs232A, SioElectricalLevel::Rs232Positive), Some(false));
    }

    #[test]
    fn sio_b_is_non_inverting_ttl() {
        assert_eq!(encode_output(SioInterface::TtlB, true), SioElectricalLevel::TtlHigh);
        assert_eq!(encode_output(SioInterface::TtlB, false), SioElectricalLevel::TtlLow);
        assert_eq!(decode_input(SioInterface::TtlB, SioElectricalLevel::TtlHigh), Some(true));
        assert_eq!(decode_input(SioInterface::TtlB, SioElectricalLevel::TtlLow), Some(false));
    }

    #[test]
    fn sio_c_high_logic_conducts_current_loop_and_input_is_non_inverting() {
        assert_eq!(encode_output(SioInterface::TtyC, true), SioElectricalLevel::CurrentLoopConducting);
        assert_eq!(encode_output(SioInterface::TtyC, false), SioElectricalLevel::CurrentLoopOpen);
        assert_eq!(decode_input(SioInterface::TtyC, SioElectricalLevel::CurrentLoopConducting), Some(true));
        assert_eq!(decode_input(SioInterface::TtyC, SioElectricalLevel::CurrentLoopOpen), Some(false));
    }

    #[test]
    fn wrong_electrical_family_is_rejected_not_coerced() {
        assert_eq!(decode_input(SioInterface::Rs232A, SioElectricalLevel::TtlHigh), None);
        assert_eq!(decode_input(SioInterface::TtlB, SioElectricalLevel::Rs232Negative), None);
        assert_eq!(decode_input(SioInterface::TtyC, SioElectricalLevel::TtlLow), None);
    }
}
