const VIEWER: &str = include_str!("../src/app/ui/memory_viewer.rs");
const PRESENTATION: &str = include_str!("../src/app/ui/s100_memory_inspection.rs");

#[test]
fn ram_viewer_uses_physical_s100_mapping_instead_of_flat_capacity() {
    assert!(VIEWER.contains("inspect_memory_mapping(address)"));
    assert!(VIEWER.contains("mapping_cell_text(&inspection)"));
    assert!(VIEWER.contains("visible_ram_value(&inspection)"));
    assert!(VIEWER.contains("S-100 RAM cards / physical map"));
    assert!(VIEWER.contains("Patch physical RAM byte"));
    assert!(VIEWER.contains("CPU board S{slot:02}"));
    assert!(!VIEWER.contains("MEMORY_BOARD_SIZE"));
    assert!(!VIEWER.contains("config.machine.ram_size"));
    assert!(!VIEWER.contains("guest reads return 00h"));

    assert!(PRESENTATION.contains("UNMAPPED · open bus"));
    assert!(PRESENTATION.contains("OVERLAP · slots"));
    assert!(PRESENTATION.contains("CONTENTION"));
    assert!(PRESENTATION.contains("visible_ram_value"));
}

#[test]
fn ram_viewer_refuses_to_hide_overlap_by_editing_one_arbitrary_card() {
    assert!(VIEWER.contains("inspection.drivers.len() != 1"));
    assert!(VIEWER.contains("choosing one physical card would hide the overlap"));
    assert!(VIEWER.contains("debugger will not choose one card to edit"));
}
