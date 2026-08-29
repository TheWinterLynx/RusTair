use rustair::backend::{
    CpuState, CycleAccurateMachineBackend, Intel8080State, MachineBackend, NativeMachineBackend,
};

fn intel_state<B: MachineBackend>(backend: &mut B) -> Intel8080State {
    match backend.cpu_state().expect("CPU state must be available") {
        CpuState::Intel8080(state) => state,
        CpuState::Z80(_) => panic!("expected Intel 8080 backend"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DormantFastCpuState {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
    f: u8,
    pc: u16,
    sp: u16,
    inte: bool,
    halted: bool,
    cycles: u64,
}

fn dormant_fast_cpu_state(backend: &CycleAccurateMachineBackend) -> DormantFastCpuState {
    let cpu = &backend.machine().cpu;
    DormantFastCpuState {
        a: cpu.a,
        b: cpu.b,
        c: cpu.c,
        d: cpu.d,
        e: cpu.e,
        h: cpu.h,
        l: cpu.l,
        f: cpu.f,
        pc: cpu.pc,
        sp: cpu.sp,
        inte: cpu.inte,
        halted: cpu.halted,
        cycles: cpu.cycles,
    }
}

fn poison_dormant_fast_cpu(backend: &mut CycleAccurateMachineBackend) -> DormantFastCpuState {
    let cpu = &mut backend.machine_mut().cpu;
    cpu.a = 0x11;
    cpu.b = 0x22;
    cpu.c = 0x33;
    cpu.d = 0x44;
    cpu.e = 0x55;
    cpu.h = 0x66;
    cpu.l = 0x77;
    cpu.f = 0xd7;
    cpu.pc = 0x3456;
    cpu.sp = 0x789a;
    cpu.inte = true;
    cpu.halted = true;
    cpu.cycles = 0xdead_beef;
    dormant_fast_cpu_state(backend)
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

    // No legal 8080 instruction approaches this many T-states. Leave generous
    // headroom so this helper fails loudly if execution loses a boundary.
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
fn fast_backend_uses_altair_machine_cpu_as_its_execution_authority() {
    let mut backend = NativeMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0x0100, &[0x3e, 0x5a]).unwrap(); // MVI A,5Ah

    // Fast deliberately keeps AltairMachine.cpu as its real execution object.
    // The Cycle refactor must not change this contract.
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
fn cycle_execution_neither_reads_nor_rewrites_dormant_fast_cpu() {
    let mut backend = CycleAccurateMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x3e, 0x5a]).unwrap(); // MVI A,5Ah

    let authoritative_a = backend.cpu().registers().a;
    let dormant = poison_dormant_fast_cpu(&mut backend);

    cycle_step_machine_cycle(&mut backend); // M1 fetch only.
    assert_eq!(backend.cpu().registers().pc, 1);
    assert_eq!(backend.cpu().registers().a, authoritative_a);
    assert_eq!(backend.cpu().total_t_states(), 4);
    assert_eq!(dormant_fast_cpu_state(&backend), dormant);

    cycle_step_machine_cycle(&mut backend); // M2 operand read completes MVI.
    assert_eq!(backend.cpu().registers().pc, 2);
    assert_eq!(backend.cpu().registers().a, 0x5a);
    assert_eq!(backend.cpu().total_t_states(), 7);
    assert_eq!(
        dormant_fast_cpu_state(&backend),
        dormant,
        "Cycle must not use its embedded AltairMachine Cpu8080 as a mirror"
    );
}

#[test]
fn cycle_chassis_controls_leave_dormant_fast_cpu_untouched() {
    let mut backend = CycleAccurateMachineBackend::default();
    let dormant = poison_dormant_fast_cpu(&mut backend);

    // Power-on now seeds Cpu8080Cycle directly instead of randomizing/copying
    // AltairMachine.cpu.
    backend.power(true).unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "power on");

    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "RESET");

    backend.load_bytes(0, &[0x00, 0x00]).unwrap();
    backend.step().unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "physical SINGLE STEP");

    backend.run().unwrap();
    backend.service_execution(4).unwrap();
    backend.halt().unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "RUN/host pause");

    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0x0123, &[0xa5, 0x5a]).unwrap();
    backend.set_switch_register(0x0123).unwrap();
    backend.panel_examine(false).unwrap();
    backend.set_switch_register(0x005a).unwrap();
    backend.panel_deposit(false).unwrap();
    backend.protect_current_board(false).unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "front panel operations");

    backend.request_hold(true).unwrap();
    backend.run().unwrap();
    backend.service_execution(5).unwrap();
    backend.request_hold(false).unwrap();
    backend.service_execution(1).unwrap();
    backend.halt().unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "HOLD/HLDA");

    backend.commit_panel_activity(std::time::Duration::from_millis(16)).unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "panel lamp integration");

    backend.power(false).unwrap();
    assert_eq!(dormant_fast_cpu_state(&backend), dormant, "power off");
}

#[test]
fn fast_and_cycle_backends_match_architectural_state_through_shared_machine_path() {
    // A real 8080 does not define A/flags/general-register/SP contents after
    // power-on, and RESET deliberately does not clear those programmer-visible
    // registers. Two independently powered emulator instances are therefore
    // allowed to begin with different random values. Make that undefined state
    // deterministic through real guest instructions before comparing engines.
    let program = [
        0x31, 0x00, 0xf0, // LXI SP,F000h
        0x01, 0x00, 0x00, // LXI B,0000h
        0x11, 0x00, 0x00, // LXI D,0000h
        0x21, 0x00, 0x00, // LXI H,0000h
        0xaf,             // XRA A -> A=0 and deterministic flags
        // Workload compared instruction by instruction from here.
        0x3e, 0x12,       // MVI A,12h
        0x06, 0x34,       // MVI B,34h
        0x80,             // ADD B
        0x32, 0x00, 0x02, // STA 0200h
        0x21, 0x00, 0x02, // LXI H,0200h
        0x4e,             // MOV C,M
        0x0c,             // INR C
        0xfb,             // EI (delayed enable)
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
        // RAM power-on contents are independently randomized just like CPU
        // registers. Seed the location compared below so equality is meaningful
        // even before the workload's STA 0200h has executed.
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
    assert_eq!(
        intel_state(&mut fast).pc,
        expected_end,
        "Fast differential must stop at the first byte after the deterministic program"
    );
    assert_eq!(
        intel_state(&mut cycle).pc,
        expected_end,
        "Cycle differential must stop at the first byte after the deterministic program"
    );
}
