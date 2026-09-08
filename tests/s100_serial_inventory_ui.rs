const HARDWARE_SOURCE: &str = include_str!("../src/config/s100_hardware.rs");
const UI_SOURCE: &str = include_str!("../src/app/ui/s100_hardware.rs");

#[test]
fn physical_s100_validation_does_not_forbid_multiple_serial_cards() {
    assert!(HARDWARE_SOURCE.contains("pub fn serial_slots"));
    assert!(!HARDWARE_SOURCE.contains("UnsupportedSerialCardCount"));
    assert!(HARDWARE_SOURCE.contains(
        "validation_preserves_multiple_serial_cards_for_physical_bus_resolution"
    ));
}

#[test]
fn current_editor_does_not_create_ambiguous_second_serial_card() {
    let compact: String = UI_SOURCE.split_whitespace().collect();
    let lower = UI_SOURCE.to_ascii_lowercase();

    assert!(UI_SOURCE.contains("fn is_serial_kind"));
    assert!(compact.contains("hardware.serial_slots().any(|(serial_slot,_)|serial_slot!=slot)"));
    assert!(UI_SOURCE.contains("slot + channel"));
    assert!(lower.contains("the s-100 fabric supports it"));
}

#[test]
fn s100_editor_describes_one_adaptive_machine_not_retired_fast_cycle_split() {
    assert!(UI_SOURCE.contains("Adaptive Cycle machine"));
    assert!(UI_SOURCE.contains("Full and Partial are internal execution strategies"));
    assert!(!UI_SOURCE.contains("used by both Fast and Cycle"));
    assert!(!UI_SOURCE.contains("transitional serial runtime"));
    assert!(!UI_SOURCE.contains("Fast/Cycle are execution engines"));
}
