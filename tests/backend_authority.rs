use rustair::backend::{
    CpuState, CycleAccurateMachineBackend, Intel8080State, MachineBackend, NativeMachineBackend,
};
use rustair::machine::AltairChassis;

fn intel_state<B: MachineBackend>(backend: &mut B) -> Intel8080State {
    match backend.cpu_state().expect("CPU state must be available") {
        CpuState::Intel8080(state) => state,
        CpuState::Z80(_) => panic!("expected Intel 8080 backend"),
    }
}

/// Advance one exact 8080 machine cycle without using the Altair front-panel
/// SINGLE STEP sequencer. Differential/core-authority tests compare CPU timing,
/// so panel PSYNC/TW parking must not be counted as guest execution.
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

fn cycle_step_instruction(backend: &mut CycleAccurateMachineBackend) {
    let start_completed = backend.cpu().completed_instructions();
    let start_t_states = backend.cpu().total_t_states();
    backend.run().expect("cycle backend must run");

    for _ in 0..128 {
        backend
            .service_execution(1)
            .expect("cycle T-state service must succeed");
        if backend.cpu().completed_instructions() > start_completed
            || backend.cpu().is_halted()
            || backend.cpu().is_holding()
        {
            backend.halt().expect("logical instruction stop must succeed");
            assert!(
                backend.cpu().total_t_states() > start_t_states,
                "instruction step must consume at least one T-state"
            );
            return;
        }
    }

    backend.halt().expect("logical instruction stop must succeed");
    panic!("cycle backend did not reach the next instruction completion");
}

#[test]
fn fast_backend_uses_fast_machine_cpu_as_its_execution_authority() {
    let mut backend = NativeMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0x0100, &[0x3e, 0x5a]).unwrap(); // MVI A,5Ah

    {
        let cpu = &mut backend.machine_mut().cpu;
        cpu.pc = 0x0100;
        cpu.sp = 0x3456;
        cpu.a = 0x11;
    }

    let before = intel_state(&mut backend);
    assert_eq!(before.pc, 0x0100);
    assert_eq!(before.sp, 0x3456);
    assert_eq!(before.a, 0x11);

    backend.step().unwrap();
    let after = intel_state(&mut backend);
    assert_eq!(after.pc, 0x0102);
    assert_eq!(after.sp, 0x3456);
    assert_eq!(after.a, 0x5a);
    assert_eq!(backend.machine().cpu.pc, after.pc);
    assert_eq!(backend.machine().cpu.a, after.a);
}

#[test]
fn cycle_backend_physically_owns_a_cpu_free_altair_chassis() {
    let mut backend = CycleAccurateMachineBackend::default();
    let _: &AltairChassis = backend.machine();

    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x3e, 0x5a]).unwrap();

    let initial_a = backend.cpu().registers().a;
    cycle_step_machine_cycle(&mut backend);
    assert_eq!(backend.cpu().registers().pc, 1);
    assert_eq!(backend.cpu().registers().a, initial_a);
    assert_eq!(backend.cpu().total_t_states(), 4);

    cycle_step_machine_cycle(&mut backend);
    assert_eq!(backend.cpu().registers().pc, 2);
    assert_eq!(backend.cpu().registers().a, 0x5a);
    assert_eq!(backend.cpu().total_t_states(), 7);
}

#[test]
fn cycle_chassis_controls_are_driven_by_cycle_cpu_authority() {
    let mut backend = CycleAccurateMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();

    backend.load_bytes(0x0123, &[0xa5, 0x5a]).unwrap();
    backend.set_switch_register(0x0123).unwrap();
    backend.panel_examine(false).unwrap();
    assert_eq!(backend.cpu().registers().pc, 0x0123);
    assert_eq!(backend.machine().address_leds(), 0x0123);

    backend.set_switch_register(0x005a).unwrap();
    backend.panel_deposit(false).unwrap();
    assert_eq!(backend.peek_memory(0x0123).unwrap(), Some(0x5a));

    backend.request_hold(true).unwrap();
    backend.run().unwrap();
    backend.service_execution(5).unwrap();
    assert!(backend.cpu().is_holding());
    backend.request_hold(false).unwrap();
    backend.service_execution(1).unwrap();
    assert!(!backend.cpu().is_holding());
    backend.halt().unwrap();
}

#[test]
fn fast_and_cycle_backends_match_architectural_state_through_shared_chassis_path() {
    let program = [
        0x31, 0x00, 0xf0, // LXI SP,F000h
        0x01, 0x00, 0x00, // LXI B,0000h
        0x11, 0x00, 0x00, // LXI D,0000h
        0x21, 0x00, 0x00, // LXI H,0000h
        0xaf,             // XRA A
        0x3e, 0x12,       // MVI A,12h
        0x06, 0x34,       // MVI B,34h
        0x80,             // ADD B
        0x32, 0x00, 0x02, // STA 0200h
        0x21, 0x00, 0x02, // LXI H,0200h
        0x4e,             // MOV C,M
        0x0c,             // INR C
        0xfb,             // EI
        0x00,             // NOP
        0xf3,             // DI
        0x91,             // SUB C
        0x00,             // NOP
    ];
    const SETUP_INSTRUCTIONS: usize = 5;
    const WORKLOAD_INSTRUCTIONS: usize = 12;

    let mut fast = NativeMachineBackend::default();
    let mut cycle = CycleAccurateMachineBackend::default();

    for backend in [&mut fast as &mut dyn MachineBackend, &mut cycle as &mut dyn MachineBackend] {
        backend.power(true).unwrap();
        backend.assert_reset().unwrap();
        backend.release_reset().unwrap();
        backend.load_bytes(0, &program).unwrap();
        backend.load_bytes(0x0200, &[0xa5]).unwrap();
    }

    for _ in 0..SETUP_INSTRUCTIONS {
        fast.step().unwrap();
        cycle_step_instruction(&mut cycle);
    }

    assert_eq!(
        intel_state(&mut cycle),
        intel_state(&mut fast),
        "backends must agree once guest code initializes RESET-undefined state"
    );

    for instruction_index in 0..WORKLOAD_INSTRUCTIONS {
        fast.step().unwrap();
        cycle_step_instruction(&mut cycle);

        let fast_state = intel_state(&mut fast);
        let cycle_state = intel_state(&mut cycle);
        assert_eq!(
            cycle_state, fast_state,
            "architectural mismatch after workload instruction #{instruction_index}"
        );
        assert_eq!(
            cycle.peek_memory(0x0200).unwrap(),
            fast.peek_memory(0x0200).unwrap(),
            "memory mismatch after workload instruction #{instruction_index}"
        );
    }

    let expected_end = program.len() as u16;
    assert_eq!(intel_state(&mut fast).pc, expected_end);
    assert_eq!(intel_state(&mut cycle).pc, expected_end);
}
