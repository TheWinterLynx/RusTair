# RusTair hardware-fidelity audit

Status snapshot: 2026-08-29, `feature/didactic-ram-debugger`.

This document records verified architectural/electrical findings that should survive individual UI/debugger iterations. It deliberately distinguishes confirmed emulation errors from compatibility mirrors and from historically optional board-level fidelity.

## Source-of-truth rules

The intended machine model is:

- `Cpu8080Cycle` is the CPU authority in Cycle Accurate mode.
- `Cpu8080` remains the CPU authority in Fast mode; while Cycle is active the `AltairMachine.cpu` instance is only a compatibility mirror and must never drive the exact core during normal execution.
- `Memory` is the RAM-content/protection authority.
- `S100BusState::signals()` is the raw chassis/S-100 electrical state.
- `PanelLampIntegrator` is optical persistence/presentation only.
- Bus/T-state Teacher snapshots are observations, never machine inputs.

## Confirmed fixes already made

### CPU package pins are view-only

Control pins in the 8080 package diagram consume backend pin/control truth directly. The UI does not reconstruct CPU signals from LEDs, machine-cycle labels or other presentation state.

Exact undriven address/data pins are displayed as HI-Z/released. A stable RESET-released STOP-WAIT is the deliberate lifecycle exception where CPU address ownership and memory-driven data are electrically known even though no numbered T-state sample is fabricated.

Fast `Reconstructed` S-100/front-panel address/data observations are **not** projected back onto A0-A15/D0-D7 package pins.

### POWER/RESET INTE coherence

Cycle Accurate now seeds the cycle core from the same undefined power-on CPU sample used by the chassis, including the INTE flip-flop. RESET then establishes the documented disabled-interrupt state.

### HOLD/HLDA ownership

HOLD is an external request. In Cycle Accurate mode, HLDA remains an 8080 output: releasing HOLD does not force HLDA low before the exact CPU core clocks out of THOLD.

### STOP while HLDA is active

A held STOP request cannot clear RUN while HLDA suppresses useful PSYNC. It remains pending and is captured at the first real PSYNC after HOLD release (or by the documented STOP+RESET recovery path).

## Confirmed remaining electrical error: READY versus WAIT

Current `S100BusState::set_ready()` couples the two lines:

```rust
self.signals.ready = ready;
self.signals.wait = !ready && !self.signals.reset;
```

That is only a convenient stopped/running approximation; it is not exact 8080 timing.

Intel 8080 documentation defines:

- READY: CPU input used by memory/I/O to request wait states.
- READY is sampled during T2/TW.
- If READY is low, the processor enters TW at the end of T2.
- WAIT: CPU output acknowledging that the processor is actually in the wait state.

Therefore Cycle Accurate must not make raw WAIT an instantaneous inverse of READY. READY ownership must be external/chassis arbitration; WAIT ownership must remain the exact CPU state/output.

### Required migration

1. Separate an input-only READY mutation from Fast-mode approximation helpers.
2. Ensure Cycle RUN/STOP changes READY without fabricating WAIT.
3. On physical STOP/SINGLE STEP, let the exact CPU reach the T2 -> TW boundary before presenting stable WAIT.
4. Preserve debugger pause semantics separately from the physical STOP switch; a debugger host pause is not automatically an Altair STOP electrical event.
5. Add lifecycle tests proving the sequence PSYNC -> READY low -> T2 -> TW/WAIT high and the reverse when READY is released.

Primary reference: Intel, *8080 Microcomputer Systems User's Manual*, State Transition Sequence / READY-WAIT timing (1975), especially the description that TW is entered at the end of T2 and WAIT acknowledges entry into TW.

## Confirmed machine-fidelity gap: original MITS 1K RAM wait states

The 1975 MITS *Altair 8800 Theory of Operation Manual & Schematics*, section **1K Static Memory Board Operation**, documents a Processor Slow Down Circuit. Intel 8101 RAM required about 850 ns for stable read data, so the MITS board pulled PRDY low to insert **two wait cycles (approximately 1 microsecond) on reads**.

RusTair currently models RAM capacity/content/protection but not a memory-board timing profile or a READY contributor. Consequently the Cycle Accurate CPU can execute exact wait-state mechanics when READY is supplied, but the abstract RAM does not request the waits an original MITS 1K static board would have requested.

