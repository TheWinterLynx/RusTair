# RusTair Developer Guide

This document is the main onboarding guide for developers who have never seen RusTair before. It assumes no prior knowledge of Rust, emulation, the Intel 8080, the Altair 8800, or the S-100 bus.

For a file-by-file index, see [SOURCE_REFERENCE.md](SOURCE_REFERENCE.md). For the exact machine architecture, see [EMULATION_ARCHITECTURE.md](EMULATION_ARCHITECTURE.md).

---

## 1. What RusTair is

RusTair is a native Rust emulator of the **MITS Altair 8800**, a 1970s microcomputer built around plug-in boards connected by the S-100 system bus.

Many emulators are primarily software-compatible: they reproduce enough CPU instructions and I/O behavior to run programs. RusTair deliberately goes further. Its architecture tries to preserve the structure of the physical machine:

```text
Altair 8800
├── Chassis
│   └── S-100 backplane
│       ├── MITS 8080 CPU board
│       ├── RAM board(s)
│       ├── 88-SIO / 88-2SIO serial board(s)
│       └── additional S-100 cards as implemented
└── Display/Control front panel
    └── connected to the same system bus
```

The CPU board does not directly call a RAM object or serial object as if those devices were ordinary application services. Instead, cards participate through the S-100 bus model. The front panel observes and controls that same machine.

This choice is the most important fact to understand before editing the project.

---

## 2. A minimal historical/emulation primer

### 2.1 The Intel 8080

The Intel 8080 is an 8-bit microprocessor. The most important state visible to a programmer is:

- 8-bit registers: `A`, `B`, `C`, `D`, `E`, `H`, `L`;
- the flags register (`F`), containing status such as Zero, Sign, Carry, Parity and Auxiliary Carry;
- 16-bit program counter (`PC`), the address of the instruction stream;
- 16-bit stack pointer (`SP`);
- interrupt-enable state (`INTE`).

Pairs `BC`, `DE` and `HL` are often treated as 16-bit register pairs.

An instruction such as `MOV A,B` or `JMP 1234h` is not physically instantaneous. The processor performs one or more **machine cycles**, and each machine cycle contains several **T-states**.

### 2.2 T-states

A T-state is the basic digital timing quantum used by RusTair's exact 8080 model. RusTair represents:

- `T1`
- `T2`
- `Tw` — an inserted wait state
- `T3`
- `T4`
- `T5`
- `Thalt` — halt dwell
- `Thold` — bus-release dwell while HOLD remains asserted

The physical 8080 uses two non-overlapping clock phases, `PHI1` and `PHI2`. The exact path can decompose a T-state into four digital edges:

```text
PHI1 rising
PHI1 falling
PHI2 rising
PHI2 falling
```

RusTair models the ordering and digital ownership of these edges. It does not claim analog transistor-level pulse shape, slew rate or metastability modeling.

### 2.3 Machine cycles

Common 8080 machine cycles include:

- instruction fetch;
- memory read;
- memory write;
- stack read/write;
- I/O input/output;
- interrupt acknowledge;
- halt acknowledge;
- internal cycles.

During T1, the 8080 exposes a status byte identifying the current machine-cycle class. That status is relevant to the Altair CPU board, S-100 bus and front panel.

### 2.4 READY and wait states

A slow device can hold the processor by controlling `READY`. If READY is not accepted at the required sampling point, the CPU inserts one or more `Tw` states. RusTair therefore cannot model all waits as a convenient instruction-level delay without losing low-level timing fidelity.

### 2.5 HOLD / HLDA

Another device can request ownership of the system bus with `HOLD`. The 8080 responds with `HLDA` (hold acknowledge) and releases the bus at the defined point in its cycle. This is different from simply pausing the CPU at an arbitrary instruction boundary.

### 2.6 Interrupts

Interrupt request, interrupt enable and interrupt acknowledge are timed hardware events. `EI` has a delayed enable rule; `DI` changes interrupt enable at a specific point. RusTair contains exact tests for these transitions because moving them by one T-state can change visible hardware behavior.

### 2.7 The S-100 bus

The Altair's cards connect to a common edge-connector bus. Conceptually, the bus carries:

