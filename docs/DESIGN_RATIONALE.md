# RusTair Design Rationale

This document explains **why** the current architecture looks the way it does. `EMULATION_ARCHITECTURE.md` describes the current structure; `ARCHITECTURAL_INVARIANTS.md` states the rules that must remain true. This file records the engineering rationale behind those rules so a future contributor does not accidentally "simplify" away an intentional fidelity property.

The decisions below are architectural, not claims that no implementation detail can ever change.

---

## 1. Model the computer, not only the software interface

### Decision

Represent the Altair as a chassis, a shared S-100 backplane, plug-in cards and a bus-connected Display/Control panel.

### Why

A pure software-compatibility emulator can make memory a byte array and I/O ports a switch statement. That is sufficient for many programs but loses behavior that RusTair explicitly wants to expose:

- front-panel ADDRESS/DATA/STATUS behavior;
- wait states and READY timing;
- bus ownership/HOLD;
- multiple selected cards;
- High-Z/open bus;
- protection and board-level address decode;
- interrupt wiring;
- historically meaningful board configuration.

### Consequence

The physical architecture constrains the software architecture. A card should not directly call another card just because both are Rust objects.

### Rejected simplification

A central `match port { ... }` or flat memory dispatch that is itself the guest-visible hardware authority. Predecoded responder indices are permitted only when they retain the same physical responder set and still execute card/backplane behavior.

---

## 2. Keep one authoritative architectural CPU

### Decision

At synchronization boundaries, `Cpu8080Cycle` is the Intel 8080 architectural authority.

### Why

The project historically explored faster/instruction-oriented execution models. Maintaining multiple persistent CPU objects creates a difficult class of synchronization bugs:

- PC/registers can drift;
- EI/DI delayed state can differ;
- HALT/interrupt state can diverge;
- reset semantics can update one object and not the other;
- debugging shows whichever copy happened to be queried.

### Consequence

Full may use a transient instruction-level `Cpu8080` only inside an admitted window. It is exported from and committed back to the exact authority at explicit boundaries.

### Rejected simplification

A permanent Fast CPU mirror synchronized at instruction boundaries.

---

## 3. Use an exact path as the oracle for an accelerated path

### Decision

Adaptive Cycle contains exact **Partial** execution and accelerated **Full** execution over one machine.

### Why

A fully exact board/bus model is valuable but expensive on a modern host because a single guest T-state can trigger multiple host-side edge/card/resolver operations. A separate instruction-level emulator would be faster but would create a second machine model.

The Adaptive approach gets both:

- Partial supplies a trustworthy physical reference;
- Full removes host work only where the omitted work can be reconstructed/proved equivalent.

### Consequence

Optimization work has a built-in differential oracle. Coverage expands conservatively rather than by assumption.

### Rejected alternatives

- two user-selectable Fast/Cycle machines with synchronization/mirrors;
- always-instruction-level execution with approximate panel/bus behavior;
- always-edge-level execution with no attempt to recover host performance.

---

## 4. Full and Partial are implementation strategies, not product modes

### Decision

Do not expose Full versus Partial as user-selectable emulators.

### Why

They do not represent different historical machines. They are internal ways of advancing the **same** state. Making them product modes would imply that a user can choose fidelity versus speed and would encourage divergence between two implementations.

### Consequence

The user chooses host execution speed (Authentic/5×/10×/Unlimited), while the engine decides internally when Full is safe.

---

## 5. Treat physical RAM cards as the memory store

### Decision

The installed runtime RAM cards own guest-visible bytes.

### Why

A flat shadow array would make the normal case fast but break the meaning of:

- unmapped address space;
- overlapping boards;
- per-board protection;
- card-specific timing/waits;
- debugger/RAM Viewer physical mapping;
- future hardware that shares or competes for address space.

### Consequence

Full, Partial, loaders and debugger tools must reach the same underlying card storage. Fast paths can cache decode/results but cannot own an independent RAM image.

