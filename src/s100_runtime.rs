//! Live S-100 chassis fabric shared by Fast and Cycle execution engines.
//!
//! Persisted slot configuration is materialized here into electrical card
//! instances. Card-family branching belongs to this chassis assembler; the
//! backplane resolver itself remains card-agnostic.

use crate::config::{
    RamInit, S100HardwareConfig, S100HardwareConfigError, S100InstalledCardConfig,
    MAX_S100_SLOTS,
};
use crate::cpu8080_cycle::{Cpu8080Inputs, Cpu8080Pins};
use crate::machine::RuntimeSerialCardHandle;
use crate::s100::S100Signal;
use crate::s100_backplane::{
    s100_slot_mask, S100Backplane, S100BackplaneError, S100BusSample, S100CardDrive,
    S100SlotMask,
};
use crate::s100_cpu::{Mits8080CpuBoard, Mits8080CpuBoardHandle};
use crate::s100_io::S100IoDecodeIndex;
use crate::s100_io_card::{
    S100IoCardAdapter, MITS_88_2SIO_IO_CARD, MITS_88_SIO_IO_CARD,
};
use crate::s100_runtime_ram::{RuntimeRamCard, RuntimeRamConfig, RuntimeRamHandle};

pub const S100_OPEN_BUS_VALUE: u8 = 0xff;
const DIGITAL_SETTLE_DELTAS: usize = 3;
const S100_ADDRESS_SPACE: usize = 1 << 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayControlLines {
    pub ready: bool,
    pub run: bool,
    pub hold: bool,
    pub reset: bool,
    pub external_clear: bool,
    pub protect: bool,
    pub unprotect: bool,
}

impl DisplayControlLines {
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
        if self.drivers.len() == 1 {
            Some(self.drivers[0].value)
        } else {
            None
        }
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

#[derive(Clone)]
struct RuntimeSerialSlot {
    slot: usize,
    handle: RuntimeSerialCardHandle,
}

pub struct S100RuntimeFabric {
    hardware: S100HardwareConfig,
    backplane: S100Backplane,
    cpu_slot: usize,
    cpu: Mits8080CpuBoardHandle,
    ram: Vec<RuntimeRamSlot>,
    serial: Vec<RuntimeSerialSlot>,
    /// Serial UART state may currently advance through its host endpoint handle
    /// between bus edges. Refresh these physical slots once when a new edge or
    /// Fast transaction phase begins; subsequent zero-time deltas use the cached
    /// connector drive until an S-100 input wakes the card.
    externally_mutable_slots: S100SlotMask,
    /// Predecoded hardware response for every 16-bit address. A bit means that
    /// the card in that physical connector can decode the address. Real cards
    /// see the address bus in parallel; this table is the software equivalent of
    /// their parallel TTL address decoders, not a CPU-visible dispatch table.
    memory_responders: Box<[S100SlotMask]>,
    /// Slot -> RAM vector index, used only after the responder mask has already
    /// established which physical card(s) can participate.
    ram_by_slot: [Option<usize>; MAX_S100_SLOTS],
    /// Compiled A0..A7 and interrupt-pad decode for the installed serial cards.
    /// Multiple responder bits are deliberately retained for electrical overlap.
    io_decode: S100IoDecodeIndex,
}

impl S100RuntimeFabric {
    pub fn new(
        hardware: S100HardwareConfig,
        init: RamInit,
    ) -> Result<Self, S100RuntimeBuildError> {
        let hardware = hardware
            .validate()
            .map_err(S100RuntimeBuildError::InvalidHardware)?;
        let io_decode = S100IoDecodeIndex::from_hardware(hardware);
        let mut backplane = S100Backplane::new(hardware.fitted_connectors());
        let mut cpu_slot = None;
        let mut cpu_handle = None;
        let mut ram = Vec::new();
        let mut serial = Vec::new();

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
                S100InstalledCardConfig::Mits88Sio(config) => {
                    let (device, handle) = RuntimeSerialCardHandle::new_sio(config);
                    let card = S100IoCardAdapter::new(
                        &MITS_88_SIO_IO_CARD,
                        config.address.status(),
                        2,
                        device,
                    );
                    backplane
                        .insert(slot, Box::new(card))
                        .map_err(S100RuntimeBuildError::Backplane)?;
                    serial.push(RuntimeSerialSlot { slot, handle });
                }
                S100InstalledCardConfig::Mits88TwoSio {
                    straps,
                    interrupt_wiring,
                } => {
                    let (device, handle) =
                        RuntimeSerialCardHandle::new_two_sio(straps, interrupt_wiring);
                    let card = S100IoCardAdapter::new(
                        &MITS_88_2SIO_IO_CARD,
                        straps.address.base(),
                        4,
                        device,
                    );
                    backplane
                        .insert(slot, Box::new(card))
                        .map_err(S100RuntimeBuildError::Backplane)?;
                    serial.push(RuntimeSerialSlot { slot, handle });
                }
            }
        }