- address lines;
- CPU-to-card data lines;
- card-to-CPU data lines;
- status lines;
- memory and I/O control lines;
- interrupt lines;
- READY/WAIT and HOLD-related signals;
- front-panel control signals;
- clock and reset-related signals.

Cards can drive, release or observe lines. Some outputs are tri-state or open-collector. More than one card can be electrically selected, so an emulator must preserve overlap/contention behavior rather than assuming exactly one responder.

### 2.8 High impedance and open bus

A card that is not selected may electrically release a line (**high impedance**, often written High-Z). An unclaimed bus read is therefore not the same thing as reading a synthetic zero-filled memory array. RusTair has explicit open-bus and high-Z behavior.

### 2.9 The front panel

The Altair front panel is not just a GUI representation of CPU registers. Its ADDRESS, DATA and STATUS lamps are connected to bus-visible state, and its switches can control operations such as:

- RUN / STOP;
- RESET;
- EXAMINE;
- DEPOSIT;
- PROTECT / UNPROTECT;
- address/data switch input.

RusTair's UI may visually integrate/persist lamp brightness, but presentation state must never become the source of truth for the raw bus.

---

## 3. The core design: one machine, two execution strategies

RusTair has one production engine: **Adaptive Cycle 8080**.

It can execute through two internal strategies.

### Partial

Partial is the exact physical oracle. It advances the stateful 8080 through T-states/clock phases, drives the MITS CPU-board model, resolves the live S-100 bus, lets cards observe bus changes, and samples control inputs where the hardware requires them.

Use Partial when an intermediate hardware event matters or could matter.

### Full

Full is an acceleration strategy. At a proven clean boundary, RusTair may export the architectural CPU state into the instruction-level `Cpu8080` semantic executor and execute one or many supported instructions while analytically reconstructing the externally relevant machine-cycle/panel behavior.

At the next synchronization boundary, the semantic state is committed back into `Cpu8080Cycle`.

The semantic executor is **transient**. It is not a second emulated computer.

### The invariant

A correct Full window must be equivalent to the corresponding exact Partial execution for every relevant state:

```text
same architectural CPU state
+ same elapsed T-states
+ same RAM/card state
+ same S-100-visible boundary state
+ same front-panel observable behavior
+ same interrupt/READY/HOLD semantics
```

If the proof does not hold, execution must stay in or return to Partial.

---

## 4. Sources of truth

The project intentionally separates authoritative emulated state from derived/UI state.

### Authoritative state

| Domain | Authority |
| --- | --- |
| CPU registers, flags, PC, SP, INTE, exact CPU state | `Cpu8080Cycle` at synchronization boundaries |
| Physical chassis power | `AltairChassis` |
| RUN/STOP physical latch | `S100BusState::signals.run` |
| Raw S-100 electrical state | S-100 resolver / `S100BusState` |
| Installed physical cards and RAM contents | `S100RuntimeFabric` and installed runtime card objects |
| UART/serial-card internal state | installed runtime serial-card instance |
| Full-window semantic CPU | transient `Cpu8080`; authoritative only inside the admitted window and committed once at exit |

### Derived/presentation state

Examples that must never drive the machine:

- smoothed LED brightness;
- debugger history;
- inferred call stacks;
- loop detection;
- teaching snapshots;
- memory activity history;
- UI window state;
- Adaptive performance metrics.

A useful review question is: **"If I delete this derived observer, does the emulated machine still behave identically?"** For presentation/debugging data, the answer should be yes.

---

## 5. Project layout

```text
src/
├── app/                 Desktop app, controllers, persistence, UI
├── backend/             Host-facing machine API and Adaptive Cycle dispatcher
├── config/              Typed physical/application configuration
├── cpu8080_cycle/       Exact stateful 8080 timing core
├── io/                  Host TCP/COM transports and routing
├── machine/             Altair chassis/front-panel/serial integration
├── peripherals/asr33/   ASR-33 peripheral model
├── cpu8080.rs           Instruction-level semantic 8080 executor
├── s100*.rs             S-100 signals, cards, backplane and runtime fabric
├── debugger*.rs         Debugging analysis/control
├── decoder8080.rs       Disassembly/metadata decoder
├── trace8080.rs         Instruction/effect history
└── ...                  Supporting tools and observers

tests/                   Integration/fidelity/architecture/performance tests
assets/                  Embedded ROM/binary/art/audio assets
docs/                    Architecture, fidelity research and developer docs
tools/                   Profiling/developer scripts
```

