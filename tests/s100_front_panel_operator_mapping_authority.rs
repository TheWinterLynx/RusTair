const SOURCE: &str = include_str!("../src/app/ui/front_panel_operator.rs");

#[test]
fn front_panel_operator_executes_panel_cycle_before_host_side_mapping_observation() {
    let deposit = SOURCE
        .split("fn standalone_operator_execute_deposit")
        .nth(1)
        .expect("deposit helper")
        .split("fn standalone_switch_tooltip")
        .next()
        .expect("deposit body");

    let deposit_call = deposit.find("self.machine.deposit(deposit_next)").expect("real DEPOSIT path");
    let inspect_call = deposit.find("self.machine.inspect_memory_mapping(address)").expect("post-cycle S-100 inspection");
    assert!(deposit_call < inspect_call, "mapping must observe the result; it must not choose the guest/panel target");

    assert!(!deposit.contains("installed_ram_bytes"));
    assert!(!deposit.contains("peek_memory"));
    assert!(!deposit.contains("write_memory"));
    assert!(deposit.contains("mapping_summary(&inspection)"));
    assert!(deposit.contains("mapping_detail(address, &inspection)"));
}

#[test]
fn front_panel_operator_row_status_requires_one_real_ram_responder() {
    assert!(SOURCE.contains("inspection.drivers.as_slice()"));
    assert!(SOURCE.contains("[driver] if driver.value == byte"));
    assert!(SOURCE.contains("exactly one S-100 RAM card"));
}
