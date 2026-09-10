# Larger Full runtime optimization investigation

The user raised the target to 50-100% throughput improvement. The earlier 4%
result is preserved separately and does not meet that target. Baseline for this
investigation: `0a4402e` on `agent/codex-global-performance-review`.

## Sampling evidence

Windows WPR CPU recording failed with missing performance-profiling privilege
(`0xc5585011`), even outside the sandbox. The local process-only sampler in
`tools/profile_full_cpu.ps1` instead launches CPUTEST, selects its hottest
thread, briefly suspends it to read RIP, always resumes it in `finally`, and
resolves symbols with DbgHelp. It samples approximately every 1 ms. No production
timing hooks or disabled hardware observations are used.

The normal stripped binary produced misleading nearest-symbol attribution;
discard that initial profile. Build matching symbols for the package:

```powershell
$env:RUSTFLAGS='-Dwarnings'
cargo --config profile.release.package.rustair.debug=1 --config profile.release.package.rustair.strip=false test --locked --release --test cpu8080_adaptive_classic_diagnostics --no-run
powershell -NoProfile -ExecutionPolicy Bypass -File tools/profile_full_cpu.ps1 -Executable target/release/deps/cpu8080_adaptive_classic_diagnostics-bf46a6f3077b0dfe.exe
```

Use the executable path printed by Cargo if its hash differs. Five runs passed
canonical assertions. Of 4,346 successful RIP samples (5 failed attempts):

| Exclusive symbol attribution | Share |
| --- | ---: |
| `FullPanelActivity::add_histogram` | 28.2% |
| `service_execution_compiled` (includes inlined Full/semantic code) | 23.9% |
| `FullPanelActivity::project_machine_cycle` | 13.8% |
| `FullInstructionBus::read` | 7.4% |
| `record_cpu_diagnostic_instruction` | 3.7% |
| `resolve_cached_selected_drives` | 2.6% |
| `set_cpu_package_pins` | 2.4% |

This is statistical, compiler-symbol-level attribution, not exact function
timing. Inlined callees belong to their containing symbol. Setup/teardown can
contribute. Do not use sampled workload wall time for speedup claims.
Evidence: `target/global-review-sampled-symbols.log`.

Panel recording accounts for about 42% of samples. Even eliminating all of it
would imply at most about 72% higher throughput; reaching +50% requires removing
about 80% of this cost. Small cache/branch tweaks are not a credible plan for
that target.

## Candidate: independent byte-duty accumulation

Replace hashing complete address/data/status states with four independent
256-bin duty distributions: address low byte, address high byte, DATA and
latched STATUS, plus INTE/PROT on-time. Lamp duty depends on those individual
bit counts, not cross-lamp correlations. Keep the newest physical machine
cycle and predecessor presentation for chronological final replay.

This removes hashing, collision probing, composite-key rebuilding and repeated
address accumulation at T1/later states. The distributions occupy the same
4 KiB as the old histogram. They are a transient representation of the same
observations, not another CPU/RAM/bus state. Final folding must use the canonical
panel integrator and preserve all weights, latch delay, retained DATA and final
pins. Near integrator saturation, conservatively retain Partial execution.

Candidate implemented. Four distributions now replace the composite histogram;
address weight is added once per cycle, first/later DATA and STATUS preserve the
8212 delay, and the existing predecessor/final cycle is replayed canonically.
Package pins continue to come from the retained final transfer. No RAM/card path
or semantic CPU code changed.

Focused validation passed with warnings denied: all 12 Full-path tests, plus
the canonical marginal-counter test. The test-only original histogram is kept
unchanged as an independent observer oracle. Eight seeded sequences of 1,000
varied read/write/fetch/stack cycles, including large internal tails, matched
raw duty, final status and retained DATA. All 256 status bytes and INTE/PROT
combinations match the canonical integrator's exact integer counter planes.
Admission at the u64 integrator-capacity boundary is tested conservatively.

Uninstrumented release measurement and full validation are pending. Do not yet
claim a speedup or that the raised target has been achieved.