The exact responsibility of every Rust source file is documented in [SOURCE_REFERENCE.md](SOURCE_REFERENCE.md).

---

## 6. How Rust maps to this architecture

### 6.1 Modules

Rust modules are used to express boundaries. For example:

```rust
mod full;
include!("cycle/partial_impl.rs");
```

in `src/backend/cycle.rs` composes Full acceleration with the existing exact backend implementation.

A directory module often has a `mod.rs` that declares children and exposes selected types.

### 6.2 Visibility

- `pub`: externally visible from the crate/module API.
- `pub(crate)`: visible anywhere inside RusTair, but not intended as an external API.
- no `pub`: private to its module hierarchy.

Do not make an internal physical state public merely because a UI needs it. Prefer a read-only snapshot or backend method that preserves authority boundaries.

### 6.3 Traits

Traits express behavior contracts. The instruction-level CPU accepts any type implementing `cpu8080::Bus`:

```rust
pub trait Bus {
    fn read(&mut self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);
    // ...
}
```

Full execution uses a `FullInstructionBus` implementation to make semantic instructions interact with the real machine model without giving the semantic CPU direct knowledge of RAM or cards.

The S-100 side uses card interfaces so the backplane resolver can remain card-family agnostic.

### 6.4 Ownership and borrowing

Rust's ownership model is useful here because emulated state should have one owner. If a function needs temporary mutable access, it receives `&mut` access rather than creating a second copy.

Be cautious with `clone()`: cloning configuration or a snapshot is normal; cloning authoritative live emulated hardware state can accidentally create a second machine.

### 6.5 `Option` and High-Z / absence

`Option<T>` is often a natural representation when a value may not exist. In hardware code, however, do not assume every `None` means High-Z. Read the type's contract. Some types distinguish floating, contention, unmapped memory and disabled devices explicitly.

### 6.6 `Result`

Configuration and hardware assembly use `Result` to reject impossible/invalid physical configurations. Prefer returning a meaningful error to silently constructing a topology that could not exist.

### 6.7 `#[cfg(test)]`

Test-only helpers may expose low-level operations that production intentionally hides. Do not promote them to production APIs just to make a new test easier unless the production architecture genuinely needs the operation.

### 6.8 `include!`

`src/backend/cycle.rs` includes `cycle/partial_impl.rs`. Treat the included file as part of the `cycle` module even though it is physically separated for maintainability.

---

## 7. Application lifecycle

The executable entry point is intentionally tiny:

```text
src/main.rs
    -> rustair::app::run()
        -> eframe window
            -> RusTairApp
                -> update() once per GUI frame
```

`RusTairApp` owns host/application concerns such as:

- persisted configuration;
- backend facade (`BackendHost`);
- serial cable routing;
- TCP/COM endpoint state;
- ASR-33 and terminal UI/controller state;
- audio;
- GUI timing/debt accounting.

It does **not** own a duplicate 8080, RAM array or UART.

During `eframe::App::update` the application:

1. loads/updates persisted configuration;
2. mounts changed physical S-100 hardware while POWER is off;
3. services diagnostic workflows and observer capture settings;
4. calculates how much virtual CPU time may run;
5. asks the backend to execute that budget;
6. services serial endpoints/peripheral mechanics;
7. renders menus, panels and tools.

This separation is important: the UI asks the machine to do things; it does not become the machine.

---

## 8. Timing: guest clock versus host speed

The installed MITS 8080 CPU board has its historical clock. The user can select Authentic, 5×, 10× or Unlimited host execution speed.

These settings affect **how quickly the host consumes virtual machine time**, not the historical board's internal timing relationships.

`src/app/execution_clock.rs` converts host elapsed time into guest T-state budget for throttled modes. `src/app/execution_frame.rs` runs the backend in responsive chunks/deadlines. Unlimited mode is host-deadline limited rather than repaint-budget limited.

