from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected RUN/RESET test anchor not found in {path}: {old[:180]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# Fast deliberately does not claim exact READY/RESET electrical truth in its
# reconstructed Teacher. Keep that contract intact; verify the raw S-100 model
# separately and require exact sampled/control truth only from Cycle.
replace_once(
    "tests/run_reset_timing.rs",
    '''        let held = host.bus_teaching_snapshot().expect("RESET teaching state");
        assert_eq!(held.reset, Some(true));
        assert_eq!(held.ready, Some(true), "{engine:?}: PRDY follows the RUN latch even while PRESET is asserted");

        host.release_front_panel_reset();''',
    '''        let held = host.bus_teaching_snapshot().expect("RESET teaching state");
        if engine == EmulationEngine::RustCycleAccurate8080 {
            assert_eq!(held.reset, Some(true));
            assert_eq!(held.ready, Some(true), "Cycle: PRDY follows the RUN latch even while PRESET is asserted");
        } else {
            assert_eq!(held.reset, None, "Fast must not invent an exact RESET input sample");
            assert_eq!(held.ready, None, "Fast must not invent an exact READY input sample");
        }

        host.release_front_panel_reset();''',
)

panel = Path("src/machine/panel_bus.rs")
text = panel.read_text(encoding="utf-8")
marker = "run_latch_keeps_prdy_released_while_reset_is_held"
if marker not in text:
    text += r'''

#[cfg(test)]
mod reset_run_ready_tests {
    use super::*;

    #[test]
    fn run_latch_keeps_prdy_released_while_reset_is_held() {
        let mut bus = S100BusState::default();
        bus.assert_front_panel_reset(true);
        let signals = bus.signals();
        assert!(signals.reset);
        assert!(signals.run);
        assert!(signals.front_panel_ready);
        assert!(signals.ready);
        assert!(!signals.wait, "WAIT is an 8080 output and is inactive during RESET");
    }

    #[test]
    fn stopped_latch_keeps_prdy_low_while_reset_is_held() {
        let mut bus = S100BusState::default();
        bus.assert_front_panel_reset(false);
        let signals = bus.signals();
        assert!(signals.reset);
        assert!(!signals.run);
        assert!(!signals.front_panel_ready);
        assert!(!signals.ready);
        assert!(!signals.wait);
    }
}
'''
    panel.write_text(text, encoding="utf-8")
