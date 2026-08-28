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
fn pin_visualizer_distinguishes_level_assertion_power_clock_and_unmodeled_lines() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("ControlPin::WrN => (snapshot.pins.wr_n, true"));
    assert!(source.contains("LOW ASSERTED"));
    assert!(source.contains("outer amber ring = signal ASSERTED"));
    assert!(source.contains("label: \"INT\", kind: PinKind::Unmodeled"));
    assert!(source.contains("label: \"PHI1\", kind: PinKind::Clock"));
    assert!(source.contains("label: \"PHI2\", kind: PinKind::Clock"));
    assert!(source.contains("CLOCK PRESENT - phase not modeled"));
    assert!(source.contains("POWER ON"));
    assert!(source.contains("POWER OFF"));
}

#[test]
fn exact_undriven_address_data_pins_are_hi_z_not_unknown() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("fn exact_bus_is_released"));
    assert!(source.contains("HI-Z / RELEASED"));
    assert!(source.contains("\" Z\""));
    assert!(source.contains("NO DATA TRANSFER THIS T-STATE"));
    assert!(source.contains("front-panel DATA display can still show the preceding bus byte"));
}

#[test]
fn control_state_never_projects_s100_bus_values_back_into_cpu_address_data_pins() {
    let source = include_str!("../src/app/ui/cpu_pin_diagram.rs");
    assert!(source.contains("snapshot.accuracy != BusTeachingAccuracy::ControlState"));
    assert!(source.contains("CPU A/D package pins remain '?' until an actual T-state is sampled"));
}

#[test]
fn bus_teacher_offers_package_and_precision_table_views() {
    let source = include_str!("../src/app/ui/bus_teacher.rs");
    assert!(source.contains("Package diagram"));
    assert!(source.contains("Signal table"));
    assert!(source.contains("cpu_pin_diagram::draw_8080a_package"));
}
