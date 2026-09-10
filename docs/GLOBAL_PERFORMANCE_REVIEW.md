# Focused runtime performance review

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
Focused tests and real-workload comparison are pending. Keep/revert it based on
incremental performance versus the boundary-only executable, not baseline alone.

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
