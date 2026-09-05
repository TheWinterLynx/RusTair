use rustair::adaptive_metrics;
use rustair::backend::BackendHost;
use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

fn static_16k_hardware() -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::altair_8800b(18)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(
                S100RamBoardModel::Mits16KStatic88_16Mcs,
                0,
            ))),
        )
        .unwrap();
    hardware.validate().unwrap()
}

#[test]
fn adaptive_metrics_account_for_full_partial_full_barriers_without_losing_t_states() {
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(static_16k_hardware(), RamInit::Zeroed);
    machine.power(true);
    machine.front_panel_reset();
    // NOP is Full-safe, IN FFh is an exact Partial barrier, JMP returns to NOP.
    machine.load_bytes(0, &[0x00, 0xdb, 0xff, 0xc3, 0x00, 0x00]);

    let before = machine.intel8080_state().total_t_states.unwrap();
    adaptive_metrics::begin_measurement();
    machine.set_running(true);
    machine.run_cycles(5_000);
    let stats = adaptive_metrics::end_measurement();
    let after = machine.intel8080_state().total_t_states.unwrap();
    let actual = after.saturating_sub(before);

    assert_eq!(stats.total_t_states(), actual);
    assert!(stats.full_t_states > 0, "NOP/JMP path must enter Full");
    assert!(stats.partial_t_states > 0, "IN FFh must execute in exact Partial");
    assert!(stats.full_windows > 0);
    assert!(stats.full_to_partial > 0);
    assert!(stats.partial_to_full > 0);
    assert!(stats.fallbacks.opcode_barrier > 0, "IN must be classified as the Partial barrier");
}
