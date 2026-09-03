use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{FastRamCompatibilityConfig, RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::s100_chassis::S100ChassisConfig;

fn cpu_in_slot_four() -> S100HardwareConfig {
    let chassis = S100ChassisConfig::altair_8800b(6).unwrap();
    let mut hardware = S100HardwareConfig::empty(chassis).unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::FastRamCompatibility(
                FastRamCompatibilityConfig::no_wait(0x0000, 0x1000),
            )),
        )
        .unwrap();
    hardware
        .set_slot(4, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware.validate().unwrap()
}

#[test]
fn fast_and_cycle_execute_with_the_same_cpu_board_installed_outside_slot_one() {
    let hardware = cpu_in_slot_four();

    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut machine = BackendHost::from_engine(engine).unwrap();
        machine.configure_s100_hardware(hardware, RamInit::Zeroed);
        assert_eq!(machine.s100_hardware().cpu_slots().collect::<Vec<_>>(), vec![4]);

        machine.load_bytes(0, &[0x00, 0x76]); // NOP; HLT
        machine.power(true);
        machine.assert_front_panel_reset();
        machine.release_front_panel_reset();
        machine.set_running(true);
        machine.run_cycles(64);

        let cpu = machine.intel8080_state();
        assert!(cpu.pc >= 1, "{engine:?} did not execute through the installed CPU board");
    }
}
