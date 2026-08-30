mod machine_probe {
    #[derive(Default)]
    pub(super) struct AltairBus;

    mod chassis {
        include!("../src/machine/chassis.rs");
    }

    pub(super) fn construct_chassis() {
        let chassis = chassis::AltairChassis::default();
        let _ = &chassis.bus;
        assert!(!chassis.powered);
        assert!(!chassis.running);
        assert!(!chassis.stop_switch_asserted);
        assert!(!chassis.run_switch_asserted);
    }
}

#[test]
fn staged_chassis_compiles_as_cpu_free_data_container() {
    machine_probe::construct_chassis();
}

#[test]
fn staged_chassis_contains_no_cpu_or_deref_escape_hatch() {
    let source = include_str!("../src/machine/chassis.rs");

    assert!(source.contains("struct AltairChassis"));
    assert!(!source.contains("Cpu8080"), "the physical chassis must not own a Fast CPU");
    assert!(!source.contains("Deref"), "the chassis migration must not hide ownership behind Deref");
    assert!(!source.contains("DerefMut"), "the chassis migration must preserve explicit field ownership");
}
