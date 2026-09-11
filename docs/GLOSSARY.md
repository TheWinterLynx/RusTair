# RusTair Glossary

This glossary defines terms used in source, tests and documentation. It intentionally mixes Rust, Intel 8080, Altair/S-100 and RusTair-specific vocabulary so a new contributor does not need several external glossaries open at once.

## A–D

**ACIA** — Asynchronous Communications Interface Adapter. The 88-2SIO uses Motorola MC6850 ACIAs. RusTair models the chip separately from board straps/wiring.

**Adaptive Cycle** — RusTair's single production 8080 execution engine. It chooses between exact Partial and accelerated Full execution while operating one machine state.

**Address bus** — Lines carrying the address of the memory or I/O location involved in a bus transaction.

**Altair 8800** — MITS microcomputer family modeled by RusTair. CPU, RAM and I/O are plug-in cards connected by the system/S-100 bus and observed/controlled by the front panel.

**AltairBus** — RusTair machine-layer composition around memory, front-panel controller, S-100 state and diagnostic metering. It is not a second CPU.

**AltairChassis** — CPU-free physical machine container/lifecycle object. Physical power belongs here; RUN is derived from the bus latch.

**Architecture guard** — Test that enforces a structural rule such as "no second backend" or "no duplicate CPU authority", even if behavior tests would currently pass.

**Backplane** — Passive/interconnection structure that connects S-100 card slots. In RusTair, `S100Backplane` is the generic electrical resolver and deliberately knows no card families.

**BDOS** — CP/M Basic Disk Operating System service entry used by classic 8080 diagnostics. RusTair's diagnostic meter normalizes well-known diagnostic harness calls for reference comparison.

**Bus contention** — Two or more outputs drive incompatible levels on the same line. Must not be silently converted into a unique value.

**Bus drive** — The electrical contribution of one card to connector lines: high, low, open-collector low or High-Z depending on modeled contact type.

**Bus sample** — Resolved state of S-100 contacts after card drives are combined.

**Card** — Physical S-100 plug-in board. In architecture discussions, prefer card/board for physical hardware and endpoint/controller for host-side helpers.

**Cargo** — Rust build/test/package tool. `Cargo.toml` defines package metadata, dependencies and profiles.

**Chassis** — Physical enclosure/backplane/card inventory. The chassis is not synonymous with CPU.

**Clock edge** — One transition of PHI1 or PHI2. RusTair exact timing distinguishes rising/falling edges when hardware ordering depends on them.

**COM endpoint** — Host physical serial port transport. It is an endpoint connected to an emulated serial port, not the guest UART/card.

**Contention** — See bus contention.

**CPU board** — S-100 card containing the processor and board-level interface logic. The Intel 8080 chip and MITS 8080 CPU board are different modeling layers.

**Cpu8080** — Instruction-level semantic 8080 executor used transiently in Full windows and as a reference/semantic core.

**Cpu8080Cycle** — Stateful exact CPU core and architectural CPU authority at synchronization boundaries. Models machine cycles, T-states, clock edges, pins and control timing.

**crate** — Rust compilation/library package. RusTair's library crate starts at `src/lib.rs`.

**Derived state** — Data calculated from authoritative emulation state for display/debugging, e.g. LED persistence, history or inferred call stack. Must not drive emulation.

**DI** — 8080 Disable Interrupts instruction. Exact timing of the INTE transition matters to Full/Partial equivalence.

## E–H

**egui** — Immediate-mode Rust GUI library used through `eframe`.

**eframe** — Application framework hosting egui and the WGPU renderer in RusTair.

**EI** — 8080 Enable Interrupts instruction. The 8080 delays actual interrupt enable until after the following instruction. RusTair has exact oracles for this behavior.

**Electrical oracle** — The exact reference path used to decide whether an optimization is behaviorally equivalent. In Adaptive Cycle this is Partial execution.

**EmulationEngine** — Public backend engine identity. Current production has one: `RustCycleAccurate8080` / Adaptive Cycle.

**Endpoint** — Host or peripheral device attached through serial routing, e.g. ASR-33, text terminal, TCP or COM. Not the same as the emulated SIO card.

**Full** — Accelerated Adaptive Cycle strategy. Executes safe instruction windows semantically while preserving exact totals, physical storage, panel duty and synchronization boundary state.

**FullInstructionBus** — `cpu8080::Bus` implementation used by Full. Bridges semantic instructions to physical guest memory/card behavior and exact projected machine-cycle/panel accounting.

**Full window** — Consecutive interval of admitted instructions executed by transient `Cpu8080` before state is committed back to `Cpu8080Cycle`.

**Guest** — Software running inside the emulated Altair, as opposed to the host computer running RusTair.

**High-Z** — High impedance: an output is electrically released rather than driving high or low.

**HLDA** — HOLD Acknowledge from the 8080. Indicates bus ownership release after a HOLD request at the correct hardware point.

**HLT** — 8080 Halt instruction. CPU enters a hardware halt state rather than simply ending emulator execution.

**HOLD** — Input asking the CPU to release the system bus. Must be modeled with correct sampling and HLDA response.

**Host** — Real computer/operating system running RusTair.

## I–P

**IN / OUT** — 8080 I/O instructions using 8-bit I/O port addresses.

**INTE** — Interrupt-enable state/output associated with the 8080. Visible to the Altair hardware/front panel model and timing-sensitive around DI/EI.

**Interrupt acknowledge / INTA** — Bus cycle in which the CPU acknowledges an interrupt and receives/proceeds with interrupt behavior.

