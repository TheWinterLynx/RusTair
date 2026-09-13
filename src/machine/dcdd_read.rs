//! MITS 88-DCDD Board #1 read-data latch and NRDA electronics.
//!
//! The FD-400 supplies time-stamped physical byte events. This state belongs to
//! controller Board #1: a new byte overwrites the previous latch whether or not
//! the 8080 consumed it, NRDA stays asserted until IN 0Ah, and reading data does
//! not stop the physical byte stream.

#[derive(Debug)]
pub(super) struct Board1ReadElectronics {
    data_latch: u8,
    generation: Option<u64>,
    nrda: bool,
}

impl Board1ReadElectronics {
    pub(super) const fn new(power_up_latch: u8) -> Self {
        Self {
            data_latch: power_up_latch,
            generation: None,
            nrda: false,
        }
    }

    /// Catch up to the newest physical byte event. Re-observing the same event
    /// must not reassert NRDA after the CPU has already consumed that byte.
    pub(super) fn observe_event(&mut self, event: Option<(u64, u8)>) {
        let Some((generation, byte)) = event else {
            return;
        };
        if self.generation == Some(generation) {
            return;
        }
        self.generation = Some(generation);
        self.data_latch = byte;
        self.nrda = true;
    }

    pub(super) const fn nrda(&self) -> bool {
        self.nrda
    }

    #[cfg(test)]
    pub(super) const fn data_latch(&self) -> u8 {
        self.data_latch
    }

    /// IN 0Ah returns the retained latch and resets NRDA. A later physical byte
    /// event can immediately overwrite the latch and assert NRDA again.
    pub(super) fn consume_data(&mut self) -> u8 {
        self.nrda = false;
        self.data_latch
    }
}

