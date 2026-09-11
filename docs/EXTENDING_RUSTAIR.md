# Extending RusTair Safely

This document gives practical recipes for adding functionality without breaking RusTair's physical-machine architecture.

The repeated theme is: **decide which physical or host layer owns the new behavior before writing code**.

---

## 1. General workflow for a new feature

Before coding:

1. Identify whether the feature belongs to the emulated hardware, host integration, UI, or diagnostics.
2. Identify the authoritative existing state it must read/change.
3. Read the hardware-specific project docs and primary sources already cited there.
4. Find tests that protect adjacent invariants.
5. Define what the project will and will not claim to model.
6. Implement the smallest layer-correct change.
7. Add focused tests before broad validation.

Avoid beginning with a UI control and then inventing a state field behind it. Start from the physical/software ownership model.

---

## 2. Add or fix an Intel 8080 instruction

There are two CPU representations with different responsibilities.

### Semantic behavior

Start in `src/cpu8080.rs` if the final programmer-visible result is wrong:

- register values;
- flags;
- PC/SP;
- memory value written;
- I/O value;
- semantic DI/EI behavior.

Add/extend semantic tests.

### Exact timeline

Start in `src/cpu8080_cycle/` if the final result is right but the physical timeline is wrong:

- machine-cycle class/order;
- T-state count/placement;
- PHI1/PHI2 edge;
- address/data pins;
- READY/HOLD sampling;
- interrupt/halt/reset behavior.

Useful files:

- `decode.rs` — internal opcode decode;
- `alu.rs` — ALU/flags;
- `control_flow.rs` — control-flow sequencing;
- `timing.rs` / `timing/phase.rs` — timing vocabulary/edges;
- `mod.rs` — main exact state machine.

### Acceptance

At a clean instruction boundary the semantic and exact cores should agree on architectural state, while exact tests additionally prove the physical schedule.

---

## 3. Add an instruction/family to Full

Full support is a fidelity project, not merely an opcode-table edit.

### Step 1: document the exact Partial schedule

Record:

- total T-states;
- each external machine cycle;
- address/data direction/value;
- Intel status word;
- internal T4/T5 placement;
- stack behavior;
- INTE transition if any;
- possible READY/HOLD/interrupt observation points.

### Step 2: determine whether Full can represent it

Ask whether any omitted intermediate state can be observed by:

- installed RAM/cards;
- serial timing;
- front panel;
- interrupt logic;
- memory protection;
- CPU-board latch state;
- debugger/watchpoint behavior that is defined as execution-observable.

If yes, either model that state exactly or keep the instruction on Partial.

### Step 3: implement projection

Edit `src/backend/cycle/full.rs` and, when needed, Full panel/boundary helpers. Do not bypass `FullInstructionBus` for memory.

### Step 4: add independent oracles

Compare Full to forced Partial for:

- CPU state;
- exact T-state total;
- memory/card state;
- panel duty;
- final pins/latches;
- blockers/interrupt timing.

### Step 5: benchmark last

Only after equivalence is green should the new coverage be treated as a performance change.

---

## 4. Add a new S-100 RAM board

A RAM board is more than a byte array.

Model:

- historical board identity;
- address decode straps/range;
- populated capacity;
- read/write timing/wait behavior;
- protection behavior if present;
- S-100 connector contacts/drives;
- power-on initialization policy where appropriate.

Likely files:

- `src/s100_memory.rs` — board description/config rules;
- `src/config/s100_hardware.rs` — installed-card variant;
- `src/config/s100_codec.rs` / persistence — save/load;
- `src/s100_runtime_ram.rs` — live physical card implementation;
- `src/s100_runtime.rs` — fabric assembly;
- `src/app/ui/s100_hardware.rs` — configuration UI;
- `src/app/ui/s100_memory_inspection.rs` — inspection presentation if useful.

Do not add a special `if board X` direct-memory path in the CPU. Static responder tables can be extended as acceleration metadata after the card is physically modeled.

Tests should cover decode boundaries, overlap, unpopulated regions, waits, writes/protection and persistence.

---

## 5. Add a new S-100 I/O card

### Physical/configuration layer

Create typed configuration for straps/jumpers/addressing/interrupt targets.

### Device/chip layer

If the card contains a reusable chip (as 88-2SIO contains MC6850), model chip behavior separately from board wiring where that separation is real and useful.

### Card electrical layer

Implement normal S-100 card observation/drive behavior through the common card interface. Keep fixed decode data in the hardware configuration and compile it into runtime responder masks if performance requires it.

### Runtime assembly

Teach `S100RuntimeFabric` to instantiate the card from `S100HardwareConfig`.

### Host inspection

If UI/endpoints need controlled access, expose a handle to the same live card state. Never construct a second UART/device for host access.

### Tests

Cover:

- every decoded port;
- overlap/open I/O;
- data/status semantics;
- reset/power behavior;
- interrupt wiring;
- timing/READY if relevant;
- external connector signals;
- configuration round-trip.

