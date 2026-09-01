# RusTair hardware-fidelity audit

Status snapshot: **historical CPU/chassis checkpoint from 2026-08-29**, `agent/cycle-hardware-fidelity-audit`.

Current base-hardware closeout status is tracked in `docs/BASE_HARDWARE_FIDELITY_CLOSEOUT.md`. In particular, the 88-SIO and 88-2SIO serial workstreams that were still separate from this snapshot are now **PASS** after final local validation on 2026-09-02. The architectural-debt list below is retained as the historical CPU/chassis audit record and must be re-audited against current code before treating any item as an active blocker.

This document is the durable checkpoint for the Altair 8800 / Intel 8080 electrical-fidelity work. It separates verified fixes from remaining architectural debt. UI presentation must consume backend/chassis truth; it must never become an electrical authority.

## Canonical source-of-truth rules

- `Cpu8080Cycle` is the CPU authority in Cycle Accurate mode.
- `Cpu8080` is the CPU authority in Fast mode. The `AltairMachine.cpu` instance still present while Cycle is active is a compatibility mirror only; normal Cycle execution does not read it back as CPU authority.
- `Memory` owns RAM contents, protection and board timing profile.
- `S100BusState::signals()` owns current raw S-100/chassis electrical state.
- CPU-board inputs READY, HOLD, RESET and PINT come from that canonical chassis contract.
- CPU outputs WAIT, HLDA, INTE and acknowledge/status are driven by CPU-board samples, not inferred from front-panel lamps.
- `PanelLampIntegrator` is optical persistence/presentation only.
- Bus/T-state Teacher snapshots are observations, never machine inputs.

## Completed and regression-guarded

### RESET and RUN/STOP latch

8080 RESET clears processor reset-defined state without clearing the programmer-visible general registers/SP/flags that Intel leaves unspecified across RESET.

The original Altair Display/Control RUN/STOP R-S latch is now modelled independently from RESET:

- RESET itself preserves the physical RUN latch.
- RUN can asynchronously set the latch while RESET/PRESET is held.
- STOP requires the qualifying processor synchronization opportunity; while HLT/HLDA/RESET prevents it, STOP remains pending.
- Cycle Accurate captures a held STOP at the first real post-reset/resumed T1/PSYNC and clocks the genuine T2 -> TW/WAIT transition before host execution freezes.
- Fast approximates the same event at its reconstructed first-fetch boundary because it has no exact sub-instruction PSYNC.

Primary reference: MITS, *Altair 8800 Theory of Operation Manual & Schematics* (1975), Display/Control board.

### READY input versus WAIT output

Cycle Accurate has a true input-only READY path. Lowering READY does not fabricate WAIT; the 8080 must clock from T2 into TW before WAIT is asserted. Raising READY likewise does not clear WAIT until the CPU leaves TW.

The Fast backend retains its explicitly approximate instruction-level READY/WAIT reconstruction.

Primary reference: Intel, *8080 Microcomputer Systems User's Manual* (1975), READY/WAIT timing.

### HOLD/HLDA, physical STOP, SINGLE STEP and EXAMINE

- HOLD is an external request; HLDA remains a CPU output in Cycle Accurate mode.
- Releasing HOLD does not force HLDA low before the exact core clocks out of THOLD.
- Physical STOP is captured at a real PSYNC and parks through a real T2 -> TW handshake.
- STOP asserted during HLDA remains pending until the CPU owns the bus and produces the next qualifying PSYNC.
- Physical SINGLE STEP releases READY for one real machine cycle and parks at the next real external PSYNC/TW, continuing across internal timing cycles that have no PSYNC.
- EXAMINE/EXAMINE NEXT execute the real front-panel jammed JMP/NOP sequence through the CPU instead of assigning PC from the GUI, then park on a clocked waiting fetch.
- Debugger logical T-state/machine-cycle/instruction stepping remains separate from these physical panel semantics.

### CPU D bus, S-100 DI/DO and front-panel DATA are separate

The Intel 8080 package's bidirectional D0-D7 and the original Altair CPU board's separate S-100 DI/DO domains are no longer collapsed.

The canonical sample/state distinguishes:

- CPU package data bus,
- S-100 data-in,
- S-100 data-out,
- front-panel retained/display data.

Regression coverage verifies read, write, interrupt-acknowledge, front-panel jam/deposit and released-bus cases. The DIP-40 renderer consumes package-pin truth and does not project reconstructed Fast S-100 data back onto CPU pins.

Primary reference: MITS 8800 CPU Board / System Bus schematics 880-101/880-105.

### Canonical S-100 PINT and serial-card interrupts

The selected 88-SIO / 88-2SIO board owns its interrupt-enable state and generates level-sensitive UART interrupt conditions. These project onto canonical S-100 PINT rather than being injected directly into a CPU backend.

