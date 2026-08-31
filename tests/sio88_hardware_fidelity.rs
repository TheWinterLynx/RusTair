use rustair::config::{SioAddressPair, SioHardwareConfig, SioRevision};
use rustair::machine::AltairMachine;

#[test]
fn rev1_status_and_timing_are_owned_by_the_88_sio_card() {
    let mut machine = AltairMachine::default();
    assert_eq!(machine.bus.peek_io_port(0x00), 0x01, "Rev1 idle status is not-RDA on D0 with active-low TBMT ready on D7");

    machine.bus.serial_receive(b'R');
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x01, 0x01, "RDA must not change until the serial frame completes");
    assert_eq!(machine.bus.peek_io_port(0x01), 0x00);

    machine.bus.advance_serial_hardware_time(200_000);
    assert_eq!(machine.bus.peek_io_port(0x00) & 0xc1, 0x00, "completed Rev1 RX drives D0 low and must never fabricate D6");
    assert_eq!(machine.bus.peek_io_port(0x01), b'R');
    assert_eq!(machine.bus.debugger_input_port(0x01), b'R');
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x01, 0x01);

    machine.bus.debugger_output_port(0x01, b'T');
    assert_eq!(machine.bus.serial_tx_front(), None, "TX byte must cross the COM2502 shift register before reaching the endpoint");
    machine.bus.advance_serial_hardware_time(200_000);
    assert_eq!(machine.bus.serial_tx_front(), Some(b'T'));
}

#[test]
fn physical_address_pair_moves_decode_and_old_ports_become_open_bus() {
    let mut machine = AltairMachine::default();
    machine.configure_sio_hardware(SioHardwareConfig {
        address: SioAddressPair::try_new(0x06).unwrap(),
        ..SioHardwareConfig::default()
    });

    assert_eq!(machine.bus.peek_io_port(0x00), 0xff);
    assert_eq!(machine.bus.peek_io_port(0x01), 0xff);
    assert_eq!(machine.bus.peek_io_port(0x06), 0x01);
    assert!(machine.bus.debugger_inject_serial_rx(0x07, b'J'));
    assert_eq!(machine.bus.peek_io_port(0x07), b'J');
}

#[test]
fn rev0_exposes_original_active_high_rda_and_tbmt_bits() {
    let mut machine = AltairMachine::default();
    machine.configure_sio_hardware(SioHardwareConfig {
        revision: SioRevision::Rev0,
        ..SioHardwareConfig::default()
    });

    assert_eq!(machine.bus.peek_io_port(0x00), 0x02, "Rev0 idle TBMT is D1 active high");
    assert!(machine.bus.debugger_inject_serial_rx(0x01, b'A'));
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x22, 0x22, "Rev0 RDA is D5 active high while TBMT remains D1 high");
}

#[test]
fn com2502_overrun_overwrites_old_unread_byte_at_public_bus_boundary() {
    let mut machine = AltairMachine::default();
    assert!(machine.bus.debugger_inject_serial_rx(0x01, b'A'));
    assert!(machine.bus.debugger_inject_serial_rx(0x01, b'B'));
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x10, 0x10);
    assert_eq!(machine.bus.debugger_input_port(0x01), b'B');
}