**I/O decode index** — Precomputed responder masks for fixed serial-card port straps. Performance metadata that must still preserve multiple responders and normal electrical transactions.

**Machine cycle** — 8080 bus-operation class such as instruction fetch, memory read/write, stack access, I/O, interrupt acknowledge or halt acknowledge. Contains one or more T-states.

**Marginal panel duty** — Optimized Full representation that accumulates lamp duty by independent ADDRESS/DATA/STATUS byte distributions plus scalar signals instead of a huge composite state histogram.

**MC6850** — Motorola ACIA used on MITS 88-2SIO. Chip-level behavior is modeled in `src/mc6850.rs`.

**Memory responder mask** — Precompiled set of physical slots whose fixed address decode can respond to a 16-bit address. Does not bypass card semantics.

**MITS** — Micro Instrumentation and Telemetry Systems, manufacturer of the Altair.

**module** — Rust namespace/file hierarchy. Declared with `mod`, exported with `pub mod` as appropriate.

**Open bus** — Read from a region with no active responder. Not equivalent to an allocated zero-filled RAM byte; RusTair models its chosen physical open-bus result explicitly.

**Open-collector** — Output style that can actively pull a line low but otherwise releases it; it does not actively drive high.

**Option<T>** — Rust type representing either `Some(T)` or `None`.

**Partial** — Exact Adaptive Cycle strategy. Steps the 8080's physical timeline and live S-100 fabric; authoritative fidelity oracle.

**PC** — Program Counter, 16-bit 8080 register pointing into instruction stream. Do not confuse it with front-panel ADDRESS bus state.

**PHI1 / PHI2** — Two non-overlapping clock phases required by the 8080. Exact path models their digital edge ordering.

**PINT** — Processor interrupt request on the Altair/S-100 path in project terminology. Full admission may block when it can affect execution.

**Presentation state** — UI-only representation such as smoothed LED brightness/window selection. Not emulation authority.

**PROTECT** — Altair front-panel/memory protection behavior. Physical board/memory protection affects writes and panel signals.

**pub / pub(crate)** — Rust visibility. `pub` exposes an item publicly; `pub(crate)` exposes only within RusTair.

## R–S

**RAM card** — Physical S-100 memory board with decode, population, timing and protection properties. Actual bytes live in runtime card instances.

**READY** — CPU control input. When low at the appropriate sampling point, the 8080 inserts `Tw` wait states.

**Reconciliation** — Host-side import of retained Full CPU-board latch/phase state before exact Partial resumes, performed without adding fake emulated time or exposing fake intermediate bus deltas.

**RESET** — Processor/system reset control with defined hardware effects. It is not "zero every register".

**Result<T,E>** — Rust success/error type: `Ok(T)` or `Err(E)`.

**RUN/STOP latch** — Physical Display/Control latch represented authoritatively by `S100BusState::signals.run`.

**S-100** — Name commonly used for the Altair system bus/100-contact connector family. RusTair models historical signals and card electrical roles.

**S100BusState** — Machine/front-panel canonical raw S-100 status/control state. Must not be reconstructed from visible LED brightness.

**S100RuntimeFabric** — Live mounted CPU/RAM/serial card inventory plus backplane and compiled fixed decode metadata. Main physical card runtime authority.

**Semantic core** — Instruction-level `Cpu8080` implementation that computes instruction behavior without permanently owning the physical CPU timeline.

**Serial card** — Emulated 88-SIO/88-2SIO board on S-100. Different from host TCP/COM/terminal endpoint.

**SerialRouter** — Host/application cable-routing model connecting endpoint devices to emulated serial ports.

**Snapshot** — Read-only captured/derived state for UI/debugging. A snapshot may become stale and should not feed back into emulation.

**SP** — 8080 Stack Pointer.

**Static topology** — Physical card inventory/strap facts that change only when POWER is off and cards/configuration are changed. Safe candidate for compiled decode metadata.

**SYNC** — 8080/S-100 timing/status signal associated with T1/status presentation.

**Synchronization boundary** — Point where Full can hand state back to exact execution with a completely defined equivalent CPU/physical state.

## T–W

**T-state** — Fundamental 8080 timing state. RusTair models T1, T2, Tw, T3, T4, T5 plus halt/hold dwell states.

**TCP endpoint** — Host network transport carrying serial data to/from an emulated connection. Not a UART model.

**trait** — Rust behavior interface implemented by types. `cpu8080::Bus` is a central example.

**tri-state** — Output that can drive high/low when enabled and otherwise become High-Z.

**Tw** — 8080 wait T-state inserted due to READY behavior.

**UART** — Universal Asynchronous Receiver/Transmitter. Generic term for serial hardware; RusTair's actual modeled chips/boards include MITS 88-SIO hardware and MC6850-based 88-2SIO.

**Unlimited** — Host execution mode that runs virtual machine time as fast as practical while yielding on host deadlines. It does not change historical CPU/peripheral timing ratios.

**WGPU** — Graphics backend used by eframe in RusTair.

**watchpoint** — Debugger condition that stops on memory read/write activity at an address.

**window (Full)** — See Full window. Do not confuse with GUI window.

## RusTair naming note

Some historical documentation/commits use older terms such as "Fast" versus "Cycle". Current production terminology is:

```text
one Adaptive Cycle engine
├── Full strategy
└── Partial strategy
```

Historical files may deliberately preserve old wording when describing the implementation that was tested at that time. Current architecture documents explicitly label those records as historical.