Independent hardware such as UART baud generators still advances according to emulated elapsed time, including cases where useful CPU instruction execution is stopped.

---

## 9. The S-100 model

### 9.1 Signal definitions

`src/s100.rs` defines the physical signal vocabulary and connector roles. Signal-to-pin mapping is based on the original Altair bus definition.

### 9.2 Card contract

All live cards must appear to the bus through the common S-100 electrical interface. The backplane should not contain branches such as "if this is an 88-SIO". Card-specific logic belongs in the card adapter/model.

### 9.3 Backplane resolution

`src/s100_backplane.rs` resolves drives from installed cards:

- driven HIGH/LOW;
- High-Z;
- open-collector behavior;
- contention;
- cached deltas/selected drives for performance.

The optimized data structures are implementation details; their output must remain the same physical bus result.

### 9.4 Runtime fabric

`S100RuntimeFabric` materializes persisted slot configuration into live electrical card objects. It also compiles static topology facts such as address/I/O decoder responder masks.

These indices are safe optimization metadata because a real fixed card topology also has fixed strap/decode logic. They must preserve multiple responders and open/unmapped regions.

### 9.5 RAM

RAM is stored in installed runtime RAM card instances. Guest execution, debugger inspection and Full execution must all refer to this physical storage rather than separate arrays.

### 9.6 Serial cards

MITS 88-SIO and 88-2SIO state lives in installed serial card instances. Host TCP, COM, ASR-33 and text-terminal endpoints are connected to emulated ports through routing/cabling layers; endpoints are not the serial card itself.

---

## 10. Front-panel architecture

There are three concepts that new developers often confuse:

### Raw electrical state

This is the resolved machine state on the S-100/front-panel boundary. It is authoritative for hardware behavior.

### Exact/analytical panel activity

Partial can observe exact bus states T-state by T-state. Full aggregates equivalent lamp duty/activity for safe windows, preserving the physical result without literally replaying every host operation.

### Visible LED persistence

The GUI visually integrates lamp duty so LEDs look like real lamps rather than infinitely fast bits. This visual state is presentation only.

Never read a smoothed LED value back into CPU, bus or card logic.

---

## 11. Serial architecture

Think in layers:

```text
host endpoint (TCP / COM / ASR-33 / text terminal)
        |
serial routing / cable selection
        |
emulated connector/electrical interface
        |
installed 88-SIO or 88-2SIO card
        |
S-100 bus
```

A host TCP socket is not an emulated UART. It feeds or receives bytes/signals through the endpoint/routing layer while UART status, baud timing, interrupt state and register behavior remain in the emulated card.

When adding a serial feature, decide first which layer it belongs to.

---

## 12. Configuration and persistence

Physical hardware configuration is slot-native. The current S-100 card inventory is authoritative.

Legacy aggregate configuration fields are retained where needed for migration. They must not be allowed to construct a parallel runtime machine.

Rules:

- physical card changes require POWER OFF;
- validate slot count/card combinations;
- preserve backward-compatible migration unless deliberately versioned;
- distinguish host preferences from hardware straps;
- do not persist transient live CPU/UART state as if it were configuration.

---

## 13. Debugging and teaching tools

RusTair includes more introspection than a typical emulator:

- disassembly and instruction metadata;
- breakpoints/watchpoints/run-to/step over/out;
- bounded instruction history;
- inferred call stack;
- simple-loop detection;
- memory activity history;
- I/O inspector;
- CPU pin diagram;
- S-100 bus teaching snapshots;
- memory viewer and S-100 card inspection.

These tools should observe the canonical backend/machine state. Their histories are explicitly allowed to be stale or incomplete; they must never feed state back into emulation.

See [DEBUGGING_AND_PERFORMANCE.md](DEBUGGING_AND_PERFORMANCE.md).

---

## 14. Loading software

RusTair intentionally distinguishes convenience from authenticity.

### Quick/direct loading

A binary or bundled BASIC image can be written directly into physical RAM through the emulator's controlled loading path. This is a developer/user convenience.

### Authentic loading

The authentic loader models the historical paper-tape workflow: bootstrap code executes on the emulated CPU and bytes arrive through the installed serial hardware/peripheral path.

