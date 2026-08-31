use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort, EmulationEngine};
use rustair::config::{RamInit, RamSize, SerialBoard};

const PORT0_STATUS: u8 = 0x10;
const PORT0_DATA: u8 = 0x11;
const CONTROL_110_BAUD_8N2: u8 = 0x11;
const RDRF: u8 = 0x01;

fn machine(engine: EmulationEngine) -> BackendHost {
    let mut machine = BackendHost::from_engine(engine).expect("built-in Rust backend");
    machine.configure_memory(RamSize::K1, RamInit::Zeroed);
    machine.configure_serial_board(SerialBoard::TwoSio88);
    machine.power(true);
    machine.assert_front_panel_reset();
    machine.release_front_panel_reset();
    machine.debugger_output_port(PORT0_STATUS, CONTROL_110_BAUD_8N2);
    machine
}

fn assert_frame_still_shifting(machine: &mut BackendHost) {
    assert_eq!(machine.peek_io_port(PORT0_STATUS) & RDRF, 0);
    assert!(!machine.serial_rx_empty(BackendSerialPort::Port0));
}

fn assert_frame_reached_rdr(machine: &mut BackendHost, expected: u8) {
    assert_eq!(machine.peek_io_port(PORT0_STATUS) & RDRF, RDRF);
    assert_eq!(machine.peek_io_port(PORT0_DATA), expected);
}

#[test]
fn stopped_cpu_does_not_freeze_independent_88_2sio_baud_clock() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut machine = machine(engine);
        assert!(!machine.running());

        machine.serial_receive(BackendSerialPort::Port0, b'S');
        assert_frame_still_shifting(&mut machine);

        // Historical bootstrap control 11h is /16, 8N2. At the Port-0 110-baud
        // strap that is exactly 11 bits / 110 bit/s = 100 ms per character.
        machine.commit_panel_activity(Duration::from_millis(99));
        assert_eq!(machine.peek_io_port(PORT0_STATUS) & RDRF, 0, "{engine:?} completed a 110-baud frame too early while STOPped");
        machine.commit_panel_activity(Duration::from_millis(1));
        assert_frame_reached_rdr(&mut machine, b'S');
    }
}

#[test]
fn reset_held_does_not_freeze_independent_88_2sio_baud_clock() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut machine = machine(engine);
        machine.set_running(true);
        machine.assert_front_panel_reset();
        machine.serial_receive(BackendSerialPort::Port0, b'R');
        assert_frame_still_shifting(&mut machine);

        machine.commit_panel_activity(Duration::from_millis(100));
        assert_frame_reached_rdr(&mut machine, b'R');
        machine.release_front_panel_reset();
    }
}

#[test]
fn hold_hlda_does_not_freeze_independent_88_2sio_baud_clock() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut machine = machine(engine);
        machine.set_running(true);
        machine.request_hold(true);
        machine.run_cycles(32);

        machine.serial_receive(BackendSerialPort::Port0, b'H');
        assert_frame_still_shifting(&mut machine);
        machine.commit_panel_activity(Duration::from_millis(100));
        assert_frame_reached_rdr(&mut machine, b'H');

        machine.request_hold(false);
    }
}

#[test]
fn running_cpu_t_states_remain_the_only_88_2sio_clock_source_during_run() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut machine = machine(engine);
        machine.set_running(true);
        machine.serial_receive(BackendSerialPort::Port0, b'C');
        assert_frame_still_shifting(&mut machine);

        // A visual/wall-clock commit must not double-count the card clock while
        // RUN is active. CPU execution below is the authority in this state.
        machine.commit_panel_activity(Duration::from_millis(100));
        assert_eq!(machine.peek_io_port(PORT0_STATUS) & RDRF, 0, "{engine:?} advanced the 88-2SIO from both RUN T-states and wall time");

        machine.run_cycles(200_000);
        assert_frame_reached_rdr(&mut machine, b'C');
    }
}
