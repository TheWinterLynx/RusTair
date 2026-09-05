use rustair::backend::BackendHost;
use rustair::config::{RamInit, RamSize, SerialBoard};

fn prepared() -> BackendHost {
    let mut host = BackendHost::default();
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host
}

#[test]
fn changing_serial_board_while_powered_resets_and_stops_adaptive_cycle() {
    let mut host = prepared();
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, &[0x00, 0x00]);
    host.step();
    assert_eq!(host.intel8080_state().pc, 1);

    host.set_running(true);
    assert!(host.running());
    host.configure_serial_board(SerialBoard::TwoSio88);

    assert_eq!(host.serial_board(), SerialBoard::TwoSio88);
    assert!(
        !host.running(),
        "serial card replacement must leave the chassis stopped"
    );
    assert_eq!(
        host.intel8080_state().pc,
        0,
        "serial card replacement must reset the active CPU core"
    );
}

#[test]
fn selecting_the_already_installed_serial_board_is_a_noop() {
    let mut host = prepared();
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, &[0x00, 0x00]);
    host.step();
    assert_eq!(host.intel8080_state().pc, 1);

    host.set_running(true);
    host.configure_serial_board(SerialBoard::Sio88);

    assert_eq!(host.serial_board(), SerialBoard::Sio88);
    assert!(
        host.running(),
        "re-selecting the same card must not disturb RUN"
    );
    assert_eq!(
        host.intel8080_state().pc,
        1,
        "re-selecting the same card must not reset the CPU"
    );
}
