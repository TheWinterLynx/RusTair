use crate::backend::BackendHost;
use crate::config::{
    RamInit, S100InstalledCardConfig, SerialBoard, TwoSioInterruptWiring, TwoSioStraps,
};

/// Unit-test-only bridge for legacy setup code that predates slot-native S-100
/// configuration. Production builds do not expose these aggregate selectors.
/// Every call below rewrites the physical card inventory and remounts the live
/// S-100 fabric; no parallel UART or machine-wide serial authority is created.
impl BackendHost {
    pub fn configure_serial_board(&mut self, board: SerialBoard) {
        let mut hardware = self.s100_hardware();
        let serial_slots = hardware
            .serial_slots()
            .map(|(slot, _)| slot)
            .collect::<Vec<_>>();
        let target_slot = serial_slots
            .first()
            .copied()
            .or_else(|| {
                (1..=hardware.fitted_connectors())
                    .find(|&slot| hardware.slot(slot).is_none())
            })
            .expect("test assembly needs one free S-100 connector for the serial card");

        for slot in serial_slots {
            hardware.set_slot(slot, None).unwrap();
        }
        let card = match board {
            SerialBoard::Sio88 => S100InstalledCardConfig::Mits88Sio(Default::default()),
            SerialBoard::TwoSio88 => S100InstalledCardConfig::Mits88TwoSio {
                straps: TwoSioStraps::default(),
                interrupt_wiring: TwoSioInterruptWiring::default(),
            },
        };
        hardware.set_slot(target_slot, Some(card)).unwrap();
        self.configure_s100_hardware(hardware.validate().unwrap(), RamInit::Zeroed);
    }

    pub fn configure_two_sio_straps(&mut self, straps: TwoSioStraps) {
        let mut hardware = self.s100_hardware();
        let (slot, interrupt_wiring) = hardware
            .serial_slots()
            .find_map(|(slot, card)| match card {
                S100InstalledCardConfig::Mits88TwoSio {
                    interrupt_wiring,
                    ..
                } => Some((slot, interrupt_wiring)),
                _ => None,
            })
            .expect("test setup requires an installed 88-2SIO before changing straps");
        hardware
            .set_slot(
                slot,
                Some(S100InstalledCardConfig::Mits88TwoSio {
                    straps,
                    interrupt_wiring,
                }),
            )
            .unwrap();
        self.configure_s100_hardware(hardware.validate().unwrap(), RamInit::Zeroed);
    }
}
