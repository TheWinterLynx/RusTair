#[test]
fn intel_8080a_package_is_code_drawn_not_an_image_asset() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("pub(super) fn draw_8080a_package"));
    assert!(source.contains("painter.rect_filled"));
    assert!(source.contains("painter.line_segment"));
    assert!(source.contains("painter.circle_stroke"));
    assert!(!source.contains("egui::Image"));
    assert!(!source.contains("include_bytes!"));
}

#[test]
fn intel_8080a_dip40_pinout_keeps_reference_numbering() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    for expected in [
        "number: 1, label: \"A10\"",
        "number: 2, label: \"GND\"",
        "number: 10, label: \"D0\"",
        "number: 12, label: \"RESET\"",
        "number: 14, label: \"INT\"",
        "number: 18, label: \"/WR\"",
        "number: 19, label: \"SYNC\"",
        "number: 20, label: \"+5V\"",
        "number: 21, label: \"HLDA\"",
        "number: 23, label: \"READY\"",
        "number: 25, label: \"A0\"",
        "number: 28, label: \"+12V\"",
        "number: 36, label: \"A15\"",
        "number: 40, label: \"A11\"",
    ] {
        assert!(source.contains(expected), "missing reference DIP-40 mapping {expected}");
    }
}

#[test]
fn pin_visualizer_distinguishes_level_assertion_power_and_exact_clock_lines() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("ControlPin::WrN => (snapshot.pins.wr_n, true"));
    assert!(source.contains("LOW ASSERTED"));
    assert!(source.contains("outer amber ring = signal ASSERTED"));
    assert!(source.contains("label: \"INT\", kind: PinKind::Control(ControlPin::Interrupt)"));
    assert!(source.contains("label: \"PHI1\", kind: PinKind::Clock(ClockPin::Phi1"));
    assert!(source.contains("label: \"PHI2\", kind: PinKind::Clock(ClockPin::Phi2"));
    assert!(source.contains("ClockPin::Phi1 => snapshot.pins.phi1"));
    assert!(source.contains("ClockPin::Phi2 => snapshot.pins.phi2"));
    assert!(source.contains("HIGH / PHASE ACTIVE"));
    assert!(source.contains("LOW / PHASE INACTIVE"));
    assert!(source.contains("UNKNOWN / NO EDGE SAMPLE"));
    assert!(!source.contains("phase not modeled"));
    assert!(source.contains("POWER ON"));
    assert!(source.contains("POWER OFF"));
}

#[test]
fn exact_undriven_cpu_data_pins_are_hi_z_not_front_panel_data() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("fn exact_bus_is_released"));
    assert!(source.contains("HI-Z / RELEASED"));
    assert!(source.contains("\" Z\""));
    assert!(source.contains("snapshot.cpu_data.map"));
    assert!(source.contains("never from S-100 DI/DO or optical DATA-lamp persistence"));
    assert!(source.contains("S-100 DI/DO and the front-panel DATA presentation are separate domains"));
}

#[test]
fn cpu_address_data_pin_truth_requires_exact_sample_or_stable_stop_wait() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("fn cpu_bus_pin_levels_available"));
    assert!(source.contains("snapshot.accuracy == BusTeachingAccuracy::Exact"));
    assert!(source.contains("snapshot.accuracy == BusTeachingAccuracy::ControlState"));
    assert!(source.contains("snapshot.machine_cycle == BusMachineCycle::ResetReleasedStopped"));
    assert!(source.contains("A reconstructed Fast snapshot may"));
    assert!(source.contains("projected back into the 8080 package"));
    assert!(source.contains("RESET RELEASED / STOP-WAIT is a special stable control state"));
    assert!(source.contains("CPU owns the address bus at PC=0000h"));
    assert!(source.contains("memory DI passes through the CPU-board input buffer onto the processor D bus"));
}

#[test]
fn reconstructed_fast_bus_is_explicitly_not_cpu_package_pin_truth() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("RECONSTRUCTED: Fast mode can show the front-panel DATA observation"));
    assert!(source.contains("DI, DO and 8080 D0-D7 remain unknown rather than being inferred from it"));
}

#[test]
fn cpu_control_pin_renderer_uses_backend_pin_truth_without_reconstructing_signals() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    for expected in [
        "ControlPin::Interrupt => (snapshot.interrupt",
        "ControlPin::Inte => (snapshot.pins.inte",
        "ControlPin::Dbin => (snapshot.pins.dbin",
        "ControlPin::WrN => (snapshot.pins.wr_n",
        "ControlPin::Sync => (snapshot.pins.sync",
        "ControlPin::Wait => (snapshot.pins.wait",
        "ControlPin::Hlda => (snapshot.pins.hlda",
    ] {
        assert!(source.contains(expected), "control pin must come from backend snapshot: {expected}");
    }
    assert!(source.contains("canonical S-100 PINT line"));
    assert!(source.contains("this UI never reconstructs a"));
    assert!(source.contains("signal from S-100 lamps, machine-cycle names or other presentation state"));
}

#[test]
fn package_and_teacher_keep_cpu_d_di_do_and_panel_data_separate() {
    let diagram = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    let teacher = include_str!("../src/app/ui/bus_teacher.rs");
    for expected in ["snapshot.cpu_data", "snapshot.s100_di", "snapshot.s100_do", "snapshot.panel_data"] {
        assert!(diagram.contains(expected), "package summary must expose split domain {expected}");
        assert!(teacher.contains(expected), "Teacher must expose split domain {expected}");
    }
    assert!(diagram.contains("\"CPU D\""));
    assert!(diagram.contains("\"S-100 DI\""));
    assert!(diagram.contains("\"S-100 DO\""));
    assert!(diagram.contains("\"PANEL DATA\""));
    assert!(!diagram.contains("snapshot.data.map(|value| value & (1u8 << bit)"), "DIP-40 D pins must never consume the compatibility/front-panel DATA field");
}

#[test]
fn bus_teacher_offers_package_and_precision_table_views() {
    let source = include_str!("../src/app/ui/bus_teacher.rs");
    assert!(source.contains("Package diagram"));
    assert!(source.contains("Signal table"));
    assert!(source.contains("INT/PINT"));
    assert!(source.contains("INT/SINTA"));
    assert!(source.contains("cpu_pin_diagram::draw_8080a_package"));
}
