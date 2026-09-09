//! Release-mode reproduction of the GUI's long execution batch and retained output.
use std::time::Instant;

use rustair::adaptive_metrics;
use rustair::backend::{BackendHost, BackendSerialPort};
use rustair::config::{FastRamCompatibilityConfig, RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

fn machine(compatibility: bool, output: bool) -> BackendHost {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(4)).unwrap();
    hardware.set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu)).unwrap();
    hardware.set_slot(2, Some(if compatibility {
        S100InstalledCardConfig::FastRamCompatibility(FastRamCompatibilityConfig::no_wait(0, 32768))
    } else {
        S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(S100RamBoardModel::Mits16KStatic88_16Mcs, 0))
    })).unwrap();
    hardware.set_slot(3, Some(S100InstalledCardConfig::Mits88TwoSio {
        straps: Default::default(), interrupt_wiring: Default::default(),
    })).unwrap();
    let mut machine = BackendHost::default();
    machine.configure_s100_hardware(hardware, RamInit::Zeroed);
    machine.power(true);
    machine.set_running(false);
    machine.reset();
    // Program the real UART and emit A, then stay in a compute loop. Keeping
    // the completed byte models an endpoint waiting for its next GUI update.
    let mut program = vec![0x31, 0, 0x0f, 0x01, 0, 0, 0x11, 0, 0, 0x21, 0, 0, 0xaf];
    program.extend_from_slice(if output {
        &[0x3e, 0x15, 0xd3, 0x12, 0x3e, b'A', 0xd3, 0x13, 0x00, 0xc3, 21, 0x00]
    } else {
        &[0x00, 0xc3, 13, 0x00]
    });
    machine.load_bytes(0, &program);
    machine.set_running(true);
    machine
}

#[test]
fn retained_output_has_identical_cpu_and_uart_results_in_full_and_observed_partial() {
    for compatibility in [false, true] {
        let mut full = machine(compatibility, true);
        let mut partial = machine(compatibility, true);
        partial.set_instruction_trace_enabled(true);
        for budget in [3_000, 7_003, 40_000] {
            full.run_cycles(budget);
            partial.run_cycles(budget);
            assert_eq!(full.intel8080_state(), partial.intel8080_state());
            assert_eq!(full.serial_tx_front(BackendSerialPort::Port1), Some(b'A'));
            assert_eq!(partial.serial_tx_front(BackendSerialPort::Port1), Some(b'A'));
        }
        assert_eq!(full.serial_tx_complete(BackendSerialPort::Port1), partial.serial_tx_complete(BackendSerialPort::Port1));
        assert_eq!(full.serial_tx_complete(BackendSerialPort::Port1), None);
        assert_eq!(partial.serial_tx_complete(BackendSerialPort::Port1), None);
    }
}

#[test]
fn gui_batch_with_migrated_ram_and_pending_console_output() {
    for compatibility in [false, true] {
        for output in [false, true] {
            let mut machine = machine(compatibility, output);
            let before = machine.intel8080_state().total_t_states.unwrap();
            adaptive_metrics::begin_measurement();
            let start = Instant::now();
            machine.run_cycles(1_000_000);
            let elapsed = start.elapsed();
            let stats = adaptive_metrics::end_measurement();
            assert_eq!(machine.intel8080_state().total_t_states.unwrap() - before, 1_000_000);
            assert_eq!(stats.total_t_states(), 1_000_000);
            assert!(stats.full_percent() > 99.0, "completed host output and zero-wait migration RAM must not prevent Full");
            assert_eq!(machine.serial_tx_complete(BackendSerialPort::Port1), output.then_some(b'A'));
            assert_eq!(machine.serial_tx_complete(BackendSerialPort::Port1), None);
            eprintln!("[GUI BATCH] compatibility={compatibility} retained_output={output}: {elapsed:.3?}, Full={:.2}% Partial={:.2}%", stats.full_percent(), stats.partial_percent());
        }
    }
}
