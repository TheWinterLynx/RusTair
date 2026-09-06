use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{RamInit, RamSize, SerialBoard};

const CYCLE_HOST_SOURCE: &str = include_str!("../src/backend/cycle_host.rs");
const CHASSIS_SOURCE: &str = include_str!("../src/machine/chassis.rs");
const PORT0_STATUS: u8 = 0x10;
const PORT0_DATA: u8 = 0x11;
const CONTROL_110_BAUD_8N2: u8 = 0x11;
const RDRF: u8 = 0x01;

fn machine() -> BackendHost {
    let mut machine = BackendHost::default();
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

fn function_body<'a>(source: &'a str, start: &str, next: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing function boundary {start}"));
    let tail = &source[start..];
    let end = tail
        .find(next)
        .unwrap_or_else(|| panic!("missing following function boundary {next}"));
    &tail[..end]
}

#[test]
fn idle_serial_wall_clock_has_one_scheduler_owner() {
    let host_service = function_body(
        CYCLE_HOST_SOURCE,
        "fn service_idle_chassis_clock",
        "fn invalidate_partial_trace_for_external_memory_change",
    );
    assert!(host_service.contains("let covered = self.last_panel_commit_cpu_t_states"));
    assert!(host_service.contains("let parked = powered && (!running || reset || self.inner.cpu().is_holding())"));
    assert!(host_service.contains("due.saturating_sub(covered)"));
    assert!(host_service.contains("advance_serial_hardware_time(missing)"));

    let host_commit = function_body(
        CYCLE_HOST_SOURCE,
        "fn commit_panel_activity",
        "fn assert_run_stop",
    );
    assert!(host_commit.contains("self.service_idle_chassis_clock(dt)"));
    assert!(host_commit.contains("self.inner.commit_panel_activity(dt)"));

    let chassis_commit = function_body(
        CHASSIS_SOURCE,
        "fn cycle_commit_panel_activity",
        "fn cycle_front_panel_set_memory_protection",
    );
    assert!(chassis_commit.contains("self.bus.commit_panel_activity(dt, dynamic)"));
    assert!(!chassis_commit.contains("advance_serial_hardware_time"));
    assert!(!chassis_commit.contains("CLOCK_HZ"));
}

#[test]
fn stopped_cpu_does_not_freeze_independent_88_2sio_baud_clock() {
    let mut machine = machine();
    assert!(!machine.running());

    machine.serial_receive(BackendSerialPort::Port0, b'S');
    assert_frame_still_shifting(&mut machine);

    // Historical bootstrap control 11h is /16, 8N2. At the Port-0 110-baud
    // strap that is exactly 11 bits / 110 bit/s = 100 ms per character.
    machine.commit_panel_activity(Duration::from_millis(99));
    assert_eq!(
        machine.peek_io_port(PORT0_STATUS) & RDRF,
        0,
        "Adaptive Cycle completed a 110-baud frame too early while STOPped"
    );
    machine.commit_panel_activity(Duration::from_millis(1));
    assert_frame_reached_rdr(&mut machine, b'S');
}

#[test]
fn reset_held_does_not_freeze_independent_88_2sio_baud_clock() {
    let mut machine = machine();
    machine.set_running(true);
    machine.assert_front_panel_reset();
    machine.serial_receive(BackendSerialPort::Port0, b'R');
    assert_frame_still_shifting(&mut machine);

    machine.commit_panel_activity(Duration::from_millis(100));
    assert_frame_reached_rdr(&mut machine, b'R');
    machine.release_front_panel_reset();
}

#[test]
fn hold_hlda_does_not_freeze_independent_88_2sio_baud_clock() {
    let mut machine = machine();
    machine.set_running(true);
    machine.request_hold(true);
    machine.run_cycles(32);

    machine.serial_receive(BackendSerialPort::Port0, b'H');
    assert_frame_still_shifting(&mut machine);
    machine.commit_panel_activity(Duration::from_millis(100));
    assert_frame_reached_rdr(&mut machine, b'H');

    machine.request_hold(false);
}

#[test]
fn running_cpu_t_states_remain_the_only_88_2sio_clock_source_during_run() {
    let mut machine = machine();
    machine.set_running(true);
    machine.serial_receive(BackendSerialPort::Port0, b'C');
    assert_frame_still_shifting(&mut machine);

    // A visual/wall-clock commit must not double-count the card clock while
    // RUN is active. CPU execution below is the authority in this state.
    machine.commit_panel_activity(Duration::from_millis(100));
    assert_eq!(
        machine.peek_io_port(PORT0_STATUS) & RDRF,
        0,
        "Adaptive Cycle advanced the 88-2SIO from both RUN T-states and wall time"
    );

    machine.run_cycles(200_000);
    assert_frame_reached_rdr(&mut machine, b'C');
}
