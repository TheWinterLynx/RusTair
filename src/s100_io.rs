//! Predecoded S-100 I/O and interrupt wiring indices.
//!
//! The real CPU broadcasts each I/O address on A0..A7 and every installed card
//! sees those lines in parallel. A software emulator therefore does not need to
//! serially ask every card whether it responds on every IN/OUT cycle: the fixed
//! jumper/strap decode can be compiled once when the POWER-OFF chassis assembly
//! is mounted. The resulting masks are acceleration metadata only; selected
//! cards must still perform their normal electrical `S100ElectricalCard`
//! transaction and contend normally if more than one decoder matches.

use crate::config::{
    S100HardwareConfig, S100InstalledCardConfig, SioInterruptTarget, TwoSioInterruptTarget,
};
use crate::s100_backplane::{S100SlotMask, s100_slot_mask};

pub const S100_IO_PORT_COUNT: usize = 256;

/// Static responders that can possibly participate in one I/O or interrupt
/// transaction for the currently mounted card inventory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S100IoDecodeIndex {
    port_responders: [S100SlotMask; S100_IO_PORT_COUNT],
    pint_drivers: S100SlotMask,
    vi_drivers: [S100SlotMask; 8],
}

impl Default for S100IoDecodeIndex {
    fn default() -> Self {
        Self {
            port_responders: [0; S100_IO_PORT_COUNT],
            pint_drivers: 0,
            vi_drivers: [0; 8],
        }
    }
}

impl S100IoDecodeIndex {
    pub fn from_hardware(hardware: S100HardwareConfig) -> Self {
        let mut index = Self::default();

        for (slot, card) in hardware.installed_cards() {
            let slot_mask = s100_slot_mask(slot);
            match card {
                S100InstalledCardConfig::Mits88Sio(config) => {
                    index.add_port(config.address.status(), slot_mask);
                    index.add_port(config.address.data(), slot_mask);
                    index.add_sio_interrupt(config.interrupt_wiring.input, slot_mask);
                    index.add_sio_interrupt(config.interrupt_wiring.output, slot_mask);
                }
                S100InstalledCardConfig::Mits88TwoSio {
                    straps,
                    interrupt_wiring,
                } => {
                    index.add_port(straps.address.port0_status(), slot_mask);
                    index.add_port(straps.address.port0_data(), slot_mask);
                    index.add_port(straps.address.port1_status(), slot_mask);
                    index.add_port(straps.address.port1_data(), slot_mask);
                    index.add_two_sio_interrupt(interrupt_wiring.port0, slot_mask);
                    index.add_two_sio_interrupt(interrupt_wiring.port1, slot_mask);
                }
                S100InstalledCardConfig::Mits88DcddBoard1
                | S100InstalledCardConfig::Mits88DcddBoard2 => {
                    // Board #1 owns the actual A8..A15 address decoder. Board #2
                    // is nevertheless a physical participant in the same OUT
                    // transactions because DCL/CD/WDS reach it over the dedicated
                    // controller harness. Fast selection therefore keeps *both*
                    // fitted boards present for 08h-0Ah; this mask is scheduling
                    // metadata, not a claim that Board #2 independently decodes
                    // an S-100 address.
                    for port in 0x08..=0x0a {
                        index.add_port(port, slot_mask);
                    }
                    // Controller interrupt wiring is intentionally Phase 7. Do
                    // not pre-claim PINT or VI7 merely because later MITS options
                    // exist in the source material.
                }
                S100InstalledCardConfig::Mits8080Cpu
                | S100InstalledCardConfig::Ram(_)
                | S100InstalledCardConfig::FastRamCompatibility(_) => {}
            }
        }

        index
    }

    fn add_port(&mut self, port: u8, slot_mask: S100SlotMask) {
        self.port_responders[port as usize] |= slot_mask;
    }

    fn add_sio_interrupt(&mut self, target: SioInterruptTarget, slot_mask: S100SlotMask) {
        if target.drives_pint() {
            self.pint_drivers |= slot_mask;
        }
        if let Some(level) = target.vector_level() {
            self.vi_drivers[level as usize] |= slot_mask;
        }
    }

    fn add_two_sio_interrupt(&mut self, target: TwoSioInterruptTarget, slot_mask: S100SlotMask) {
        if target.drives_pint() {
            self.pint_drivers |= slot_mask;
        }
        if let Some(level) = target.vector_level() {
            self.vi_drivers[level as usize] |= slot_mask;
        }
    }

    /// Slots whose fixed address decode can select this I/O port.
    ///
    /// Zero means open/unclaimed I/O space. Multiple bits deliberately preserve
    /// a mis-strapped overlap so the electrical backplane can expose contention.
    pub const fn port_responders(&self, port: u8) -> S100SlotMask {
        self.port_responders[port as usize]
    }

    pub const fn pint_possible_drivers(&self) -> S100SlotMask {
        self.pint_drivers
    }

    pub const fn vi_possible_drivers(&self, level: u8) -> S100SlotMask {
        if level < 8 {
            self.vi_drivers[level as usize]
        } else {
            0
        }
    }

    pub fn port_responder_count(&self, port: u8) -> u32 {
        self.port_responders(port).count_ones()
    }

