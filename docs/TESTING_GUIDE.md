# RusTair Testing and Validation Guide

RusTair treats tests as part of the hardware specification. A green test suite is necessary, but the **kind of test** matters: an instruction-result test cannot prove a bus-edge timing invariant, and a GUI test cannot prove an S-100 electrical rule.

This guide explains the layers of evidence and which tests to run for different classes of change.

---

## 1. Normal commands

Fast normal development:

```powershell
cargo test --locked
```

Full local pre-merge validation:

```powershell
cargo fmt --check; if ($LASTEXITCODE -eq 0) { $env:RUSTFLAGS='-Dwarnings'; cargo test --locked --all-targets }; if ($LASTEXITCODE -eq 0) { cargo build --locked --release }
```

`cargo fmt --check` is a gate, not a cleanup suggestion: formatting failures are fixed before interpreting later test/build output.

`RUSTFLAGS=-Dwarnings` is intentional. Do not merge new warnings or hide them behind broad lint suppressions.

The test profile uses optimization level 2 because some emulator regressions execute very large numbers of T-states through the live machine. Debug-unoptimized execution can make a correct test appear hung.

---

## 2. Test taxonomy

### 2.1 Pure semantic/unit tests

Purpose: prove local calculations independent of the complete Altair machine.

Examples:

- 8080 ALU/flags;
- decoder metadata;
- configuration validation;
- ASR keyboard/tape helpers;
- debugger/call-stack inference.

Useful when changing a small algorithm, but insufficient for board/timing changes.

### 2.2 Exact CPU-cycle tests

Purpose: prove the stateful 8080 timeline.

Look under `src/cpu8080_cycle/*_tests.rs` and related integration tests.

Typical invariants:

- machine-cycle sequence;
- T1/T2/Tw/T3/T4/T5 placement;
- PHI1/PHI2 transitions;
- package pins;
- READY insertion;
- HOLD/HLDA behavior;
- HLT dwell;
- interrupt acknowledge;
- DI/EI timing;
- reset semantics.

Run these whenever exact CPU timing changes.

### 2.3 S-100 electrical tests

Purpose: prove card/backplane behavior rather than only CPU state.

Typical invariants:

- physical pin mapping;
- card contact role;
- High-Z release;
- open-collector resolution;
- contention;
- address decode;
- multiple responders;
- open bus;
- CPU-board status/control projection;
- card wakeup on changed bus signals.

A shortcut that makes software work while changing these results is not acceptable for the hardware-fidelity path.

### 2.4 Machine/front-panel tests

Purpose: prove the Display/Control panel sees the physical machine.

Typical invariants:

- ADDRESS/DATA/STATUS source;
- RUN/STOP latch ownership;
- RESET behavior;
- EXAMINE/DEPOSIT bus cycles;
- PROTECT/UNPROTECT;
- lamp duty/persistence separation;
- exact final Full boundary state.

Do not replace these tests with assertions on PC/registers.

### 2.5 Serial-card and serial-clock tests

Purpose: prove the 88-SIO / 88-2SIO / MC6850 hardware **and** the independent serial physical-time contract.

Typical invariants:

- status/data register bits;
- revision-specific behavior;
- selected baud and word-format timing;
- CTS/DCD/RTS/BREAK and other electrical signals;
- input/output ready latches;
- overrun behavior;
- interrupt target/wiring;
- address straps;
- port independence;
- serial progress while CPU execution is STOPped/RESET-held/HOLD-parked;
- host CPU speed does not multiply/divide card baud;
- quiet UARTs publish no scheduler deadline while retaining oscillator phase;
- active UARTs publish the next effective hardware boundary;
- 88-2SIO keeps its external 16× tap phase while scheduling `/1`, `/16` or `/64` effective MC6850 boundaries;
- a serial `OUT` is causally aligned so newly active hardware cannot inherit pre-write elapsed time;
- DCDD/chassis virtual time remains separate from serial physical time.

Host TCP/COM tests are separate: they prove the endpoint, not the guest UART.

See `SERIAL_CLOCK_DOMAINS.md` for the current scheduler contract.

### 2.6 Architecture authority tests

Purpose: prevent structural regressions even when runtime behavior still appears correct.

Examples include tests named around:

- backend authority;
- lifecycle authority;
- chassis architecture;
- debugger architecture;
- CPU-board authority;
- S-100 app/runtime authority;
- state-source architecture;
- unified cycle architecture;
- retired SIMH surface.

These tests protect rules such as "one CPU authority", "no old backend", "host does not own a T-state loop", and "physical configuration reaches one live topology".

### 2.7 Full-versus-Partial differential tests

Purpose: prove an accelerated Full path is equivalent to the exact Partial oracle.

A useful Full test compares more than final registers. Depending on the instruction/hardware path it may compare:

- registers and flags;
- PC/SP/INTE;
- elapsed T-states;
- memory bytes and protection state;
- raw panel duty;
- final CPU package pins;
- CPU-board status latch;
- S-100 boundary state;
- serial/card state;
- transition/fallback behavior.

