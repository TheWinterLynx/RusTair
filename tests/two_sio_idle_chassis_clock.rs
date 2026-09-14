use std::time::Duration;

use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{RamInit, S100HardwareConfig};

const CYCLE_HOST_SOURCE: &str = include_str!("../src/backend/cycle_host.rs");
const MACHINE_SOURCE: &str = include_str!("../src/machine/mod.rs");
const SERIAL_BUS_SOURCE: &str = include_str!("../src/machine/serial_bus.rs");
const RUNTIME_SOURCE: &str = include_str!("../src/s100_runtime.rs");
const PORT0_STATUS: u8 = 0x10;
const PORT0_DATA: u8 = 0x11;
const CONTROL_110_BAUD_8N2: u8 = 0x11;
const RDRF: u8 = 0x01;

fn machine() -> BackendHost {
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(
        S100HardwareConfig::historical_8800b_18_slot_starter()
            .validate()
            .unwrap(),
        RamInit::Zeroed,
    );
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
fn serial_and_dcdd_clock_domains_have_distinct_scheduler_owners() {
    let serial_elapsed = function_body(
        CYCLE_HOST_SOURCE,
        "fn advance_serial_physical_elapsed",
        "fn service_serial_wall_clock",
    );
    assert!(serial_elapsed.contains("advance_serial_hardware_time"));
    assert!(!serial_elapsed.contains("advance_chassis_hardware_time"));

    let serial_service = function_body(
        CYCLE_HOST_SOURCE,
        "fn service_serial_wall_clock",
        "fn service_idle_chassis_clock",
    );
    assert!(serial_service.contains("Instant::now"));
    assert!(serial_service.contains("advance_serial_physical_elapsed"));
    assert!(!serial_service.contains("advance_chassis_hardware_time"));

    let idle_chassis_service = function_body(
        CYCLE_HOST_SOURCE,
        "fn service_idle_chassis_clock",
        "fn invalidate_partial_trace_for_external_memory_change",
    );
    assert!(idle_chassis_service.contains("advance_chassis_hardware_time"));
    assert!(!idle_chassis_service.contains("advance_serial_hardware_time"));

    let cpu_t_state = function_body(
        MACHINE_SOURCE,
        "pub(crate) fn drive_cpu_board_sample",
        "fn refresh_protect_line",
    );
    assert!(cpu_t_state.contains("advance_dcdd_time(1)"));
    assert!(!cpu_t_state.contains("advance_serial_time"));
    assert!(!cpu_t_state.contains("advance_serial_hardware_time"));

    let chassis_time = function_body(
        SERIAL_BUS_SOURCE,
        "pub(crate) fn advance_chassis_hardware_time",
        "pub(crate) fn advance_serial_hardware_time",
    );
    assert!(chassis_time.contains("advance_dcdd_time"));
    assert!(!chassis_time.contains("advance_serial_time"));

    let serial_time = function_body(
        SERIAL_BUS_SOURCE,
        "pub(crate) fn advance_serial_hardware_time",
        "pub fn serial_port1_receive",
    );
    assert!(serial_time.contains("advance_serial_time"));
    assert!(!serial_time.contains("advance_dcdd_time"));

    let fabric_serial = function_body(
        RUNTIME_SOURCE,
        "pub(crate) fn advance_serial_time",
        "pub(crate) fn advance_dcdd_time",
    );
    assert!(fabric_serial.contains("installed.handle.advance_t_states"));
    assert!(!fabric_serial.contains("advance_mechanics_t_states"));
}

#[test]
fn stopped_cpu_does_not_freeze_independent_88_2sio_baud_clock() {
    let mut machine = machine();
    assert!(!machine.running());

    machine.serial_receive(BackendSerialPort::Port0, b'S');
    assert_frame_still_shifting(&mut machine);

    // 110 baud, 8N2 is exactly 100 ms per character. The physical serial
    // oscillator continues while the CPU is STOPped; no guest T-state is needed.
    std::thread::sleep(Duration::from_millis(115));
    assert_frame_reached_rdr(&mut machine, b'S');
}

#[test]
fn reset_held_does_not_freeze_independent_88_2sio_baud_clock() {
    let mut machine = machine();
    machine.set_running(true);
    machine.assert_front_panel_reset();
    machine.serial_receive(BackendSerialPort::Port0, b'R');
    assert_frame_still_shifting(&mut machine);

    std::thread::sleep(Duration::from_millis(115));
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
    std::thread::sleep(Duration::from_millis(115));
    assert_frame_reached_rdr(&mut machine, b'H');

    machine.request_hold(false);
}

#[test]
fn running_cpu_is_not_required_to_clock_88_2sio_serial_frames() {
    let mut machine = machine();
    machine.set_running(true);
    machine.serial_receive(BackendSerialPort::Port0, b'C');
    assert_frame_still_shifting(&mut machine);

    // Deliberately execute no CPU cycles. RUN may be asserted, but the selected
    // 110-baud oscillator still completes one physical 100 ms frame on its own.
    std::thread::sleep(Duration::from_millis(115));
    assert_frame_reached_rdr(&mut machine, b'C');
}
