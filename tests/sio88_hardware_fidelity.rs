use std::time::Duration;

use rustair::backend::{CycleAccurateMachineBackend, MachineBackend};
use rustair::config::{
    SioAddressPair, SioHardwareConfig, SioInterruptTarget, SioInterruptWiring, SioRevision,
};
use rustair::machine::AltairMachine;

#[test]
fn rev1_status_and_timing_are_owned_by_the_88_sio_card() {
    let mut backend = CycleAccurateMachineBackend::default();
    backend.power(true).unwrap();
    backend.halt().unwrap();
    assert_eq!(backend.machine().bus.peek_io_port(0x00), 0x01, "Rev1 idle status is not-RDA on D0 with active-low TBMT ready on D7");

    backend.machine_mut().bus.serial_receive(b'R');
    assert_eq!(backend.machine().bus.peek_io_port(0x00) & 0x01, 0x01, "RDA must not change until the serial frame completes");
    assert_eq!(backend.machine().bus.peek_io_port(0x01), 0x00);

    backend.commit_panel_activity(Duration::from_millis(100)).unwrap();
    assert_eq!(backend.machine().bus.peek_io_port(0x00) & 0xc1, 0x00, "completed Rev1 RX drives D0 low and must never fabricate D6");
    assert_eq!(backend.machine().bus.peek_io_port(0x01), b'R');
    assert_eq!(backend.machine_mut().bus.debugger_input_port(0x01), b'R');
    assert_eq!(backend.machine().bus.peek_io_port(0x00) & 0x01, 0x01);

    backend.machine_mut().bus.debugger_output_port(0x01, b'T');
    assert_eq!(backend.machine().bus.serial_tx_front(), None, "TX byte must cross the COM2502 shift register before reaching the endpoint");
    backend.commit_panel_activity(Duration::from_millis(100)).unwrap();
    assert_eq!(backend.machine().bus.serial_tx_front(), Some(b'T'));
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
fn rev0_exposes_uart_flags_and_external_device_ready_as_independent_status_sources() {
    let mut machine = AltairMachine::default();
    machine.configure_sio_hardware(SioHardwareConfig {
        revision: SioRevision::Rev0,
        ..SioHardwareConfig::default()
    });

    assert_eq!(machine.bus.peek_io_port(0x00), 0x83, "Rev0 starts with external D0/D7 ready latches reset and COM2502 TBMT on D1");
    assert!(machine.bus.debugger_inject_serial_rx(0x01, b'A'));
    assert_eq!(machine.bus.peek_io_port(0x00) & 0xa3, 0xa3, "COM2502 RDA/TBMT must not fabricate RIN/ROT device-ready state");
    assert!(machine.bus.pulse_sio_input_device_ready());
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x21, 0x20, "explicit RIN ready pulls D0 low while RDA remains independently high");
    assert_eq!(machine.bus.sio_handshake_lines(), Some((true, false, true, false)));
    assert_eq!(machine.bus.debugger_input_port(0x01), b'A');
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x21, 0x01, "DATA IN clears RDA and the Rev0 input-ready latch");
    assert_eq!(machine.bus.sio_handshake_lines(), Some((false, false, false, false)));
}

#[test]
fn rev0_external_ready_routes_to_vi_then_data_cycles_clear_it_at_public_boundary() {
    let mut machine = AltairMachine::default();
    machine.configure_sio_hardware(SioHardwareConfig {
        revision: SioRevision::Rev0,
        ..SioHardwareConfig::default()
    });
    machine.configure_sio_interrupt_wiring(SioInterruptWiring {
        input: SioInterruptTarget::Vi3,
        output: SioInterruptTarget::Vi4,
    });
    machine.bus.debugger_output_port(0x00, 0x03);

    assert!(machine.bus.pulse_sio_input_device_ready());
    assert_eq!(machine.bus.sio_vector_interrupt_requests(), 1 << 3);
    let _ = machine.bus.debugger_input_port(0x01);
    assert_eq!(machine.bus.sio_vector_interrupt_requests(), 0);

    assert!(machine.bus.pulse_sio_output_device_ready());
    assert_eq!(machine.bus.sio_vector_interrupt_requests(), 1 << 4);
    machine.bus.debugger_output_port(0x01, b'O');
    assert_eq!(machine.bus.sio_vector_interrupt_requests(), 0);
}

#[test]
fn com2502_overrun_overwrites_old_unread_byte_at_public_bus_boundary() {
    let mut machine = AltairMachine::default();
    assert!(machine.bus.debugger_inject_serial_rx(0x01, b'A'));
    assert!(machine.bus.debugger_inject_serial_rx(0x01, b'B'));
    assert_eq!(machine.bus.peek_io_port(0x00) & 0x10, 0x10);
    assert_eq!(machine.bus.debugger_input_port(0x01), b'B');
}
