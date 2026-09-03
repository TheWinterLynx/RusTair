use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{
    RamInit, S100HardwareConfig, S100InstalledCardConfig,
};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

fn topology_with_gap_and_overlap() -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(6)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(
                    S100RamBoardModel::Mits4KStatic88_4Mcs,
                    0x2000,
                ),
            )),
        )
        .unwrap();
    hardware
        .set_slot(
            3,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(
                    S100RamBoardModel::Mits4KDynamic88_4Mcd,
                    0x2000,
                ),
            )),
        )
        .unwrap();
    hardware
        .set_slot(
            4,
            Some(S100InstalledCardConfig::Ram(
                S100RamCardConfig::fully_populated(
                    S100RamBoardModel::Mits4KStatic88_4Mcs,
                    0x4000,
                ),
            )),
        )
        .unwrap();
    hardware.validate().unwrap()
}

#[test]
fn fast_and_cycle_mount_the_same_slot_native_memory_topology() {
    let hardware = topology_with_gap_and_overlap();

    for engine in EmulationEngine::ALL {
        let mut host = BackendHost::from_engine(engine).unwrap();
        host.configure_s100_hardware(hardware, RamInit::Zeroed);

        assert_eq!(host.s100_hardware(), hardware, "{engine:?}");
        assert_eq!(host.installed_ram_bytes(), 12 * 1024, "{engine:?}");

        let gap = host.inspect_memory_mapping(0x1000);
        assert!(gap.is_unmapped(), "{engine:?}");
        assert_eq!(host.peek_memory(0x1000), None, "{engine:?}");

        let overlap = host.inspect_memory_mapping(0x2000);
        assert!(overlap.is_overlap(), "{engine:?}");
        assert_eq!(
            overlap.drivers.iter().map(|driver| driver.slot).collect::<Vec<_>>(),
            vec![2, 3],
            "{engine:?}"
        );
        assert_eq!(host.peek_memory(0x2000), None, "overlap is not a unique host byte: {engine:?}");

        let unique = host.inspect_memory_mapping(0x4000);
        assert_eq!(unique.drivers.len(), 1, "{engine:?}");
        assert_eq!(unique.drivers[0].slot, 4, "{engine:?}");
        assert_eq!(host.peek_memory(0x4000), Some(0), "{engine:?}");
    }
}
