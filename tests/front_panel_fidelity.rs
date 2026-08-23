use rustair::machine::AltairMachine;

#[test]
fn ldax_d_exposes_de_on_front_panel_address_bus() {
    let mut machine = AltairMachine::default();
    machine.power(true);

    // LDAX D reads the byte addressed by DE. KillBit deliberately repeats this
    // operation so the value in D dominates the high address lamps.
    machine.bus.load(0x0000, &[0x1a]);
    machine.cpu.set_de(0x8000);
    machine.step();

    assert_eq!(machine.address_leds(), 0x8000);
    let lamps = machine.panel_lamps();
    assert_eq!(lamps.address[15], 1.0);
    assert_eq!(lamps.memr, 1.0);
    assert_eq!(lamps.m1, 0.0);
}

#[test]
fn in_ff_exposes_ffff_on_address_bus_and_reads_sense_switches() {
    let mut machine = AltairMachine::default();
    machine.power(true);
    machine.toggle_sense_switch(15);

    // MVI A is unnecessary here: IN FFh leaves its result in A.
    machine.bus.load(0x0000, &[0xdb, 0xff]);
    machine.step();

    assert_eq!(machine.cpu.a, 0x80);
    assert_eq!(machine.address_leds(), 0xffff);
    let lamps = machine.panel_lamps();
    assert_eq!(lamps.inp, 1.0);
    assert_eq!(lamps.memr, 0.0);
}
