use rustair::backend::{
    CpuState, CycleAccurateMachineBackend, Intel8080State, MachineBackend, NativeMachineBackend,
};
use rustair::cpu8080_cycle::{MachineCycle, TState};

fn intel_state<B: MachineBackend>(backend: &mut B) -> Intel8080State {
    match backend.cpu_state().expect("CPU state must be available") {
        CpuState::Intel8080(state) => state,
        CpuState::Z80(_) => panic!("expected Intel 8080 backend"),
    }
}

fn assert_cycle_mirror_matches_authority(
    backend: &CycleAccurateMachineBackend,
    context: &str,
) {
    let authoritative = backend.cpu().registers();
    let mirror = &backend.machine().cpu;

    assert_eq!(mirror.a, authoritative.a, "{context}: A mirror");
    assert_eq!(mirror.b, authoritative.b, "{context}: B mirror");
    assert_eq!(mirror.c, authoritative.c, "{context}: C mirror");
    assert_eq!(mirror.d, authoritative.d, "{context}: D mirror");
    assert_eq!(mirror.e, authoritative.e, "{context}: E mirror");
    assert_eq!(mirror.h, authoritative.h, "{context}: H mirror");
    assert_eq!(mirror.l, authoritative.l, "{context}: L mirror");
    assert_eq!(mirror.f, authoritative.f, "{context}: flags mirror");
    assert_eq!(mirror.pc, authoritative.pc, "{context}: PC mirror");
    assert_eq!(mirror.sp, authoritative.sp, "{context}: SP mirror");
    assert_eq!(
        mirror.inte,
        backend.cpu().interrupts_enabled(),
        "{context}: INTE mirror"
    );
    assert_eq!(
        mirror.halted,
        backend.cpu().is_halted(),
        "{context}: HALT mirror"
    );
    assert_eq!(
        mirror.cycles,
        backend.cpu().total_t_states(),
        "{context}: T-state mirror"
    );
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

    // In the instruction-accurate backend AltairMachine.cpu is intentionally
    // the real CPU, not a compatibility copy. Put execution at a non-panel PC
    // and prove both cpu_state() and STEP follow that exact object.
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
fn cycle_backend_ignores_a_poisoned_fast_cpu_mirror_during_execution() {
    let mut backend = CycleAccurateMachineBackend::default();
    backend.power(true).unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x3e, 0x5a]).unwrap(); // MVI A,5Ah

    let authoritative = backend.cpu().registers();
    let authoritative_inte = backend.cpu().interrupts_enabled();

    // Deliberately corrupt every important legacy Cpu8080 field. If Cycle ever
    // executes or consults this object as CPU authority, the assertions below
    // will immediately fail. The next exact tick must instead overwrite it from
    // Cpu8080Cycle.
    {
        let mirror = &mut backend.machine_mut().cpu;
        mirror.a = authoritative.a ^ 0xff;
        mirror.b = authoritative.b ^ 0xff;
        mirror.c = authoritative.c ^ 0xff;
        mirror.d = authoritative.d ^ 0xff;
        mirror.e = authoritative.e ^ 0xff;
        mirror.h = authoritative.h ^ 0xff;
        mirror.l = authoritative.l ^ 0xff;
        mirror.f = authoritative.f ^ 0xd5;
        mirror.pc = 0x3456;
        mirror.sp = 0x789a;
        mirror.inte = !authoritative_inte;
        mirror.halted = true;
        mirror.cycles = 0xdead_beef;
    }

    cycle_step_machine_cycle(&mut backend); // M1 fetch only, no panel parking.
    assert_eq!(backend.cpu().registers().pc, 1);
    assert_eq!(backend.cpu().registers().a, authoritative.a);
    assert_eq!(backend.cpu().total_t_states(), 4);
    assert_cycle_mirror_matches_authority(&backend, "after poisoned-mirror fetch");

    cycle_step_machine_cycle(&mut backend); // M2 operand read completes MVI.
    assert_eq!(backend.cpu().registers().pc, 2);
    assert_eq!(backend.cpu().registers().a, 0x5a);
    assert_eq!(backend.cpu().total_t_states(), 7);
    assert_cycle_mirror_matches_authority(&backend, "after poisoned-mirror MVI");
}