---

## 6. Keep the backplane resolver ignorant of card family

### Decision

`S100Backplane` resolves electrical drives, not device semantics.

### Why

In real hardware the backplane does not know that slot 1 is CPU or slot 5 is serial. It only connects conductors. Keeping this separation:

- makes contention/open bus explicit;
- prevents central-device shortcuts;
- allows future cards to use the same contract;
- makes electrical tests independent of device-family implementation.

### Consequence

Card-family assembly belongs in `S100RuntimeFabric`; electrical resolution belongs in `S100Backplane`.

---

## 7. Predecode fixed hardware straps, but preserve electrical results

### Decision

Compile fixed RAM/I/O responder information when the POWER-OFF hardware inventory is mounted.

### Why

A real card's TTL decoder evaluates the same fixed address straps repeatedly. Re-running software configuration logic for every bus edge is unnecessary host work.

A precomputed responder bitmask is therefore analogous to compiling the fixed hardware decode, provided it does not change which cards participate.

### Consequence

Multiple responders remain multiple responders. The mask selects which physical card implementations need to run; it does not decide the final data value itself.

### Rejected simplification

A table containing the final read byte/device result, which would bypass live card state and contention.

---

## 8. Separate CPU package behavior from the MITS CPU board

### Decision

Model Intel 8080 package pins/timing separately from the MITS 8080 S-100 board that translates/latches them.

### Why

The Altair does not expose the bare CPU pins directly as its entire system bus. Board logic such as the 8212/status handling and S-100 signal conditioning has its own behavior and timing.

### Consequence

A processor change and a CPU-board change have different review surfaces/tests. Full→Partial reconciliation must restore board-level retained state as well as CPU registers.

---

## 9. Keep raw panel state separate from visual LED persistence

### Decision

Model raw bus/panel signals independently from the host-side optical persistence used to draw LEDs.

### Why

Physical LEDs are visually persistent compared with fast bus transitions. A GUI that renders only the latest raw value flickers poorly; a GUI that treats smoothed brightness as electrical truth is incorrect.

### Consequence

Raw state can be tested exactly while a separate integrator produces usable visual brightness. Full acceleration accumulates time-weighted duty rather than merely publishing final lamp bits.

---

## 10. Treat panel duty as separable observable time, not a giant transition log

### Decision

Where mathematically valid, accumulate front-panel lamp duty using compact marginal distributions instead of recording/replaying every intermediate panel sample.

### Why

The physical display cares about the time each lamp is on. Storing every complete `(address,data,status,...)` combination introduces huge host overhead and correlation that individual lamps do not need.

### Consequence

Optimized accumulation must remain checked against an independent/reference implementation. If a future visual observable depends on cross-signal correlation, the representation must be revisited instead of assuming marginal independence.

---

## 11. Configuration describes physical installation

### Decision

Use a slot-native `S100HardwareConfig` as the desired physical inventory.

### Why

Aggregate settings such as "RAM size" or one singleton serial configuration cannot faithfully represent multiple boards, overlaps, straps or slot identity.

### Consequence

Normal hardware editing is a POWER-OFF operation. Legacy aggregate fields are migration inputs, not parallel live hardware models.

---

## 12. Keep host endpoints outside the serial-card model

### Decision

ASR-33, text terminal, TCP and physical COM are endpoints attached to an emulated serial port; they are not alternate UART implementations.

### Why

The guest should observe the same card registers/timing regardless of which external device is attached. Mixing host transport with UART state would make hardware behavior endpoint-dependent.

### Consequence

Serial routing/cabling is explicit. Physical interface compatibility matters. Host code can service bytes/control lines but the installed card owns guest-visible status/data/interrupt state.

---

## 13. Model chip behavior and board behavior separately when useful

### Decision

For the 88-2SIO, keep MC6850 chip semantics in `mc6850.rs` and board-level dual-port/strap/wiring behavior in `machine/two_sio.rs`.

### Why

