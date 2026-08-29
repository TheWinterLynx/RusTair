use rustair::backend::{BackendHost, BusTState, EmulationEngine};
use rustair::config::{RamBoardProfile, RamInit, RamSize};

fn prepared(profile: RamBoardProfile, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(EmulationEngine::RustCycleAccurate8080)
        .expect("built-in Cycle backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.configure_memory_board_profile(profile);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

#[test]
fn mits_1k_opcode_fetch_emits_exactly_two_tw_states() {
    let mut host = prepared(RamBoardProfile::Mits1KStatic1975, &[0x00, 0x00]);
    let mut states = Vec::new();
    let mut ready = Vec::new();
    let mut wait = Vec::new();

    for _ in 0..6 {
        host.debugger_step_t_state();
        let sample = host.bus_teaching_snapshot().expect("exact sample");
        states.push(sample.t_state);
        ready.push(sample.ready);
        wait.push(sample.pins.wait);
    }

    assert_eq!(states, vec![
        BusTState::T1,
        BusTState::T2,
        BusTState::Tw,
        BusTState::Tw,
        BusTState::T3,
        BusTState::T4,
    ]);
    assert_eq!(ready, vec![
        Some(false),
        Some(false),
        Some(false),
        Some(true),
        Some(true),
        Some(true),
    ]);
    assert_eq!(wait[2], Some(true));
    assert_eq!(wait[3], Some(true));
    assert_eq!(host.intel8080_state().total_t_states, Some(6));
}

#[test]
fn fast_memory_profile_keeps_standard_nop_at_four_t_states() {
    let mut host = prepared(RamBoardProfile::FastNoWait, &[0x00, 0x00]);
    host.debugger_step_instruction();
    assert_eq!(host.intel8080_state().total_t_states, Some(4));
}

#[test]
fn mits_1k_mvi_has_two_slow_reads_not_a_global_instruction_penalty() {
    let mut host = prepared(RamBoardProfile::Mits1KStatic1975, &[0x3e, 0x42, 0x00]);
    host.debugger_step_instruction();
    let cpu = host.intel8080_state();
    assert_eq!(cpu.a, 0x42);
    // MVI A,imm is 7T normally: M1 fetch + operand memory read. Each addressed
    // MITS 1K read contributes exactly two TW states, therefore 7 + 2 + 2 = 11.
    assert_eq!(cpu.total_t_states, Some(11));
}
