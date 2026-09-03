use rustair::config::{AppConfig, CpuBoard, CpuModel};

#[test]
fn default_machine_exposes_the_physical_mits_8080_board() {
    let machine = AppConfig::default().machine;
    let board = machine
        .s100_hardware
        .active_cpu_board()
        .expect("default S-100 assembly has one CPU board");

    assert_eq!(board, CpuBoard::Mits8080);
    assert_eq!(board.cpu_model(), CpuModel::Intel8080);
    assert_eq!(board.clock_hz(), 2_000_000);
}

#[test]
fn no_placeholder_second_cpu_or_board_is_advertised() {
    assert_eq!(CpuModel::ALL, [CpuModel::Intel8080]);
    assert_eq!(CpuBoard::ALL, [CpuBoard::Mits8080]);
}

#[test]
fn z80_is_documented_as_future_board_work_not_current_runtime_state() {
    let backend = include_str!("../src/backend/mod.rs");
    let config = include_str!("../src/config/machine.rs");

    assert!(!backend.contains("CpuState::Z80"));
    assert!(!backend.contains("Z80State"));
    assert!(!config.contains("ZilogZ80,"));
    assert!(!config.contains("CromemcoZpu,"));
}
