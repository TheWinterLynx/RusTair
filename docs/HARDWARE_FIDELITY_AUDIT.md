# RusTair hardware-fidelity audit

Status snapshot: 2026-08-29, `agent/cycle-hardware-fidelity-audit`.

This document records verified architectural/electrical findings that should survive individual UI/debugger iterations. It deliberately distinguishes confirmed emulation errors from compatibility mirrors and from historically optional board-level fidelity.

## Source-of-truth rules

The intended machine model is:

- `Cpu8080Cycle` is the CPU authority in Cycle Accurate mode.
- `Cpu8080` remains the CPU authority in Fast mode; while Cycle is active the `AltairMachine.cpu` instance is only a compatibility mirror and must never drive the exact core during normal execution.
- `Memory` is the RAM-content/protection authority.
- `S100BusState::signals()` is the raw chassis/S-100 electrical state.
- `PanelLampIntegrator` is optical persistence/presentation only.
- Bus/T-state Teacher snapshots are observations, never machine inputs.
- External CPU-board inputs such as READY, HOLD, RESET and PINT must come from the canonical S-100/chassis contract rather than from backend-local shadow state.

## Confirmed fixes already made

### CPU package pins are view-only

Control pins in the 8080 package diagram consume backend pin/control truth directly. The UI does not reconstruct CPU signals from LEDs, machine-cycle labels or other presentation state.

Exact undriven address/data pins are displayed as HI-Z/released. A stable RESET-released STOP-WAIT is the deliberate lifecycle exception where CPU address ownership and memory-driven data are electrically known even though no numbered T-state sample is fabricated.

Fast `Reconstructed` S-100/front-panel address/data observations are **not** projected back onto A0-A15/D0-D7 package pins.

### POWER/RESET INTE coherence

Cycle Accurate seeds the cycle core from the same undefined power-on CPU sample used by the chassis, including the INTE flip-flop. RESET then establishes the documented disabled-interrupt state.

### HOLD/HLDA ownership

HOLD is an external request. In Cycle Accurate mode, HLDA remains an 8080 output: releasing HOLD does not force HLDA low before the exact CPU core clocks out of THOLD.

### READY input versus WAIT output

The old generic `set_ready()` approximation remains intentionally available to the instruction-level Fast backend, but Cycle Accurate no longer uses it as electrical truth.

Cycle has an input-only READY path. Lowering READY does **not** fabricate WAIT. The exact CPU core must clock the real transition through T2 into TW before WAIT goes high, and raising READY does not clear WAIT until the CPU itself leaves the wait state.

This distinction is covered by Cycle regressions and is also preserved for debugger pause: a host/debugger pause can lower the execution request without pretending that the physical 8080 has already produced WAIT.

Primary reference: Intel, *8080 Microcomputer Systems User's Manual*, State Transition Sequence / READY-WAIT timing (1975).

### Physical STOP, STOP during HLDA, SINGLE STEP and EXAMINE parking

Physical STOP is captured at PSYNC and then clocks the real READY-low -> T2 -> first TW/WAIT transition before host execution freezes.

A held STOP request cannot clear RUN while HLDA suppresses useful PSYNC. It remains pending and is captured at the first real PSYNC after HOLD release (or by the documented STOP+RESET recovery path), followed by the same real T2 -> TW handshake.

Cycle Accurate SINGLE STEP releases READY for one real machine cycle, keeps it released across internal timing that has no PSYNC, then uses the next actual PSYNC to withdraw READY and parks on a CPU-generated TW.

EXAMINE/EXAMINE NEXT execute the real front-panel jammed JMP/NOP sequence through the CPU and likewise park on a clocked waiting fetch rather than assigning PC or synthesizing WAIT from the GUI.

Primary references: MITS, *Altair 8800 Theory of Operation Manual & Schematics* (1975), Display/Control board operation; Intel 8080 READY/WAIT timing.

### Serial interrupts now enter through canonical S-100 PINT

The selected 88-SIO / 88-2SIO board now owns its UART interrupt-enable state and produces level-sensitive interrupt conditions. Those board conditions project onto canonical S-100 PINT rather than being injected directly into either CPU backend.

Both Fast and Cycle consume the same `S100CpuControlLines.interrupt` input:

- 88-SIO receive/transmit interrupt enables are modelled.
- 88-2SIO ACIA receive/transmit interrupt enables are modelled for both ports.
- Port 1 and debugger UART mutations refresh the canonical PINT line instead of waiting for a later unrelated CPU operation.
- EXT CLR / serial reset removes the corresponding interrupt condition.

The direct stock interrupt path supplies `FFh` (`RST 7`) during interrupt acknowledge, so the processor pushes the return PC and vectors to `0038h`.