#[cfg(test)]
mod tests {
    use super::super::Mits88DcddHarness;
    use super::*;
    use crate::config::RamInit;
    use crate::cpu8080_cycle::Cpu8080Cycle;
    use crate::machine::fd400::{Fd400Time, MitsDiskUnit, test_readable_media};
    use crate::s100_backplane::{S100Backplane, S100SlotMask};
    use crate::s100_cpu::{Mits8080CpuBoard, Mits8080CpuBoardHandle};
    use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};
    use crate::s100_runtime::DisplayControlLines;
    use crate::s100_runtime_ram::{RuntimeRamCard, RuntimeRamHandle};

    const FIVE_SECONDS_T_STATES: u64 = 10_000_000;

    fn settle_test_bus(backplane: &mut S100Backplane, display: DisplayControlLines) {
        // This is zero-time digital propagation, not guest clock advancement.
        // Use the same event-driven cache/resolve/observe sequence as production
        // so cards that correctly report external_drive_dirty() == false still
        // publish connector changes caused by the S-100 inputs they just saw.
        let selected = S100SlotMask::MAX;
        for _ in 0..6 {
            backplane.refresh_cached_drives(selected).unwrap();
            let display_drive = display.drive(backplane.sample());
            let change = backplane.resolve_cached_selected_drives(selected, &[display_drive]);
            backplane
                .observe_changed_cards(change, 0, selected)
                .unwrap();
        }
    }

    fn cpu_dcdd_fixture() -> (
        S100Backplane,
        Mits8080CpuBoardHandle,
        RuntimeRamHandle,
        Mits88DcddHarness,
    ) {
        let harness = Mits88DcddHarness::new();
        let (cpu_card, cpu_board) = Mits8080CpuBoard::new();
        let ram_config =
            S100RamCardConfig::fully_populated(S100RamBoardModel::Mits4KStatic88_4Mcs, 0);
        let (ram_card, ram) = RuntimeRamCard::historical(ram_config, RamInit::Zeroed).unwrap();

        let mut backplane = S100Backplane::new(6);
        backplane.insert(1, Box::new(cpu_card)).unwrap();
        backplane.insert(2, Box::new(ram_card)).unwrap();
        backplane.insert(3, harness.board1_card()).unwrap();
        backplane.insert(4, harness.board2_card()).unwrap();

        let mut unit = MitsDiskUnit::new(3).unwrap();
        unit.insert_media(test_readable_media()).unwrap();
        unit.drive_mut().set_power(true, Fd400Time::ZERO);
        unit.drive_mut().set_door_open(false, Fd400Time::ZERO);
        unit.drive_mut().set_motor_on(true, Fd400Time::ZERO);
        harness.install_test_unit(unit);

        (backplane, cpu_board, ram, harness)
    }

    #[test]
    fn unread_bytes_are_overwritten_by_the_newest_physical_event() {
        let mut read = Board1ReadElectronics::new(0x5a);
        assert!(!read.nrda());
        assert_eq!(read.data_latch(), 0x5a);

        read.observe_event(Some((10, 0x81)));
        read.observe_event(Some((11, 0x22)));
        read.observe_event(Some((12, 0x33)));
        assert!(read.nrda());
        assert_eq!(read.data_latch(), 0x33);
    }

    #[test]
    fn data_in_clears_nrda_without_rearming_on_the_same_generation() {
        let mut read = Board1ReadElectronics::new(0x00);
        read.observe_event(Some((42, 0xa5)));
        assert!(read.nrda());
        assert_eq!(read.consume_data(), 0xa5);
        assert!(!read.nrda());

        read.observe_event(Some((42, 0xa5)));
        assert!(!read.nrda(), "the same physical byte cannot arrive twice");

        read.observe_event(Some((43, 0x19)));
        assert!(read.nrda());
        assert_eq!(read.data_latch(), 0x19);
    }

    #[test]
    fn real_8080_reads_mounted_fd400_media_only_through_s100_dcdd_ports() {
        let (mut backplane, cpu_board, ram, harness) = cpu_dcdd_fixture();
        let display = DisplayControlLines {
            ready: true,
            run: true,
            ..DisplayControlLines::default()
        };

        // Let the real Disk Buffer stabilization timer expire before the CPU is
        // started. The platter and buffer are independent hardware and may have
        // been powered for seconds before the operator presses RUN.
        harness.advance_mechanics_t_states(FIVE_SECONDS_T_STATES);

        // 0000  MVI A,03h     ; drive 3
        // 0002  OUT 08h       ; Disk Control / select
        // 0004  MVI A,04h     ; Head Load
        // 0006  OUT 09h
        // 0008: IN  08h       ; wait Head Status (active low D2)
        // 000A  ANI 04h
        // 000C  JNZ 0008h
        // 000F: IN  08h       ; wait NRDA (active low D7)
        // 0011  ANI 80h
        // 0013  JNZ 000Fh
        // 0016  IN  0Ah       ; consume physical read-data latch
        // 0018  STA 0100h
        // 001B  HLT
        let program = [
            0x3e, 0x03, 0xd3, 0x08, 0x3e, 0x04, 0xd3, 0x09, 0xdb, 0x08, 0xe6, 0x04, 0xc2, 0x08,
            0x00, 0xdb, 0x08, 0xe6, 0x80, 0xc2, 0x0f, 0x00, 0xdb, 0x0a, 0x32, 0x00, 0x01, 0x76,
        ];
        assert_eq!(ram.load(0, &program), program.len());

        settle_test_bus(&mut backplane, display);
        let mut cpu = Cpu8080Cycle::new();
        let mut halted = false;

        for _ in 0..250_000 {
            let initial = cpu_board.package_inputs();
            let trace = cpu.tick_with_live_phi2_inputs(initial, |_edge, pins| {
                cpu_board.set_package_pins(pins);
                settle_test_bus(&mut backplane, display);
                cpu_board.package_inputs()
            });
            assert_eq!(trace.fault, None);

            // Production advances DCDD mechanics once per exact 8080 T-state at
            // the CPU-board boundary. Mirror that clock bridge here exactly.
            harness.advance_mechanics_t_states(1);

            if cpu.is_halted() {
                halted = true;
                break;
            }
        }

        assert!(halted, "8080 never reached HLT after polling Head Status/NRDA");
        assert!(
            cpu.total_t_states() >= 80_000,
            "head-load delay must consume at least the documented 40 ms"
        );
        let read_byte = ram
            .read_byte(0x0100)
            .expect("physical RAM must contain the byte stored by the 8080");
        assert_eq!(read_byte, cpu.registers().a);

        println!(
            "8080 -> S-100 -> 88-DCDD -> FD-400 -> media: IN 0Ah read {read_byte:02X} after {} T-states",
            cpu.total_t_states()
        );
    }
}
