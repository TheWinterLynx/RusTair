# RusTair Architectural Invariants

This document is the short, review-oriented statement of the rules that must remain true as RusTair evolves. It complements the more descriptive architecture in [EMULATION_ARCHITECTURE.md](EMULATION_ARCHITECTURE.md) and the detailed state ownership map in [STATE_SOURCES.md](STATE_SOURCES.md).

A contributor should be able to use this file as a pre-change and pre-review checklist.

---

## 1. One emulated machine

RusTair models one physical Altair machine.

```text
Altair chassis
├── Display/Control front panel
└── S-100 backplane
    ├── MITS 8080 CPU board
    ├── RAM cards
    ├── 88-SIO / 88-2SIO cards
    └── other installed cards
```

There must not be a hidden second CPU, RAM image, UART, card inventory or front-panel state used by a faster execution path, debugger, loader or UI.

**Review question:** does this change create a second place that can independently decide guest-visible state?

---

## 2. Cards communicate through the S-100 bus

A card may inspect the resolved S-100 bus and drive only the connector contacts it owns. Card-family logic belongs in the card, not in the generic backplane resolver.

Forbidden patterns include:

- CPU code calling a serial card directly;
- RAM code calling the CPU or front panel;
- a serial card writing CPU interrupt state directly instead of driving the appropriate S-100 interrupt line;
- the backplane branching on card family to manufacture a result;
- an I/O fast path that suppresses multiple responders or electrical contention.

Predecoded address/port responder tables are allowed only as acceleration metadata. They must preserve the same responder set and electrical result as the physical decode.

---

## 3. `Cpu8080Cycle` is the synchronization-boundary CPU authority

At exact synchronization boundaries the authoritative Intel 8080 architectural state is `Cpu8080Cycle` inside the Adaptive Cycle backend.

It owns the canonical:

- A/B/C/D/E/H/L registers;
- flags;
- PC and SP;
- INTE and delayed-EI state;
- HALT/HOLD/reset-related processor state;
- exact T-state and machine-cycle progression.

The instruction-level `Cpu8080` executor is allowed to own a transient exported copy **only while an admitted Full window is active**. The completed result is committed back to `Cpu8080Cycle` at the synchronization boundary.

There must never be a persistent `Cpu8080` mirror that can drift from the exact core.

---

## 4. Partial is the electrical/timing oracle

Adaptive Cycle has two internal execution strategies:

- **Partial** — exact T-state/clock-edge execution through the live S-100 fabric;
- **Full** — accelerated semantic execution when equivalence can be proven.

Partial defines the reference behavior for hardware-observable timing.

A Full optimization is acceptable only if it preserves, at the required observation boundaries:

- total Intel 8080 T-state count;
- architectural CPU state;
- memory contents and protection behavior;
- external machine-cycle schedule represented by the optimization;
- S-100-visible final/boundary state;
- interrupt-enable timing;
- READY/HOLD/RESET/interrupt semantics;
- front-panel raw state and T-state-weighted lamp duty where applicable.

If equivalence cannot be proved, execution must stay in Partial.

---

## 5. Full admission is conservative

Full is not a target coverage percentage that must be maximized at any cost. A fallback is correct behavior when the physical machine may observe an event that the Full representation cannot reproduce exactly.

Typical reasons to remain Partial include:

- I/O cycles not explicitly represented by a safe Full path;
- pending interrupts at sensitive boundaries;
- HOLD/HLDA;
- RESET;
- HLT;
- READY/wait behavior that cannot be analytically reproduced;
- an instruction whose intermediate external schedule has not been proved equivalent;
- a boundary that would publish an impossible CPU-board/S-100 state.

Every new Full instruction family needs focused Full-versus-Partial oracle tests.

---

## 6. Physical RAM has one storage authority

Guest-visible RAM bytes belong to the installed runtime RAM cards in `S100RuntimeFabric`.

All of these must ultimately refer to that same storage:

- Partial guest memory cycles;
- Full semantic reads/writes;
- debugger peek/write operations;
- RAM Viewer;
- loaders;
- diagnostics;
- memory-activity inspection.

A cache may contain derived acceleration metadata or cached read values, but it must have explicit invalidation/synchronization rules and must never become a second independently mutable memory image.

Open-bus, overlap, protection and wait behavior are properties of the physical card/decode model, not conveniences of a flat 64 KiB host array.

