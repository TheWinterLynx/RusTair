use super::s100_hardware::{
    FastRamCompatibilityConfig, S100HardwareConfig, S100InstalledCardConfig,
};
use super::sio::SioHardwareConfig;
use super::two_sio::{
    TwoSioAddressBlock, TwoSioBaudTap, TwoSioInterruptTarget, TwoSioInterruptWiring,
    TwoSioSignalInterface, TwoSioStraps,
};
use crate::s100_chassis::{AltairChassisModel, S100ChassisConfig};
use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};

impl S100HardwareConfig {
    /// Atomic persistence form for the complete physical S-100 assembly.
    ///
    /// `model|connectors|slot1;slot2;...` keeps one validated chassis snapshot in
    /// a single setting. Card-local commas never conflict with the top-level
    /// separators. Empty fitted connectors are encoded as `-`.
    pub fn persistence_key(self) -> String {
        let slots = (1..=self.fitted_connectors())
            .map(|slot| match self.slot(slot) {
                None => "-".to_owned(),
                Some(card) => card_persistence_key(card),
            })
            .collect::<Vec<_>>()
            .join(";");
        format!(
            "{}|{}|{}",
            chassis_model_key(self.chassis.model),
            self.chassis.fitted_connectors,
            slots
        )
    }

    pub fn from_persistence_key(value: &str) -> Option<Self> {
        let mut fields = value.splitn(3, '|');
        let model = parse_chassis_model(fields.next()?)?;
        let fitted_connectors = fields.next()?.parse::<usize>().ok()?;
        let slots_field = fields.next()?;
        let chassis = S100ChassisConfig {
            model,
            fitted_connectors,
        }
        .validate()
        .ok()?;
        let slot_fields = if slots_field.is_empty() {
            Vec::new()
        } else {
            slots_field.split(';').collect::<Vec<_>>()
        };
        if slot_fields.len() != fitted_connectors {
            return None;
        }

        let mut config = Self::empty(chassis).ok()?;
        for (index, token) in slot_fields.into_iter().enumerate() {
            let card = parse_card_persistence_key(token)?;
            config.set_slot(index + 1, card).ok()?;
        }
        config.validate().ok()
    }
}

fn chassis_model_key(model: AltairChassisModel) -> &'static str {
    match model {
        AltairChassisModel::Altair8800 => "8800",
        AltairChassisModel::Altair8800A => "8800a",
        AltairChassisModel::Altair8800B => "8800b",
    }
}

fn parse_chassis_model(value: &str) -> Option<AltairChassisModel> {
    Some(match value {
        "8800" => AltairChassisModel::Altair8800,
        "8800a" => AltairChassisModel::Altair8800A,
        "8800b" => AltairChassisModel::Altair8800B,
        _ => return None,
    })
}

fn ram_model_key(model: S100RamBoardModel) -> &'static str {
    match model {
        S100RamBoardModel::Mits1KStatic88Mcs => "1k-mcs",
        S100RamBoardModel::Mits4KDynamic88_4Mcd => "4k-mcd",
        S100RamBoardModel::Mits4KSynchronous88S4K => "s4k",
        S100RamBoardModel::Mits4KStatic88_4Mcs => "4k-mcs",
        S100RamBoardModel::Mits16KStatic88_16Mcs => "16k-mcs",
        S100RamBoardModel::Mits16KDynamic88_16Mcd => "16k-mcd",
    }
}

fn parse_ram_model(value: &str) -> Option<S100RamBoardModel> {
    Some(match value {
        "1k-mcs" => S100RamBoardModel::Mits1KStatic88Mcs,
        "4k-mcd" => S100RamBoardModel::Mits4KDynamic88_4Mcd,
        "s4k" => S100RamBoardModel::Mits4KSynchronous88S4K,
        "4k-mcs" => S100RamBoardModel::Mits4KStatic88_4Mcs,
        "16k-mcs" => S100RamBoardModel::Mits16KStatic88_16Mcs,
        "16k-mcd" => S100RamBoardModel::Mits16KDynamic88_16Mcd,
        _ => return None,
    })
}

fn card_persistence_key(card: S100InstalledCardConfig) -> String {
    match card {
        S100InstalledCardConfig::Mits8080Cpu => "cpu".to_owned(),
        S100InstalledCardConfig::Ram(config) => format!(
            "ram,{},{:04X},{}",
            ram_model_key(config.model),
            config.base_address,
            config.populated_bytes
        ),
        S100InstalledCardConfig::Mits88Sio(config) => {
            format!("sio,{}", config.persistence_key())
        }
        S100InstalledCardConfig::Mits88TwoSio {
            straps,
            interrupt_wiring,
        } => format!(
            "2sio,{:02X},{},{},{},{},{},{}",
            straps.address.base(),
            straps.port0_baud.label(),
            straps.port1_baud.label(),
            straps.port0_interface.persistence_key(),
            straps.port1_interface.persistence_key(),
            interrupt_wiring.port0.persistence_key(),
            interrupt_wiring.port1.persistence_key(),
        ),
        S100InstalledCardConfig::FastRamCompatibility(config) => format!(
            "fast,{:04X},{},{}",
            config.base_address, config.populated_bytes, config.read_wait_states
        ),
    }
}

