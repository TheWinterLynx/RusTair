use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{
    SioAddressPair, SioHardwareConfig, SioInterruptTarget, SioInterruptWiring, SioRevision,
};
use rustair::machine::AltairBus;

#[test]
fn rev1_status_and_timing_are_owned_by_the_88_sio_card() {
    // Wall-clock-to-card-clock bridging is a host scheduling responsibility.
    // Exercise the same public machine boundary as the application rather than
    // asking the lower-level Cycle core's panel-presentation hook to synthesize
    // independent oscillator time.
    let mut backend = BackendHost::default();
    backend.power(true);
    backend.set_running(false);
    assert_eq!(backend.peek_io_port(0x00), 0x01, "Rev1 idle status is not-RDA on D0 with active-low TBMT ready on D7");

    backend.serial_receive(BackendSerialPort::Port0, b'R');
    assert_eq!(backend.peek_io_port(0x00) & 0x01, 0x01, "RDA must not change until the serial frame completes");
    assert_eq!(backend.peek_io_port(0x01), 0x00);

    backend.commit_panel_activity(Duration::from_millis(100));
    assert_eq!(backend.peek_io_port(0x00) & 0xc1, 0x00, "completed Rev1 RX drives D0 low and must never fabricate D6");
    assert_eq!(backend.peek_io_port(0x01), b'R');
    assert_eq!(backend.debugger_input_port(0x01), b'R');
    assert_eq!(backend.peek_io_port(0x00) & 0x01, 0x01);

    backend.debugger_output_port(0x01, b'T');
    assert_eq!(backend.serial_tx_front(BackendSerialPort::Port0), None, "TX byte must cross the COM2502 shift register before reaching the endpoint");
    backend.commit_panel_activity(Duration::from_millis(100));
    assert_eq!(backend.serial_tx_front(BackendSerialPort::Port0), Some(b'T'));
}

#[test]
fn physical_address_pair_moves_decode_and_old_ports_become_open_bus() {
    let mut bus = AltairBus::default();
    bus.configure_sio_hardware(SioHardwareConfig {
        address: SioAddressPair::try_new(0x06).unwrap(),
        ..SioHardwareConfig::default()
    });

    assert_eq!(bus.peek_io_port(0x00), 0xff);
    assert_eq!(bus.peek_io_port(0x01), 0xff);
    assert_eq!(bus.peek_io_port(0x06), 0x01);
    assert!(bus.debugger_inject_serial_rx(0x07, b'J'));
    assert_eq!(bus.peek_io_port(0x07), b'J');
}

#[test]
fn rev0_exposes_uart_flags_and_external_device_ready_as_independent_status_sources() {
    let mut bus = AltairBus::default();
    bus.configure_sio_hardware(SioHardwareConfig {
        revision: SioRevision::Rev0,
        ..SioHardwareConfig::default()
    });

    assert_eq!(bus.peek_io_port(0x00), 0x83, "Rev0 starts with external D0/D7 ready latches reset and COM2502 TBMT on D1");
    assert!(bus.debugger_inject_serial_rx(0x01, b'A'));
    assert_eq!(bus.peek_io_port(0x00) & 0xa3, 0xa3, "COM2502 RDA/TBMT must not fabricate RIN/ROT device-ready state");
    assert!(bus.pulse_sio_input_device_ready());
    assert_eq!(bus.peek_io_port(0x00) & 0x21, 0x20, "explicit RIN ready pulls D0 low while RDA remains independently high");
    assert_eq!(bus.sio_handshake_lines(), Some((true, false, true, false)));
    assert_eq!(bus.debugger_input_port(0x01), b'A');
    assert_eq!(bus.peek_io_port(0x00) & 0x21, 0x01, "DATA IN clears RDA and the Rev0 input-ready latch");
    assert_eq!(bus.sio_handshake_lines(), Some((false, false, false, false)));
}

#[test]
fn rev0_external_ready_routes_to_vi_then_data_cycles_clear_it_at_public_boundary() {
    let mut bus = AltairBus::default();
    bus.configure_sio_hardware(SioHardwareConfig {
        revision: SioRevision::Rev0,
        ..SioHardwareConfig::default()
    });
    bus.configure_sio_interrupt_wiring(SioInterruptWiring {
        input: SioInterruptTarget::Vi3,
        output: SioInterruptTarget::Vi4,
    });
    bus.debugger_output_port(0x00, 0x03);

    assert!(bus.pulse_sio_input_device_ready());
    assert_eq!(bus.sio_vector_interrupt_requests(), 1 << 3);
    let _ = bus.debugger_input_port(0x01);
    assert_eq!(bus.sio_vector_interrupt_requests(), 0);

    assert!(bus.pulse_sio_output_device_ready());
    assert_eq!(bus.sio_vector_interrupt_requests(), 1 << 4);
    bus.debugger_output_port(0x01, b'O');
    assert_eq!(bus.sio_vector_interrupt_requests(), 0);
}

#[test]
fn com2502_overrun_overwrites_old_unread_byte_at_public_bus_boundary() {
    let mut bus = AltairBus::default();
    assert!(bus.debugger_inject_serial_rx(0x01, b'A'));
    assert!(bus.debugger_inject_serial_rx(0x01, b'B'));
    assert_eq!(bus.peek_io_port(0x00) & 0x10, 0x10);
    assert_eq!(bus.debugger_input_port(0x01), b'B');
}
