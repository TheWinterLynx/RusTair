use rustair::backend::CycleAccurateMachineBackend;
use rustair::config::SerialBoard;
use rustair::machine::AltairBus;

const PORT0_CONTROL: u8 = 0x10;
const PORT0_STATUS: u8 = 0x10;

fn exercise_modem_pin_contract(bus: &mut AltairBus) {
    assert_eq!(bus.serial_modem_lines(0), None, "88-SIO must not fabricate MC6850 modem pins");
    assert!(!bus.set_serial_modem_inputs(0, true, true));

    bus.configure_serial_board(SerialBoard::TwoSio88);
    assert_eq!(bus.serial_modem_lines(0), Some((false, false, false, false)));
    assert_eq!(bus.serial_modem_lines(1), Some((false, false, false, false)));
    assert_eq!(bus.serial_modem_lines(2), None);

    // Historical 88-TYA documentation names these exact values: 021 octal
    // (11h) keeps Reader Run/RTS low; 121 octal (51h) raises physical RTS.
    bus.debugger_output_port(PORT0_CONTROL, 0x11);
    assert_eq!(bus.serial_modem_lines(0), Some((false, false, false, false)));
    bus.debugger_output_port(PORT0_CONTROL, 0x51);
    assert_eq!(bus.serial_modem_lines(0), Some((true, false, false, false)));

    // CR6:CR5=11 is BREAK and must return RTS to physical LOW.
    bus.debugger_output_port(PORT0_CONTROL, 0x71);
    assert_eq!(bus.serial_modem_lines(0), Some((false, true, false, false)));

    // Restore RX interrupts and drive the external modem inputs. CTS and DCD
    // must be observable in the real status register, not host-side metadata.
    bus.debugger_output_port(PORT0_CONTROL, 0x91);
    assert!(bus.set_serial_modem_inputs(0, true, false));
    assert_eq!(bus.serial_modem_lines(0), Some((false, false, true, false)));
    assert_eq!(bus.peek_io_port(PORT0_STATUS) & 0x08, 0x08);

    assert!(bus.set_serial_modem_inputs(0, false, true));
    assert_eq!(bus.serial_modem_lines(0), Some((false, false, false, true)));
    assert_eq!(bus.peek_io_port(PORT0_STATUS) & 0x84, 0x84, "DCD high must project both status and enabled IRQ");

    // The DCD interrupt remains latched after the input returns low until the
    // documented status-read then data-read clearing sequence completes.
    assert!(bus.set_serial_modem_inputs(0, false, false));
    assert_eq!(bus.peek_io_port(PORT0_STATUS) & 0x84, 0x84);
    let _ = bus.debugger_input_port(PORT0_STATUS);
    let _ = bus.debugger_input_port(0x11);
    assert_eq!(bus.peek_io_port(PORT0_STATUS) & 0x84, 0);
}

#[test]
fn adaptive_cycle_exposes_the_physical_88_2sio_modem_pin_contract() {
    let mut cycle = CycleAccurateMachineBackend::default();
    exercise_modem_pin_contract(&mut cycle.machine_mut().bus);
}
