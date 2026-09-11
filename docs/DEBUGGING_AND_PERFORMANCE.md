# Debugging, Tracing and Performance Work in RusTair

This document explains how to investigate a problem without accidentally confusing derived observer data with authoritative emulation state, and how to profile performance without weakening hardware fidelity.

---

## 1. Debugger architecture

The debugger is layered deliberately:

```text
Authoritative machine
    ↓
backend snapshots / instruction effects
    ↓
trace/history/analysis helpers
    ↓
UI viewers
```

Control operations travel back through explicit backend APIs:

```text
UI command
    ↓
BackendHost / debugger control
    ↓
Adaptive Cycle machine
```

A viewer opening or reading state should not alter guest execution.

---

## 2. Main debugger-related modules

| Module | Role |
| --- | --- |
| `src/debugger_control.rs` | Breakpoints, watchpoints, run-to, step-over/out policy and stop reasons. |
| `src/trace8080.rs` | Bounded instruction/effect trace observations. |
| `src/decoder8080.rs` | Disassembly and semantic metadata used by tools. |
| `src/explain8080.rs` | Human-readable instruction teaching/explanation. |
| `src/debugger8080.rs` | Conservative code/loop analysis. |
| `src/callstack8080.rs` | Inferred call stack from retained history. |
| `src/memory_activity8080.rs` | Derived memory-access activity. |
| `src/backend/bus_teaching.rs` | Stable bus/pin/machine-cycle teaching snapshots. |
| `src/app/ui/debugger_controls.rs` | Debugger control UI. |
| `src/app/ui/instruction_history.rs` | History/disassembly UI. |
| `src/app/ui/loop_inspector.rs` | Loop-analysis UI. |
| `src/app/ui/bus_teacher.rs` | T-state/bus teaching UI. |
| `src/app/ui/cpu_pin_diagram.rs` | 8080 pin visualization. |
| `src/app/ui/memory_viewer.rs` | Physical RAM inspection/mutation UI through backend APIs. |
| `src/app/ui/io_inspector.rs` | Guest I/O + endpoint trace UI. |

---

## 3. Breakpoints versus watchpoints

### Execute breakpoint

Stops when execution reaches a PC. `DebugExecutionControl` includes resume-skip handling so continuing from a breakpoint can execute the stopped instruction once rather than immediately re-triggering it.

### Run-to

Temporary execution target. Step-over/out can additionally require an SP value so the same PC reached at a deeper call nesting level does not stop prematurely.

### Memory watchpoint

Stops when an instruction performs a matching read/write transfer. The debugger associates the memory effect with the instruction PC and value.

Watchpoint capture must remain consistent with the physical/semantic execution path. Full execution cannot bypass an observer that is part of active debugger semantics; its admission/trace behavior must force the necessary synchronization path.

---

## 4. Instruction history

Instruction history is bounded. Older entries may be evicted.

Consequences:

- an inferred call stack may begin in the middle of a call chain;
- sequence gaps make stack inference incomplete;
- loop analysis can only claim structures it can prove from available memory/instruction boundaries;
- UI should display uncertainty rather than invent missing frames.

Do not use history as a source for current PC/SP/register state.

---

## 5. Bus Teacher accuracy

The teaching UI distinguishes exact captured physical samples from lifecycle/control snapshots.

Important principle:

- an exact captured sample represents a specific past physical moment;
- later controls may change the live machine;
- the old snapshot should remain historical rather than being rewritten to match current state.

Likewise, visible LED persistence is not a substitute for raw S-100 status.

---

## 6. Debugging Full/Partial divergence

When a bug appears only under Adaptive Full:

1. Reproduce with forced/exact Partial if a test/helper exists.
2. Compare CPU architectural state at the same synchronization boundary.
3. Compare exact elapsed T-states.
4. Compare memory bytes/protection/card state.
5. Compare raw front-panel duty/state and final retained cycle.
6. Compare INTE and pending interrupt state.
7. Compare CPU-board latch/pins after reconciliation.
8. Check whether Full should have been blocked earlier.

Typical categories:

