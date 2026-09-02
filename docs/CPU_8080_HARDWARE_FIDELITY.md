# Intel 8080 / MITS 8800 CPU-board hardware fidelity

Status: **PASS — final local release certification green on 2026-09-02 at the documented digital edge/cycle boundary.**

Documentation standard: `docs/HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`.

## 1. Fidelity claim boundary

RusTair's Cycle engine claims **digital edge/cycle fidelity** for the Intel 8080 package plus the original MITS 8800 CPU-board/S-100 boundary. It does not claim transistor/SPICE or analog electrical simulation.

In scope:

- authoritative PHI1/PHI2 edge ordering;
- documented package-pin transitions at those edges;
- READY/TW/WAIT and HOLD/HLDA sequencing;
- address/data ownership and high-impedance release under HOLD;
- RESET, INT/INTE and interrupt acknowledge behavior;
- MITS CPU-board buffering between the bidirectional 8080 D bus and S-100 DI/DO;
- the MITS 8212 status-latch relationship to SYNC + PHI1;
- S-100 PHI1, PHI2 and the separately retained 2.000 MHz CLOC digital clock net;
- PSYNC, PDBIN, /PWR and the existing canonical S-100 status/control domains.

Explicit non-claims:

- exact analog voltage/current magnitude;
- rise/fall shape and line impedance/loading;
- propagation-delay dispersion and marginal setup/hold violations;
- crystal/one-shot component tolerance and nanosecond pulse-width variation;
- transistor/gate-by-gate SPICE behavior where it has no separately observable digital consequence.

## 2. Primary sources

### Intel 8080

Primary Intel 8080 hardware/timing documentation is the authority for package behavior. The implemented contract includes:

- READY is sampled during T2/TW; a LOW sample selects/continues TW;
- WAIT does not pre-assert merely because READY is LOW before the sampling edge: WAIT rises on the leading PHI1 of the actual TW state;
- when READY permits exit from TW at PHI2, WAIT remains asserted through that PHI2 and falls on the following PHI1;
- HOLD is accepted at the processor sampling edge and HLDA subsequently changes in the PHI1 domain;
- on HOLD release, the internal HOLD latch clears at PHI2, HLDA remains HIGH through that edge and returns LOW following the next PHI1;
- address/data remain released while HLDA is HIGH and are restored only after the documented release sequence;
- address/data, SYNC and DBIN transitions retain their PHI2-referenced ownership;
- /WR remains PHI1-owned;
- INTE changes are exposed on their documented processor timing boundary.

Primary reference family: Intel, *8080/8080A Microcomputer Systems User's Manual* and Intel 8080 timing/state-transition documentation preserved by Bitsavers.

### Original MITS 8800 CPU board

Primary reference: MITS, *Altair 8800 Theory of Operation Manual & Schematics* (1975), schematics 880-101 through 880-103:

- https://vtda.org/docs/computing/MITS/MITS_Altair8800TheoryOperation_1975.pdf

The board-level contract implemented from that documentation is:

- the 8080 bidirectional D0-D7 package bus is split by CPU-board buffers into S-100 DI and DO;
- address, data, status and command/control signals are buffered at the CPU-board boundary;
- PRDY and PHOLD are synchronized to the leading edge of PHI2 before affecting processor timing;
- the CPU board contains the 2.000 MHz crystal oscillator and generates the two non-overlapping processor phases;
- TTL PHI1 and PHI2 are exported to S-100 pins 25 and 24;
- CLOC is independently buffered to S-100 pin 49 rather than inferred by consumers from PHI1/PHI2;
- processor status is emitted on D0-D7 at machine-cycle start and captured by the MITS 8212 status latch when SYNC and PHI1 coincide.

Jim Drygiannakis' MIT-licensed `jdryg/8080Emu` remains an attributed independent implementation cross-check; it is not the primary authority over Intel/MITS documentation.

## 3. Implemented execution shape

### Edge engine

`Cpu8080Cycle` exposes four ordered digital clock edges per T-state:

```text
PHI1 rising
PHI1 falling
PHI2 rising
PHI2 falling
```

The test-only split-input helper keeps PHI1-side and PHI2-side package inputs separable for focused regressions. Production Cycle uses the live-PHI2 input path so external S-100 hardware may react after PHI1 and still be sampled correctly by the processor at PHI2.

The semantic T-state transition occurs at PHI2; PHI1-owned WAIT, /WR and HLDA are preserved across that transition rather than being overwritten by a synthetic all-at-once T-state projection.

### READY / WAIT

The Cycle core distinguishes **READY being LOW** from **the processor actually being in TW**:

```text
T2 PHI1: WAIT LOW
T2 PHI2: sample READY LOW -> select TW
TW PHI1: WAIT HIGH
TW PHI2: sample READY
...
TW PHI2 with READY HIGH -> select T3
following T3 PHI1: WAIT LOW
```

This prevents a half-cycle-early WAIT assertion/release.

### HOLD / HLDA

HOLD release is explicitly two-edge rather than host-call instantaneous:

```text
THOLD, HLDA HIGH
PHI1: HLDA remains HIGH
PHI2: sample PHOLD/HOLD LOW; clear internal HOLD state
      HLDA remains HIGH; address/data remain released
next PHI1: HLDA LOW
next PHI2: CPU bus drive resumes
```

This prevents the backend from restoring the CPU bus while the documented package HLDA output is still asserted.

### MITS CPU-board clocks

The canonical S-100 state contains independent digital nets for:

- PHI1;
- PHI2;
- CLOC.

CLOC retains its previous logic level through the PHI1/PHI2 non-overlap intervals, so consumers do not lose its state when both phase pins are LOW. At the project's digital claim boundary, the exact Cycle edge sequence provides one CLOC cycle per CPU T-state; analog one-shot widths and component tolerances remain outside scope.

### Status and bus mapping

The MITS CPU-board adapter retains the physical split:

```text
Intel 8080 D0-D7 <-> CPU-board buffers <-> S-100 DI / DO
```

Status traffic leaving the processor uses the CPU/DO domain and cannot masquerade as S-100 DI/front-panel DATA. The 8212 status latch is updated by the CPU-board edge path rather than reconstructed from panel lamps.

The 88-2SIO PRDY regression also exercises the CPU-board boundary across a genuine sub-T-state dependency: SINP is latched at T2 PHI1, the card may pull PRDY LOW before T2 PHI2, and PWAIT at TW PHI1 releases it before the following PHI2 sample. This validates that external hardware can affect READY on the documented edge without moving the physical S-100 transition earlier.

## 4. Interrupt boundary

Interrupt acknowledge remains a bus transaction. No serial/device card is allowed to call an internal CPU `interrupt(RSTn)` shortcut.

The physical contract for the next 88-VI block remains:

```text
peripheral IRQ output
    -> S-100 VI0..VI7
    -> installed MITS 88-VI
    -> S-100 PINT
    -> MITS CPU-board INT buffer
    -> Intel 8080 INT

8080 interrupt acknowledge
    -> CPU status / MITS 8212
    -> S-100 SINTA
    -> interrupting hardware drives opcode on S-100 DI
    -> MITS CPU-board input buffer
    -> Intel 8080 D0-D7
    -> CPU samples external opcode
```

The CPU therefore knows PINT/INT and the byte presented during INTA; it does not know which future VI input caused the request.

## 5. Regression coverage

Focused regressions now cover, among other existing CPU tests:

- four non-overlapping PHI1/PHI2 edges per T-state;
- PHI2-owned T1 address/status/SYNC behavior;
- /WR assertion on PHI1 of a real memory-write cycle;
- READY sampled at T2/TW PHI2;
- WAIT assertion only on actual TW PHI1 and release on the following PHI1;
- PHI1/PHI2 input changes without retroactive edge changes;
- live external input settlement between PHI1 and PHI2;
- HOLD dwell, HLDA and high-impedance bus state;
- HOLD release: PHI2 internal clear, next-PHI1 HLDA release, following-PHI2 bus recovery;
- backend HOLD/STOP interaction using the same physical sequence;
- S-100 PHI1/PHI2/CLOC as canonical CPU-board clock state, including CLOC retention through dead time;
- MITS 8212 status-latch timing;
- CPU D vs S-100 DI vs S-100 DO vs front-panel DATA separation;
- PINT vs SINTA separation and external INTA opcode path;
- exact 88-2SIO `SINP -> PRDY LOW -> TW/PWAIT -> PRDY HIGH` interaction;
- RESET and RUN/STOP/READY interactions;
- Fast/Cycle architectural differential and classic 8080 diagnostics.

## 6. Final local certification

The completed CPU-board branch was locally release-certified green on 2026-09-02 after the final edge-closeout and 88-2SIO/PRDY correction. The reported green gate covered:

1. `cargo test --release`
2. `cargo test --release --test cpu8080_cycle_differential`
3. `cargo test --release --test cpu8080_cycle_classic_diagnostics -- --include-ignored --nocapture`

The classic-diagnostic regressions enforce the established exact reference totals:

| Diagnostic | Instructions | T-states |
|---|---:|---:|
| 8080PRE.COM | 1,061 | 7,817 |
| TST8080.COM | 651 | 4,924 |
| CPUTEST.COM | 33,971,311 | 255,653,383 |
| 8080EXM.COM | 2,919,050,698 | 23,803,381,171 |

Because the full gate was reported green, those exact-reference assertions also passed; this PASS is not inferred from earlier runs.

## 7. PASS conclusion

**PASS.** The Intel 8080 plus original MITS 8800 CPU-board digital hardware block is closed at the documented edge/cycle fidelity boundary above.

A future primary-source discrepancy or reproducible regression may reopen a specific claim. Otherwise the CPU board should not be revisited as generic fidelity debt.

The separate architectural idea of representing the whole machine as an explicit S-100 backplane with uniform plug-in card interfaces is intentionally **PARKED/backlog** after the initial scaffold on `agent/s100-card-backplane-architecture`; it is not a blocker to this CPU-board fidelity PASS.