    pub fn unique_port_slot(&self, port: u8) -> Option<usize> {
        let mask = self.port_responders(port);
        (mask.count_ones() == 1).then_some(mask.trailing_zeros() as usize + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        S100InstalledCardConfig, SioAddressPair, SioHardwareConfig, SioInterruptWiring,
        TwoSioAddressBlock, TwoSioInterruptWiring, TwoSioStraps,
    };
    use crate::s100_chassis::S100ChassisConfig;

    fn six_slot_with_cpu() -> S100HardwareConfig {
        let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
        hardware
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        hardware
    }

    #[test]
    fn sio_and_two_sio_port_straps_compile_to_physical_slot_masks() {
        let mut hardware = six_slot_with_cpu();
        let sio = SioHardwareConfig {
            address: SioAddressPair::try_new(0x06).unwrap(),
            ..SioHardwareConfig::default()
        };
        let two_sio = TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        };
        hardware
            .set_slot(3, Some(S100InstalledCardConfig::Mits88Sio(sio)))
            .unwrap();
        hardware
            .set_slot(
                5,
                Some(S100InstalledCardConfig::Mits88TwoSio {
                    straps: two_sio,
                    interrupt_wiring: TwoSioInterruptWiring::default(),
                }),
            )
            .unwrap();
        let index = S100IoDecodeIndex::from_hardware(hardware.validate().unwrap());

        assert_eq!(index.port_responders(0x05), 0);
        assert_eq!(index.port_responders(0x06), s100_slot_mask(3));
        assert_eq!(index.port_responders(0x07), s100_slot_mask(3));
        assert_eq!(index.port_responders(0x08), 0);
        for port in 0x44..=0x47 {
            assert_eq!(index.port_responders(port), s100_slot_mask(5));
            assert_eq!(index.unique_port_slot(port), Some(5));
        }
        assert_eq!(index.port_responders(0x48), 0);
    }

    #[test]
    fn dcdd_pair_compiles_fixed_ports_without_claiming_interrupt_wiring() {
        let mut hardware = six_slot_with_cpu();
        hardware
            .set_slot(3, Some(S100InstalledCardConfig::Mits88DcddBoard1))
            .unwrap();
        hardware
            .set_slot(4, Some(S100InstalledCardConfig::Mits88DcddBoard2))
            .unwrap();
        let index = S100IoDecodeIndex::from_hardware(hardware.validate().unwrap());
        let pair = s100_slot_mask(3) | s100_slot_mask(4);

        assert_eq!(index.port_responders(0x07), 0);
        assert_eq!(index.port_responders(0x08), pair);
        assert_eq!(index.port_responders(0x09), pair);
        assert_eq!(index.port_responders(0x0a), pair);
        assert_eq!(index.port_responders(0x0b), 0);
        assert_eq!(index.pint_possible_drivers(), 0);
        assert_eq!(index.vi_possible_drivers(7), 0);
    }

    #[test]
    fn overlapping_serial_decoders_remain_multiple_physical_responders() {
        let mut hardware = six_slot_with_cpu();
        let sio = SioHardwareConfig {
            address: SioAddressPair::try_new(0x44).unwrap(),
            ..SioHardwareConfig::default()
        };
        let two_sio = TwoSioStraps {
            address: TwoSioAddressBlock::try_new(0x44).unwrap(),
            ..TwoSioStraps::default()
        };
        hardware
            .set_slot(2, Some(S100InstalledCardConfig::Mits88Sio(sio)))
            .unwrap();
        hardware
            .set_slot(
                6,
                Some(S100InstalledCardConfig::Mits88TwoSio {
                    straps: two_sio,
                    interrupt_wiring: TwoSioInterruptWiring::default(),
                }),
            )
            .unwrap();

        let index = S100IoDecodeIndex::from_hardware(hardware.validate().unwrap());
        let overlap = s100_slot_mask(2) | s100_slot_mask(6);
        assert_eq!(index.port_responders(0x44), overlap);
        assert_eq!(index.port_responders(0x45), overlap);
        assert_eq!(index.port_responder_count(0x44), 2);
        assert_eq!(index.unique_port_slot(0x44), None);
        assert_eq!(index.port_responders(0x46), s100_slot_mask(6));
        assert_eq!(index.port_responders(0x47), s100_slot_mask(6));
    }

    #[test]
    fn static_interrupt_routing_tracks_possible_pint_and_vi_drivers_by_slot() {
        let mut hardware = six_slot_with_cpu();
        let sio = SioHardwareConfig {
            interrupt_wiring: SioInterruptWiring {
                input: SioInterruptTarget::Vi3,
                output: SioInterruptTarget::Pint,
            },
            ..SioHardwareConfig::default()
        };
        let two_sio_wiring = TwoSioInterruptWiring {
            port0: TwoSioInterruptTarget::Vi3,
            port1: TwoSioInterruptTarget::Vi7,
        };
        hardware
            .set_slot(2, Some(S100InstalledCardConfig::Mits88Sio(sio)))
            .unwrap();
        hardware
            .set_slot(
                4,
                Some(S100InstalledCardConfig::Mits88TwoSio {
                    straps: TwoSioStraps::default(),
                    interrupt_wiring: two_sio_wiring,
                }),
            )
            .unwrap();

        let index = S100IoDecodeIndex::from_hardware(hardware.validate().unwrap());
        assert_eq!(index.pint_possible_drivers(), s100_slot_mask(2));
        assert_eq!(
            index.vi_possible_drivers(3),
            s100_slot_mask(2) | s100_slot_mask(4)
        );
        assert_eq!(index.vi_possible_drivers(7), s100_slot_mask(4));
        assert_eq!(index.vi_possible_drivers(2), 0);
        assert_eq!(index.vi_possible_drivers(8), 0);
    }
}