- incorrect opcode admission;
- missing machine-cycle projection;
- wrong internal T-state tail;
- stale T1 DATA behavior lost;
- cache invalidation error;
- final cycle replay/reconciliation wrong;
- asynchronous serial/interrupt event crossed by a Full window;
- debugger observer not treated as a barrier.

Do not repair a Full divergence by changing the exact Partial result unless independent hardware evidence proves Partial wrong.

---

## 7. Adaptive metrics

`src/adaptive_metrics.rs` can count:

- Full instructions;
- Full T-states;
- Partial T-states;
- Full windows;
- Partial entries;
- Full→Partial and Partial→Full transitions;
- fallback reasons such as chassis, serial, READY, HOLD, interrupt, budget tail, instruction boundary, stop/fault/reset, opcode barrier and unavailable Full window.

Metrics are explicitly opt-in. This prevents production from paying per-T-state observer overhead when no measurement is active.

Use metrics to answer questions such as:

- Is performance limited by Full coverage or by cost inside Full?
- Which blocker fragments windows?
- Did a change alter strategy behavior even though final diagnostics still pass?

---

## 8. Sampling profiler

The maintained Windows sampling helper is:

```text
tools/profile_full_cpu.ps1
```

It is intended to identify where optimized execution spends host CPU time.

For a useful symbolized executable, build a release test artifact with debug symbols and stripping disabled, then pass the emitted executable to the profiler. A typical pattern is:

```powershell
$artifact = cargo --config profile.release.package.rustair.debug=1 --config profile.release.package.rustair.strip=false test --locked --release --test cpu8080_adaptive_classic_diagnostics --no-run --message-format=json | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'cpu8080_adaptive_classic_diagnostics' -and $_.executable } | Select-Object -Last 1; if ($LASTEXITCODE -ne 0) { throw 'Build failed' }; powershell -NoProfile -ExecutionPolicy Bypass -File tools/profile_full_cpu.ps1 -Executable $artifact.executable -TestName full_system_runs_8080exm_with_reference_totals -Rounds 1
```

The profiler build is not the normal stripped production release. Do not compare its absolute MHz directly to an ordinary production run.

---

## 9. Reading a sampling profile correctly

A sample percentage answers approximately:

> "Where was the instruction pointer when the sampler interrupted the program?"

It is not automatically an inclusive call-tree percentage.

Inlining changes attribution. For example, after `FullInstructionBus::project_machine_cycle` was forced inline, the function disappeared as a separate symbol and much of its work became attributed to callers such as `Cpu8080::step`/read callbacks. That does not mean panel projection vanished.

Before optimizing a 20% hotspot, determine whether the sampled region includes mandatory useful work or removable overhead.

---

## 10. Amdahl's law

If fraction `f` of total runtime is improved by local speedup `s`, total speedup is:

```text
1 / ((1 - f) + f/s)
```

This prevents wasting effort on micro-optimizations with tiny total ceilings.

Example: if a helper is 4% of runtime, making it infinitely fast can improve the whole program by only about 4.17%.

Use sampled fractions as hypotheses, not perfect truth, because inlining and profiler overhead affect attribution.

---

## 11. Safe performance categories

### Category A — host/compiler-only

Examples:

- better inlining;
- fewer copies;
- hot/cold code separation;
- register retention;
- data layout;
- avoiding allocations on a known hot path.

Low fidelity risk, but still benchmark/test because compiler changes can regress performance.

### Category B — redundant physical recomputation removal

Examples:

- cached fixed address decode;
- cached connector drive until its inputs change;
- incremental backplane drive counts;
- event/deadline advancement of quiet hardware;
- panel duty aggregation over mathematically equivalent intervals.

Requires explicit invalidation/dependency reasoning.

### Category C — execution granularity change

Examples:

- new Full opcode families;
- longer semantic blocks;
- skipping intermediate low-level host steps.

Requires strong Full-versus-Partial equivalence oracles.

### Category D — fidelity reduction

Examples:

- direct RAM bypass that ignores protection/overlap/bus ownership;
- instruction-boundary READY/HOLD;
- approximate panel from PC/registers;
- instant serial byte queues;
- ignoring contention.

Reject for the high-fidelity production path unless deliberately introduced as a separately named lower-fidelity mode with explicit scope.

---

