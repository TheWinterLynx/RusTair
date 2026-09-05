use rustair::backend::BackendHost;

#[test]
fn ldax_d_exposes_de_on_front_panel_address_bus() {
    let mut machine = BackendHost::default();
    machine.power(true);
    machine.front_panel_reset();

    // LXI D establishes the register pair through the real CPU, then LDAX D
    // reads the byte addressed by DE. KillBit deliberately repeats this
    // operation so the value in D dominates the high address lamps.
    machine.load_bytes(0x0000, &[0x11, 0x00, 0x80, 0x1a]);
    machine.debugger_step_instruction();
    machine.debugger_step_instruction();

    let panel = machine.front_panel_state();
    assert_eq!(panel.address, 0x8000);
    assert_eq!(panel.lamps.address[15], 1.0);
    assert_eq!(panel.lamps.memr, 1.0);
    assert_eq!(panel.lamps.m1, 0.0);
}

#[test]
fn in_ff_exposes_ffff_on_address_bus_and_reads_sense_switches() {
    let mut machine = BackendHost::default();
    machine.power(true);
    machine.front_panel_reset();
    machine.toggle_sense_switch(15);

    // MVI A is unnecessary here: IN FFh leaves its result in A.
    machine.load_bytes(0x0000, &[0xdb, 0xff]);
    machine.debugger_step_instruction();

    assert_eq!(machine.intel8080_state().a, 0x80);
    let panel = machine.front_panel_state();
    assert_eq!(panel.address, 0xffff);
    assert_eq!(panel.lamps.inp, 1.0);
    assert_eq!(panel.lamps.memr, 0.0);
}
