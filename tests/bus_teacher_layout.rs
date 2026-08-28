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
    assert!(
        BUS_TEACHER_SOURCE.contains(".num_columns(4)"),
        "instruction/timing data should use paired fields across four columns",
    );
}
