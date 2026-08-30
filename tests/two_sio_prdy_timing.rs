use rustair::backend::{
    BackendHost, BusMachineCycle, BusTState, EmulationEngine,
};
use rustair::config::{RamInit, RamSize, SerialBoard};

fn prepared_host(engine: EmulationEngine, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(engine).expect("built-in Rust backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.configure_serial_board(SerialBoard::TwoSio88);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host.set_running(true);
    host
}

#[test]
fn two_sio_input_adds_one_t_state_in_both_rust_engines() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut host = prepared_host(engine, &[0xdb, 0x10]); // IN 10h
        let before = host.intel8080_state().total_t_states.unwrap_or(0);
        host.run_cycles(11);
        let after = host.intel8080_state();

        assert_eq!(after.pc, 0x0002, "{} must complete exactly one IN", engine.label());
        assert_eq!(
            after.total_t_states.unwrap_or(0) - before,
            11,
            "{}: 88-2SIO IN must be base 10T + one documented 500 ns TW",
            engine.label()
        );
    }
}

#[test]
fn two_sio_output_and_unmapped_input_do_not_inherit_the_input_wait() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut output = prepared_host(engine, &[0xd3, 0x10]); // OUT 10h
        let before = output.intel8080_state().total_t_states.unwrap_or(0);
        output.run_cycles(10);
        let after = output.intel8080_state();
        assert_eq!(after.pc, 0x0002);
        assert_eq!(
            after.total_t_states.unwrap_or(0) - before,
            10,
            "{}: MITS documents the 88-2SIO wait only for input",
            engine.label()
        );

        let mut unmapped = prepared_host(engine, &[0xdb, 0x14]); // outside 10h..13h
        let before = unmapped.intel8080_state().total_t_states.unwrap_or(0);
        unmapped.run_cycles(10);
        let after = unmapped.intel8080_state();
        assert_eq!(after.pc, 0x0002);
        assert_eq!(
            after.total_t_states.unwrap_or(0) - before,
            10,
            "{}: an unselected 88-2SIO must not pull PRDY low",
            engine.label()
        );
        assert_eq!(after.a, 0xff, "unmapped IN still observes S-100 open bus");
    }
}

#[test]
fn cycle_88_2sio_input_exposes_one_real_tw_and_releases_prdy_in_tw() {
    let mut host = prepared_host(
        EmulationEngine::RustCycleAccurate8080,
        &[0xdb, 0x10], // IN 10h (ACIA 0 status)
    );

    let mut input_samples = Vec::new();
    for _ in 0..11 {
        host.run_cycles(1);
        if let Some(sample) = host.bus_teaching_snapshot() {
            if sample.machine_cycle == BusMachineCycle::InputRead {
                input_samples.push(sample);
            }
        }
    }

    assert_eq!(input_samples.len(), 4, "one 88-2SIO IN must contain exactly one TW");
    assert_eq!(
        input_samples.iter().map(|sample| sample.t_state).collect::<Vec<_>>(),
        vec![BusTState::T1, BusTState::T2, BusTState::Tw, BusTState::T3],
    );

    let t1 = input_samples[0];
    let t2 = input_samples[1];
    let tw = input_samples[2];
    let t3 = input_samples[3];

    assert_eq!(t1.ready, Some(true));
    assert_eq!(t2.ready, Some(false), "SINP/V must pull PRDY low at the T2 READY sample");
    assert_eq!(tw.ready, Some(true), "PWAIT clears V so PRDY is released during the sole TW");
    assert_eq!(tw.pins.wait, Some(true), "the 8080 must expose a real WAIT/TW output");
    assert_eq!(t3.ready, Some(true));
    assert_eq!(t3.cpu_data, Some(0x02), "empty MC6850 status currently reports TDRE");
    assert_eq!(t3.s100_di, Some(0x02));

    let cpu = host.intel8080_state();
    assert_eq!(cpu.pc, 0x0002);
    assert_eq!(cpu.total_t_states, Some(11));
    assert_eq!(cpu.a, 0x02);
}