---

## 6. Add a new serial host endpoint

Examples: another network protocol, pseudo-terminal, file-based serial source.

This normally belongs under `src/io/` plus app/controller/UI integration.

The endpoint should:

- connect through `SerialRouter` or an equivalent explicit cable model;
- consume/produce host data;
- represent host-side modem/control signals if applicable;
- never own guest UART registers/status;
- never silently change serial-card straps;
- be disconnectable.

If the endpoint needs a physical electrical level converter, model that explicitly rather than connecting incompatible interfaces behind the user's back.

---

## 7. Add a new external peripheral

An external peripheral such as a teletype should be separated into:

1. **device model** — mechanical/logical peripheral behavior independent of egui;
2. **controller** — application scheduling/routing/timing integration;
3. **UI** — presentation and user interaction;
4. **connection** — explicit route to an emulated port/card.

The ASR-33 implementation is the main example:

```text
src/peripherals/asr33/*       physical/peripheral model
src/app/asr33_*               application control/state
src/app/ui/asr33*             GUI
src/io/serial_router.rs       connection to emulated serial port
```

Do not put peripheral state inside the serial card unless the hardware actually lives on that card.

---

## 8. Add a new debugger/teaching view

Prefer a read-only snapshot API.

Good design:

```text
machine -> backend snapshot -> derived analysis -> UI
```

Avoid:

```text
UI -> concrete private hardware object -> arbitrary mutation
```

If the tool needs to control execution (breakpoint, DEPOSIT, memory edit), add an explicit backend command with documented semantics.

Derived analysis should tolerate bounded/missing history and communicate uncertainty rather than fabricating certainty.

---

## 9. Add a new front-panel control

First determine what the real switch/button electrically does.

Then locate the physical path:

- UI gesture in `src/app/ui/front_panel*`;
- backend control request;
- `FrontPanelController` / `AltairChassis`;
- S-100 signal/latch or CPU interaction;
- exact timing tests.

Do not implement a front-panel control by directly editing PC/registers unless the real hardware's bus sequence is explicitly represented by that backend operation.

EXAMINE and DEPOSIT are examples where the point is precisely to model a front-panel/system-bus action, not a debugger memory poke.

---

## 10. Add a chassis or CPU board

This is a major architecture change.

### New chassis model

Update `src/s100_chassis.rs` with historically supported connector/motherboard topology, then expose it through configuration validation/UI/persistence.

### New CPU board

A new CPU board requires more than adding a processor enum.

For a future Z80 board, for example:

1. choose/document the exact historical S-100 board;
2. implement the processor core;
3. model board-specific signal adaptation;
4. integrate it as an S-100 card;
5. add engine/board compatibility rules;
6. preserve the same RAM/serial/backplane/front-panel authorities;
7. require POWER OFF for board replacement;
8. add debugger/state/disassembly support only when the actual core exists.

Never create a fake placeholder CPU that cannot execute the hardware.

---

## 11. Add configuration fields

Decide whether a setting is:

- physical hardware configuration/strap;
- host preference;
- compatibility option;
- transient runtime state.

Only the first three normally belong in persistence.

For persisted changes:

- define typed config;
- validate values;
- preserve old config migration when practical;
- add round-trip tests;
- ensure old aggregate compatibility fields do not become a second runtime topology;
- update UI labels to explain whether POWER OFF is required.

---

## 12. Add an optimization

Classify it before implementation.

### SAFE

Purely changes host work without changing modeled results, e.g. inlining, layout, avoiding recomputation of an unchanged result.

### PROVABLE

Changes execution granularity/representation but can be proven equivalent, e.g. cached decoder, event-driven card reevaluation, Full panel aggregation.

Requires an independent oracle.

### REJECT for the high-fidelity path

Would remove or relocate physically observable behavior, e.g. instruction-boundary READY, fake panel state, direct RAM bypass, instant UART queue.

For performance work, measure Amdahl impact and benchmark baseline/candidate under matched host conditions.

---

## 13. Add or change assets

Built-in assets are embedded. If adding artwork/audio/binaries:

- place them under the appropriate `assets/` subtree;
- register/look them up through `embedded_assets` where required;
- document third-party license/source in `THIRD_PARTY.md` when applicable;
- avoid committing generated editor/temp files;
- do not put runtime-generated logs/binaries under version control.

---

## 14. Definition of done for a feature

A feature is not complete merely because the UI works.

A strong completion checklist is:

- physical/software ownership is clear;
- no duplicate authoritative state was introduced;
- configuration/persistence is correct;
- focused unit/fidelity tests exist;
- Full/Partial equivalence is proven if relevant;
- docs/source reference are updated;
- `cargo test --locked --all-targets` passes with `-Dwarnings`;
- release build succeeds;
- no generated/scratch artifacts remain;
- performance regression is checked if a hot path changed;
- known non-claims/limitations are documented.
