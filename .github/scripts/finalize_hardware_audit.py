from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text(encoding="utf-8")
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"cleanup anchor not found in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1), encoding="utf-8")


# The original Display/Control RESET line restarts the processor but does not
# itself reset the RUN/STOP R-S latch. Fast cannot expose the first post-reset
# PSYNC, so its release_reset path approximates capture at that boundary.
replace_once(
    "src/machine/mod.rs",
    '''        machine.assert_run_stop(false);\n        assert!(machine.running, "STOP cannot latch without PSYNC while halted");\n        machine.assert_front_panel_reset();\n        assert!(!machine.running, "held STOP must latch when RESET supplies recovery");\n        machine.release_front_panel_reset();\n        machine.release_run_stop(false);\n        assert!(machine.wait_led());''',
    '''        machine.assert_run_stop(false);\n        assert!(machine.running, "STOP cannot latch without PSYNC while halted");\n        machine.assert_front_panel_reset();\n        assert!(machine.running, "RESET itself must preserve the physical RUN/STOP latch");\n        machine.release_front_panel_reset();\n        assert!(!machine.running, "Fast must capture held STOP at its reconstructed first post-reset fetch boundary");\n        machine.release_run_stop(false);\n        assert!(machine.wait_led());''',
)

# Remove a now-unused convenience accessor; canonical callers consume the full
# S100CpuControlLines contract instead.
replace_once(
    "src/machine/mod.rs",
    '''    pub(crate) fn interrupt_requested(&self) -> bool {\n        self.s100.signals().interrupt\n    }\n\n''',
    '''''',
)

# This import became obsolete when the authority test stopped peeking at raw
# cycle enums directly.
replace_once(
    "tests/backend_authority.rs",
    '''use rustair::cpu8080_cycle::{MachineCycle, TState};\n''',
    '''''',
)

# Mark the S-100 interrupt work that this audit has actually completed.
replace_once(
    "TODO.md",
    '''- [ ] **[P1] Add a real S-100 interrupt-request path and interrupt-producing device model** before claiming interrupt-capable peripheral fidelity.''',
    '''- [x] ~~**[P1] Add a real S-100 interrupt-request path and interrupt-producing device model** before claiming interrupt-capable peripheral fidelity.~~ Canonical PINT plus 88-SIO/88-2SIO level-sensitive IRQ sources are implemented for both Rust backends.''',
)
replace_once(
    "TODO.md",
    '''- [ ] **[P1] Connect serial IRQ generation to the future S-100 interrupt path.**''',
    '''- [x] ~~**[P1] Connect serial IRQ generation to the future S-100 interrupt path.~~ 88-SIO/88-2SIO IRQ conditions now drive canonical S-100 PINT and direct RST 7 acknowledge.''',
)

# Retire branch-only migration helpers. Their resulting Rust code is now the
# source of truth; future CI must validate, never rewrite, the checkout.
for name in [
    "complete_dual_state.py",
    "complete_memory_ready.py",
    "finish_memory_ready.py",
    "finish_memory_ready_v2.py",
    "fix_backend_authority_run_reset.py",
    "fix_run_reset_semantics.py",
    "fix_run_reset_tests.py",
    "remove_cpu_inte_mirror.py",
]:
    p = Path(".github/scripts") / name
    if p.exists():
        p.unlink()

# Remove this one-shot helper from the resulting commit too. The workflow is
# updated separately through the GitHub API because Actions tokens are not
# allowed to rewrite workflow files without the workflows permission.
Path(__file__).unlink()
