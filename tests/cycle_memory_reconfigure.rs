use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};

const CYCLE_HOST: &str = include_str!("../src/backend/cycle_host.rs");

#[test]
fn unpowered_cycle_memory_reconfigure_preserves_unrelated_chassis_configuration() {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("cycle backend must be built in");

    host.configure_serial_board(SerialBoard::TwoSio88);
    host.configure_memory_board_profile(RamBoardProfile::Mits1KStatic1975);
    host.set_switch_register(0xa55a);

    host.configure_memory(RamSize::K16, RamInit::Zeroed);

    assert_eq!(host.installed_ram_bytes(), 16 * 1024);
    assert_eq!(host.serial_board(), SerialBoard::TwoSio88);
    assert_eq!(host.switch_register(), 0xa55a);
}

#[test]
fn powered_cycle_memory_reconfigure_resets_cpu_without_replacing_chassis_state() {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("cycle backend must be built in");

    host.configure_serial_board(SerialBoard::TwoSio88);
    host.set_switch_register(0xa55a);
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, &[0x00, 0x00]);
    host.step();
    assert_eq!(host.intel8080_state().pc, 1);

    host.set_running(true);
    host.configure_memory(RamSize::K16, RamInit::Zeroed);

    assert!(host.powered(), "RAM replacement must not power-cycle the chassis");
    assert!(!host.running(), "RAM replacement must leave the powered machine stopped");
    assert_eq!(host.intel8080_state().pc, 0, "powered RAM replacement must reset the active Cycle CPU");
    assert_eq!(host.installed_ram_bytes(), 16 * 1024);
    assert_eq!(host.serial_board(), SerialBoard::TwoSio88);
    assert_eq!(host.switch_register(), 0xa55a);
    assert_eq!(host.peek_memory(0), Some(0));
}

#[test]
fn cycle_memory_reconfigure_cannot_rebuild_the_backend() {
    let body = CYCLE_HOST
        .split("fn configure_memory(&mut self, size: RamSize, init: RamInit) -> BackendResult<()> {")
        .nth(1)
        .expect("CycleHostBackend configure_memory implementation")
        .split("fn configure_memory_board_profile")
        .next()
        .expect("configure_memory body");

    assert!(
        body.contains("self.inner.machine_mut().bus.configure_memory(size, init);"),
        "Cycle RAM replacement must mutate the existing chassis bus directly"
    );
    assert!(!body.contains("self.inner ="), "Cycle RAM replacement must not replace its backend");
    assert!(
        !body.contains("CycleAccurateMachineBackend::default"),
        "Cycle RAM replacement must not construct a fresh Cycle backend"
    );
    assert!(!body.contains("*self ="), "Cycle RAM replacement must not replace CycleHostBackend");
}