        let cpu_slot = cpu_slot.ok_or(S100RuntimeBuildError::MissingCpu)?;
        let cpu = cpu_handle.ok_or(S100RuntimeBuildError::MissingCpu)?;
        let externally_mutable_slots = serial
            .iter()
            .fold(0, |mask, installed| mask | s100_slot_mask(installed.slot));

        let mut memory_responders = vec![0; S100_ADDRESS_SPACE].into_boxed_slice();
        let mut ram_by_slot = [None; MAX_S100_SLOTS];
        for (ram_index, installed) in ram.iter().enumerate() {
            let config = installed.handle.config();
            let start = config.base_address() as usize;
            let end = start + config.populated_bytes();
            let responder = s100_slot_mask(installed.slot);
            for mask in &mut memory_responders[start..end] {
                *mask |= responder;
            }
            ram_by_slot[installed.slot - 1] = Some(ram_index);
        }

        let mut fabric = Self {
            hardware,
            backplane,
            cpu_slot,
            cpu,
            ram,
            serial,
            externally_mutable_slots,
            memory_responders,
            ram_by_slot,
            io_decode,
        };
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

    pub(crate) fn serial_handle_for_slot(&self, slot: usize) -> Option<RuntimeSerialCardHandle> {
        self.serial
            .iter()
            .find(|installed| installed.slot == slot)
            .map(|installed| installed.handle.clone())
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

    fn cpu_slot_mask(&self) -> S100SlotMask {
        s100_slot_mask(self.cpu_slot)
    }

    fn memory_responder_mask(&self, address: u16) -> S100SlotMask {
        self.memory_responders[address as usize]
    }

    fn fast_memory_slot_mask(&self, address: u16) -> S100SlotMask {
        self.cpu_slot_mask() | self.memory_responder_mask(address)
    }

    fn io_responder_mask(&self, port: u8) -> S100SlotMask {
        self.io_decode.port_responders(port)
    }

    fn fast_io_slot_mask(&self, port: u8) -> S100SlotMask {
        self.cpu_slot_mask() | self.io_responder_mask(port)
    }

    #[inline]
    fn io_bus_address(port: u8) -> u16 {
        u16::from(port) * 0x0101
    }

    fn ram_for_slot(&self, slot: usize) -> Option<&RuntimeRamSlot> {
        let index = *self.ram_by_slot.get(slot.checked_sub(1)?)?;
        index.map(|index| &self.ram[index])
    }

    fn unique_ram_for_address(&self, address: u16) -> Option<&RuntimeRamSlot> {
        let mask = self.memory_responder_mask(address);
        if mask.count_ones() != 1 {
            return None;
        }
        self.ram_for_slot(mask.trailing_zeros() as usize + 1)
    }

    /// Event-driven zero-time propagation. The connector still settles through
    /// the same causal deltas as the physical cards, but Rust does not execute a
    /// card merely because some unrelated S-100 net changed. Each slot's input
    /// sensitivity comes directly from its historical connector descriptor and
    /// its output drive remains cached until one of those inputs wakes it.
    ///
    /// The CPU slot is forced to observe the first delta because Intel package
    /// pins are on the non-S-100 side of that board. Serial slots are refreshed
    /// once at the start because their UART state can currently advance through
    /// host endpoint handles between edges. Neither exception bypasses the bus:
    /// their resulting connector drives still resolve electrically here.
    pub fn settle(
        &mut self,
        display: DisplayControlLines,
        extra_drives: &[S100CardDrive],
    ) -> Result<&S100BusSample, S100BackplaneError> {
        let selected = S100SlotMask::MAX;
        let cpu_slot = self.cpu_slot_mask();
        self.backplane
            .refresh_cached_drives(cpu_slot | self.externally_mutable_slots)?;

        let mut display_drive = display.drive(self.backplane.sample());
        let mut forced_observe = cpu_slot;

        for _ in 0..DIGITAL_SETTLE_DELTAS {
            let change = if extra_drives.is_empty() {
                self.backplane.resolve_cached_selected_drives(
                    selected,
                    std::slice::from_ref(&display_drive),
                )
            } else {
                let mut chassis = Vec::with_capacity(extra_drives.len() + 1);
                chassis.push(display_drive);
                chassis.extend(extra_drives.iter().copied());
                self.backplane
                    .resolve_cached_selected_drives(selected, &chassis)
            };

            let changed_drives = self.backplane.observe_changed_cards(
                change,
                forced_observe,
                selected,
            )?;
            forced_observe = 0;

            let next_display_drive = display.drive(self.backplane.sample());
            let display_changed = next_display_drive != display_drive;
            display_drive = next_display_drive;

            if changed_drives == 0 && !display_changed {
                break;
            }
        }
        Ok(self.backplane.sample())
    }

    fn fast_display() -> DisplayControlLines {
        DisplayControlLines {
            ready: true,
            run: true,
            ..DisplayControlLines::default()
        }
    }

    /// Resolve one causal propagation delta for a predecoded Fast transaction.
    /// Fast still exercises the installed CPU/RAM/I/O cards and the same
    /// electrical resolver. `package_changed` only tells the compiled fabric that
    /// the CPU board's non-S-100 package side needs one forced observation.
    fn fast_delta(
        &mut self,
        selected: S100SlotMask,
        display: DisplayControlLines,
        package_changed: bool,
    ) -> Result<(), S100BackplaneError> {
        let cpu_slot = self.cpu_slot_mask();
        let externally_dirty = if package_changed {
            cpu_slot | (selected & self.externally_mutable_slots)
        } else {
            0
        };
        if externally_dirty != 0 {
            self.backplane.refresh_cached_drives(externally_dirty)?;
        }

        let display_drive = display.drive(self.backplane.sample());
        let change = self
            .backplane
            .resolve_cached_selected_drives(selected, &[display_drive]);
        let forced = if package_changed { cpu_slot } else { 0 };
        let _ = self
            .backplane
            .observe_changed_cards(change, forced, selected)?;
        Ok(())
    }

    /// Update wire levels without replaying a card state transition. The CPU
    /// package has changed, so refresh that card's cached drive once, then resolve
    /// the physical connector state with the selected responders still present.
    fn fast_resolve_only(
        &mut self,
        selected: S100SlotMask,
        display: DisplayControlLines,
    ) -> Result<(), S100BackplaneError> {
        let refresh = self.cpu_slot_mask() | (selected & self.externally_mutable_slots);
        self.backplane.refresh_cached_drives(refresh)?;
        let display_drive = display.drive(self.backplane.sample());
        let _ = self
            .backplane
            .resolve_cached_selected_drives(selected, &[display_drive]);
        Ok(())
    }

    /// Fast reconstructs one memory-read machine cycle. The CPU does not poll
    /// cards: the chassis has already compiled the parallel address decoders into
    /// `memory_responders`, and only electrically possible responders participate.
    /// Three deltas model the causal chain:
    ///   1. SYNC+PHI1 reaches the CPU board and latches status;
    ///   2. latched sMEMR reaches the selected RAM decoder while SYNC is held;
    ///   3. DBIN/PHI2 and the RAM's DI/PRDY outputs resolve back to the CPU board.
    pub fn fast_memory_read(
        &mut self,
        address: u16,
        status_word: u8,
    ) -> Result<u8, S100BackplaneError> {
        let selected = self.fast_memory_slot_mask(address);
        let display = Self::fast_display();

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(address),
            data_out: Some(status_word),
            sync: true,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;
        self.fast_delta(selected, display, false)?;

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: false,
            phi2: true,
            address: Some(address),
            data_out: None,
            sync: false,
            dbin: true,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;
        Ok(self.cpu_package_inputs().data_in)
    }

    /// Fast write follows the same physical ownership as Cycle: CPU drives pWR
    /// and DO, Display/Control derives MWRT, and the predecoded RAM responder(s)
    /// see that bus line. The final resolve releases pWR without replaying a RAM
    /// write edge/state update.
    pub fn fast_memory_write(
        &mut self,
        address: u16,
        value: u8,
        status_word: u8,
    ) -> Result<(), S100BackplaneError> {
        let selected = self.fast_memory_slot_mask(address);
        let display = Self::fast_display();

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(address),
            data_out: Some(status_word),
            sync: true,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(address),
            data_out: Some(value),
            sync: false,
            dbin: false,
            wr_n: false,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;
        self.fast_delta(selected, display, false)?;

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: false,
            phi2: true,
            address: Some(address),
            data_out: Some(value),
            sync: false,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_resolve_only(selected, display)?;
        Ok(())
    }

    /// Fast IN reconstructs the real CPU-board status latch and DBIN strobe. The
    /// register adapter performs its side effect only when sINP and DBIN overlap;
    /// one additional propagation delta then returns the selected card(s)' DI to
    /// the 8080 package input. Overlapping cards remain simultaneous responders.
    pub fn fast_io_read(&mut self, port: u8) -> Result<u8, S100BackplaneError> {
        let selected = self.fast_io_slot_mask(port);
        let display = Self::fast_display();
        let address = Self::io_bus_address(port);

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(address),
            data_out: Some(0x42),
            sync: true,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;
        self.fast_delta(selected, display, false)?;

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: false,
            phi2: true,
            address: Some(address),
            data_out: None,
            sync: false,
            dbin: true,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;
        self.fast_delta(selected, display, false)?;
        Ok(self.cpu_package_inputs().data_in)
    }

    /// Fast OUT is the electrical counterpart of `fast_io_read`: status 10h is
    /// latched by the CPU board, then DO + active-low pWR reach every decoder in
    /// the compiled responder mask. The adapter guarantees one register write
    /// even though a second digital delta is required for causal propagation.
    pub fn fast_io_write(&mut self, port: u8, value: u8) -> Result<(), S100BackplaneError> {
        let selected = self.fast_io_slot_mask(port);
        let display = Self::fast_display();
        let address = Self::io_bus_address(port);

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(address),
            data_out: Some(0x10),
            sync: true,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: true,
            phi2: false,
            address: Some(address),
            data_out: Some(value),
            sync: false,
            dbin: false,
            wr_n: false,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_delta(selected, display, true)?;
        self.fast_delta(selected, display, false)?;

        self.set_cpu_package_pins(Cpu8080Pins {
            phi1: false,
            phi2: true,
            address: Some(address),
            data_out: Some(value),
            sync: false,
            dbin: false,
            wr_n: true,
            inte: false,
            wait: false,
            hlda: false,
        });
        self.fast_resolve_only(selected, display)?;
        Ok(())
    }

    pub fn fast_read_wait_states(&self, address: u16) -> u8 {
        let mut responders = self.memory_responder_mask(address);
        let mut waits = 0u8;
        while responders != 0 {
            let slot = responders.trailing_zeros() as usize + 1;
            responders &= responders - 1;
            if let Some(ram) = self.ram_for_slot(slot) {
                waits = waits.max(ram.handle.config().read_wait_states());
            }
        }
        waits
    }

    pub fn inspect_memory(&self, address: u16) -> RuntimeMemoryInspection {
        let responder_mask = self.memory_responder_mask(address);
        let mut responders = responder_mask;
        let mut drivers = Vec::with_capacity(responder_mask.count_ones() as usize);
        while responders != 0 {
            let slot = responders.trailing_zeros() as usize + 1;
            responders &= responders - 1;
            let Some(ram) = self.ram_for_slot(slot) else {
                continue;
            };
            let Some(value) = ram.handle.read_byte(address) else {
                continue;
            };
            drivers.push(RuntimeRamDriver {
                slot,
                value,
                protected: ram.handle.is_protected(address),
                config: ram.handle.config(),
            });
        }
        RuntimeMemoryInspection { drivers }
    }

    pub fn peek_unique_memory(&self, address: u16) -> Option<u8> {
        self.unique_ram_for_address(address)
            .and_then(|ram| ram.handle.read_byte(address))
    }

    pub fn mapped_ram_card_count(&self, address: u16) -> usize {
        self.memory_responder_mask(address).count_ones() as usize
    }

    pub fn installed_ram_bytes(&self) -> usize {
        self.ram
            .iter()
            .map(|ram| ram.handle.config().populated_bytes())
            .sum()
    }

    pub fn write_unique_memory(
        &self,
        address: u16,
        value: u8,
        respect_protection: bool,
    ) -> bool {
        self.unique_ram_for_address(address)
            .map(|ram| {
                ram.handle
                    .write_byte(address, value, respect_protection)
            })
            .unwrap_or(false)
    }

    pub fn memory_is_protected(&self, address: u16) -> bool {
        self.unique_ram_for_address(address)
            .map(|ram| ram.handle.is_protected(address))
            .unwrap_or(false)
    }

    pub fn set_unique_memory_protection(&self, address: u16, protected: bool) -> bool {
        self.unique_ram_for_address(address)
            .map(|ram| ram.handle.set_protected(address, protected))
            .unwrap_or(false)
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
    use crate::config::{S100InstalledCardConfig, SioAddressPair, SioHardwareConfig};
    use crate::s100_chassis::S100ChassisConfig;
    use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};

    fn simple_hardware() -> S100HardwareConfig {
        let mut config =
            S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
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

    fn serial_hardware() -> S100HardwareConfig {
        let mut config = simple_hardware();
        config
            .set_slot(
                3,
                Some(S100InstalledCardConfig::Mits88Sio(SioHardwareConfig::default())),
            )
            .unwrap();
        config
            .set_slot(
                4,
                Some(S100InstalledCardConfig::Mits88TwoSio {
                    straps: crate::config::TwoSioStraps::default(),
                    interrupt_wiring: crate::config::TwoSioInterruptWiring::default(),
                }),
            )
            .unwrap();
        config
    }

    #[test]
    fn configured_cpu_and_ram_are_live_slots_on_one_backplane() {
        let fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
        assert_eq!(fabric.cpu_slot(), 1);
        assert_eq!(
            fabric.backplane().slots()[0].descriptor().unwrap().key,
            "mits-8080-cpu"
        );
        assert_eq!(
            fabric.backplane().slots()[1].descriptor().unwrap().key,
            "mits-88-4mcs"
        );
        assert_eq!(fabric.installed_ram_bytes(), 4 * 1024);
    }

    #[test]
    fn configured_serial_cards_are_live_slots_on_the_same_backplane() {
        let fabric = S100RuntimeFabric::new(serial_hardware(), RamInit::Zeroed).unwrap();
        assert_eq!(
            fabric.backplane().slots()[2].descriptor().unwrap().key,
            "mits-88-sio-live-io"
        );
        assert_eq!(
            fabric.backplane().slots()[3].descriptor().unwrap().key,
            "mits-88-2sio-live-io"
        );
        assert_eq!(fabric.io_responder_mask(0x00), s100_slot_mask(3));
        assert_eq!(fabric.io_responder_mask(0x01), s100_slot_mask(3));
        for port in 0x10..=0x13 {
            assert_eq!(fabric.io_responder_mask(port), s100_slot_mask(4));
        }
    }

    #[test]
    fn fast_read_and_write_cross_cpu_board_backplane_and_ram_card() {
        let mut fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
        fabric.fast_memory_write(0x0123, 0x5a, 0x00).unwrap();
        assert_eq!(fabric.peek_unique_memory(0x0123), Some(0x5a));
        assert_eq!(fabric.fast_memory_read(0x0123, 0x82).unwrap(), 0x5a);
        assert_eq!(fabric.cpu_latched_status_word(), 0x82);
    }

    #[test]
    fn fast_sio_input_consumes_the_existing_com2502_through_the_backplane() {
        let mut fabric = S100RuntimeFabric::new(serial_hardware(), RamInit::Zeroed).unwrap();
        let handle = fabric.serial_handle_for_slot(3).unwrap();
        assert_eq!(handle.board(), crate::config::SerialBoard::Sio88);
        assert_eq!(handle.base(), 0x00);
        assert!(handle.receive(0, b'R'));
        handle.advance_t_states(200_000);
        assert!(!handle.rx_empty(0));
        assert_eq!(fabric.fast_io_read(0x01).unwrap(), b'R');
        assert!(handle.rx_empty(0));
        assert_eq!(fabric.cpu_latched_status_word(), 0x42);
    }

    #[test]
    fn fast_two_sio_output_crosses_backplane_to_the_existing_mc6850() {
        let mut fabric = S100RuntimeFabric::new(serial_hardware(), RamInit::Zeroed).unwrap();
        let handle = fabric.serial_handle_for_slot(4).unwrap();
        assert_eq!(handle.board(), crate::config::SerialBoard::TwoSio88);
        assert_eq!(handle.base(), 0x10);
        fabric.fast_io_write(0x10, 0x15).unwrap();
        fabric.fast_io_write(0x11, b'T').unwrap();
        handle.advance_t_states(200_000);
        assert_eq!(handle.tx_front(0), Some(b'T'));
        assert_eq!(fabric.cpu_latched_status_word(), 0x10);
    }

    #[test]
    fn overlapping_serial_decoders_contend_on_live_di_instead_of_first_match_wins() {
        let mut config = simple_hardware();
        let sio = SioHardwareConfig {
            address: SioAddressPair::try_new(0x10).unwrap(),
            ..SioHardwareConfig::default()
        };
        config
            .set_slot(3, Some(S100InstalledCardConfig::Mits88Sio(sio)))
            .unwrap();
        config
            .set_slot(
                4,
                Some(S100InstalledCardConfig::Mits88TwoSio {
                    straps: crate::config::TwoSioStraps::default(),
                    interrupt_wiring: crate::config::TwoSioInterruptWiring::default(),
                }),
            )
            .unwrap();
        let mut fabric = S100RuntimeFabric::new(config, RamInit::Zeroed).unwrap();
        assert_eq!(
            fabric.io_responder_mask(0x10),
            s100_slot_mask(3) | s100_slot_mask(4)
        );
        assert_eq!(fabric.fast_io_read(0x10).unwrap(), S100_OPEN_BUS_VALUE);
        assert!(
            (0..8).any(|bit| fabric.sample().signal_is_contended(S100Signal::DataIn(bit))),
            "different status bytes must appear as real DI contention"
        );
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
        assert_eq!(fabric.mapped_ram_card_count(0x0010), 1);
        assert_eq!(fabric.mapped_ram_card_count(0x0800), 2);
        assert_eq!(fabric.mapped_ram_card_count(0x0c00), 1);
        assert_eq!(fabric.mapped_ram_card_count(0x1800), 0);
        assert_eq!(fabric.mapped_ram_card_count(0x3000), 0);
        assert_eq!(fabric.peek_unique_memory(0x3000), None);
    }

    #[test]
    fn compiled_memory_responders_match_physical_address_decoders() {
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
        assert_eq!(fabric.memory_responder_mask(0x0010), s100_slot_mask(2));
        assert_eq!(
            fabric.memory_responder_mask(0x0800),
            s100_slot_mask(2) | s100_slot_mask(3)
        );
        assert_eq!(fabric.memory_responder_mask(0x1800), 0);
    }

    #[test]
    fn fast_unmapped_read_keeps_only_cpu_in_transaction_and_returns_open_bus() {
        let mut fabric = S100RuntimeFabric::new(simple_hardware(), RamInit::Zeroed).unwrap();
        assert_eq!(fabric.fast_memory_slot_mask(0x3000), s100_slot_mask(1));
        assert_eq!(fabric.fast_memory_read(0x3000, 0x82).unwrap(), S100_OPEN_BUS_VALUE);
    }
}