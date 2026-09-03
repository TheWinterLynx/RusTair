use crate::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use crate::cpu8080_cycle::{Cpu8080Cycle, Cpu8080Inputs};
use crate::s100_chassis::S100ChassisConfig;
use crate::s100_memory::{S100RamBoardModel, S100RamCardConfig};
use crate::s100_runtime::{DisplayControlLines, S100RuntimeFabric};

fn live_cycle_hardware() -> S100HardwareConfig {
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
                    S100RamBoardModel::Mits16KStatic88_16Mcs,
                    0,
                ),
            )),
        )
        .unwrap();
    config
}

#[test]
fn isolated_cycle_core_writes_through_cpu_board_mwrt_and_ram_card() {
    let mut fabric = S100RuntimeFabric::new(live_cycle_hardware(), RamInit::Zeroed).unwrap();
    assert_eq!(
        fabric.load_bytes(0, &[0x3e, 0x5a, 0x32, 0x00, 0x1f, 0x00]),
        6
    );
    let mut cpu = Cpu8080Cycle::new();
    let display = DisplayControlLines {
        ready: true,
        run: true,
        ..DisplayControlLines::default()
    };

    for _ in 0..96 {
        let mut initial = fabric.cpu_package_inputs();
        initial.ready = true;
        let _ = cpu.tick_with_live_phi2_inputs(initial, |_edge, pins| {
            fabric.set_cpu_package_pins(pins);
            fabric.settle(display, &[]).unwrap();
            let mut live = fabric.cpu_package_inputs();
            live.ready = true;
            live
        });
        if fabric.peek_unique_memory(0x1f00) == Some(0x5a) {
            break;
        }
    }

    assert_eq!(fabric.peek_unique_memory(0x1f00), Some(0x5a));
    assert_eq!(cpu.registers().pc, 5);
}

#[test]
fn isolated_cycle_memory_read_samples_live_di_without_host_memory_override() {
    let mut fabric = S100RuntimeFabric::new(live_cycle_hardware(), RamInit::Zeroed).unwrap();
    assert_eq!(fabric.load_bytes(0, &[0x3e, 0xa6]), 2);
    let mut cpu = Cpu8080Cycle::new();
    let display = DisplayControlLines {
        ready: true,
        run: true,
        ..DisplayControlLines::default()
    };

    for _ in 0..48 {
        let initial = fabric.cpu_package_inputs();
        let _ = cpu.tick_with_live_phi2_inputs(initial, |_edge, pins| {
            fabric.set_cpu_package_pins(pins);
            fabric.settle(display, &[]).unwrap();
            fabric.cpu_package_inputs()
        });
        if cpu.registers().pc == 2 && cpu.registers().a == 0xa6 {
            break;
        }
    }

    assert_eq!(cpu.registers().pc, 2);
    assert_eq!(cpu.registers().a, 0xa6);
}
