# MITS RAM hardware fidelity

Status: **PASS — bus-visible digital RAM hardware fidelity is closed for the supported MITS RAM inventory.**

Closeout date: **2026-09-12**.

Closeout implementation was promoted to `main` through commit `a27d7ea794e6d2e8326c97e1ab61645fb06eb09c` after focused RAM timing/oracle tests, the complete local test suite, `cargo test --locked --all-targets`, and `cargo build --locked --release` all passed. No GitHub Actions were used for this closeout.

## Scope and fidelity claim

This PASS covers the **digital, bus-visible behavior** of the supported MITS RAM boards in RusTair:

- board identity, capacity/population and address-window decoding;
- one authoritative physical RAM storage instance per installed card;
- S-100 DI/DO/MWRT behavior, open bus and overlapping responders;
- card-scoped protection where the historical board supports it;
- documented CPU-visible READY/WAIT behavior;
- documented dynamic-RAM refresh scheduling where it can affect CPU-visible behavior;
- exact Full/Partial accounting of dynamic-RAM timing at synchronization boundaries.

It does **not** claim analog DRAM-cell simulation. Capacitor leakage, analog charge decay, component tolerances, temperature-dependent retention margin, exact pulse rise/fall time and destructive failure after missed refresh remain outside the model.

`FastRamCompatibility` is a migration/compatibility device, not part of this historical-hardware PASS.

## Supported physical boards

| Model | Capacity | Digital timing / refresh claim |
| --- | ---: | --- |
| MITS 88-MCS / 88-1MCS | 256 B–1 KiB populated | Original static board; two read WAIT states retained. |
| MITS 88-4MCD | 4 KiB | Refresh every 32 CPU clock periods; a selected access colliding with refresh receives the documented one- or two-WAIT delay depending on phase. |
| MITS 88-S4K | 4 KiB | CPU-synchronous dynamic refresh; corrected later MITS T4 detection uses the trailing edge of pDBIN while sM1 is asserted; refresh does not assert PRDY in normal operation. STOP/HLTA refresh is represented. |
| MITS 88-4MCS | 4 KiB | Static no-WAIT board at the modeled 2 MHz-class Altair timing boundary. |
| MITS 88-16MCS | 16 KiB | Static no-WAIT board at the modeled boundary. |
| MITS 88-16MCD | 16 KiB | On-board crystal-controlled refresh; normal full-front-panel operation is processor-transparent/no-WAIT. STOP/HLTA refresh behavior is represented at the digital boundary. |

## Physical hardware and timing notes

### 88-4MCD

The contemporary MITS product description specifies 4,096 words, a 420 ns maximum access time, automatic refresh every 32 clock pulses at SYNC time, and one or two processor WAIT states when an addressed access coincides with refresh. RusTair models both collision phases rather than flattening them into a single generic wait.

The exact 8080 edge ordering matters: at the beginning of T1, the CPU-board clock edge occurs before SYNC becomes visible later in the T-state. RusTair therefore preserves whether refresh became due on the immediately preceding clock edge so the two-WAIT phase is not misclassified as the older one-WAIT phase.

### 88-S4K

The 88-S4K is a synchronous dynamic board. The later MITS correction, documented in the November 1976 addendum and described in W. Howard Adams' *An 88-S4K Design Update*, replaces the original free-running PHI1-count method of locating T4 with the trailing edge of pDBIN while sM1 is active. RusTair follows this corrected/later behavior because it remains valid across inserted WAIT states.

The digital model advances the synchronous refresh cadence from PHI2 and consumes pending refresh in the hidden M1 T4 slot without asserting PRDY. STOP/HLTA refresh is represented through the documented parked-CPU path.

### 88-16MCD

The 88-16MCD manual describes an on-board crystal-controlled refresh sequencer that advances through all 64 RAM row addresses. The manual states the refresh interval as approximately 15 microseconds per row, giving about 0.96 ms for all 64 rows, within the required 1 ms interval. It also documents separate RUN-low STOP behavior and HLTA-assisted refresh.

For a full front-panel system the manual states that the board does not use WAIT in normal operation. RusTair therefore keeps 88-16MCD refresh processor-transparent at the digital S-100 boundary instead of fabricating CPU stalls merely because the physical board contains DRAM.

## RusTair ownership model