#[test]
fn cycle_backend_mirror_stays_synced_across_cpu_and_chassis_transitions() {
    let mut backend = CycleAccurateMachineBackend::default();
    assert_cycle_mirror_matches_authority(&backend, "default");

    backend.power(true).unwrap();
    assert_cycle_mirror_matches_authority(&backend, "power on");
    backend.assert_reset().unwrap();
    assert_cycle_mirror_matches_authority(&backend, "RESET asserted");
    backend.release_reset().unwrap();
    assert_cycle_mirror_matches_authority(&backend, "RESET released");

    backend.load_bytes(0, &[0x00, 0x00]).unwrap();
    backend.step().unwrap();
    assert_cycle_mirror_matches_authority(&backend, "physical SINGLE STEP");
    backend.run().unwrap();
    backend.service_execution(4).unwrap();
    assert_cycle_mirror_matches_authority(&backend, "RUN execution");
    backend.halt().unwrap();
    assert_cycle_mirror_matches_authority(&backend, "host STOP");

    // EXAMINE and DEPOSIT exercise the shared chassis while the exact CPU is
    // parked in a fetch wait. The compatibility Cpu8080 must still only mirror.
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0x0123, &[0xa5, 0x5a]).unwrap();
    backend.set_switch_register(0x0123).unwrap();
    backend.panel_examine(false).unwrap();
    assert_cycle_mirror_matches_authority(&backend, "EXAMINE");
    backend.set_switch_register(0x005a).unwrap();
    backend.panel_deposit(false).unwrap();
    assert_cycle_mirror_matches_authority(&backend, "DEPOSIT");

    // HOLD/HLDA is owned by the exact core and S-100 control lines; mirroring
    // architectural CPU state must remain exact while ownership changes.
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x00, 0x00]).unwrap();
    backend.request_hold(true).unwrap();
    backend.run().unwrap();
    backend.service_execution(5).unwrap();
    assert!(backend.cpu().is_holding());
    assert_cycle_mirror_matches_authority(&backend, "HOLD/HLDA entered");
    backend.request_hold(false).unwrap();
    backend.service_execution(1).unwrap();
    assert!(!backend.cpu().is_holding());
    assert_cycle_mirror_matches_authority(&backend, "HOLD/HLDA released");

    // HLT is the historical corner case where STOP cannot latch until RESET
    // supplies a recovery condition. assert_run_stop() first synchronizes the
    // passive mirror specifically so the common chassis helper observes the
    // authoritative HALT state.
    backend.halt().unwrap();
    backend.assert_reset().unwrap();
    backend.release_reset().unwrap();
    backend.load_bytes(0, &[0x76]).unwrap(); // HLT
    backend.run().unwrap();
    backend.service_execution(16).unwrap();
    assert!(backend.cpu().is_halted());
    assert_cycle_mirror_matches_authority(&backend, "HLT dwell");

    backend.assert_run_stop(false).unwrap();
    assert!(
        backend.front_panel_state().unwrap().running,
        "STOP alone cannot latch without PSYNC while the 8080 is halted"
    );
    assert_cycle_mirror_matches_authority(&backend, "STOP requested during HLT");

    backend.assert_reset().unwrap();
    assert!(
        !backend.front_panel_state().unwrap().running,
        "held STOP must latch when RESET supplies recovery"
    );
    assert_cycle_mirror_matches_authority(&backend, "STOP+RESET recovery");
    backend.release_reset().unwrap();
    backend.release_run_stop(false).unwrap();
    assert_cycle_mirror_matches_authority(&backend, "RESET/STOP released");
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
    assert_cycle_mirror_matches_authority(&cycle, "after deterministic setup");

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
        assert_cycle_mirror_matches_authority(
            &cycle,
            &format!("differential workload instruction #{instruction_index}"),
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