Do not silently replace one path with the other. Tests and UI labels deliberately distinguish them.

---

## 15. How to approach a bug

Before editing code, classify the symptom:

1. **CPU semantic bug** — wrong registers/flags/result independent of bus timing.
2. **CPU timing bug** — right final instruction result, wrong T-state or machine-cycle schedule.
3. **CPU-board/S-100 bug** — pins or bus translation/resolution wrong.
4. **memory-card bug** — decode/population/protection/wait behavior wrong.
5. **serial-card bug** — UART/card registers, electrical signals, baud or interrupt behavior wrong.
6. **host transport bug** — TCP/COM endpoint problem while emulated card is correct.
7. **front-panel physical bug** — wrong bus-derived panel behavior.
8. **presentation bug** — physical state is right but UI rendering/history is wrong.
9. **Adaptive Full bug** — Partial is correct, Full diverges or rejoins incorrectly.
10. **persistence/configuration bug** — loaded desired hardware differs from saved/validated configuration.

Then start at the file family listed in [SOURCE_REFERENCE.md](SOURCE_REFERENCE.md).

---

## 16. How to add a new feature safely

### New 8080 behavior

Implement or fix semantic behavior and exact cycle timing separately, then use differential tests to prove they agree at clean boundaries.

### New Full instruction family

Do not add an opcode to the Full table merely because its final registers match. Document:

- every external machine cycle;
- address/data/status presentation;
- internal T-state placement;
- INTE transitions;
- possible asynchronous hardware observation points;
- required exit/re-entry state.

Add exact Full-versus-Partial oracles before benchmarking.

### New S-100 card

Define physical configuration/straps, implement the card's electrical connector behavior, mount it through `S100RuntimeFabric`, expose host inspection only as a non-authoritative handle, add persistence/UI configuration, then validate against documentation.

### New peripheral

Keep the peripheral separate from the S-100 card unless the real hardware is the card. For example, the ASR-33 is an external device connected to serial hardware; it is not part of the 88-SIO object.

Detailed recipes are in [EXTENDING_RUSTAIR.md](EXTENDING_RUSTAIR.md).

---

## 17. Testing philosophy

RusTair uses tests as executable hardware documentation. A good fidelity test states a physical invariant, not only an implementation detail.

Examples of important invariants:

- the front panel sees bus activity, not invented CPU state;
- an unmapped read has correct open-bus behavior;
- READY inserts wait states at the correct place;
- HOLD releases/resumes the bus correctly;
- delayed EI enables interrupts on the correct edge;
- Full and Partial produce identical boundary state;
- a card's fixed address straps produce the expected physical decode;
- persisted hardware reconstructs the same slot inventory.

See [TESTING_GUIDE.md](TESTING_GUIDE.md).

---

## 18. Performance philosophy

The fastest code is not automatically the best emulator. The target is **maximum safe performance under the physical model**.

Good optimizations:

- cache values that cannot change until known inputs change;
- predecode static card straps/topology;
- resolve only changed electrical drives;
- aggregate equivalent panel duty over a proven interval;
- inline/specialize hot code without changing behavior;
- use Full only while an explicit safety predicate holds.

Bad optimizations:

- bypass the backplane for convenience;
- skip card behavior because a benchmark does not use it;
- move interrupts/READY/HOLD to instruction boundaries;
- use PC as the ADDRESS lamp;
- treat serial hardware as an always-ready byte queue;
- hide contention or open-bus behavior.

Always measure the optimized release build. Debug/symbolized/profiler builds can have very different absolute throughput.

---

## 19. Recommended reading order for a new contributor

1. `README.md`
2. `CONTRIBUTING.md`
3. this guide
4. [GLOSSARY.md](GLOSSARY.md)
5. [EMULATION_ARCHITECTURE.md](EMULATION_ARCHITECTURE.md)
6. [SOURCE_REFERENCE.md](SOURCE_REFERENCE.md)
7. [TESTING_GUIDE.md](TESTING_GUIDE.md)
8. the hardware-specific documents under `docs/` for the subsystem you will modify
9. only then the implementation files for that subsystem

This order prevents a common mistake: understanding a local Rust function while missing the physical invariant it exists to preserve.