| Physical element | RusTair owner | Contract |
| --- | --- | --- |
| Installed RAM model/address/population | `S100RamCardConfig` / `S100RamBoardModel` in `src/s100_memory.rs` | Historical board identity and validated physical configuration. |
| Guest RAM bytes | `RuntimeRamState::bytes` in `src/s100_runtime_ram.rs` | The one authoritative storage used by guest execution and host inspection tools. |
| Protection latches | `RuntimeRamState::protected` | Card-owned state; not a host shadow map. |
| Partial/exact refresh phase and WAIT state | `RuntimeRamState` | Updated from actual S-100 observations in exact Partial execution. |
| Adaptive Full 88-4MCD / 88-S4K timing window | `RuntimeRamTimingWindow` in `src/s100_runtime_ram/full_timing.rs` | Transactional copy of timing latches only; never contains guest RAM bytes or protection. Commits back at Full boundaries. |
| 88-16MCD refresh effect at CPU boundary | board model + runtime card state | Processor-transparent/no-WAIT in normal full-front-panel operation. |

The architecture deliberately does not contain a second flat 64 KiB guest-memory authority.

## Supporting implementation contracts

`S100RamBoardModel::timing_model()` keeps the board-specific CPU-visible timing distinction explicit:

```rust
Mits1KStatic88Mcs => FixedReadWaits(2),
Mits4KDynamic88_4Mcd => RefreshCollision {
    interval_clocks: 32,
    min_waits: 1,
    max_waits: 2,
},
Mits4KSynchronous88S4K
| Mits4KStatic88_4Mcs
| Mits16KStatic88_16Mcs
| Mits16KDynamic88_16Mcd => NoWait,
```

`RuntimeRamTimingWindow` is an acceleration of timing state, not an alternate RAM device: the common 88-4MCD and 88-S4K paths snapshot only refresh/WAIT phase, run locally for the Full window, and commit those latches back to the same installed runtime card.

## Adaptive Full versus Partial

Partial remains the exact electrical/T-state oracle. It advances the installed RAM card by observing the real S-100 signal sequence, including CLOC, SYNC, PHI2, sM1, pDBIN, RUN, HLTA, PRDY and memory-cycle signals as applicable.

Full is allowed to accelerate supported historical RAM only when the rest of the chassis is Full-safe. It must preserve the same external timing result:

- 88-4MCD advances the same 32-clock refresh phase and returns one/two TW states into semantic instruction timing when a selected access collides;
- 88-S4K advances the same PHI2/T4 refresh phase without fabricating PRDY;
- 88-16MCD remains CPU-transparent because its on-board refresh has no normal CPU-visible WAIT effect;
- Full timing state is committed before exact Partial resumes;
- long differential regressions require equal T-state totals, instruction progression, registers, guest RAM and front-panel state.

The optimized Full representation is checked against a canonical runtime-state oracle so performance work cannot silently change the physical timing rules.

## Regression evidence

Key focused regressions include:

- `four_mcd_pending_before_sync_inserts_one_wait` — an already-pending refresh collision inserts exactly one WAIT;
- `four_mcd_refresh_due_on_sync_inserts_two_waits` — the just-due CLOC/SYNC phase inserts exactly two WAITs using real edge ordering;
- `four_mcd_write_collision_keeps_refresh_active_until_mwrt` — write-side collision behavior survives through MWRT;
- `full_four_mcd_timing_preserves_both_collision_phases` — Full preserves both 4MCD one/two-WAIT phases;
- `s4k_refresh_uses_pdbin_falling_edge_as_hidden_t4` — corrected T4 detection follows pDBIN falling while sM1 is asserted;
- `s4k_request_after_pdbin_fall_waits_for_next_m1_t4` — a late refresh request is not consumed in an already-passed hidden T4;
- `full_s4k_timing_consumes_pending_refresh_without_waiting` — Full advances S4K refresh without PRDY;
- `full_s4k_request_born_on_m1_t5_remains_pending_until_next_m1` — refresh requested after hidden T4 remains pending until the next valid slot;
- `s4k_pending_refresh_completes_while_cpu_is_stopped` — parked-CPU refresh path remains live;
- `specialized_four_mcd_window_matches_runtime_state_oracle` and `specialized_s4k_window_matches_runtime_state_oracle` — 1024-step optimized-window differential oracles;
- `dynamic_historical_ram_full_matches_forced_partial_exactly` — long Full-versus-forced-Partial equality across 88-4MCD, 88-S4K and 88-16MCD;
- `tests/memory_wait_timing.rs` and the broader S-100/open-bus/protection tests protect integration behavior.

The complete local suite, all targets and release build were green at closeout on 2026-09-12.

## User-observable validation

A normal user can validate the broad RAM/card contract through the S-100 hardware editor plus the front panel, RAM viewer and Bus Teacher:

1. power off and install a supported RAM board at a valid base address;
2. power on, deposit/read known bytes and verify the RAM viewer observes the same physical storage rather than a separate image;
3. for protect-capable boards, assert protection and verify writes are rejected while reads remain stable;
4. use exact/teaching views around RAM accesses to confirm READY/WAIT behavior where the board is documented to stall the CPU;
5. repeat equivalent workloads under Adaptive execution and verify no guest-visible memory or CPU-state divergence occurs when Full is admitted.

