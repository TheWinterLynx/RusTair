use rustair::backend::{CpuState, CycleAccurateMachineBackend, Intel8080State, MachineBackend};
use rustair::config::{RamInit, S100HardwareConfig, S100InstalledCardConfig};
use rustair::machine::AltairChassis;
use rustair::s100_chassis::S100ChassisConfig;
use rustair::s100_memory::{S100RamBoardModel, S100RamCardConfig};

fn intel_state<B: MachineBackend>(backend: &mut B) -> Intel8080State {
    let CpuState::Intel8080(state) = backend.cpu_state().expect("CPU state must be available");
    state
}

fn static_4k_hardware() -> S100HardwareConfig {
    let mut hardware = S100HardwareConfig::empty(S100ChassisConfig::original_8800(1)).unwrap();
    hardware
        .set_slot(1, Some(S100InstalledCardConfig::Mits8080Cpu))
        .unwrap();
    hardware
        .set_slot(
            2,
            Some(S100InstalledCardConfig::Ram(S100RamCardConfig::fully_populated(
                S100RamBoardModel::Mits4KStatic88_4Mcs,
                0,
            ))),
        )
        .unwrap();
    hardware.validate().unwrap()
}

fn cycle_step_machine_cycle(backend: &mut CycleAccurateMachineBackend) {
    let start_cycle = backend.cpu().machine_cycle();
    let start_index = backend.cpu().machine_cycle_index();
    let start_t_states = backend.cpu().total_t_states();
    backend.run().expect("cycle backend must run");

    for _ in 0..32 {
        backend
            .service_execution(1)
            .expect("cycle T-state service must succeed");
        if backend.cpu().total_t_states() > start_t_states
            && (backend.cpu().machine_cycle() != start_cycle
                || backend.cpu().machine_cycle_index() != start_index
                || backend.cpu().is_halted()
                || backend.cpu().is_holding())
        {
            backend.halt().expect("logical machine-cycle stop must succeed");
            return;
        }
    }

    backend.halt().expect("logical machine-cycle stop must succeed");
    panic!("cycle backend did not finish one machine cycle");
}

#[test]
fn cycle_backend_physically_owns_cpu_free_altair_chassis() {
    let mut backend = CycleAccurateMachineBackend::default();
    let _: &AltairChassis = backend.machine();

    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x3e, 0x5a]).unwrap(); // MVI A,5Ah

    let authoritative_a = backend.cpu().registers().a;
    cycle_step_machine_cycle(&mut backend); // M1 fetch only.
    assert_eq!(backend.cpu().registers().pc, 1);
    assert_eq!(backend.cpu().registers().a, authoritative_a);
    assert_eq!(backend.cpu().total_t_states(), 4);

    cycle_step_machine_cycle(&mut backend); // M2 operand read completes MVI.
    assert_eq!(backend.cpu().registers().pc, 2);
    assert_eq!(backend.cpu().registers().a, 0x5a);
    assert_eq!(backend.cpu().total_t_states(), 7);
}

#[test]
fn cycle_chassis_controls_use_exact_cpu_and_physical_bus_state() {
    let mut backend = CycleAccurateMachineBackend::default();
    let _: &AltairChassis = backend.machine();

    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    let reset_panel = backend.front_panel_state().unwrap();
    assert_eq!(reset_panel.address, 0xffff);
    assert_eq!(reset_panel.data, 0xff);
    assert_eq!(backend.cpu().registers().pc, 0);
    backend.release_reset().unwrap();
    assert_eq!(backend.front_panel_state().unwrap().address, 0x0000);

    backend.load_bytes(0x0123, &[0xa5, 0x5a]).unwrap();
    backend.set_switch_register(0x0123).unwrap();
    backend.panel_examine(false).unwrap();
    assert_eq!(backend.cpu().registers().pc, 0x0123);
    assert_eq!(backend.machine().address_leds(), 0x0123);
    assert_eq!(backend.machine().data_leds(), 0xa5);

    backend.set_switch_register(0x005a).unwrap();
    backend.panel_deposit(false).unwrap();
    assert_eq!(backend.peek_memory(0x0123).unwrap(), Some(0x5a));
    assert_eq!(backend.cpu().registers().pc, 0x0123);

    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x00, 0x00]).unwrap();
    backend.run().unwrap();
    backend.request_hold(true).unwrap();
    backend.service_execution(5).unwrap();
    assert!(backend.cpu().is_holding());
    backend
        .commit_panel_activity(std::time::Duration::from_millis(16))
        .unwrap();
    assert_eq!(backend.front_panel_state().unwrap().lamps.hlda, 1.0);

    backend.request_hold(false).unwrap();
    backend.service_execution(1).unwrap();
    assert!(!backend.cpu().is_holding());
    backend.service_execution(1).unwrap();
    backend.halt().unwrap();
    backend
        .commit_panel_activity(std::time::Duration::from_millis(16))
        .unwrap();
    assert_eq!(backend.front_panel_state().unwrap().lamps.hlda, 0.0);

    backend.power(false).unwrap();
    assert!(!backend.machine().powered);
}

#[test]
fn adaptive_cycle_matches_forced_partial_oracle_for_same_t_state_budget() {
    // The prefix deterministically initializes every RESET-undefined register and
    // flags before entering a stable memory/ALU/branch loop. Both machines own
    // independent physical S-100 RAM cards; only the execution strategy differs.
    let program = [
        0x31, 0x00, 0x03, // LXI SP,0300h
        0x01, 0x00, 0x00, // LXI B,0000h
        0x11, 0x00, 0x00, // LXI D,0000h
        0x21, 0x00, 0x02, // LXI H,0200h
        0xaf,             // XRA A
        0x3e, 0x12,       // MVI A,12h
        0x06, 0x34,       // MVI B,34h
        0x80,             // ADD B
        0x77,             // MOV M,A
        0x4e,             // MOV C,M
        0x0c,             // INR C
        0x79,             // MOV A,C
        0xc3, 0x13, 0x00, // JMP 0013h (MOV C,M)
    ];
    const BUDGET: u32 = 14_000;

    let mut adaptive = CycleAccurateMachineBackend::default();
    let mut partial = CycleAccurateMachineBackend::default();
    for backend in [&mut adaptive, &mut partial] {
        backend
            .machine_mut()
            .bus
            .configure_s100_hardware_memory(static_4k_hardware(), RamInit::Zeroed)
            .unwrap();
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.load_bytes(0, &program).unwrap();
        backend.release_reset().unwrap();
        backend.run().unwrap();
    }

    adaptive.service_execution(BUDGET).unwrap();
    for _ in 0..BUDGET {
        partial.service_execution(1).unwrap();
    }

    assert_eq!(adaptive.cpu().total_t_states(), u64::from(BUDGET));
    assert_eq!(partial.cpu().total_t_states(), u64::from(BUDGET));
    assert_eq!(intel_state(&mut adaptive), intel_state(&mut partial));
    assert_eq!(adaptive.peek_memory(0x0200).unwrap(), partial.peek_memory(0x0200).unwrap());
    assert_eq!(
        adaptive.machine().bus.raw_panel_lamp_duty(),
        partial.machine().bus.raw_panel_lamp_duty(),
        "Adaptive Full/Partial dispatch must be invisible to the front panel"
    );
}