---

## 7. S-100 electrical resolution remains card-agnostic

`S100Backplane` resolves connector drives. It may optimize representation with masks, cached drives and driver counts, but the logical result must still follow the declared electrical contracts:

- High-Z means no active drive;
- open-collector outputs may pull low or release;
- normal/tri-state outputs drive according to their enabled state;
- multiple drivers are retained;
- opposing drives produce contention rather than silently choosing one card;
- floating/unclaimed space is not converted into synthetic device data.

The resolver must not know that a slot contains a CPU, RAM or serial card in order to resolve the bus.

---

## 8. Front-panel raw state comes from the machine, not the UI

ADDRESS, DATA, STATUS, INTE and related raw panel-visible values are derived from canonical machine/S-100 state.

The GUI may apply visual persistence/brightness integration, but visible LED intensity is presentation state only.

Forbidden shortcuts include:

- deriving ADDRESS lamps from PC merely because the CPU is executing;
- deriving DATA lamps from a register when the actual bus differs;
- reading a brightness integrator back into hardware logic;
- making Full report only final lamp bits when correct duty requires time-weighted activity.

The physical panel controls also act through the machine/bus model; UI widgets do not directly edit CPU registers to mimic EXAMINE/DEPOSIT/RUN/STOP.

---

## 9. Host execution speed is not peripheral clock speed

Authentic/2×/5×/10×/Unlimited are host CPU scheduling policies. They do not change the installed MITS CPU board's historical clock definition or the physical rate selected on an independently clocked peripheral.

The 88-SIO/88-2SIO baud generator is therefore a separate physical-time domain. A configured 110-baud channel remains 110 baud in every CPU speed mode. Host wall time is used only by the serial scheduler to meter elapsed physical time into the installed card; COM2502/MC6850 bit, frame, status, oscillator/divider phase and interrupt state remain owned by that card and resulting outputs still propagate through the normal S-100 connector graph.

In throttled CPU modes the application replays CPU debt and serial physical time on one causal timeline using **card-owned event deadlines**, not a fixed host slice. `execution_frame.rs` asks for the earliest installed serial deadline before a normal CPU service interval:

- quiet UARTs publish no deadline and retain the normal 4096-T-state Adaptive service slice;
- an active 88-SIO publishes its next effective UART boundary;
- an active 88-2SIO retains its free-running external 16× tap phase but publishes the next effective MC6850 `/1`, `/16` or `/64` boundary rather than every intermediate tap pulse.

A guest `OUT` to installed serial hardware is a causal synchronization barrier. The Adaptive executor stops before the output, exact T-state progression across the barrier remains inside the Cycle backend, elapsed pre-write serial physical time is settled, and scheduling is then replanned from the changed card state. A newly activated transmitter must never inherit physical time from before the guest write.

An independently clocked active UART is not by itself a reason to pin CPU execution in Partial. Full may run between observable serial boundaries because the card is advanced only when serial physical time is explicitly settled. Guest serial I/O remains an exact barrier.

Unlimited has no CPU-to-wall-time ratio and therefore uses the backend's direct `Instant` physical-time source instead. Transitions between managed throttled timing and Unlimited must have one explicit handoff boundary so elapsed serial time is neither replayed twice nor dropped.

The GUI repaint cadence is never a serial clock. STOP, RESET/HOLD parking and HALT may stop CPU T-states while physical UART time continues; conversely, a fast Full CPU window must not multiply baud.

CPU/chassis-time peripherals such as the current DCDD/FD-400 mechanics retain their own explicit virtual-time path. Do not reuse the serial physical-time scheduler to advance unrelated devices merely because both need elapsed time.

Endpoint pacing and card baud are also separate configuration domains. ASR-33, terminal, TCP or COM configuration must not silently restrap the installed card. Deliberate mismatches are valid configurations even where analog/remote-bit corruption is outside the current endpoint fidelity claim.

`execution_clock.rs`, `execution_frame.rs` and the backend scheduler may decide when host work is performed, but they must not:

- skip modeled guest T-states;
- multiply or divide a selected serial baud because CPU execution speed changed;
- make READY/HOLD/interrupt events synthetic UI state;
- cap Unlimited execution at repaint frequency;
- make `CycleHostBackend` own an exact T-state loop;
- turn a presentation timer into a second hardware clock authority;
- merge serial physical time with DCDD/chassis virtual time.

