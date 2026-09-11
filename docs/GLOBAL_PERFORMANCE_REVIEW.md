# Focused runtime performance review

> Historical performance investigation. Branches, profiles, results and pending work below describe the recorded revisions, not current production. Later changes include control-flow/DI/EI fidelity and forced projection inlining (`b15512e`). Retained `target/` executables and logs are local artifacts, not repository dependencies; rerun the maintained benchmarks for current results.

Branch: `agent/codex-global-performance-review`, from fetched `main` at
`1690583`. No main edits/merges or GitHub Actions. Existing branches and local
untracked logs are preserved.

## Baseline and audit

The fetched main contains no-PROTECT compilation and whole-plane connector
writes, but **does not contain** the earlier internal-tail accumulation change:
`FullPanelActivity::commit_pending` still performs separate later/tail inserts.
This differs from the brief; do not attribute this revision's performance to
that optimization or silently use a different baseline.

Rust 1.97.1, Windows/MSVC, release ThinLTO, `RUSTFLAGS=-Dwarnings`.
Baseline CPUTEST executable retained as `target/global-review-baseline.exe`.
Three initial runs pinned to logical processor 2 (mask 4):
1.664 / 1.619 / 1.633 s; median 1.633 s, 156.73 MHz.
All canonical counts passed: 33,971,311 normalized instructions,
255,653,383 reference T-states, 256,000,000 actual T-states.
Use subsequent alternating pairs for before/after claims, not this warmup series.

Focused inspection covered Full's memory/cache paths, panel projection,
instruction completion, opcode prefetch, window admission/commit and their
physical-memory and diagnostic-meter callees. No recursive UI/asset review.
Prior call-count/cache evidence supplied in the brief is historical evidence,
not newly measured attribution. Inclusive timer percentages are not reused.

## Ranked hypotheses before experiments

| Path | Avoidable work / evidence | Expected impact | Fidelity risk |
| --- | --- | --- | --- |
| `FullInstructionBus::remember_{read,write}_boundary` | Rebuilds package-pin state on every machine cycle (~68M in supplied CPUTEST profile), consumed only by `finish` (~32K windows). Final pending panel cycle already retains address, write direction and data. Derive the pins there. | Medium | Low: verify final reads/writes/internal tails/INTE and Full-to-Partial rejoin. |
| `FullInstructionBus::guest_read` | Supplied trace has 99.23% hits; each hit loads valid/address/value. A packed tag/value can reduce valid+address comparisons without increasing entry size or changing mapping. | Medium, uncertain | Low: full 16-bit tags, invalid sentinel, conflicts and guest-write invalidation must remain exact. |
| `FullPanelActivity::commit_pending` | Separate same-key later/tail contributions; prior branch measured 16.9M redundant calls. Known candidate absent from current main. | Medium | Low: preserve saturation, final chronological replay and raw panel duty. |
| `FullInstructionBus::opcode_fetch` / window loop | The admission read is handed through an Option and address comparison into `Cpu8080::step`; per-instruction overhead, but avoiding it requires a semantic-core API change. | Low/medium, unmeasured | Medium: fetch/PC/wait/completion ordering. Defer behind local candidates. |
| `cycle_instruction_complete` / diagnostic meter | Runs every instruction; duplicate optional-meter gate and normalization branches, but the counters are required observable diagnostic work. | Low, unmeasured | Medium: do not disable/change metering to improve CPUTEST. |

Deprioritized: `begin/commit_full_execution_window` already copy registers only
per window; chassis range allocation is per service call, not per instruction.
`Memory::read` dispatches unique installed physical RAM without the generic
`fast_memory_read` resolver; that expensive resolver is the overlap fallback,
not the normal admitted Full hit path. Improving cache *hit rate* has little
headroom; any cache experiment must target host hit-path work instead.

## Experiment status

Boundary candidate `11fbc87` passed 10 focused Full tests. Its new 12-case test
covers empty windows, ordinary/stack reads and writes, opcode fetch, internal
tails, earlier overwritten transfers and INTE changes after the final transfer.
All five paired CPUTEST runs passed canonical assertions and favored it:

