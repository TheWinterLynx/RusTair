from pathlib import Path

p = Path("tests/backend_authority.rs")
text = p.read_text(encoding="utf-8")
old = '''    // HLT is the historical corner case where STOP cannot latch until RESET
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
'''
new = '''    // HLT is the historical corner case where STOP cannot latch because no
    // qualifying PSYNC is produced. RESET restarts the processor, but RESET
    // itself does not clear the Display/Control RUN/STOP R-S latch: the held
    // STOP is captured by the first real post-reset T1/PSYNC.
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
        backend.front_panel_state().unwrap().running,
        "RESET itself must preserve the physical RUN/STOP latch"
    );
    assert_cycle_mirror_matches_authority(&backend, "RESET asserted with STOP held");

    backend.release_reset().unwrap();
    assert!(
        backend.front_panel_state().unwrap().running,
        "RUN remains set until the first post-reset PSYNC is actually clocked"
    );
    assert_cycle_mirror_matches_authority(&backend, "RESET released before STOP capture");

    backend.service_execution(1).unwrap();
    assert!(
        !backend.front_panel_state().unwrap().running,
        "held STOP must latch at the first real post-reset PSYNC"
    );
    assert_cycle_mirror_matches_authority(&backend, "post-reset PSYNC STOP capture");
    backend.release_run_stop(false).unwrap();
'''
if new not in text:
    if old not in text:
        raise SystemExit("expected stale backend-authority RESET block not found")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")