See [SERIAL_CLOCK_DOMAINS.md](SERIAL_CLOCK_DOMAINS.md) for the full current timing contract and validation evidence.

---

## 10. Serial cards own UART state

The installed physical 88-SIO/88-2SIO card instance owns guest-visible status/data/interrupt state and the progress of its COM2502/MC6850 shift registers.

ASR-33, text terminal, TCP and COM support are external endpoints. They may exchange bytes/signals with the installed serial card through the configured cable/router path but must not own a duplicate UART or complete a UART frame on the card's behalf. Endpoint mechanics/presentation may impose their own downstream rate (for example the ASR-33 printer/distributor), but that rate is not a substitute for card baud timing.

Electrical interface choices and modem/control signal polarity are part of the physical configuration. Directly incompatible interfaces must not be silently connected through an invisible converter.

---

## 11. Configuration is desired topology, not live hardware state

`S100HardwareConfig` describes the desired mounted hardware inventory. The live physical authority is the runtime fabric built from that configuration.

Changing cards/straps requires POWER OFF because the operation models physically moving or reconfiguring hardware.

Legacy aggregate configuration fields may be accepted as migration inputs, but new execution must normalize to the slot-native model rather than keep a parallel legacy topology alive.

---

## 12. Debugger and teaching tools are observers/controllers, not authorities

The debugger may request stops, breakpoints, watchpoints, run-to and controlled memory edits through backend contracts. Teaching/history tools may retain snapshots and infer loops/call stacks.

They must not:

- keep a shadow CPU that drives execution;
- maintain a shadow RAM map;
- infer a call stack and then use it as actual CPU stack state;
- infer a loop and alter execution based on the inference;
- let a frozen UI snapshot feed back into guest hardware.

History can be incomplete. Inference should be conservative and label uncertainty.

---

## 13. Quick Load and Authentic Load are deliberately different

A direct/quick loader is an emulator convenience path and may write physical RAM through a documented host control interface.

Authentic loading must execute the historical bootstrap and transport bytes through the emulated peripheral/serial hardware path.

Do not make Authentic Load faster by secretly writing the destination bytes directly to RAM.

---

## 14. Compatibility workarounds are explicit

Historical bugs, awkward behavior or software assumptions are not silently repaired in the base hardware model.

If a compatibility workaround is needed:

- it must be named and visible;
- it must be opt-in unless the project explicitly documents a different policy;
- it must not redefine the baseline hardware claim;
- it needs a focused regression proving when it applies and when it does not.

---

## 15. Digital fidelity is not an analog-electronics claim

RusTair models digital ordering and bus/card behavior at the documented level. The exact 8080 timing model decomposes T-states into PHI1/PHI2 digital edges.

This does **not** claim simulation of analog transistor-level effects such as:

- voltage slew;
- exact pulse shape;
- trace impedance;
- analog propagation-delay distributions;
- metastability;
- power-supply ripple/noise.

When a feature depends on an analog effect that RusTair does not model, document the non-claim rather than implying circuit-level simulation.

---

## 16. Tests and documentation are part of the architecture

Architecture guards are intentional. A test that prevents a second CPU mirror or synthetic memory path is not "overly coupled" merely because it constrains structure: the structure is part of the fidelity model.

When changing an invariant:

1. update the implementation;
2. update or add the relevant oracle/architecture tests;
3. update current architecture/source/runtime-flow documentation;
4. preserve historical performance/fidelity records as historical evidence rather than rewriting them to match the new code.

---

## 17. Reviewer checklist

Before approving a hardware/core change, answer all of these:

- Where is the authoritative state before and after the change?
- Does any new cache/mirror exist, and if so is it strictly derived with explicit invalidation?
- Could two physical cards that should be electrically independent now call each other directly?
- Does Partial still produce the same exact result?
- If Full changed, what proves Full→Partial equivalence and boundary rejoin?
- Are READY, HOLD, RESET, HLT, INTE and interrupt timing still sampled at the correct point?
- Do open-bus/overlap/contention cases still behave physically rather than conveniently?
- Does the front panel still report bus truth rather than CPU/UI guesses?
- Did host scheduling or debugger instrumentation accidentally become guest-visible?
- Are independent time domains still independent, especially serial physical time versus CPU/chassis time?
- Are configuration and persistence still describing one live topology?
- Are the appropriate focused and broad tests present?