Both Rust backends consume the same PINT source. The stock direct path supplies `FFh` (`RST 7`) during interrupt acknowledge, vectoring to `0038h`.

Cycle Accurate exposes the real acknowledge cycles:

- `23h` status for normal INTA,
- `2Bh` status for INTA while waking HLT,
- CPU-owned INTE clearing on acceptance,
- level-sensitive PINT remaining asserted until the UART condition is removed.

A UART mutation during T3 refreshes current PINT immediately after the I/O side effect, while the exact Teacher sample deliberately preserves the PINT level the CPU actually sampled for that T-state.

Teacher/DIP-40 explicitly distinguish CPU **INT/PINT** input, CPU **INTE** output, and S-100/front-panel **SINTA** acknowledge status.

### Canonical INTE

The redundant `AltairBus.cpu_inte` boolean has been removed. `S100Signals.inte` is the sole raw chassis/S-100 INTE state; Fast and Cycle adapters update it through the common CPU-board contract. CI contains a guard against reintroducing the old mirror.

### Teacher historical sample versus current chassis

`BusTeachingSnapshot` now has an explicit dual-state contract:

- the latest exact/reconstructed CPU observation, and
- `current_chassis`, representing the current controls/S-100 state after any later host or device mutation.

Exact READY/PINT/HOLD/RESET values remain historical inputs sampled with the displayed T-state. A subsequent debugger pause or I/O side effect does not rewrite that sample.

### MITS 1K RAM READY timing

Memory now has explicit board timing profiles including `Mits1KStatic1975` and a no-wait profile.

The authentic MITS 1K profile pulls the memory READY contribution low so reads insert the documented two wait cycles. Cycle Accurate exposes the two real TW states; Fast accounts for the equivalent instruction-level wait time. READY recovers normally for continuous execution.

Primary reference: MITS *Altair 8800 Theory of Operation Manual & Schematics* (1975), 1K Static Memory Board “Processor Slow Down Circuit”, 880-107/880-108.

### Backend authority

Regression tests deliberately poison the legacy `AltairMachine.cpu` object while Cycle Accurate is running and prove that `Cpu8080Cycle` remains execution authority. The mirror is therefore architectural debt in this historical snapshot, not a demonstrated split-brain execution bug.

## Remaining work recorded by the 2026-08-29 snapshot

The entries in this section are preserved for traceability. They are **not automatically current blockers**; compare them with present code before resuming this audit.

### 1. Remove the Cycle compatibility `AltairMachine.cpu` mirror

At this snapshot, Cycle still copied exact registers/HALT/INTE/timing into a second `Cpu8080` because several common chassis/front-panel helpers were originally written around the Fast CPU object. Power-on undefined CPU-state generation also passed through that structure.

Recorded safe removal order:

1. replace shared chassis reads of `cpu.halted`/CPU-specific state with explicit CPU-agnostic inputs;
2. move undefined power-on register generation into a neutral state structure/function;
3. seed Fast and Cycle from that neutral lifecycle state independently;
4. remove `sync_machine_cpu()` consumers;
5. only then remove the duplicate Cycle `Cpu8080` object.

Do not replace it with another shadow CPU-state structure that becomes a second authority.

### 2. Propagate Cycle core faults explicitly

At this snapshot, some Cycle execution loops could stop when `TickTrace.fault` was present while returning success to higher layers. A future re-audit should verify whether backend errors/diagnostics now carry such faults explicitly.

### 3. Preserve chassis state during Cycle memory reconfiguration

At this snapshot, the unpowered Cycle reconfiguration path could rebuild the backend while preserving only selected settings. A future re-audit should verify that RAM reconfiguration cannot discard unrelated chassis/device state.

## Broader architecture backlog recorded by the snapshot

These were considered worthwhile but not required to keep this historical electrical audit open:

- harden `BackendHost` error handling and remove panic-style application paths;
- gate UI operations strictly through backend capabilities;
- restrict concrete `machine()/machine_mut()/into_machine()` escape hatches;
- add a regression preventing `src/app` from depending on concrete machine/CPU internals;
- eventually consolidate duplicated RUN representation after callers are migrated.

Serial/UART base hardware is no longer an open item here: its current status is PASS in `docs/BASE_HARDWARE_FIDELITY_CLOSEOUT.md`.

## Validation policy

Long CPUTEST/8080EXM runs were repeatedly certified during the CPU/chassis audit and are not executed on every unrelated checkpoint. Focused hardware/authority regressions plus the normal all-target suite are the default. Long CPU diagnostics should be run again when CPU semantics/timing are changed or for a dedicated release/CPU certification pass.
