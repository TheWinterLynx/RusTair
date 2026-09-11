# Post-performance housekeeping audit

Base: `b15512e79638723d21347b3c14db8abda6889c27`.
Branch: `agent/post-performance-housekeeping`.

This is a behavior-preserving maintenance record, not a new performance study.
The audit used the tracked-file inventory, repository-wide declaration/call-site
searches, lint/feature searches, dependency usage and targeted implementation reads.

## SAFE changes

- Remove the `simh-ffi` Cargo feature and `build.rs`. The script only linked the
  retired native front-panel library; no Rust FFI implementation, feature-gated
  consumer or workflow reference remains. The existing retirement test now also
  guards against reintroducing that feature or native linkage.
- Remove `BackendHost::adaptive_cycle`, an uncalled alias of `Default`.
- Remove `BackendHost::clear_io`, an uncalled pulse wrapper. The physical
  `assert_front_panel_clear` / `release_front_panel_clear` operations remain.
- Remove `S100RuntimeFabric::fast_read_wait_states`, an uncalled aggregate wait
  query left behind by the old execution path. Card timing and live READY/WAIT
  resolution are unchanged.
- Remove the sole `allow(dead_code)` suppression, on the physical
  `machine::two_sio::TwoSioBaudTap`. All eight variants are constructed by the
  exhaustive `card_baud_tap` configuration mapping; no variant was removed.
- Correct current CPU-board/persistence, RUN-latch and Full-window authority docs,
  scheduling comments, the test-only opcode-classifier comment and the roadmap's
  already-completed conditional CALL/RET work.
- Label earlier performance investigations and hardware closeout implementation
  records as historical. Preserve their measurements, hardware sources and original
  validation instead of silently presenting them as current results.

Deleted emulator symbols are exactly the three methods listed above. The deleted
build entry point is `build.rs::main`. No other production symbol was removed.

## CONSERVATIVE findings retained

- Legacy configuration keys, version handling, aggregate-to-slot migration,
  compatibility RAM and the explicit BASIC memory-probe guard remain unchanged.
  Their unusual branches are compatibility contracts, not unused experiments.
- Public decoder predicates, chassis constructors, physical inspection/mutation
  primitives, address helpers and ASR-33 controls were not removed solely because
  the declaration scan found no current in-repository caller. Unlike the retired
  wrappers above, these are meaningful public hardware/tooling interfaces without
  evidence of a superseded contract.
- The private physical baud enum remains separate from the configuration enum.
  Removing its obsolete suppression needs no type/layout or timing-path rewrite.
- Best-effort host cleanup/channel sends, optional asset decoding and tolerant
  configuration parsing retain their current error policy. Converting them to
  panics or propagating new errors would change behavior. Guest electrical fallback
  paths likewise retain their existing resolution and open-bus behavior.
- The two similarly named `sounds/down-print-spaces-02*.wav` assets have different
  SHA-256 hashes. They are distinct licensed recordings, not proven duplicates;
  both recordings and attribution are preserved.

## RETAIN findings

- Exact Partial execution, Full admission, guarded EI -> LHLD, DI/T5 timing,
  stale-sMEMR T1 handling, marginal panel duty, final-cycle replay, reconciliation,
  caches, protection handling, forced projection inlining and Unlimited scheduling.
- Every fidelity test and assertion. Existing Full test helpers are already gated
  with `cfg(test)`; distinct fixtures/oracles were not merged for cosmetic reasons.
- `tools/profile_full_cpu.ps1`, diagnostic metering, Adaptive metrics and intentionally
  ignored long diagnostics/profilers. The semantic opcode profilers and physical
  machine diagnostics answer different questions and are not duplicate tests.
- The configuration-save error diagnostic and test-only GUI benchmark output.
  No stray production `dbg!` or temporary printing was found.
- Every dependency: all seven direct dependencies have source consumers. No version,
  renderer-feature or Cargo.lock change is warranted by this audit.
- Public visibility of physical/tooling APIs. Removing the three obsolete methods
  reduces the API surface without a broad visibility or module reorganization.

No test was removed, renamed, ignored or weakened. No fidelity oracle was weakened.
No generated log, binary, patch or scratch file was present in the tracked inventory.
Validation scratch files are confined to `target/housekeeping` and removed after use;
pre-existing ignored build/performance artifacts are left untouched.

## Validation and follow-up

All commands use `$env:RUSTFLAGS='-Dwarnings'`:

```powershell
cargo test --locked --test no_simh
cargo test --locked --lib
cargo test --locked --test ei_interrupt_timing_fidelity --test memory_wait_timing --test two_sio_prdy_timing --test backend_authority
cargo test --locked --all-targets
cargo build --locked --release
```

Results: the retirement guard passed all 4 tests; the library passed all 496;
the focused backend/EI/READY suites passed all 13. Final all-target validation
passed 708 tests across 71 test targets, with zero failures and 13 unchanged,
intentionally ignored diagnostics/profilers. The release build succeeded.
No compiler warnings or new lint suppressions remain.

The executable-path changes only remove provably uncalled code; live CPU/S-100,
Full/Partial, panel and scheduler calculations/data representations are unchanged. A separate
performance experiment is not required for this housekeeping scope.

Future work should separately assess public API compatibility before trimming more
inspection/control primitives, slot-aware routing for multiple serial cards, and
revalidation of archived hardware-document API examples. None is authorization to
change physical behavior or remove migration support.

Main is not modified or merged. No GitHub Actions are run.
