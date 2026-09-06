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

    // `panel.address` is the current front-panel address latch. The per-lamp
    // floats are presentation-duty integrators and remain zero until a visible
    // activity interval is committed, so they are not an electrical-state oracle.
    let panel = machine.front_panel_state();
    assert_eq!(panel.address, 0x8000);

    // Teaching retains the exact final S-100 memory-read sample even when no
    // presentation interval has been committed.
    let exact = machine.bus_teaching_snapshot().expect("LDAX must retain an exact Cycle sample");
    assert_eq!(exact.address, Some(0x8000));
    assert_ne!(exact.address.unwrap() & 0x8000, 0, "A15 must be asserted on the exact S-100 address bus");
    assert_eq!(exact.status.memr, Some(true));
    assert_eq!(exact.status.m1, Some(false));
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

    let exact = machine.bus_teaching_snapshot().expect("IN FFh must retain an exact Cycle sample");
    assert_eq!(exact.address, Some(0xffff));
    assert_eq!(exact.status.inp, Some(true));
    assert_eq!(exact.status.memr, Some(false));
}
