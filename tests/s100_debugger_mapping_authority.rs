const DEBUGGER_SOURCE: &str = include_str!("../src/app/ui/debugger_controls.rs");
const INSPECTION_SOURCE: &str = include_str!("../src/app/ui/s100_memory_inspection.rs");

#[test]
fn debugger_reports_live_s100_mapping_instead_of_assuming_flat_installed_ram() {
    assert!(DEBUGGER_SOURCE.contains("inspect_memory_mapping(execution_address)"));
    assert!(DEBUGGER_SOURCE.contains("mapping_summary(&inspection)"));
    assert!(DEBUGGER_SOURCE.contains("mapping_detail(execution_address, &inspection)"));
    assert!(DEBUGGER_SOURCE.contains("not uniquely mapped"));
    assert!(DEBUGGER_SOURCE.contains("Watchpoints observe guest bus transfers"));
    assert!(INSPECTION_SOURCE.contains("UNMAPPED · open bus"));
    assert!(INSPECTION_SOURCE.contains("OVERLAP · slots"));
    assert!(INSPECTION_SOURCE.contains("CONTENTION"));
}
