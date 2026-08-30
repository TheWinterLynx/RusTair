#[test]
fn staged_chassis_contains_no_cpu_or_deref_escape_hatch() {
    let source = include_str!("../src/machine/chassis.rs");

    assert!(source.contains("struct AltairChassis"));
    assert!(!source.contains("Cpu8080"), "the physical chassis must not own a Fast CPU");
    assert!(!source.contains("Deref"), "the chassis migration must not hide ownership behind Deref");
    assert!(!source.contains("DerefMut"), "the chassis migration must preserve explicit field ownership");
}
