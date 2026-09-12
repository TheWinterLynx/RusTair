use rustair::backend::{BackendHost, BusMachineCycle, BusTState, EmulationEngine};
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

fn dcdd_hardware() -> S100HardwareConfig {
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
                    0x0000,
                ),
            )),
        )
        .unwrap();
    hardware
        .set_slot(3, Some(S100InstalledCardConfig::Mits88DcddBoard1))
        .unwrap();
    hardware
        .set_slot(4, Some(S100InstalledCardConfig::Mits88DcddBoard2))
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

#[test]
fn dcdd_no_drive_register_surface_is_reached_only_through_real_8080_io_cycles() {
    let mut host = BackendHost::default();
    host.configure_s100_hardware(dcdd_hardware(), RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();

    // MVI A,00 / OUT 08 attempts to select drive 0. With no cable/drive the
    // controller must remain disabled. Then real IN instructions capture the
    // source-backed disabled status and the genuinely disabled sector drivers.
    // A later D7 clear exercises the same output surface before a second status
    // read. Results are stored by the 8080 in physical RAM for observation.
    let program = [
        0x3e, 0x00,       // MVI A,00
        0xd3, 0x08,       // OUT 08
        0xdb, 0x08,       // IN 08
        0x32, 0x00, 0x01, // STA 0100
        0xdb, 0x09,       // IN 09
        0x32, 0x01, 0x01, // STA 0101
        0x3e, 0x80,       // MVI A,80
        0xd3, 0x08,       // OUT 08 -- Disk Control clear
        0xdb, 0x08,       // IN 08
        0x32, 0x02, 0x01, // STA 0102
        0x76,             // HLT
    ];
    host.load_bytes(0x0000, &program);
    host.set_running(true);
    host.run_cycles(500);

    assert_eq!(host.peek_memory(0x0100), Some(0xe7));
    assert_eq!(
        host.peek_memory(0x0101),
        Some(0xff),
        "without Head Status the IN 09 sector-position drivers are physically high-Z"
    );
    assert_eq!(host.peek_memory(0x0102), Some(0xe7));
    assert_eq!(host.intel8080_state().halted, Some(true));

    // A chassis power cycle and CPU/front-panel reset cannot conjure a drive or
    // an enabled controller. Reload a tiny guest program because power-off also
    // applies the configured RAM initialization policy.
    host.power(false);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0x0000, &[0xdb, 0x08, 0x32, 0x03, 0x01, 0x76]);
    host.set_running(true);
    host.run_cycles(200);
    assert_eq!(host.peek_memory(0x0103), Some(0xe7));
}

#[test]
fn dcdd_input_byte_is_visible_on_the_same_exact_s100_cycle_as_the_front_panel() {
    let mut host = BackendHost::default();
    host.configure_s100_hardware(dcdd_hardware(), RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0x0000, &[0xdb, 0x08, 0x76]); // IN 08 / HLT

    let mut observed = None;
    for _ in 0..64 {
        host.debugger_step_t_state();
        let Some(sample) = host.bus_teaching_snapshot() else {
            continue;
        };
        if sample.machine_cycle == BusMachineCycle::InputRead && sample.t_state == BusTState::T3 {
            observed = Some(sample);
            break;
        }
    }

    let sample = observed.expect("real IN 08 must reach an exact InputRead T3 sample");
    assert_eq!(sample.address, Some(0x0808));
    assert_eq!(sample.s100_di, Some(0xe7));
    assert_eq!(sample.cpu_data, Some(0xe7));
    assert_eq!(sample.panel_data, Some(0xe7));
    assert_eq!(sample.status.inp, Some(true));
}
