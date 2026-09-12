// Board-specific serial hardware lives below this module so the S-100 I/O
// wrapper remains the only route from the machine to a UART implementation.
#[path = "sio.rs"]
pub(super) mod sio;
#[path = "sio_interface.rs"]
pub(super) mod sio_interface;

use crate::config::{SioConnectorOutputs, SioElectricalLevel};

impl super::AltairBus {
    /// Physical revision and interrupt-pad destinations installed on the 88-SIO.
    ///
    /// The values come from the actual card in the live S-100 inventory. There
    /// is no dormant/global 88-SIO configuration beside that slot anymore.
    pub fn sio_physical_wiring(
        &self,
    ) -> Option<(
        super::SioRevision,
        super::SioInterruptTarget,
        super::SioInterruptTarget,
    )> {
        let config = self.memory.primary_sio_hardware()?;
        Some((
            config.revision,
            config.interrupt_wiring.input,
            config.interrupt_wiring.output,
        ))
    }

    /// Logical 88-SIO line state in board TTL terms:
    /// `(RSI, RIN-ready-latched, ROT-ready-latched, TSO, BIN, BOT)`.
    ///
    /// RIN/ROT are pulse inputs, so the stable observable state is the respective
    /// device-ready flip-flop that the pulse set. RSI/TSO are the instantaneous
    /// asynchronous serial levels; idle is MARK/HIGH. BIN/BOT are board outputs.
    pub fn sio_logical_lines(&self) -> Option<(bool, bool, bool, bool, bool, bool)> {
        self.memory.sio_handshake_lines()
    }

    /// STSO/SBIN/SBOT after the physically selected A/B/C line interface.
    /// RS-232 voltage polarity, TTL level and current-loop conduction remain
    /// distinct typed states instead of being collapsed to an ambiguous bool.
    pub fn sio_connector_outputs(&self) -> Option<SioConnectorOutputs> {
        let config = self.memory.primary_sio_hardware()?;
        let lines = self.memory.sio_handshake_lines()?;
        Some(sio_interface::connector_outputs(
            config.interface,
            lines.3,
            lines.4,
            lines.5,
        ))
    }

    /// Translate one SRSI/SRIN/SROT connector level through the selected A/B/C
    /// interface back to the board's common TTL logic domain. A level belonging
    /// to another electrical family is rejected rather than silently coerced.
    pub fn sio_decode_connector_input(&self, level: SioElectricalLevel) -> Option<bool> {
        let config = self.memory.primary_sio_hardware()?;
        sio_interface::decode_input(config.interface, level)
    }

    /// Prepared Adaptive-Cycle Full memory access. This is a normal guest
    /// transaction on the bus-owned physical S-100 memory fabric, never a
    /// debugger/inspection shortcut. The Full dispatcher may call it only after
    /// proving a unique non-overlapping responder and no wait-state/event barrier.
    /// No connector/presentation replay occurs here; Full projects observable bus
    /// duty separately and materializes the exact fabric before returning Partial.
    #[inline]
    pub(crate) fn cycle_full_guest_read(&mut self, address: u16) -> u8 {
        self.memory.read(address)
    }

    /// Guest write counterpart to `cycle_full_guest_read`. RuntimeRamCard storage,
    /// S-100 decode and the physical protection latch remain authoritative.
    #[inline]
    pub(crate) fn cycle_full_guest_write(&mut self, address: u16, value: u8) {
        self.memory.write(address, value);
    }

    /// DI/EI are CPU-internal instructions, but INTE is a real processor-board
    /// output visible on the S-100/front panel. Full updates that same canonical
    /// bus state directly instead of routing through a second machine.
    #[inline]
    pub(crate) fn cycle_full_set_inte(&mut self, enabled: bool) {
        self.s100.set_inte(enabled);
    }

    /// Canonical instruction-completion hook shared by both Adaptive Cycle paths.
    /// It exists only for optional diagnostic metering; guest execution remains
    /// owned by FullInstructionBus or the exact Partial T-state path.
    #[inline]
    pub(crate) fn cycle_instruction_complete(&mut self, address: u16, t_states: u32) {
        if self.diagnostic_meter.is_some() {
            self.record_cpu_diagnostic_instruction(address, t_states);
        }
    }

