use rustair::backend::{BackendHost, EmulationEngine};
use rustair::config::{RamInit, RamSize, SerialBoard};

fn for_each_builtin_rust_backend(mut test: impl FnMut(&mut BackendHost)) {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
        host.configure_memory(RamSize::K1, RamInit::Zeroed);
        test(&mut host);
    }
}

#[test]
fn changing_serial_board_while_powered_resets_and_stops_both_rust_backends() {
    for_each_builtin_rust_backend(|host| {
        host.power(true);
        host.front_panel_reset();
        host.load_bytes(0, &[0x00, 0x00]);
        host.step();
        assert_eq!(host.intel8080_state().pc, 1);

        host.set_running(true);
        assert!(host.running());
        host.configure_serial_board(SerialBoard::TwoSio88);

        assert_eq!(host.serial_board(), SerialBoard::TwoSio88);
        assert!(!host.running(), "serial card replacement must leave the chassis stopped");
        assert_eq!(host.intel8080_state().pc, 0, "serial card replacement must reset the active CPU core");
    });
}

#[test]
fn selecting_the_already_installed_serial_board_is_a_noop() {
    for_each_builtin_rust_backend(|host| {
        host.power(true);
        host.front_panel_reset();
        host.load_bytes(0, &[0x00, 0x00]);
        host.step();
        assert_eq!(host.intel8080_state().pc, 1);

        host.set_running(true);
        host.configure_serial_board(SerialBoard::Sio88);

        assert_eq!(host.serial_board(), SerialBoard::Sio88);
        assert!(host.running(), "re-selecting the same card must not disturb RUN");
        assert_eq!(host.intel8080_state().pc, 1, "re-selecting the same card must not reset the CPU");
    });
}
