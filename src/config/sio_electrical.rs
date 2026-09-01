/// Electrical state at the external wafer connector of a MITS 88-SIO A/B/C
/// interface. These values intentionally preserve the physical family instead
/// of collapsing RS-232 voltage, TTL voltage and current-loop state to one bool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SioElectricalLevel {
    /// 88-SIO A: approximately +3 V or greater.
    Rs232Positive,
    /// 88-SIO A: negative level (nominally around -12 V in the MITS circuit).
    Rs232Negative,
    /// 88-SIO B: TTL LOW.
    TtlLow,
    /// 88-SIO B: TTL HIGH.
    TtlHigh,
    /// 88-SIO C: high impedance / no loop current.
    CurrentLoopOpen,
    /// 88-SIO C: output transistor conducting loop current.
    CurrentLoopConducting,
}

/// The three signals driven from the 88-SIO toward the attached device after
/// the selected A/B/C electrical interface: serial transmit, input busy and
/// output busy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SioConnectorOutputs {
    pub stso: SioElectricalLevel,
    pub sbin: SioElectricalLevel,
    pub sbot: SioElectricalLevel,
}