    /// Inherent Partial-path entry point. This intentionally is not a
    /// `cpu8080::Bus` implementation: it only forwards completed-instruction
    /// metering into the same neutral Cycle hook used by Full.
    #[inline]
    pub(crate) fn instruction_complete(&mut self, address: u16, _opcode: u8, t_states: u32) {
        self.cycle_instruction_complete(address, t_states);
    }

    /// Full uses the same neutral completion hook as Partial.
    #[inline]
    pub(crate) fn cycle_full_instruction_complete(&mut self, address: u16, t_states: u32) {
        self.cycle_instruction_complete(address, t_states);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        RamInit, S100HardwareConfig, S100InstalledCardConfig, SioHardwareConfig, SioInterface,
        SioInterruptTarget, SioInterruptWiring, SioRevision,
    };
    use crate::s100_chassis::S100ChassisConfig;

    fn bus_with_sio(config: SioHardwareConfig) -> super::super::AltairBus {
        let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
        hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        hardware
            .set_slot(2, Some(S100InstalledCardConfig::Mits88Sio(config)))
            .unwrap();
        let mut bus = super::super::AltairBus::default();
        bus.configure_s100_hardware_memory(hardware.validate().unwrap(), RamInit::Zeroed)
            .unwrap();
        bus
    }

    #[test]
    fn bus_exposes_installed_sio_revision_and_interrupt_pad_wiring() {
        let mut config = SioHardwareConfig::default();
        config.revision = SioRevision::Rev0;
        config.interrupt_wiring = SioInterruptWiring {
            input: SioInterruptTarget::Vi2,
            output: SioInterruptTarget::Pint,
        };
        let bus = bus_with_sio(config);
        assert_eq!(
            bus.sio_physical_wiring(),
            Some((
                SioRevision::Rev0,
                SioInterruptTarget::Vi2,
                SioInterruptTarget::Pint
            ))
        );
    }

    #[test]
    fn bus_exposes_all_six_original_sio_logical_signals() {
        let config = SioHardwareConfig {
            revision: SioRevision::Rev0,
            ..SioHardwareConfig::default()
        };
        let mut bus = bus_with_sio(config);
        assert_eq!(
            bus.sio_logical_lines(),
            Some((true, false, false, true, false, false))
        );
        assert!(bus.pulse_sio_input_device_ready());
        assert!(bus.pulse_sio_output_device_ready());
        assert_eq!(
            bus.sio_logical_lines(),
            Some((true, true, true, true, true, true))
        );
    }

    #[test]
    fn bus_projects_same_logic_through_each_physical_abc_interface() {
        for (interface, expected) in [
            (
                SioInterface::Rs232A,
                (
                    SioElectricalLevel::Rs232Negative,
                    SioElectricalLevel::Rs232Negative,
                    SioElectricalLevel::Rs232Negative,
                ),
            ),
            (
                SioInterface::TtlB,
                (
                    SioElectricalLevel::TtlHigh,
                    SioElectricalLevel::TtlHigh,
                    SioElectricalLevel::TtlHigh,
                ),
            ),
            (
                SioInterface::TtyC,
                (
                    SioElectricalLevel::CurrentLoopConducting,
                    SioElectricalLevel::CurrentLoopConducting,
                    SioElectricalLevel::CurrentLoopConducting,
                ),
            ),
        ] {
            let config = SioHardwareConfig {
                revision: SioRevision::Rev0,
                interface,
                ..SioHardwareConfig::default()
            };
            let mut bus = bus_with_sio(config);
            assert!(bus.pulse_sio_input_device_ready());
            assert!(bus.pulse_sio_output_device_ready());
            let outputs = bus.sio_connector_outputs().unwrap();
            assert_eq!((outputs.stso, outputs.sbin, outputs.sbot), expected);
        }

        let rs232 = bus_with_sio(SioHardwareConfig {
            revision: SioRevision::Rev0,
            interface: SioInterface::Rs232A,
            ..SioHardwareConfig::default()
        });
        assert_eq!(
            rs232.sio_decode_connector_input(SioElectricalLevel::Rs232Negative),
            Some(true)
        );
        assert_eq!(
            rs232.sio_decode_connector_input(SioElectricalLevel::TtlHigh),
            None
        );
    }
}
