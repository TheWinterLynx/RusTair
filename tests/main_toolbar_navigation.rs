const RUNTIME_SOURCE: &str = include_str!("../src/app/runtime.rs");

#[test]
fn main_toolbar_exposes_t_state_teacher_directly() {
    assert!(RUNTIME_SOURCE.contains("ui.button(\"T-STATE TEACHER\")"));
    assert!(RUNTIME_SOURCE.contains("self.open_bus_teacher(ctx)"));
}

#[test]
fn main_toolbar_groups_tcp_and_com_under_external_menu() {
    assert!(RUNTIME_SOURCE.contains("ui.menu_button(\"EXTERNAL\", |ui|"));
    assert!(!RUNTIME_SOURCE.contains("ui.button(\"EXTERNAL TCP\")"));
    assert!(!RUNTIME_SOURCE.contains("ui.button(\"EXTERNAL COM\")"));
}

#[test]
fn configuration_groups_external_transport_settings() {
    assert!(RUNTIME_SOURCE.contains("ui.menu_button(\"External\", |ui|"));
    assert!(RUNTIME_SOURCE.contains("ui.menu_button(\"TCP\", |ui| { self.draw_external_serial_config_menu(ui); })"));
    assert!(RUNTIME_SOURCE.contains("ui.menu_button(\"COM\", |ui| { self.draw_external_com_config_menu(ui); })"));
    assert!(!RUNTIME_SOURCE.contains("ui.menu_button(\"External TCP\""));
    assert!(!RUNTIME_SOURCE.contains("ui.menu_button(\"External COM\""));
}