fn parse_card_persistence_key(value: &str) -> Option<Option<S100InstalledCardConfig>> {
    if value == "-" {
        return Some(None);
    }
    if value == "cpu" {
        return Some(Some(S100InstalledCardConfig::Mits8080Cpu));
    }
    if let Some(value) = value.strip_prefix("sio,") {
        return Some(Some(S100InstalledCardConfig::Mits88Sio(
            SioHardwareConfig::from_persistence_key(value)?,
        )));
    }

    let fields = value.split(',').collect::<Vec<_>>();
    let card = match fields.as_slice() {
        ["ram", model, base, populated] => S100InstalledCardConfig::Ram(
            S100RamCardConfig::with_population(
                parse_ram_model(model)?,
                u16::from_str_radix(base, 16).ok()?,
                populated.parse().ok()?,
            )
            .validate()
            .ok()?,
        ),
        ["fast", base, populated, waits] => {
            S100InstalledCardConfig::FastRamCompatibility(
                FastRamCompatibilityConfig {
                    base_address: u16::from_str_radix(base, 16).ok()?,
                    populated_bytes: populated.parse().ok()?,
                    read_wait_states: waits.parse().ok()?,
                }
                .validate()
                .ok()?,
            )
        }
        ["2sio", base, baud0, baud1, interface0, interface1, irq0, irq1] => {
            S100InstalledCardConfig::Mits88TwoSio {
                straps: TwoSioStraps {
                    address: TwoSioAddressBlock::try_new(
                        u8::from_str_radix(base, 16).ok()?,
                    )?,
                    port0_baud: parse_two_sio_baud(baud0)?,
                    port1_baud: parse_two_sio_baud(baud1)?,
                    port0_interface: TwoSioSignalInterface::from_persistence_key(interface0)?,
                    port1_interface: TwoSioSignalInterface::from_persistence_key(interface1)?,
                },
                interrupt_wiring: TwoSioInterruptWiring {
                    port0: TwoSioInterruptTarget::from_persistence_key(irq0)?,
                    port1: TwoSioInterruptTarget::from_persistence_key(irq1)?,
                },
            }
        }
        _ => return None,
    };
    Some(Some(card.validate().ok()?))
}

fn parse_two_sio_baud(value: &str) -> Option<TwoSioBaudTap> {
    Some(match value {
        "110" => TwoSioBaudTap::Baud110,
        "150" => TwoSioBaudTap::Baud150,
        "300" => TwoSioBaudTap::Baud300,
        "1200" => TwoSioBaudTap::Baud1200,
        "1800" => TwoSioBaudTap::Baud1800,
        "2400" => TwoSioBaudTap::Baud2400,
        "4800" => TwoSioBaudTap::Baud4800,
        "9600" => TwoSioBaudTap::Baud9600,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_s100_hardware_round_trips_atomically() {
        let config = S100HardwareConfig::historical_8800b_18_slot_starter();
        let encoded = config.persistence_key();
        assert_eq!(S100HardwareConfig::from_persistence_key(&encoded), Some(config));
    }

    #[test]
    fn malformed_or_partial_slot_inventory_is_rejected() {
        assert!(S100HardwareConfig::from_persistence_key("8800b|18|cpu").is_none());
        assert!(S100HardwareConfig::from_persistence_key(
            "8800b|6|cpu;-;-;-;-;ram,4k-mcs,0001,4096"
        )
        .is_none());
        assert!(S100HardwareConfig::from_persistence_key("8800b|6|-;-;-;-;-;-").is_none());
    }

    #[test]
    fn serial_card_instance_state_survives_round_trip() {
        let mut config = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
        config.set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
        config.set_slot(
            2,
            Some(S100InstalledCardConfig::Mits88TwoSio {
                straps: TwoSioStraps {
                    address: TwoSioAddressBlock::try_new(0x44).unwrap(),
                    port0_baud: TwoSioBaudTap::Baud300,
                    port1_baud: TwoSioBaudTap::Baud9600,
                    port0_interface: TwoSioSignalInterface::Tty20mA,
                    port1_interface: TwoSioSignalInterface::Rs232,
                },
                interrupt_wiring: TwoSioInterruptWiring {
                    port0: TwoSioInterruptTarget::Vi3,
                    port1: TwoSioInterruptTarget::Disconnected,
                },
            }),
        )
        .unwrap();
        let encoded = config.persistence_key();
        assert_eq!(S100HardwareConfig::from_persistence_key(&encoded), Some(config));
    }
}
