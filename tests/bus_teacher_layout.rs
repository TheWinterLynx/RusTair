const BUS_TEACHER_SOURCE: &str = include_str!("../src/app/ui/bus_teacher.rs");

#[test]
fn bus_teacher_uses_panorama_instead_of_one_vertical_stack() {
    assert!(
        BUS_TEACHER_SOURCE.contains("TopBottomPanel::top(\"bus-teacher-toolbar\")"),
        "Bus Teacher should keep its compact header in a dedicated top bar",
    );
    assert!(
        BUS_TEACHER_SOURCE.contains("ui.columns(2, |columns|"),
        "Bus Teacher should use the available horizontal space through two-column layouts",
    );
    assert!(
        BUS_TEACHER_SOURCE.contains("BUS_TEACHER_WIDTH: f32 = 1220.0")
            && BUS_TEACHER_SOURCE.contains("BUS_TEACHER_HEIGHT: f32 = 760.0"),
        "Bus Teacher should open with a landscape-oriented viewport",
    );
    assert!(
        BUS_TEACHER_SOURCE.contains("draw_bus_teacher_left_column")
            && BUS_TEACHER_SOURCE.contains("draw_bus_teacher_right_column"),
        "the main teaching sections should remain split into stable left/right columns",
    );
}

#[test]
fn bus_teacher_dense_signal_tables_are_split_horizontally() {
    for id in [
        "bus-teacher-pins-left",
        "bus-teacher-pins-right",
        "bus-teacher-status-left",
        "bus-teacher-status-right",
    ] {
        assert!(
            BUS_TEACHER_SOURCE.contains(id),
            "missing compact side-by-side Bus Teacher table {id}",
        );
    }
}

#[test]
fn bus_teacher_live_timing_fields_have_fixed_horizontal_geometry() {
    for constant in [
        "TIMING_LEFT_LABEL_WIDTH",
        "TIMING_LEFT_VALUE_WIDTH",
        "TIMING_RIGHT_LABEL_WIDTH",
        "TIMING_RIGHT_VALUE_WIDTH",
    ] {
        assert!(
            BUS_TEACHER_SOURCE.contains(constant),
            "missing fixed Bus Teacher timing slot {constant}",
        );
    }
    assert!(BUS_TEACHER_SOURCE.contains("fn draw_timing_row("));
    assert!(BUS_TEACHER_SOURCE.contains("Self::draw_timing_row("));
    assert!(
        !BUS_TEACHER_SOURCE.contains("Grid::new(\"bus-teacher-timing-grid\")"),
        "live timing values must not use content-sized egui::Grid columns because changing machine-cycle text would shift neighboring fields",
    );
}

#[test]
fn bus_teacher_keeps_frozen_cpu_sample_separate_from_live_chassis() {
    assert!(BUS_TEACHER_SOURCE.contains("CURRENT CHASSIS / S-100 (NOW)"));
    assert!(BUS_TEACHER_SOURCE.contains("Freeze locks LAST CPU SAMPLE only; CURRENT CHASSIS stays live."));
    assert!(BUS_TEACHER_SOURCE.contains("let current_chassis = live.and_then(|snapshot| snapshot.current_chassis)"));
    assert!(BUS_TEACHER_SOURCE.contains("state.frozen_snapshot.or(live)"));
}
