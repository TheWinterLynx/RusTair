const APP: &str = include_str!("../src/app/mod.rs");
const RUNTIME: &str = include_str!("../src/app/runtime.rs");
const EMBEDDED_DIAGNOSTICS: &str = include_str!("../src/app/embedded_cpu_diagnostics.rs");
const EXTERNAL_DIAGNOSTICS: &str = include_str!("../src/app/cpu_diagnostics.rs");

#[test]
fn new_emulator_speed_selector_exposes_only_supported_user_choices() {
    let choices = APP
        .split("const SELECTABLE_EMULATION_SPEEDS: [EmulationSpeed; 4] = [")
        .nth(1)
        .expect("selectable emulator speed list")
        .split("];")
        .next()
        .expect("selectable emulator speed list body");

    assert!(choices.contains("EmulationSpeed::Authentic"));
    assert!(choices.contains("EmulationSpeed::X5"));
    assert!(choices.contains("EmulationSpeed::X10"));
    assert!(choices.contains("EmulationSpeed::Unlimited"));
    assert!(!choices.contains("EmulationSpeed::X2"));
    assert!(RUNTIME.contains("ui.label(\"Emulator speed\")"));
}

#[test]
fn authentic_speed_label_comes_from_installed_board_clock() {
    assert!(APP.contains("Authentic hardware clock — {:.1} MHz"));
    assert!(APP.contains("board.clock_hz() as f32 / 1_000_000.0"));
    assert!(RUNTIME.contains("emulation_speed_label(speed, board)"));
    assert!(RUNTIME.contains("Authentic hardware clock: {:.1} MHz"));
}

#[test]
fn embedded_cpu_tests_offer_real_x5_x10_and_unlimited_modes() {
    assert!(EMBEDDED_DIAGNOSTICS.contains(
        "enum DiagnosticRunSpeed { Authentic, X5, X10, Unlimited }"
    ));
    assert!(EMBEDDED_DIAGNOSTICS.contains("Self::X5 => EmulationSpeed::X5"));
    assert!(EMBEDDED_DIAGNOSTICS.contains("Self::X10 => EmulationSpeed::X10"));
    assert!(EMBEDDED_DIAGNOSTICS.contains("Self::Unlimited => EmulationSpeed::Unlimited"));
    assert!(!EMBEDDED_DIAGNOSTICS.contains("Authentic2MHz"));
}

#[test]
fn cpu_diagnostic_results_report_execution_mode_not_fixed_two_mhz() {
    for source in [EMBEDDED_DIAGNOSTICS, EXTERNAL_DIAGNOSTICS] {
        assert!(source.contains("Test speed: {speed_label}"));
        assert!(!source.contains("Equivalent 8080 time at 2 MHz"));
        assert!(!source.contains("format_2mhz_duration"));
    }
}

#[test]
fn external_cpu_diagnostic_keeps_the_launch_speed_for_its_result() {
    assert!(APP.contains("cpu_diagnostic_run_speed_label: Option<String>"));
    assert!(RUNTIME.contains("self.cpu_diagnostic_run_speed_label.is_some()"));
    assert!(RUNTIME.contains("Speed locked while external CPU diagnostic runs: {speed}"));
    assert!(EXTERNAL_DIAGNOSTICS.contains(
        "self.cpu_diagnostic_run_speed_label = Some(speed_label.clone());"
    ));
    assert!(EXTERNAL_DIAGNOSTICS.contains("self.cpu_diagnostic_run_speed_label.take()"));
    assert!(EXTERNAL_DIAGNOSTICS.contains("DIAGNOSTIC_RESULT_SPEED_ID"));
}
