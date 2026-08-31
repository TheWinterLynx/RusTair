//! Architecture guards for the physical External COM <-> 88-2SIO signal bridge.
//!
//! These tests intentionally protect the electrical boundary rather than a
//! particular UI layout. The actual MC6850 pin/status semantics are covered by
//! `two_sio_modem_pins`; this file prevents the host COM adapter from silently
//! reintroducing wrong polarity or byte-encoded BREAK behavior.

const CONFIG: &str = include_str!("../src/config/external_com.rs");
const COM_TRANSPORT: &str = include_str!("../src/io/com_serial.rs");
const COM_APP: &str = include_str!("../src/app/external_com.rs");

#[test]
fn unconnected_modem_inputs_use_the_mits_grounded_wiring() {
    assert!(CONFIG.contains("ComModemInputMode"));
    assert!(CONFIG.contains("Grounded"));
    assert!(CONFIG.contains("HostPins"));
    assert!(CONFIG.contains("modem_inputs: ComModemInputMode::Grounded"));
}

#[test]
fn real_com_transport_uses_out_of_band_break_and_reads_real_modem_pins() {
    assert!(COM_TRANSPORT.contains("WorkerCommand::SetBreak"));
    assert!(COM_TRANSPORT.contains("port.set_break()"));
    assert!(COM_TRANSPORT.contains("port.clear_break()"));
    assert!(COM_TRANSPORT.contains("port.read_clear_to_send()"));
    assert!(COM_TRANSPORT.contains("port.read_carrier_detect()"));

    assert!(
        !COM_TRANSPORT.contains("Write(0x00) // BREAK")
            && !COM_TRANSPORT.contains("Write(0x00), // BREAK"),
        "BREAK must remain an electrical condition, never a fabricated data byte",
    );
}

#[test]
fn host_assertion_is_inverted_at_the_active_low_mc6850_boundary() {
    assert!(COM_APP.contains("mc6850_active_low_pin_high"));
    assert!(COM_APP.contains("!host_asserted"));
    assert!(COM_APP.contains("modem_pins_asserted()"));
    assert!(COM_APP.contains("serial_set_modem_inputs_at(connection, cts_high, dcd_high)"));
}

#[test]
fn moving_or_disconnectng_the_com_cable_cannot_leave_stale_high_inputs() {
    assert!(COM_APP.contains("previous_connection != connection"));
    assert!(COM_APP.contains("serial_set_modem_inputs_at(previous_connection, false, false)"));
}