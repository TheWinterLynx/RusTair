use rustair::backend::{
    BackendHost, BackendSerialPort, BusMachineCycle, BusTState, EmulationEngine,
};
use rustair::config::{RamInit, S100HardwareConfig};

fn physical_two_sio_host(program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080).unwrap();
    let hardware = S100HardwareConfig::historical_8800b_18_slot_starter()
        .validate()
        .unwrap();
    host.configure_s100_hardware(hardware, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host.set_running(true);
    host
}

#[test]
fn cycle_cpu_consumes_the_byte_injected_into_the_installed_serial_card() {
    let mut host = physical_two_sio_host(&[0xdb, 0x11]);
    assert!(host.debugger_inject_serial_rx(0x11, b'P'));
    assert!(!host.serial_rx_empty(BackendSerialPort::Port0));

    host.run_cycles(11);

    assert_eq!(host.intel8080_state().a, b'P');
    assert!(host.serial_rx_empty(BackendSerialPort::Port0));
}

#[test]
fn installed_two_sio_generates_its_one_tw_through_physical_prdy() {
    let mut host = physical_two_sio_host(&[0xdb, 0x10]);
    let mut samples = Vec::new();
    for _ in 0..11 {
        host.run_cycles(1);
        if let Some(sample) = host.bus_teaching_snapshot() {
            if sample.machine_cycle == BusMachineCycle::InputRead {
                samples.push(sample);
            }
        }
    }

    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.t_state)
            .collect::<Vec<_>>(),
        vec![BusTState::T1, BusTState::T2, BusTState::Tw, BusTState::T3]
    );
    assert_eq!(samples[0].ready, Some(true));
    assert_eq!(samples[1].ready, Some(false));
    assert_eq!(samples[2].ready, Some(true));
    assert_eq!(samples[3].ready, Some(true));
}
