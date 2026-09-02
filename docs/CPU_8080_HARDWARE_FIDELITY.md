# Intel 8080 / MITS 8800 CPU-board hardware fidelity

Status: **READY FOR FINAL LOCAL CERTIFICATION — implementation complete at the documented digital edge/cycle boundary; PASS awaits the post-closeout release gate.**

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

`tick_with_pin_edges_split()` keeps PHI1-side and PHI2-side package inputs separable for an external CPU-board synchronizer while the ordinary edge API remains available to callers with a stable sample.

The semantic T-state transition occurs at PHI2; PHI1-owned WAIT, /WR and HLDA are preserved across that transition rather than being overwritten by a synthetic all-at-once T-state projection.

### READY / WAIT

The Cycle core now distinguishes **READY being LOW** from **the processor actually being in TW**:

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
- split PHI1/PHI2 input samples without retroactive edge changes;
- HOLD dwell, HLDA and high-impedance bus state;
- HOLD release: PHI2 internal clear, next-PHI1 HLDA release, following-PHI2 bus recovery;
- backend HOLD/STOP interaction using the same physical sequence;
- S-100 PHI1/PHI2/CLOC as canonical CPU-board clock state, including CLOC retention through dead time;
- MITS 8212 status-latch timing;
- CPU D vs S-100 DI vs S-100 DO vs front-panel DATA separation;
- PINT vs SINTA separation and external INTA opcode path;
- RESET and RUN/STOP/READY interactions;
- Fast/Cycle architectural differential and classic 8080 diagnostics.

## 6. Existing architectural certification

Before the final edge-closeout changes, local release certification on 2026-09-02 was green and reported the established exact diagnostic totals:

| Diagnostic | Instructions | T-states |
|---|---:|---:|
| 8080PRE.COM | 1,061 | 7,817 |
| TST8080.COM | 651 | 4,924 |
| CPUTEST.COM | 33,971,311 | 255,653,383 |
| 8080EXM.COM | 2,919,050,698 | 23,803,381,171 |

The edge changes are intentionally below the architectural T-state count. The final gate nevertheless reruns the complete certification so this document does not infer that invariance.

## 7. Final PASS gate

Mark this document **PASS** only after the branch containing the completed edge/CPU-board changes is green for all of the following:

1. `cargo test --release`
2. `cargo test --release --test cpu8080_cycle_differential`
3. `cargo test --release --test cpu8080_cycle_classic_diagnostics -- --include-ignored --nocapture`
4. exact classic totals remain those listed above.

When that post-closeout gate is green, the Intel 8080 + original MITS 8800 CPU-board digital hardware block is closed. The next hardware-fidelity block may then be the MITS 88-VI.