Important in-module Full oracles live in:

- `src/backend/cycle/full/control_flow_tests.rs`
- `src/backend/cycle/full/ei_tests.rs`
- `src/backend/cycle/full/panel_histogram_reference.rs`

### 2.8 Classic diagnostic tests

Purpose: run known Intel 8080 diagnostic programs through the emulator and compare canonical totals/results.

The principal full-system harness is `tests/cpu8080_adaptive_classic_diagnostics.rs`.

Important long diagnostics include CPUTEST and 8080EXM. They are intentionally ignored in the default suite because of runtime.

Canonical CPUTEST comparison totals currently used by the project:

```text
33,971,311 reference instructions
255,653,383 reference T-states
256,000,000 actual machine T-states
```

Canonical 8080EXM comparison totals:

```text
2,919,050,698 reference instructions
23,803,381,171 reference T-states
23,806,100,000 actual machine T-states
```

These totals are useful regression oracles. They do not by themselves prove every bus-level invariant.

### 2.9 Performance/profiler tests

Purpose: measure, not define correctness.

Examples:

- `tests/emulation_speed_benchmark.rs`
- `tests/serial_scheduler_benchmark.rs`
- `tests/8080exm_full_barrier_profile.rs`
- `tests/8080exm_ei_successor_profile.rs`
- `tools/profile_full_cpu.ps1`

Many are intentionally ignored or require release/symbolized builds. Never weaken correctness tests to improve a profiler result.

`tests/serial_scheduler_benchmark.rs` deliberately has two release measurements:

- idle installed 88-2SIO, where all UARTs publish no deadline and CPU service remains at 4096T;
- continuously active RX BREAK at 110 and 9600 baud, where card-owned deadlines are exercised continuously.

The `coarse` comparator is an artificial no-intermediate-boundary reference. Its ratio is useful for host scheduling cost analysis but is not itself a fidelity target.

---

## 3. Selecting tests by subsystem

| Changed subsystem | Minimum focused evidence before the global suite |
| --- | --- |
| `cpu8080.rs` instruction semantics | semantic CPU tests + relevant classic/differential tests |
| `cpu8080_cycle` ALU | ALU tests + semantic differential |
| exact cycle/timing/phase | exact core timing tests + backend authority + relevant hardware tests |
| READY / wait states | CPU wait tests + `memory_wait_timing` + S-100 RAM timing |
| HOLD / HLDA | `hold_tests` + backend/machine HOLD tests |
| interrupts / DI / EI | interrupt-control tests + `ei_interrupt_timing_fidelity` + relevant SIO routing tests |
| Full opcode admission/projection | Full differential tests + panel oracle + classic diagnostic strategy totals |
| S-100 backplane | electrical/card tests + open-bus/overlap tests + broad suite |
| RAM card/runtime | memory decode/wait/protection/open-bus tests + RAM viewer/inspection where relevant |
| front panel physical code | front-panel fidelity + panel duty + run/reset/examine/deposit/protect tests |
| 88-SIO | SIO hardware/config/interrupt/electrical tests + serial integration + clock-domain tests when timing changes |
| 88-2SIO / MC6850 | 2SIO timing/config/interrupt/electrical tests + `two_sio_idle_chassis_clock` + scheduler benchmark when timing/deadlines change |
| serial physical-time scheduler | execution-frame unit tests + `debugger_architecture` + `two_sio_idle_chassis_clock` + idle/active `serial_scheduler_benchmark` |
| serial router/TCP/COM | endpoint/router tests + ensure serial-card tests remain unchanged |
| persistence/config | round-trip/migration/config UI tests + S-100 hardware validation |
| ASR-33 | ASR model/reader/paper-tape tests + authentic loading tests |
| debugger/trace | debugger/history/teacher tests; verify observer capture does not change execution |
| execution clock/frame | GUI execution-performance tests + authentic/throttled scheduling tests + serial clock-domain evidence if time mapping changes |

---

## 4. Running one exact test

Cargo accepts the test target and a name filter. Example:

```powershell
cargo test --locked --test ei_interrupt_timing_fidelity
```

For an exact test name and visible output:

```powershell
cargo test --locked --test ei_interrupt_timing_fidelity exact_test_name -- --exact --nocapture --test-threads=1
```

Use `--test-threads=1` for performance/timing diagnostics where concurrent tests would contaminate host measurements.

---

## 5. Running the long classic diagnostics

CPUTEST:

```powershell
cargo test --locked --release --test cpu8080_adaptive_classic_diagnostics full_system_runs_cputest_with_reference_totals -- --ignored --nocapture --exact --test-threads=1
```

8080EXM:

```powershell
cargo test --locked --release --test cpu8080_adaptive_classic_diagnostics full_system_runs_8080exm_with_reference_totals -- --ignored --nocapture --exact --test-threads=1
```

