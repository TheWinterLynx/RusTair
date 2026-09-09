const SERIAL_HARDWARE: &str = include_str!("../src/app/serial_hardware.rs");
const HISTORY: &str = include_str!("../src/app/ui/instruction_history.rs");
const IO_INSPECTOR: &str = include_str!("../src/app/ui/io_inspector.rs");

#[test]
fn serial_reader_ui_uses_physical_s100_inventory_not_global_board_selector() {
    for (name, source) in [("history", HISTORY), ("I/O inspector", IO_INSPECTOR)] {
        assert!(
            !source.contains("config.machine.serial_board"),
            "{name} must resolve I/O labels from installed S-100 cards, not the legacy global serial selector"
        );
        assert!(
            source.contains("physical_serial_port_bindings"),
            "{name} must use the shared physical serial decode helper"
        );
    }
}

#[test]
fn shared_serial_decode_walks_all_physical_serial_slots() {
    assert!(SERIAL_HARDWARE.contains("s100_hardware.serial_slots()"));
    assert!(SERIAL_HARDWARE.contains("Mits88Sio"));
    assert!(SERIAL_HARDWARE.contains("Mits88TwoSio"));
    assert!(SERIAL_HARDWARE.contains("port0_status"));
    assert!(SERIAL_HARDWARE.contains("port1_data"));
}

#[test]
fn io_inspector_keeps_contention_visible_and_ambiguous_uart_tools_disabled() {
    assert!(IO_INSPECTOR.contains("CONTENTION:"));
    assert!(IO_INSPECTOR.contains("bindings.len() > 1"));
    assert!(IO_INSPECTOR.contains("bindings.len() == 1"));
    assert!(IO_INSPECTOR.contains("first_physical_serial_data_port"));
}