A board is more than its UART IC: clocking, decode, straps, interface circuitry and interrupt wiring matter. Conversely, encoding all MC6850 register semantics directly into the board makes the implementation harder to reason about/test.

### Consequence

Chip-level changes and board-level changes can have focused tests while still composing into one physical card.

---

## 14. Quick Load and Authentic Load solve different user goals

### Decision

Keep an explicit convenience path and an explicit historical path.

### Why

Developers/users often need fast deterministic program loading, while fidelity work needs to exercise real bootstrap + serial + paper-tape behavior. Trying to make one path satisfy both goals either makes development painfully slow or makes the "authentic" path fake.

### Consequence

Quick Load may write real RAM via host control. Authentic Load must move bytes through the emulated peripheral/card path.

---

## 15. Make compatibility workarounds visible

### Decision

Do not silently change baseline hardware behavior to satisfy a specific program.

### Why

A hidden compatibility fix damages the emulator as a hardware research tool and makes debugging impossible: the same configuration no longer means the same machine.

### Consequence

Compatibility behavior is explicit, named and independently testable. Historical behavior remains reproducible.

---

## 16. Keep debugger state derived

### Decision

Instruction history, inferred call stacks, loop detection, memory activity and teaching snapshots are observations.

### Why

These tools are valuable precisely because they can be discarded/rebuilt without changing the guest. If execution begins depending on inferred/history state, debugging instrumentation changes what it observes.

### Consequence

Breakpoints/watchpoints may control when execution stops, but the emulated instruction/hardware result is still owned by the machine.

---

## 17. Separate host scheduling speed from guest clock identity

### Decision

Authentic/5×/10×/Unlimited determine how aggressively the host advances virtual time; they do not alter the historical board clock model.

### Why

A 2 MHz Altair running "10×" should behave like the same hardware simulated ten times faster in wall-clock time, not like a fictional 20 MHz 8080 board with changed device timing relationships.

### Consequence

Peripheral timing remains expressed in emulated time. Unlimited scheduling must not become tied to GUI repaint frequency.

---

## 18. Preserve historical validation/performance records

### Decision

Do not rewrite old measurement documents just because current architecture changed.

### Why

Rejected experiments and old bottleneck measurements explain why current design choices exist. Erasing them makes future contributors repeat the same work.

### Consequence

Current docs describe present architecture; historical docs retain their original branch/measurement context and get banners/cross-links when necessary.

---

## 19. Keep architecture guards as tests

### Decision

Use tests not only for output correctness but also to prevent reintroduction of known-bad structural patterns.

### Why

Some regressions compile and run programs correctly for a while yet corrupt the long-term model—for example adding a second CPU mirror or a hidden synthetic RAM mapping.

### Consequence

A source-structure/authority test can be legitimate even when it looks more coupled than a conventional unit test. The constrained architecture is itself part of the emulator's fidelity contract.

---

## 20. Optimize measured host work, not modeled guest work

### Decision

Performance optimization should remove redundant **host computation** while retaining required **guest events**.

### Why

The emulator is intentionally low-level. Deleting guest-visible bus/clock behavior to save CPU time defeats its purpose. But repeated allocation, scanning, decoding or recomputation that produces the same physical result is fair optimization territory.

### Consequence

Profile first, prove equivalence, benchmark the normal release artifact, and preserve independent exact/reference oracles where the production representation is aggressively optimized.

---

## 21. How to change a design decision safely

These decisions are not sacred merely because they are documented. If evidence shows one is wrong or prevents a better fidelity model:

1. state the physical/historical problem;
2. identify primary-source or test evidence;
3. describe the new authority/ownership model;
4. update exact/fidelity tests first where practical;
5. change current architecture/invariant/source/runtime-flow documentation;
6. preserve old historical records as history;
7. validate broad regressions and any relevant classic diagnostics/performance gates.

The standard is not "never change architecture". The standard is **change it deliberately, with a stronger physical model and evidence**.