Use these after substantial CPU/Full/S-100 changes, not after every UI edit.

---

## 6. Running the serial scheduler benchmarks

Idle installed serial hardware:

```powershell
cargo test --locked --release --test serial_scheduler_benchmark measure_managed_serial_scheduler_cost -- --ignored --nocapture --test-threads=1
```

Continuously active 110/9600-baud deadline cases:

```powershell
cargo test --locked --release --test serial_scheduler_benchmark measure_active_serial_scheduler_cost -- --ignored --nocapture --test-threads=1
```

These are host-performance measurements. Their correctness preconditions come from the focused card/deadline/causality tests and the broad suite. Interpret absolute MHz only on comparable hardware/builds.

---

## 7. What a performance run must record

For Adaptive benchmarks, record both speed and strategy invariants:

- wall-clock seconds;
- equivalent MHz/T-states per second;
- reference instruction count where applicable;
- reference T-state count where applicable;
- actual machine T-state count;
- Full T-states/percent;
- Partial T-states/percent;
- Full instruction count;
- Full windows;
- fallback reasons such as budget/opcode barrier/interrupt/READY/HOLD;
- for serial scheduling, configured card rate/divider, first deadline, realtime headroom and whether the UART is idle or active.

A faster run with changed instruction/T-state totals or changed serial hardware semantics is a correctness failure, not an optimization.

---

## 8. Benchmark hygiene

Absolute emulator speed is very sensitive to the host.

For performance A/B work:

1. Build baseline and candidate with the same release profile.
2. Avoid comparing a normal stripped release build to a symbolized profiler build.
3. Alternate baseline/candidate runs when possible (A-B-B-A or repeated pairs).
4. Record concurrent host load; ideally remove it.
5. Use medians over multiple short runs for noisy workloads.
6. Use one long EXM-style run only after the short A/B indicates a real effect.
7. Compare strategy/correctness totals before interpreting MHz.
8. For event-driven scheduling, distinguish real device-event frequency from an artificial host service-slice frequency.

Sampling profilers change absolute throughput. Use them to find hotspots, not to claim production MHz.

---

## 9. Full acceptance checklist

A Full optimization or new Full instruction should not be accepted until all applicable answers are yes:

- Does exact Partial remain unchanged?
- Does Full produce the same architectural CPU state?
- Same total T-states?
- Same physical RAM bytes/protection?
- Same external machine-cycle addresses/data/status where required?
- Same raw panel duty?
- Same final package/bus/latch state?
- Same delayed INTE behavior?
- Same interrupt blocker behavior?
- Same stale-sMEMR/pre-write T1 behavior where relevant?
- Same serial/card state at synchronization?
- Does Full immediately fall back when the safety predicate stops holding?

A benchmark improvement is evaluated only after these pass.

---

## 10. Architecture guard philosophy

Some tests inspect source/API structure. This is intentional.

Behavioral tests alone may not detect a dangerous architecture change such as:

- reintroducing a second CPU whose state happens to be synchronized in existing tests;
- adding a second RAM copy that currently receives all writes;
- resurrecting a retired backend/feature;
- moving panel truth into the UI;
- bypassing the installed S-100 inventory for one code path;
- moving exact serial barrier T-state iteration into the host scheduler.

Architecture tests make these design constraints executable.

---

## 11. Ignored tests

An ignored test is not dead code. Common reasons for `#[ignore]` include:

- multi-minute diagnostic runtime;
- profiling-only instrumentation;
- manual benchmark workload;
- specialized investigation not appropriate for every `cargo test`.

Before deleting or unignoring one, read its purpose and related documentation/commit history.

---

## 12. Test failures: how to triage

When a broad test fails after a low-level change, do not immediately patch the test.

Ask:

1. Is Partial wrong, Full wrong, or both?
2. Did the reference instruction/T-state totals change?
3. Is the failure architectural state, physical bus state, panel duty, or presentation only?
4. Did configuration/persistence mount a different card topology?
5. Did a host observer/debugger become active and alter execution cost or path?
6. Is a failure timing-sensitive because tests ran concurrently?
7. For serial changes, did the card deadline/phase change, did the host cross a deadline, or did CPU/chassis time accidentally advance serial state?

For Full regressions, reproduce first with a forced-Partial/reference execution if available. The exact path should decide whether the optimization or the test expectation is wrong.

---

## 13. Recommended pre-merge evidence in a PR

Include:

- exact commands run;
- `cargo fmt --check` result;
- test counts/results;
- focused tests relevant to the hardware invariant;
- release build result;
- for performance changes: A/B methodology and exact strategy/canonical totals;
- for serial scheduler changes: idle and active deadline evidence plus baud-independence/causality tests;
- for hardware changes: primary-source or project hardware-document reference;
- any intentionally ignored/unsupported edge case.

"All tests pass" is useful, but explaining **which physical invariant the tests protect** makes the review much stronger.
