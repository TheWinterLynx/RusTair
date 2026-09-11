# S-100 connector bus encoding performance

> Historical performance investigation. Branches, profiles, results and pending work below describe the recorded revisions, not current production. Later changes include control-flow/DI/EI fidelity and forced projection inlining (`b15512e`). Retained `target/` executables and logs are local artifacts, not repository dependencies; rerun the maintained benchmarks for current results.

Baseline: `2184186` (updated `main`). Scope: `S100CardDrive` address/data writers.

## Finding and change

The existing resolver/observer profiler isolates CPU connector updates from
resolution and card observation. Address updates repeatedly called `set_pin`,
clearing and rewriting the low, high and open-collector planes for each of the
16 contacts. These updates all target the same two words in each plane.

The bus writers now gather HIGH bits from the existing physical pin mapping and
replace each plane once. The implementation uses constant-size arrays and no
allocation, additional persistent cache, CPU/card shortcut or runtime feature
detection. Both execution strategies continue to use the existing physical
machine and resolver.

Every bus contact receives exactly the same strong LOW/HIGH drive as before;
previous open-collector ownership is cleared only on those contacts. Unrelated
contacts are preserved. No observations, propagation deltas, driver counts,
tri-state behavior, contention, READY/WAIT, IRQ or virtual clocks are changed.

The regression compares the complete connector drive with independent per-signal
writes for all 65,536 addresses and all 256 data values, starting from each of
the four drive states. Equality includes unrelated contacts and all three planes.

## Measurement method

Windows x86-64, Ryzen 9 5900X, Rust 1.97.1, MSVC target, release profile,
`RUSTFLAGS=-Dwarnings`, existing Cargo.lock. No benchmark code was changed.
Baseline executables were saved before editing production code. Three rounds
alternate baseline/candidate order, with both inheriting affinity mask `4` from
the measurement shell. Builds and other validation run outside measured rounds.

Use the existing `s100_hotpath_profile` and `s100_resolver_observer_breakdown`
ignored profilers, and the existing
`full_system_forced_partial_samples_8080exm_50m_t_states` benchmark in
`cpu8080_forced_partial_classic_diagnostics`. The latter executes exactly 50M
T-states through the physical CPU board, four RAM cards, 88-2SIO and front panel;
its 17T service calls deliberately keep Full at zero.

The supported performance claim is lower address-connector encoding and active
address-edge cost. This is not a claim that every workload, or a Full-dominated
compute loop, becomes equally faster.

## Results

Medians of three runs; lower is better:

| Existing profiler row | Baseline | Candidate | Cost reduction |
| --- | ---: | ---: | ---: |
| R1: address source update + cache | 41.84 ns | 19.93 ns | 52.4% |
| Runtime address edge, CPU + RAM | 99.71 ns | 80.12 ns | 19.6% |
| Runtime address edge, with 88-2SIO | 112.26 ns | 89.75 ns | 20.1% |
| Generic physical `fast_memory_read` | 183.57 ns | 180.28 ns | 1.8% |
| Forced Partial 8080EXM, 50M T | 24.147 s | 24.633 s | No demonstrated improvement |

The Partial timings were 24.147/23.746/25.839 s before and
24.098/24.633/25.050 s after. Their overlapping ranges do not support an
end-to-end speedup claim. The consistent address-edge improvement is the reason
to retain this small connector-encoding change. Generic transaction throughput
and the broader CPU workload are effectively unchanged at this measurement's
precision. A preceding resolved-bus decoding experiment was discarded after
failing to demonstrate a consistent wider benefit and worsening generic reads.

Candidate measurements can be repeated with:

```powershell
$env:RUSTFLAGS='-Dwarnings'; cargo test --locked --release --test s100_hotpath_profile --test s100_resolver_observer_breakdown -- --ignored --nocapture
cargo test --locked --release --test cpu8080_forced_partial_classic_diagnostics full_system_forced_partial_samples_8080exm_50m_t_states -- --ignored --nocapture
```

For comparable timing, use the same machine, toolchain, release flags and
processor affinity for both revisions. Build baseline `2184186` separately and
retain its test executables before building the candidate; alternate execution
order rather than timing a build or mixing measurements with concurrent builds.

## Validation

- `cargo test --locked --all-targets`: 692 passed; 12 existing manual tests ignored.
- `cargo build --locked --release`: passed.
- Release CPUTEST: 33,971,311 reference instructions and 255,653,383 reference
  T-states matched; 256,000,000 actual machine T-states in 1.768 s. This standalone
  correctness run is not a before/after throughput comparison.
- Both existing S-100 profilers and the 50M-T Partial sample passed in all paired
  measurement rounds. No tests were removed, weakened or newly ignored.

All Cargo validation used `RUSTFLAGS=-Dwarnings`. GitHub Actions were neither
modified nor invoked.