Cycle Accurate exposes the real acknowledge machine cycle:

- `23h` status for normal interrupt acknowledge.
- `2Bh` status for interrupt acknowledge while leaving HLT.
- INTE is cleared by the CPU when the interrupt is accepted.
- The interrupting device can keep PINT asserted after acknowledge until its level-sensitive UART condition is actually removed.

Teacher/DIP-40 now explicitly distinguish:

- **INT/PINT**: Intel 8080 pin 14, CPU input / service request.
- **INTE**: Intel 8080 CPU output indicating interrupts enabled.
- **INT/SINTA**: S-100/front-panel interrupt-acknowledge status, not the request itself.

The package renderer remains view-only; pin 14 reads `BusTeachingSnapshot.interrupt` and never infers PINT from the front-panel INT lamp.

## Confirmed small timing/observation gap: PINT mutation inside an I/O T3

The interrupt request is refreshed before every Cycle CPU tick, so the 8080 receives the correct PINT level at the start of the T-state. A UART data read or control/data write can, however, create or remove its level-sensitive interrupt condition during T3.

At present the canonical S-100 PINT projection may remain at the sampled pre-transfer level until the next Cycle tick. This is usually one T-state of host-model latency, not a wrong interrupt acceptance decision, but it matters for a strict distinction between:

- the **PINT input sampled by the CPU for the exact T-state**, and
- the **current post-transfer chassis PINT level** after the UART side effect.

Required correction: preserve the sampled input in the exact Teacher snapshot, then refresh canonical PINT immediately after the T3 I/O side effect. This should become a regression where an RX interrupt is HIGH entering the `IN` T3, the exact sample still reports HIGH, and the current chassis PINT is LOW immediately after the receive buffer is consumed.

This is also a concrete example of why the Teacher ultimately needs a dual-state contract instead of treating “latest exact sample” and “current chassis state” as the same instant.

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

The current `PanelLampIntegrator` also integrates that single field, and `drive_cpu_t_state()` does not retain a distinct DI/DO data owner. Therefore the split must migrate electrical state and presentation together rather than merely renaming one field.

### Required migration

Split the data-domain contract before claiming full S-100 electrical fidelity. At minimum distinguish S-100 DI, S-100 DO and CPU D0-D7. Front-panel DATA LEDs must consume the historically correct S-100 source, while the CPU package diagram consumes CPU-pin truth.

Regression coverage should separately prove at least:

- memory/input read: peripheral/memory drives DI and CPU D-bus sees the returned byte;
- memory/output write: CPU drives DO and no fake DI source is claimed;
- interrupt acknowledge: interrupt source drives the external opcode toward the CPU;
- front-panel DEPOSIT: Display/Control write data is represented as a front-panel/bus write source, not as ordinary CPU output data;
- HOLD/HLDA: released CPU package data pins are not confused with retained front-panel DATA persistence.

Primary reference: MITS, *Altair 8800 Theory of Operation Manual & Schematics* (1975), CPU Board Operation / 8800 System Bus Structure and schematics 880-101/880-105.

## Teacher contract gap: current chassis versus latest exact T-state

The Bus/T-state Teacher retains the last exact `TickTrace` sample. This is essential for `Step T`: after one requested T-state, the user must be able to inspect exactly what happened even though host/front-panel control may immediately return the machine to a stopped condition.

At the same time, READY/PINT/HOLD/RESET/front-panel controls can change after that exact sample without another CPU tick. Therefore “latest exact T-state” and “current chassis state” can legitimately describe two adjacent instants.

Do not solve this by silently replacing the exact sample with a reconstructed/current one. The correct UI/contract is dual-state:

- **Latest exact CPU sample**: machine cycle, T-state, exact package pins and CPU inputs actually sampled for that tick.
- **Current chassis/control state**: current S-100/control inputs, RUN/STOP/READY/PINT/HOLD/RESET and stable lifecycle outputs.

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

## Recommended implementation order from this checkpoint

1. Preserve sampled PINT while immediately refreshing current post-I/O PINT after T3 side effects.
2. Introduce the dual-state Teacher contract (latest exact CPU sample + current chassis/control state).
3. Split S-100 DI/DO and CPU D-bus domains, including front-panel DATA ownership/persistence.
4. Add explicit memory-board timing/READY contributors, including an authentic MITS 1K profile.
5. Verify original RUN-while-RESET latch semantics from the exact 8800 schematic before changing it.
6. Remove synchronized compatibility mirrors only after their callers have migrated.

These changes should be made without altering Fast-mode historical compatibility unless explicitly intended and tested.
