use rustair::backend::{BackendHost, BusTState, EmulationEngine};
use rustair::config::{RamInit, RamSize};

fn prepared(engine: EmulationEngine, program: &[u8]) -> BackendHost {
    let mut host = BackendHost::from_engine(engine).expect("built-in backend");
    host.configure_memory(RamSize::K1, RamInit::Zeroed);
    host.power(true);
    host.front_panel_reset();
    host.load_bytes(0, program);
    host
}

#[test]
fn run_sets_the_physical_latch_while_reset_is_held() {
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut host = prepared(engine, &[0x00, 0x00]);
        assert!(!host.running());

        host.assert_front_panel_reset();
        assert!(!host.running());
        host.assert_run_stop(true);
        assert!(host.running(), "{engine:?}: RUN must asynchronously set the D/C R-S latch during RESET");
        host.release_run_stop(true);

        let held = host.bus_teaching_snapshot().expect("RESET teaching state");
        if engine == EmulationEngine::RustCycleAccurate8080 {
            assert_eq!(held.reset, Some(true));
            assert_eq!(held.ready, Some(true), "Cycle: PRDY follows the RUN latch even while PRESET is asserted");
        } else {
            assert_eq!(held.reset, None, "Fast must not invent an exact RESET input sample");
            assert_eq!(held.ready, None, "Fast must not invent an exact READY input sample");
        }

        host.release_front_panel_reset();
        assert!(host.running(), "{engine:?}: releasing RESET must preserve RUN");
        host.run_cycles(4);
        assert_eq!(host.intel8080_state().pc, 1, "{engine:?}: execution must begin at reset vector zero");
    }
}

#[test]
fn stop_held_during_reset_waits_for_the_first_post_reset_fetch() {
    // HLT reproduces the classic original-8800 lock-up: STOP cannot reset the
    // RUN latch because a halted 8080 produces no qualifying PSYNC.
    for engine in [
        EmulationEngine::RustFast8080,
        EmulationEngine::RustCycleAccurate8080,
    ] {
        let mut host = prepared(engine, &[0x76, 0x00]);
        host.assert_run_stop(true);
        host.release_run_stop(true);
        host.run_cycles(16);
        assert!(host.running(), "{engine:?}: HLT leaves the physical RUN latch set");
        assert!(host.intel8080_state().halted.unwrap_or(false));

        host.assert_run_stop(false);
        assert!(host.running(), "{engine:?}: STOP cannot clear RUN while the CPU is halted");
        host.assert_front_panel_reset();
        assert!(host.running(), "{engine:?}: RESET itself must not clear the RUN/STOP latch");

        host.release_front_panel_reset();
        match engine {
            EmulationEngine::RustFast8080 => {
                // Fast has no exact PSYNC/T-state boundary and captures the held
                // STOP at the reconstructed first fetch after RESET release.
                assert!(!host.running());
                assert_eq!(host.intel8080_state().pc, 0);
            }
            EmulationEngine::RustCycleAccurate8080 => {
                // Cycle retains RUN until the real first T1/PSYNC is clocked.
                assert!(host.running());
                host.run_cycles(1);
                assert!(!host.running());
                let sample = host.bus_teaching_snapshot().expect("post-reset STOP sample");
                assert_eq!(sample.t_state, BusTState::Tw);
                assert_eq!(sample.pins.wait, Some(true));
                assert_eq!(sample.ready, Some(false));
            }
            _ => unreachable!(),
        }

        host.release_run_stop(false);
    }
}
