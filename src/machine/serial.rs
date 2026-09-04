// Board-specific serial hardware lives below this module so the S-100 I/O
// wrapper remains the only route from the machine to a UART implementation.
#[path = "sio_interface.rs"]
pub(super) mod sio_interface;
#[path = "sio.rs"]
pub(super) mod sio;

use crate::config::{SerialBoard, SioConnectorOutputs, SioElectricalLevel};

impl super::AltairBus {
    /// Physical revision and interrupt-pad destinations installed on the 88-SIO.
    ///
    /// Keeping these as hardware types (rather than strings/UI state) also makes
    /// the Fast and Cycle machines observe the exact same card configuration.
    pub fn sio_physical_wiring(
        &self,
    ) -> Option<(
        super::SioRevision,
        super::SioInterruptTarget,
        super::SioInterruptTarget,
    )> {
        if self.io.serial_board() != SerialBoard::Sio88 { return None; }
        let config = self.io.sio_hardware();
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
        if self.cycle_uses_physical_serial() {
            return self.memory.sio_handshake_lines();
        }
        let lines = self.io.sio_handshake_lines()?;
        Some((
            lines.rsi_high,
            lines.input_device_ready,
            lines.output_device_ready,
            lines.tso_high,
            lines.bin_high,
            lines.bot_high,
        ))
    }

    /// STSO/SBIN/SBOT after the physically selected A/B/C line interface.
    /// RS-232 voltage polarity, TTL level and current-loop conduction remain
    /// distinct typed states instead of being collapsed to an ambiguous bool.
    pub fn sio_connector_outputs(&self) -> Option<SioConnectorOutputs> {
        if self.io.serial_board() != SerialBoard::Sio88 { return None; }
        let lines = self.io.sio_handshake_lines()?;
        Some(sio_interface::connector_outputs(
            self.io.sio_hardware().interface,
            lines.tso_high,
            lines.bin_high,
            lines.bot_high,
        ))
    }

    /// Translate one SRSI/SRIN/SROT connector level through the selected A/B/C
    /// interface back to the board's common TTL logic domain. A level belonging
    /// to another electrical family is rejected rather than silently coerced.
    pub fn sio_decode_connector_input(&self, level: SioElectricalLevel) -> Option<bool> {
        if self.io.serial_board() != SerialBoard::Sio88 { return None; }
        sio_interface::decode_input(self.io.sio_hardware().interface, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        SioHardwareConfig, SioInterface, SioInterruptTarget, SioInterruptWiring,
        SioRevision,
    };

    #[test]
    fn bus_exposes_installed_sio_revision_and_interrupt_pad_wiring() {
        let mut bus = super::super::AltairBus::default();
        let mut config = SioHardwareConfig::default();
        config.revision = SioRevision::Rev0;
        config.interrupt_wiring = SioInterruptWiring {
            input: SioInterruptTarget::Vi2,
            output: SioInterruptTarget::Pint,
        };
        bus.configure_sio_hardware(config);
        assert_eq!(
            bus.sio_physical_wiring(),
            Some((SioRevision::Rev0, SioInterruptTarget::Vi2, SioInterruptTarget::Pint))
        );
    }

    #[test]
    fn bus_exposes_all_six_original_sio_logical_signals() {
        let mut bus = super::super::AltairBus::default();
        let mut config = bus.sio_hardware();
        config.revision = SioRevision::Rev0;
        bus.configure_sio_hardware(config);
        assert_eq!(bus.sio_logical_lines(), Some((true, false, false, true, false, false)));
        assert!(bus.pulse_sio_input_device_ready());
        assert!(bus.pulse_sio_output_device_ready());
        assert_eq!(bus.sio_logical_lines(), Some((true, true, true, true, true, true)));
    }

    #[test]
    fn bus_projects_same_logic_through_each_physical_abc_interface() {
        let mut bus = super::super::AltairBus::default();
        let mut config = bus.sio_hardware();
        config.revision = SioRevision::Rev0;
        config.interface = SioInterface::Rs232A;
        bus.configure_sio_hardware(config);
        assert!(bus.pulse_sio_input_device_ready());
        assert!(bus.pulse_sio_output_device_ready());
        let a = bus.sio_connector_outputs().unwrap();
        assert_eq!(a.stso, SioElectricalLevel::Rs232Negative);
        assert_eq!(a.sbin, SioElectricalLevel::Rs232Negative);
        assert_eq!(a.sbot, SioElectricalLevel::Rs232Negative);
        assert_eq!(bus.sio_decode_connector_input(SioElectricalLevel::Rs232Negative), Some(true));
        assert_eq!(bus.sio_decode_connector_input(SioElectricalLevel::TtlHigh), None);

        config.interface = SioInterface::TtlB;
        bus.configure_sio_hardware(config);
        let b = bus.sio_connector_outputs().unwrap();
        assert_eq!(b.stso, SioElectricalLevel::TtlHigh);
        assert_eq!(b.sbin, SioElectricalLevel::TtlLow);
        assert_eq!(b.sbot, SioElectricalLevel::TtlLow);

        config.interface = SioInterface::TtyC;
        bus.configure_sio_hardware(config);
        let c = bus.sio_connector_outputs().unwrap();
        assert_eq!(c.stso, SioElectricalLevel::CurrentLoopConducting);
        assert_eq!(c.sbin, SioElectricalLevel::CurrentLoopOpen);
        assert_eq!(c.sbot, SioElectricalLevel::CurrentLoopOpen);
    }
}