| Pair | Baseline seconds | Boundary seconds |
| --- | ---: | ---: |
| 1 | 1.593 | 1.572 |
| 2 (candidate first) | 1.662 | 1.556 |
| 3 | 1.612 | 1.601 |
| 4 (candidate first) | 1.679 | 1.640 |
| 5 | 1.708 | 1.602 |
| Median | 1.662 | 1.601 |

This is a preliminary 3.8% throughput improvement with overlapping ranges;
repeat in a three-way comparison before the final disposition. Evidence:
`target/global-review-boundary-paired.log`; exact default release executable
retained as `target/global-review-boundary.exe`.

Second experiment: pack each read-cache entry into one u32 (16-bit address tag
above 8-bit data, invalid tag outside the whole address space). Capacity remains
64 four-byte entries, with identical hash, window reset, physical miss dispatch
and write invalidation. This targets hit-path comparisons, not cache hit rate.
Candidate `97914c2` passed all 11 focused Full tests. The new cache test uses
installed physical RAM for colliding addresses, repeated hits, ordinary/stack
write invalidation and FFFFh open-bus reads.

## Final performance measurements

Seven three-way rounds, rotating execution order, same affinity/flags and no
concurrent compilation. `cache` means boundary plus packed-cache changes.

| Round | Baseline (s) | Boundary (s) | Boundary + cache (s) |
| --- | ---: | ---: | ---: |
| 0 | 1.632 | 1.610 | 1.569 |
| 1 | 1.612 | 1.603 | 1.566 |
| 2 | 1.638 | 1.620 | 1.586 |
| 3 | 1.633 | 1.577 | 1.558 |
| 4 | 1.629 | 1.591 | 1.549 |
| 5 | 1.642 | 1.599 | 1.602 |
| 6 | 1.619 | 1.588 | 1.569 |
| Median time | **1.632** | **1.599** | **1.569** |
| Median reported MHz | **156.84** | **160.09** | **163.19** |
| Time range | 1.612-1.642 | 1.577-1.620 | 1.549-1.602 |

Boundary throughput improved 2.1%; cache added 1.9%; combined throughput
improved **4.0%** (3.9% less elapsed time). Baseline and combined time ranges
do not overlap in this series. Boundary won 7/7 comparisons with baseline;
cache won 6/7 comparisons with boundary. All 21 runs passed exact diagnostic
counts. Full remained 255,006,828 T, Partial 993,172 T, Full instructions
33,928,708, Full windows/Partial entries 32,037, opcode barriers 29,488 and
budget tails 2,549. Evidence: `target/global-review-three-way.log`.

Because the cache increment is small, an independent eight-pair confirmation
alternated boundary/cache order (no excluded samples):

| Pair | Boundary (s) | Boundary + cache (s) |
| --- | ---: | ---: |
| 1 | 1.615 | 1.686 |
| 2 | 1.601 | 1.580 |
| 3 | 1.596 | 1.571 |
| 4 | 1.615 | 1.565 |
| 5 | 1.654 | 1.579 |
| 6 | 1.598 | 1.584 |
| 7 | 1.597 | 1.558 |
| 8 | 1.594 | 1.553 |
| Median | 1.5995 | 1.575 |

All 16 runs passed. Cache won 7/8 pairs; median incremental throughput gain
was 1.6%. The slow first cache run is retained. Together with the three-way
series, this supports a modest repeatable gain, not a claim of a large cache
bottleneck. Evidence: `target/global-review-cache-paired.log`.

Disposition: **keep both isolated changes**; final broad validation passed.
No temporary production instrumentation or semantic CPU changes were needed.
These are full-system CPUTEST results, not inferred microbenchmark speedups;
other workloads and GUI responsiveness were not measured.

## Correctness and rejected/deferred candidates

