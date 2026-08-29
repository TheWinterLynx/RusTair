use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamBoardProfile, RamInit, RamSize, SerialBoard};

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