The distinction between the 4MCD one-WAIT and two-WAIT refresh phases and the exact S4K hidden-T4 phase is timing-sensitive and is primarily certified by deterministic automated regressions rather than by a convenient manual UI procedure.

## Performance evidence at closeout

Performance is not part of the fidelity claim, but it verifies that historical dynamic RAM can remain modeled without forcing the emulator into the exact Partial path for every T-state.

Pinned local ceiling microbenchmark on 2026-09-12, median of 5 rounds × 250,000,000 emulated T-states after 5,000,000 T warm-up:

| Configuration | Median throughput | Same-round paired cost vs static peer |
| --- | ---: | ---: |
| 88-4MCS 4K static | 355.392 MHz | baseline |
| 88-4MCD 4K dynamic | 225.546 MHz | -36.06% (1.56x baseline/candidate) |
| 88-S4K synchronous 4K | 282.241 MHz | -19.72% (1.25x) |
| 88-16MCS 16K static | 353.261 MHz | baseline |
| 88-16MCD 16K dynamic | 351.116 MHz | -0.19% (1.00x) |

These are host-specific ceiling measurements, not product guarantees. Their purpose is to show that digital refresh/WAIT fidelity remains active inside Adaptive Full rather than being bypassed.

## Validation history

- **2026-09-12** — 88-4MCD phase-latch fix made exact Partial agree with Full for one/two-WAIT collisions.
- **2026-09-12** — long Full/Partial historical-RAM differential green for 88-4MCD, 88-S4K and 88-16MCD.
- **2026-09-12** — specialized Full timing windows matched canonical RAM timing oracles.
- **2026-09-12** — stabilized paired benchmark recorded the closeout performance above.
- **2026-09-12** — `cargo test --locked`, `cargo test --locked --all-targets` and `cargo build --locked --release` reported green locally.
- **2026-09-12** — implementation promoted to `main` by fast-forward. No GitHub Actions were run.

## Known gaps / non-goals

There are no known blockers to the **bus-visible digital RAM fidelity** claim for the supported board inventory.

Explicit non-goals remain:

- analog DRAM-cell charge/leakage/retention simulation;
- per-chip analog failure or marginal timing due to voltage, temperature or component tolerance;
- transistor/gate-level propagation-delay simulation;
- claiming every undocumented board revision or field modification as supported;
- treating `FastRamCompatibility` as historical hardware.

A future primary source that contradicts a modeled timing assumption reopens the affected board's PASS until source, implementation and regressions are reconciled.

## Primary references

1. **MITS, Altair Computer Report / system brochure (1975), 88-4MCD description, p. 11.** Specifies 4,096 words, 420 ns maximum access, automatic refresh every 32 clock pulses at SYNC time, and one/two WAIT states on collision. Archive: https://www.bitsavers.org/pdf/mits/8800/Altair_Computer_Report_1975.pdf
2. **MITS, 88-4MCD "4K RAM Board" documentation, September 1975.** Circuit description, schematic, assembly/test and errata; archive/catalog description: https://www.retrotechnology.com/herbs_stuff/d_altair.html
3. **MITS, Altair 88-S4K Synchronous 4K RAM Card documentation (1976/1977 revisions).** Logic descriptions, schematics, timing and refresh material. Archive: https://deramp.com/downloads/altair/hardware/MITS%2088-S4K%204K%20Synchronous%20RAM%20Card.pdf
4. **W. Howard Adams, "An 88-S4K Design Update," S-100 Microsystems, Vol. 2 No. 2, March/April 1981, p. 48.** Documents the MITS November 1976 addendum changing T4 detection from free-running PHI1 counting to the trailing edge of pDBIN, and explains the WAIT-state failure mode of the earlier method. Archive: https://bitsavers.org/magazines/S-100_Microsystems/v02n02.pdf
5. **MITS, Altair 88-16MCD Technical Manual, 1st Printing, July 1977, Theory of Operation §2-4 Refresh Cycle and §2-5 System Options (manual pp. 18–20).** Documents crystal-controlled refresh, 64 row counts, approximately 15 µs per row / 0.96 ms total, STOP/HLTA behavior, and normal full-front-panel no-WAIT operation. Archive: https://deramp.com/downloads/altair/hardware/MITS%2088-16MCD%2016K%20Dynamic%20RAM.pdf

## Result

**RAM hardware fidelity closeout: PASS.**

Within the stated digital S-100 scope, the supported MITS RAM inventory has no known fidelity blocker. Future RAM work should be treated as a new scoped enhancement, performance optimization, newly discovered revision/source correction, or explicit analog-model expansion rather than leaving this closeout indefinitely in progress.
