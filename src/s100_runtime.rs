//! Live S-100 chassis fabric shared by Fast and Cycle execution engines.
//!
//! This is the materialization boundary between persisted slot configuration and
//! electrical card instances.  Card-family branching is allowed here because
//! this is the chassis assembler; the backplane resolver itself remains wholly
//! generic.  CPU and RAM guest traffic uses only the resolved S-100 bus.

use crate::config::{
    RamInit, S100HardwareConfig, S100HardwareConfigError, S100InstalledCardConfig,
};
use crate::cpu8080_cycle::{Cpu8080Inputs, Cpu8080Pins};
use crate::s100::S100Signal;
use crate::s100_backplane::{
    S100Backplane, S100BackplaneError, S100BusSample, S100CardDrive,
};
use crate::s100_cpu::{Mits8080CpuBoard, Mits8080CpuBoardHandle};
use crate::s100_runtime_ram::{RuntimeRamCard, RuntimeRamConfig, RuntimeRamHandle};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayControlLines {
    /// Display/Control contribution to pRDY. False means the panel pulls pRDY low.
    pub ready: bool,
    pub run: bool,
    pub hold: bool,
    pub reset: bool,
    pub external_clear: bool,
    pub protect: bool,
    pub unprotect: bool,
}

impl DisplayControlLines {
    /// Drive the system-bus lines produced by the original Display/Control board.
    /// MWRT is generated here because the Altair front panel combines asserted
    /// pWR* with !sOUT; it is not an Intel 8080 package output.
    pub fn drive(self, sample: &S100BusSample) -> S100CardDrive {
        let mut drive = S100CardDrive::new();
        drive.pull_low(S100Signal::Ready, !self.ready);
        drive.drive_signal(S100Signal::Run, self.run);
        drive.drive_signal(S100Signal::Hold, self.hold);
        drive.drive_signal(S100Signal::Reset, self.reset);
        drive.drive_signal(S100Signal::ExternalClear, self.external_clear);
        drive.drive_signal(S100Signal::Protect, self.protect);
        drive.drive_signal(S100Signal::Unprotect, self.unprotect);

        let pwr_asserted = sample.signal_level(S100Signal::Write) == Some(false);
        let sout = sample.signal_level(S100Signal::Out) == Some(true);
        drive.drive_signal(S100Signal::MemoryWrite, pwr_asserted && !sout);
        drive
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum S100RuntimeBuildError {
    InvalidHardware(S100HardwareConfigError),
    Backplane(S100BackplaneError),
    InvalidHistoricalRam(crate::s100_memory::S100RamConfigError),
    InvalidCompatibilityRam(S100HardwareConfigError),
    MissingCpu,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRamDriver {
    pub slot: usize,
    pub value: u8,
    pub protected: bool,
    pub config: RuntimeRamConfig,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimeMemoryInspection {
    pub drivers: Vec<RuntimeRamDriver>,
}

impl RuntimeMemoryInspection {
    pub fn is_unmapped(&self) -> bool {
        self.drivers.is_empty()
    }

    pub fn is_overlap(&self) -> bool {
        self.drivers.len() > 1
    }

    pub fn unique_value(&self) -> Option<u8> {
        (self.drivers.len() == 1).then_some(self.drivers[0].value)
    }

    pub fn electrically_contended(&self) -> bool {
        let Some(first) = self.drivers.first().map(|driver| driver.value) else {
            return false;
        };
        self.drivers.iter().any(|driver| driver.value != first)
    }
}

#[derive(Clone)]
struct RuntimeRamSlot {
    slot: usize,
    handle: RuntimeRamHandle,
}

pub struct S100RuntimeFabric {
    hardware: S100HardwareConfig,
    backplane: S100Backplane,
    cpu_slot: usize,
    cpu: Mits8080CpuBoardHandle,
    ram: Vec<RuntimeRamSlot>,
    /// Serial cards are deliberately visible as unfinished runtime migration.
    /// Their existing UART implementation remains active outside this fabric
    /// until 88-2SIO and then 88-SIO become electrical S-100 cards.
    pending_serial_slots: Vec<usize>,
}

impl S100RuntimeFabric {
    pub fn new(
        hardware: S100HardwareConfig,
        init: RamInit,
    ) -> Result<Self, S100RuntimeBuildError> {
        let hardware = hardware
            .validate()
            .map_err(S100RuntimeBuildError::InvalidHardware)?;
        let mut backplane = S100Backplane::new(hardware.fitted_connectors());
        let mut cpu_slot = None;
        let mut cpu_handle = None;
        let mut ram = Vec::new();
        let mut pending_serial_slots = Vec::new();

        for (slot, config) in hardware.installed_cards() {
            match config {
                S100InstalledCardConfig::Mits8080Cpu => {
                    let (card, handle) = Mits8080CpuBoard::new();
                    backplane
                        .insert(slot, Box::new(card))
                        .map_err(S100RuntimeBuildError::Backplane)?;
                    cpu_slot = Some(slot);
                    cpu_handle = Some(handle);
                }
                S100InstalledCardConfig::Ram(config) => {
                    let (card, handle) = RuntimeRamCard::historical(config, init)
                        .map_err(S100RuntimeBuildError::InvalidHistoricalRam)?;
                    backplane
                        .insert(slot, Box::new(card))
                        .map_err(S100RuntimeBuildError::Backplane)?;
                    ram.push(RuntimeRamSlot { slot, handle });
                }
                S100InstalledCardConfig::FastRamCompatibility(config) => {
                    let (card, handle) = RuntimeRamCard::compatibility(config, init)
                        .map_err(S100RuntimeBuildError::InvalidCompatibilityRam)?;
                    backplane
                        .insert(slot, Box::new(card))
                        .map_err(S100RuntimeBuildError::Backplane)?;
                    ram.push(RuntimeRamSlot { slot, handle });
                }
                S100InstalledCardConfig::Mits88Sio(_)
                | S100InstalledCardConfig::Mits88TwoSio { .. } => {
                    pending_serial_slots.push(slot);
                }
            }
        }

        let cpu_slot = cpu_slot.ok_or(S100RuntimeBuildError::MissingCpu)?;
        let cpu = cpu_handle.ok_or(S100RuntimeBuildError::MissingCpu)?;
        let mut fabric = Self {
            hardware,
            backplane,
            cpu_slot,
            cpu,
            ram,
            pending_serial_slots,
        };
        // Establish passive-high ready/interrupt state and let the CPU card see
        // the initial resolved bus before either engine begins execution.
        fabric
            .settle(DisplayControlLines::default(), &[])
            .map_err(S100RuntimeBuildError::Backplane)?;
        Ok(fabric)
    }

    pub fn hardware(&self) -> S100HardwareConfig {
        self.hardware
    }

    pub fn backplane(&self) -> &S100Backplane {
        &self.backplane
    }

    pub fn sample(&self) -> &S100BusSample {
        self.backplane.sample()
    }

    pub fn cpu_slot(&self) -> usize {
        self.cpu_slot
    }

    pub fn pending_serial_slots(&self) -> &[usize] {
        &self.pending_serial_slots
    }

    pub fn set_cpu_package_pins(&self, pins: Cpu8080Pins) {
        self.cpu.set_package_pins(pins);
    }

    pub fn cpu_package_inputs(&self) -> Cpu8080Inputs {
        self.cpu.package_inputs()
    }

    pub fn cpu_latched_status_word(&self) -> u8 {
        self.cpu.latched_status_word()
    }

    /// Propagate one stable package-pin/chassis level through a few zero-time
    /// digital deltas. Cards edge-detect their own clock inputs, so repeated
    /// observation at the same level settles combinational dependencies without
    /// inventing extra PHI1/PHI2 edges.
    pub fn settle(
        &mut self,
        display: DisplayControlLines,
        extra_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        let mut display_drive = display.drive(self.backplane.sample());
        for _ in 0..6 {
            let mut chassis = Vec::with_capacity(extra_drives.len() + 1);
            chassis.push(display_drive);
            chassis.extend(extra_drives.iter().cloned());
            self.backplane.resolve_current_drives(&chassis)?;
            self.backplane.observe_cards();
            display_drive = display.drive(self.backplane.sample());
        }
        let mut chassis = Vec::with_capacity(extra_drives.len() + 1);
        chassis.push(display_drive);
        chassis.extend(extra_drives.iter().cloned());
        self.backplane.resolve_current_drives(&chassis)?;
        self.backplane.observe_cards();
        Ok(self.backplane.sample())
    }

    pub fn inspect_memory(&self, address: u16) -> RuntimeMemoryInspection {
        let drivers = self
            .ram
            .iter()
            .filter_map(|ram| {
                ram.handle.read_byte(address).map(|value| RuntimeRamDriver {
                    slot: ram.slot,
                    value,
                    protected: ram.handle.is_protected(),
                    config: ram.handle.config(),
                })
            })
            .collect();
        RuntimeMemoryInspection { drivers }
    }

    pub fn peek_unique_memory(&self, address: u16) -> Option<u8> {
        self.inspect_memory(address).unique_value()
    }

    pub fn mapped_ram_card_count(&self, address: u16) -> usize {
        self.ram
            .iter()
            .filter(|ram| ram.handle.contains(address))
            .count()
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.ram
            .iter()
            .map(|ram| ram.handle.config().populated_bytes())
            .sum()
    }

    /// Host debugger write. Ambiguous overlapping mappings are deliberately
    /// rejected instead of silently choosing a card by slot order.
    pub fn write_unique_memory(
        &self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> bool {
        let mut mapped = self.ram.iter().filter(|ram| ram.handle.contains(address));
        let Some(target) = mapped.next() else {
            return false;
        };
        if mapped.next().is_some() {
            return false;
        }
        target
            .handle
            .write_byte(address, value, respect_protection)
    }

    pub fn memory_is_protected(&self, address: u16) -> bool {
        let mut mapped = self.ram.iter().filter(|ram| ram.handle.contains(address));
        let Some(target) = mapped.next() else {
            return false;
        };
        mapped.next().is_none() && target.handle.is_protected()
    }

    pub fn set_unique_memory_protection(&self, address: u16, protected: bool) -> bool {
        let mut mapped = self.ram.iter().filter(|ram| ram.handle.contains(address));
        let Some(target) = mapped.next() else {
            return false;
        };
        if mapped.next().is_some() {
            return false;
        }
        target.handle.set_protected(protected)
    }

    pub fn clear_memory_protection(&self) {
        for ram in &self.ram {
            ram.handle.clear_protection();
        }
    }

    pub fn initialize_memory(&self, init: RamInit) {
        for ram in &self.ram {
            ram.handle.initialize(init);
        }
    }

    pub fn load_bytes(&self, address: u16, bytes: &[u8]) -> usize {
        let mut written = 0usize;
        for (offset, value) in bytes.iter().copied().enumerate() {
            let candidate = address.wrapping_add(offset as u16);
            if self.write_unique_memory(candidate, value, false) {
                written += 1;
            }
        }
        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::S100InstalledCardConfig;
    use crate::s100_chassis::S100ChassisConfig;
    use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};

    fn simple_hardware() -> S100HardwareConfig {
        let mut config = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
        config
            .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
            .unwrap();
        config
            .set_slot(
                2,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits4KStatic88_4Mcs,
                        0,
                    ),
                )),
            )
            .unwrap();
        config
    }

    #[test]
    fn configured_cpu_and_ram_are_live_slots_on_one_backplane() {
        let fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
        assert_eq!(fabric.cpu_slot(), 1);
        assert_eq!(fabric.backplane().slots()[0].descriptor().unwrap().key, "mits-8080-cpu");
        assert_eq!(fabric.backplane().slots()[1].descriptor().unwrap().key, "mits-88-4mcs");
        assert_eq!(fabric.installed_ram_bytes(), 4 * 1024);
    }

    #[test]
    fn debugger_handle_sees_same_bytes_that_ram_card_will_drive() {
        let mut fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
        assert!(fabric.write_unique_memory(0x0123, 0x5a, false));

        // Synthetic Fast-style memory-read machine cycle: status A2h is the
        // 8080 fetch status byte (MEMR+M1+/WO). The CPU board latches it at
        // SYNC+PHI1; subsequent deltas let RAM decode sMEMR and drive DI.
        fabric.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(0x0123),
            data_out: Some(0xa2),
            sync: true,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        fabric
            .settle(
                DisplayControlLines {
                    ready: true,
                    run: true,
                    ..DisplayControlLines::default()
                },
                &[],
            )
            .unwrap();
        assert_eq!(fabric.sample().data_in(), Some(0x5a));
        assert_eq!(fabric.cpu_package_inputs().data_in, 0x5a);
    }

    #[test]
    fn display_control_generates_mwrt_from_pwr_and_not_sout() {
        let fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
        let mut cpu = S100CardDrive::new();
        cpu.drive_signal(S100Signal::Write, false);
        cpu.drive_signal(S100Signal::Out, false);
        let sample = fabric.backplane().resolve_drive_sets(&[cpu]);
        let drive = DisplayControlLines::default().drive(&sample);
        let resolved = fabric.backplane().resolve_drive_sets(&[drive]);
        assert_eq!(resolved.signal_level(S100Signal::MemoryWrite), Some(true));
    }

    #[test]
    fn gaps_and_overlaps_are_not_collapsed_to_aggregate_capacity() {
        let mut config = simple_hardware();
        config
            .set_slot(
                3,
                Some(S100InstalledCardConfig::Ram(
                    S100RamCardConfig::fully_populated(
                        S100RamBoardModel::Mits1KStatic88Mcs,
                        0x0800,
                    ),
                )),
            )
            .unwrap();
        let fabric = S100RuntimeFabric::new(config, RamInit::Zeroed).unwrap();
        assert_eq!(fabric.mapped_ram_card_count(0x0100), 1);
        assert_eq!(fabric.mapped_ram_card_count(0x0900), 2);
        assert_eq!(fabric.mapped_ram_card_count(0x1800), 0);
        assert!(fabric.inspect_memory(0x0900).is_overlap());
        assert!(!fabric.write_unique_memory(0x0900, 0xaa, false));
    }
}