## 12. Current performance architecture to remember

The current mature Full path already has high coverage on classic workloads. Therefore a performance profile may be dominated by **cost per Full instruction/machine-cycle projection**, not Full/Partial switching.

Known intentional optimizations include:

- compiled memory responder masks;
- compiled I/O decode masks;
- cached card drives/delta resolution;
- Full guest-read cache;
- static no-PROTECT specialization;
- marginal front-panel duty accounting;
- final-cycle retention/replay;
- multi-instruction Full windows;
- conservative EI→LHLD special admission;
- `#[inline(always)]` on hot Full machine-cycle projection;
- host-deadline Unlimited GUI execution.

Before redoing one of these, read its tests and historical performance documents. Several apparently "simpler" alternatives have already been shown slower or less exact.

---

## 13. Performance benchmark workflow

For one proposed optimization:

1. Create isolated branch.
2. Keep the change singular; do not combine unrelated optimizations.
3. Run fidelity oracles first.
4. Run short repeated baseline/candidate tests under comparable host load.
5. Record medians and strategy totals.
6. Run EXM only if short evidence is meaningful.
7. Re-profile the candidate because hotspot distribution changes after a real optimization.
8. KEEP/REJECT based on measured total gain, not code cleverness.

A 1% fluctuation may be noise. Alternating A/B pairs are much more useful than comparing to a months-old headline benchmark.

---

## 14. Canonical full-system reference checks

CPUTEST reference comparison:

```text
33,971,311 instructions
255,653,383 reference T-states
256,000,000 actual machine T-states
```

8080EXM reference comparison:

```text
2,919,050,698 instructions
23,803,381,171 reference T-states
23,806,100,000 actual machine T-states
```

The distinction between reference and actual T-state totals belongs to the diagnostic harness/reference accounting. Do not "fix" the numbers to make a performance result look cleaner.

---

## 15. Logging/diagnostic discipline

Avoid permanent `println!`/`eprintln!` in hot production paths.

For an investigation:

- prefer test-only/feature-local diagnostics;
- keep counters opt-in;
- remove temporary traces after proving the issue;
- never commit profiler logs/executables under `target/`;
- document reproducible profiler commands rather than checking in generated output.

A diagnostic should not materially change the timing behavior it claims to measure.

---

## 16. When a GUI bug looks like a CPU performance bug

Host scheduling can cap visible throughput even when the emulator core is fast. Separate:

- CPU/backend throughput;
- GUI repaint cadence;
- service slice size;
- host deadline/yield behavior;
- endpoint/peripheral servicing.

Unlimited mode is designed to run multiple service slices within a host-time deadline rather than one fixed T-state chunk per repaint. When diagnosing GUI-only slowness, profile scheduling before changing CPU fidelity code.

---

## 17. Debugging checklist by symptom

| Symptom | First questions |
| --- | --- |
| Wrong register/flag | Semantic core and exact core agree? Which instruction? |
| Correct result, wrong lamp | Is raw S-100/panel projection wrong or only LED presentation? |
| Hangs on memory | READY/wait states? open bus? protection? overlap? |
| Serial byte missing | Host endpoint, cable route, electrical interface, UART/card, or guest polling? |
| Interrupt not taken | INTE timing, card request/wiring, PINT/VI, acknowledge path? |
| Full-only failure | Force Partial; inspect admission/boundary/cache/panel/INTE. |
| Unlimited GUI slow | Host scheduling/repaint deadline, not necessarily CPU core. |
| Debugger UI flickers | Is observer reading architectural PC mid-instruction? Preserve exact state but make presentation tolerant. |
| State differs after loading config | Migration or slot-native runtime mounting mismatch? |

---

## 18. What to attach to a performance/fidelity review

A useful report contains:

- exact branch/commit;
- host/compiler/release profile;
- exact command;
- correctness totals;
- Full/Partial metrics;
- wall time/MHz;
- sample count/failures for profiler runs;
- top symbols;
- explanation of what changed in assembly/host work when relevant;
- fidelity tests run;
- expected Amdahl ceiling;
- recommendation KEEP/REJECT.

This makes future developers able to reproduce the decision rather than inherit an unexplained magic optimization.