Focused Full tests: 10 passed after boundary change, 11 after cache change.
Existing Full/Partial raw-duty/internal-T5/PUSH equivalence, window rejoin,
physical memory, serial timing and admission-barrier tests remain intact.
The new final-pin test checks exact `Cpu8080Pins` equality for 12 cases.
CPUTEST passed 50 runs: 3 initial baseline, 10 boundary comparison, 21 three-way
and 16 cache confirmation. All builds/tests use `RUSTFLAGS=-Dwarnings`.
`cargo test --locked --all-targets`: **694 passed, zero failed, 12 existing
ignored tests**. No tests were weakened/deleted/newly ignored.
`cargo build --locked --release`: passed with warnings denied.
`git diff --check`: passed.

Fidelity: final package pins are derived from the existing retained physical
transfer and current INTE; all chronological panel projection/replay stays
unchanged. The read cache still resets at every Full window and misses/writes
reach the same bus-owned installed RAM. Full/Partial admission, READY/WAIT,
IRQ/HOLD/HLDA, tri-state, contention, open-collector and timing are untouched.

- Reject increasing cache capacity/hit rate as the next experiment: supplied
  99.23% hits leave little headroom, while the hit-path experiment showed an
  actual smaller host cost that can be reduced.
- Defer generic `fast_memory_read` resolver tuning: current Full's unique RAM
  miss path does not use it; its 188 ns microprofile is not Full hit-path cost.
- Defer per-window register copy/chassis allocation: frequency is much lower
  than the measured per-transfer opportunities; no timing claim is made.
- Defer prefetch Option elimination: requires a shared semantic-step API change
  and another correctness surface; two local candidates already show a win.
- Preserve diagnostic normalization/completion: optimizing away observations
  would make the CPUTEST comparison invalid. Its remaining branch cost is not
  quantified here.
- Highest-confidence follow-up: review the already validated internal-tail
  coalescing commit `94ece76` from `agent/performance-full-overhead-audit` against
  current main. It is absent from this base; this review does not silently merge
  that branch or claim its earlier gain as a new result.

## Commands / reproduction

```powershell
$env:RUSTFLAGS='-Dwarnings'
cargo test --locked --lib backend::cycle::full::tests
cargo test --locked --release --test cpu8080_adaptive_classic_diagnostics --no-run
```

Copy the executable printed by Cargo to `target/global-review-baseline.exe`
before changing production code; retain each candidate under a separate name.
Run comparisons from the repository root in a separate PowerShell process:

```powershell
[System.Diagnostics.Process]::GetCurrentProcess().ProcessorAffinity = [IntPtr]4
foreach ($round in 1..5) {
    $order = if ($round % 2) { @('baseline', 'candidate') } else { @('candidate', 'baseline') }
    foreach ($variant in $order) {
        Write-Output "ROUND=$round VARIANT=$variant"
        & "./target/global-review-$variant.exe" full_system_runs_cputest_with_reference_totals --ignored --nocapture --test-threads=1
        if ($LASTEXITCODE -ne 0) { throw "CPUTEST failed: $variant" }
    }
}
```

Never compile concurrently with timed runs. Generated executables/logs stay in
`target`; tables and conclusions are committed here. Final validation requires
`cargo test --locked --all-targets` and `cargo build --locked --release`.

For the three-way table, retain the Cargo executable from revisions `1690583`,
`11fbc87` and `97914c2` as baseline/boundary/cache, respectively. The measured
loop used seven rounds `0..6`, selecting
`@('baseline','boundary','cache')[($round + $offset) % 3]` for offsets `0..2`.
The confirmation used eight alternating boundary/cache pairs. The full-system
command for the current branch, independent of retained local executables, is:

```powershell
$env:RUSTFLAGS='-Dwarnings'; cargo test --locked --release --test cpu8080_adaptive_classic_diagnostics full_system_runs_cputest_with_reference_totals -- --ignored --nocapture --test-threads=1
```

Ordered checkpoints: `283f8aa` baseline/audit, `11fbc87` boundary experiment and
equivalence test, `97914c2` packed-cache experiment/test and initial results;
the final documentation commit records the completed comparisons/validation.
All work remains on the review branch; nothing was merged into main.
