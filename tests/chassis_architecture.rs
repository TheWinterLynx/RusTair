use rustair::machine::{AltairChassis, MEM_SIZE};

#[test]
fn real_chassis_default_is_cpu_free_physical_container() {
    let chassis = AltairChassis::default();
    assert!(!chassis.powered);
    assert!(!chassis.running);
    assert_eq!(chassis.installed_ram_bytes(), MEM_SIZE);
}

#[test]
fn chassis_source_contains_no_cpu_or_deref_escape_hatch() {
    let source = include_str!("../src/machine/chassis.rs");

    assert!(source.contains("pub struct AltairChassis"));
    assert!(!source.contains("Cpu8080"), "the physical chassis must not own a Fast CPU");
    assert!(!source.contains("Deref"), "the chassis migration must not hide ownership behind Deref");
    assert!(!source.contains("DerefMut"), "the chassis migration must preserve explicit field ownership");
}
