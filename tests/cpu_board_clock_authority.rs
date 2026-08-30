const APP: &str = include_str!("../src/app/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const CONFIG: &str = include_str!("../src/config/machine.rs");
const CPU_DIAGNOSTICS: &str = include_str!("../src/app/cpu_diagnostics.rs");
const EMBEDDED_DIAGNOSTICS: &str = include_str!("../src/app/embedded_cpu_diagnostics.rs");

#[test]
fn runtime_scheduling_uses_installed_cpu_board_clock() {
    assert!(
        RUNTIME.contains("let board = self.config.machine.cpu_board();"),
        "runtime must resolve the installed CPU board before scheduling execution"
    );
    assert!(
        RUNTIME.contains("board.clock_hz() as f64 * dt.as_secs_f64()"),
        "authentic execution budget must derive from the installed CPU board clock"
    );
    assert!(
        !RUNTIME.contains("CLOCK_HZ"),
        "runtime must not consume any fixed/reference CPU clock"
    );
    assert!(
        !APP.contains("use crate::machine::CLOCK_HZ"),
        "application code must not import the historical global 2 MHz machine clock"
    );
}

#[test]
fn cpu_diagnostics_report_execution_mode_without_a_fixed_reference_clock() {
    assert!(
        !APP.contains("const CLOCK_HZ"),
        "application diagnostics must not restore a fixed 2 MHz clock constant"
    );
    for source in [CPU_DIAGNOSTICS, EMBEDDED_DIAGNOSTICS] {
        assert!(source.contains("Test speed: {speed_label}"));
        assert!(!source.contains("Equivalent 8080 time at 2 MHz"));
        assert!(!source.contains("format_2mhz_duration"));
        assert!(!source.contains("CLOCK_HZ"));
    }
}

#[test]
fn ui_reports_board_processor_and_board_clock_separately() {
    assert!(RUNTIME.contains("Installed CPU board: {}"));
    assert!(RUNTIME.contains("let cpu = board.cpu_model();"));
    assert!(RUNTIME.contains("board.clock_hz() as f32 / 1_000_000.0"));
    assert!(
        !RUNTIME.contains("cpu.clock_hz()"),
        "processor identity must not own board-level clock configuration"
    );
}

#[test]
fn config_keeps_cpu_board_as_the_physical_timing_authority() {
    assert!(CONFIG.contains("pub enum CpuBoard"));
    assert!(CONFIG.contains("pub const fn cpu_model(self) -> CpuModel"));
    assert!(CONFIG.contains("pub const fn clock_hz(self) -> u32"));
    assert!(CONFIG.contains("pub const fn cpu_board(self) -> CpuBoard"));
}