This must **not** be fixed by globally adding two waits to every memory read. Later/faster memory boards differ. The correct architecture is explicit memory-board/timing modelling (or an explicit historically labelled timing profile) feeding READY arbitration.

Primary reference: MITS, *Altair 8800 Theory of Operation Manual & Schematics* (1975), p. 7, “PROCESSOR SLOW DOWN CIRCUIT”, schematics 880-107/880-108.

## Confirmed architecture gap: S-100 DI and DO are collapsed

The Intel 8080 package has one bidirectional D0-D7 bus, but the original Altair CPU board buffers it onto separate S-100 data directions. The MITS system-bus documentation/schematics distinguish:

- DI0-DI7: data into the CPU side / data returned by memory or I/O.
- DO0-DO7: data driven out by the CPU side.

The original front-panel DATA indication is tied to the relevant bus input/display path; it is not conceptually identical to the 8080 package's bidirectional D pins at every instant.

RusTair currently collapses this into `S100Signals.data: u8` and `S100CpuSample.data: Option<u8>`. This forces one field to mean several different things depending on context:

1. CPU D0-D7 electrical value,
2. S-100 input data,
3. S-100 output data,
4. front-panel visible/retained DATA value.

### Required migration

Split the data-domain contract before claiming full S-100 electrical fidelity. At minimum distinguish S-100 DI, S-100 DO and CPU D0-D7. Front-panel DATA LEDs must consume the historically correct S-100 source, while the CPU package diagram consumes CPU-pin truth.

Primary reference: MITS, *Altair 8800 Theory of Operation Manual & Schematics* (1975), CPU Board Operation / 8800 System Bus Structure and schematics 880-101/880-105.

## Teacher contract gap: current chassis versus latest exact T-state

The Bus/T-state Teacher currently retains the last exact `TickTrace` sample. This is essential for `Step T`: after one requested T-state, the user must be able to inspect exactly what happened even though host/front-panel control may immediately return the machine to a stopped condition.

At the same time, READY/HOLD/RESET/front-panel controls can change after that exact sample without another CPU tick. Therefore “latest exact T-state” and “current chassis state” can legitimately describe two adjacent instants.

Do not solve this by silently replacing the exact sample with a reconstructed/current one. The correct UI/contract is dual-state:

- **Latest exact CPU sample**: machine cycle, T-state and exact package pins from `TickTrace`.
- **Current chassis/control state**: current S-100/control inputs, RUN/STOP/READY/HOLD/RESET and stable lifecycle outputs.

The Freeze control may preserve either/both explicitly, but must never imply that a historical exact T-state is the current chassis instant.

## Compatibility mirrors: debt, not currently confirmed split-brain bugs

### `AltairMachine.cpu`

Fast mode still needs the instruction-level `Cpu8080`. Cycle mode mirrors the exact core into that structure for legacy chassis helpers. The reverse direction is used deliberately only to seed the common undefined power-on sample. No normal Cycle execution path has been found that reloads the exact core from the mirror.

Target: make the chassis CPU-core agnostic and remove this mirror, but do not delete it blindly while Fast/front-panel helpers depend on it.

### `AltairMachine.running` versus S-100 RUN

Two representations remain. Audited write paths currently update them together. Consolidation is desirable, but no active divergence has yet been demonstrated.

### `AltairBus.cpu_inte`

This is a compatibility mirror used by the instruction-level CPU-board adapter. The exact cycle path updates both this mirror and S-100 INTE in `cycle_drive_s100_t_state()`. It is architectural duplication, but the normal Cycle path is currently synchronized.

## Historical RUN/STOP latch detail still to verify before changing

MITS describes RUN/STOP as an R-S flip-flop, with STOP gated to the appropriate PSYNC condition. The current code prevents RUN assertion while RESET is held. Before changing that behavior, verify the exact original 8800 Display/Control schematic/latch gating (not only later variants or prose) and add a hardware-sequence regression test.

## Recommended implementation order

1. READY input / WAIT output separation in Cycle Accurate.
2. Physical STOP and SINGLE STEP parking through the real T2 -> TW transition.
3. Dual-state Teacher contract (current chassis + latest exact CPU sample).
4. Split S-100 DI/DO and CPU D-bus domains.
5. Explicit memory-board timing/READY contributors, including an authentic MITS 1K profile.
6. Remove synchronized compatibility mirrors only after their callers have migrated.

These changes should be made without altering Fast-mode historical compatibility unless explicitly intended and tested.