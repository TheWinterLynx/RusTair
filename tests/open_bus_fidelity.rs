use rustair::backend::{BackendHost, BusTState, EmulationEngine};
use rustair::config::{RamInit, RamSize, SerialBoard};
use rustair::cpu8080::Bus;
use rustair::machine::AltairBus;

const OPEN_BUS: u8 = 0xff;

#[test]
fn uninstalled_memory_is_absent_to_debugger_but_ff_to_guest() {
    let mut bus = AltairBus::default();
    bus.configure_memory(RamSize::Bytes256, RamInit::Zeroed);

    assert_eq!(bus.peek_memory(0x00ff), Some(0x00));
    assert_eq!(bus.peek_memory(0x0100), None);
    assert_eq!(bus.read(0x0100), OPEN_BUS);
    assert_eq!(bus.opcode_fetch(0x0100), OPEN_BUS);
    assert_eq!(bus.stack_read(0x0100), OPEN_BUS);
}

#[test]
fn writes_into_uninstalled_memory_do_not_create_ram_or_change_open_bus() {
    let mut bus = AltairBus::default();
    bus.configure_memory(RamSize::Bytes256, RamInit::Zeroed);

    bus.write(0x0100, 0x5a);
    assert_eq!(bus.peek_memory(0x0100), None);
    assert_eq!(bus.read(0x0100), OPEN_BUS);
}

#[test]
fn cycle_exact_t2_exposes_open_bus_on_s100_di_before_cpu_samples_t3() {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("built-in Cycle backend");
    host.configure_memory(RamSize::Bytes256, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, &[0xc3, 0x00, 0x01]); // JMP 0100h, outside installed RAM.

    host.debugger_step_instruction();
    assert_eq!(host.intel8080_state().pc, 0x0100);
    assert_eq!(host.peek_memory(0x0100), None);

    host.debugger_step_t_state(); // opcode fetch T1 at 0100h
    host.debugger_step_t_state(); // T2: external DI source is already visible
    let t2 = host.bus_teaching_snapshot().expect("exact T2 sample");

    assert_eq!(t2.t_state, BusTState::T2);
    assert_eq!(t2.address, Some(0x0100));
    assert_eq!(t2.s100_di, Some(OPEN_BUS));
    assert_eq!(t2.panel_data, Some(OPEN_BUS));

    host.debugger_step_t_state(); // T3: the 8080 samples the same released bus value
    let t3 = host.bus_teaching_snapshot().expect("exact T3 sample");
    assert_eq!(t3.t_state, BusTState::T3);
    assert_eq!(t3.s100_di, Some(OPEN_BUS));
    assert_eq!(t3.cpu_data, Some(OPEN_BUS));
}

#[test]
fn unmapped_io_reads_ff_with_88_sio_installed() {
    let mut bus = AltairBus::default();
    bus.configure_serial_board(SerialBoard::Sio88);

    assert_eq!(bus.input(0x10), OPEN_BUS);
    assert_eq!(bus.input(0x11), OPEN_BUS);
    assert_eq!(bus.input(0x12), OPEN_BUS);
    assert_eq!(bus.input(0x13), OPEN_BUS);
    assert_eq!(bus.input(0x7e), OPEN_BUS);
}

#[test]
fn unmapped_io_reads_ff_with_88_2sio_installed() {
    let mut bus = AltairBus::default();
    bus.configure_serial_board(SerialBoard::TwoSio88);

    assert_eq!(bus.input(0x00), OPEN_BUS);
    assert_eq!(bus.input(0x01), OPEN_BUS);
    assert_eq!(bus.input(0x7e), OPEN_BUS);
}

#[test]
fn open_bus_does_not_override_a_responding_device() {
    let mut bus = AltairBus::default();
    bus.configure_serial_board(SerialBoard::TwoSio88);

    // A selected 6850 responds at its status port. With no received character
    // and an empty transmitter the currently modelled status is TDRE only.
    assert_eq!(bus.input(0x10), 0x02);
    assert_eq!(bus.input(0x12), 0x02);
}
