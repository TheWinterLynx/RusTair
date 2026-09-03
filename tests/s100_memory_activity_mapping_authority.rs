const SOURCE: &str = include_str!("../src/app/ui/memory_activity.rs");

#[test]
fn memory_activity_reports_current_slot_native_s100_mapping_without_flat_ram_assumptions() {
    assert!(SOURCE.contains("inspect_memory_mapping(address)"));
    assert!(SOURCE.contains("mapping_summary(&inspection)"));
    assert!(SOURCE.contains("mapping_detail(address, &inspection)"));
    assert!(SOURCE.contains("S-100 NOW"));
    assert!(SOURCE.contains("current physical S-100 mapping"));
    assert!(SOURCE.contains("not a reconstructed historical mapping"));
    assert!(!SOURCE.contains("installed_ram_bytes"));
    assert!(!SOURCE.contains("config.machine.ram_size"));
}
