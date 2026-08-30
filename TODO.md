# RusTair — Living TODO

> Source of truth for remaining project work. Initial audit: 2026-08-26, `main` at `2402bd2` before this file.
>
> **Rule:** when an item is completed, keep it in place and change it to `- [x] ~~completed item~~` (optionally adding the commit). Do not delete completed items; the file is also the project progress log.
>
> **Priorities:** `P0` = next/core goal · `P1` = important debt/correctness · `P2` = worthwhile improvement · `P3` = optional/polish · `PARKED` = do not work on it without explicit instruction.
>
> **Validation rule:** normal changes are validated locally with `cargo test` and `cargo run --release`. Do **not** run GitHub Actions without explicit permission.

## Recommended active order

1. **Didactic RAM viewer + debugger.**
2. **Runtime/UI scheduling and native-window smoothness.**
3. **Serial-card/UART fidelity and interrupt path.**
4. **Architecture, persistence, cleanup, documentation and test hardening.**

---

## Completed — Authentic Altair paper-tape bootstrap / loader

> Completed and validated 2026-08-26 on `feature/authentic-paper-tape-bootstrap`. The normal regression suite passes, and the external `4K BASIC Ver 3-2.tap` end-to-end regression passes for Fast/Cycle Accurate × 88-SIO/88-2SIO. See `docs/AUTHENTIC_BASIC_BOOTSTRAP.md` and `docs/AUTHENTIC_BASIC_VALIDATION.md`.

- [x] ~~**[P0] Keep Quick Load and Authentic Load as explicitly separate workflows.** Quick Load may continue copying bytes directly to RAM; Authentic Load must use the emulated machine, serial board and ASR-33 reader.~~
- [x] ~~**[P0] Establish and document the historically correct bootstrap loader(s)** for the supported MITS 88-SIO / 88-2SIO configurations, including provenance and exact bytes.~~ See `docs/AUTHENTIC_BASIC_BOOTSTRAP.md`.
- [x] ~~**[P0] Support manual front-panel entry of the bootstrap** as the fully authentic path.~~
- [x] ~~**[P0] Add an optional assisted “Install bootstrap” convenience action** that performs the same deposits transparently and shows exactly what was entered; it must not silently bypass the emulated loader.~~
- [x] ~~**[P0] Make Authentic Load consume the mounted ASR-33 paper tape through the selected serial port**, so `WAIT GUEST RX` advances because the bootstrap genuinely executes `IN` instructions.~~
- [x] ~~**[P0] Preserve reader transport controls and 1× / 5× / 10× / Unlimited speed** during authentic loading; acceleration must alter host/media pacing, not the logical byte stream.~~
- [x] ~~**[P0] Add loader progress/status diagnostics**: bootstrap running, waiting for RX, bytes consumed, destination range, end of tape, checksum/validation failure where applicable.~~
- [x] ~~**[P0] Make serial-board/sense-switch requirements visible to the operator.** In particular preserve the BASIC 3.2 88-SIO/88-2SIO sense-switch distinction rather than changing switches behind the user’s back.~~
- [x] ~~**[P0] Verify Authentic Load with both Rust engines** (`RusTair — Fast 8080` and `RusTair — Cycle Accurate 8080`).~~
- [x] ~~**[P0] Regression-test that authentic BASIC loading produces the expected RAM image/state** and reaches the same BASIC entry behavior as Quick Load without direct-RAM shortcuts.~~ See `tests/authentic_basic_tape.rs`.
- [x] ~~**[P1] Add deterministic tests for bootstrap failure modes**: wrong board, wrong port, ASR OFF/LOCAL, STOP state, RX not consumed, premature end-of-tape, insufficient RAM.~~ Covered by loader/unit/integration tests plus `tests/authentic_reader_transport_guards.rs`.

---

## P0/P1 — Didactic RAM viewer and debugger

> P0 completed on `feature/didactic-ram-debugger`. The core P1 debugger/teaching scope is also implemented; remaining unchecked items in this section are deliberate P2 extensions.

### Shared 8080 decode/control-flow foundation

- [x] ~~**[P0] Extract the Intel 8080 decoder/disassembler from `memory_viewer.rs` into a shared structured decoder module.** UI, debugger, traces and future tools should use one opcode description source.~~ Implemented in `src/decoder8080.rs`.
- [x] ~~**[P0] Decoder metadata should include** mnemonic, length, operands, immediate/address targets, flags affected, nominal timing, memory/I/O behavior and control-flow type.~~
- [x] ~~**[P1] Add tests covering all 256 opcode byte values**, including undocumented aliases currently accepted by the cores.~~ Includes decoder coverage and timing cross-checks.

### Memory hover / instruction understanding

- [x] ~~**[P0] Enhance RAM-byte hover with opcode interpretation.** In addition to HEX/decimal/ASCII, show the 8080 instruction that would begin at that address, its bytes and operands.~~
- [x] ~~**[P0] Clearly distinguish “this byte can decode as…” from “CPU is executing this instruction”** so data bytes are not misleadingly presented as known code.~~
- [x] ~~**[P1] Add an “Explain instruction” view** with plain-language semantics, input/output registers, flags affected, memory/I/O accesses and T-state/machine-cycle information.~~ Semantic explanation is paired with the exact/approximate Bus/T-state Teacher for cycle-level detail.
- [x] ~~**[P1] Explain `M` contextually as memory at `[HL]`**, including the current HL address/value when relevant.~~

### Loop inspector

- [x] ~~**[P0] Detect simple backward-branch loops around the current PC.**~~
- [x] ~~**[P0] Add a closable floating Loop Inspector** showing the whole loop disassembly instead of only the current instruction.~~ Implemented as an independent native viewport rather than an embedded egui window.
- [x] ~~**[P0] Highlight the live PC inside the loop** without causing layout movement/flicker.~~ Uses stable EXEC/current-instruction semantics for Cycle Accurate mid-instruction PC movement.
- [x] ~~**[P1] Show loop entry, back-edge, exit condition and branch target.**~~
- [x] ~~**[P1] Track live iteration count where detection is unambiguous.**~~ Uses retained instruction trace and reports a lower bound if sequence gaps are actually observed.
- [x] ~~**[P1] Explain conditional loop branches as `TAKEN` / `NOT TAKEN` using the actual flags.**~~
- [ ] **[P2] Support nested/simple adjacent loops without presenting speculative boundaries as certain.**

### “What just happened?” execution history

- [x] ~~**[P0] Add a bounded instruction trace/history buffer** independent of the I/O trace.~~
- [x] ~~**[P0] For each executed instruction record before/after deltas** for PC, registers and flags.~~
- [x] ~~**[P1] Show memory reads/writes caused by the instruction.**~~ Includes data/stack accesses and attempted writes to protected/uninstalled RAM.
- [x] ~~**[P1] Show I/O operations caused by the instruction and link them to the configured MITS serial board/port.**~~ Historical entries label board mapping as current configuration when appropriate.
- [x] ~~**[P1] Add a “What just happened?” panel** explaining the last instruction in human terms.~~
- [x] ~~**[P1] Allow pausing/following history without stopping capture unintentionally.**~~ Trace ownership is centrally aggregated across all debugger consumers.

### Stack / calls / control flow

- [x] ~~**[P1] Add CALL/RET/RST stack visualization** around SP, including pushed return addresses.~~
- [x] ~~**[P1] Detect likely call frames conservatively** and label uncertainty instead of inventing symbols.~~
- [x] ~~**[P1] Add debugger `Step over`, `Step out` and `Run to cursor/address`.**~~ Step over/out use PC+SP guards; manual Run-to remains address-based.
- [x] ~~**[P1] Add execute breakpoints.**~~ Stops at true instruction boundaries; Cycle Accurate distinguishes PC(reg) from EXEC.
- [x] ~~**[P1] Add memory read/write watchpoints.**~~ Data/stack accesses only; opcode/operand fetches are deliberately excluded.
- [ ] **[P2] Add conditional breakpoints/watchpoints** over registers/flags/address/value.

### Memory activity visualization

- [x] ~~**[P1] Track READ / WRITE / EXECUTE activity separately** and provide an optional overlay/heatmap in the RAM viewer.~~ Includes explicit EXEC/READ/WRITE edge markers plus frequency tinting.
- [x] ~~**[P1] Add explicit STACK / PC / HL/M markers** without moving surrounding layout as addresses change.~~ RAM/debugger layouts reserve stable geometry; EXEC is shown separately from raw PC where required.
- [ ] **[P2] Add per-address recent access counters/timestamps with a clear/reset action.** Counters, last retained trace sequence and clear are present; wall-clock/age timestamps are still pending.
- [ ] **[P2] Link memory activity back to the instruction-history entry that caused it.** Last trace sequence is shown, but direct navigation/backlink is still pending.

### Bus / front-panel teaching

- [x] ~~**[P1] Add an educational machine-cycle/T-state view for the Cycle Accurate engine**, showing address, data, S-100 status/control lines and current machine cycle.~~ `Bus / T-state Teacher` exposes exact Cycle samples and debugger T-state/machine-cycle stepping.
- [x] ~~**[P1] Explain why the corresponding front-panel LEDs are lit for the selected/current cycle.**~~ Separates raw S-100/status state from visible optical persistence and explains active signals.
- [ ] **[P2] Provide a side-by-side “instruction → machine cycles → T-states → panel LEDs” explanation.** The Teacher currently explains the selected/live T-state, not a complete multi-cycle timeline for the whole instruction.
- [x] ~~**[P2] In Fast mode, clearly label reconstructed/synthesized bus activity as approximate.**~~ Exact T-state/pin fields remain unknown rather than being fabricated.

---

## P1 — Runtime, UI scheduling and performance

- [ ] **[P1] Investigate native secondary-window drag stutter** (`show_viewport_immediate` viewports move in visible steps even with the Altair powered off).
- [ ] **[P1] Instrument frame/update timings** so CPU time, ASR rendering, panel rendering, child viewport work and OS event latency can be measured separately.
- [ ] **[P1] Evaluate a real Windows move/resize freeze path** (`WM_ENTERSIZEMOVE` / `WM_EXITSIZEMOVE`) so DWM can move the last rendered surface smoothly while application animation is paused.
- [ ] **[P1] Evaluate decoupling CPU execution from the egui event/render loop** using cooperative slices or a worker architecture without sacrificing deterministic machine state.
- [ ] **[P1] Prevent `Unlimited` Cycle Accurate execution from monopolizing the UI thread.**
- [ ] **[P1] Fix Authentic 2 MHz long-term timing debt.** The current runtime caps a delayed frame’s `dt`, which can permanently discard elapsed emulation time under host stalls.
- [ ] **[P1] Audit all `request_repaint` / `request_repaint_after` paths** and remove wakeups when no visible/mechanical state can change.
- [ ] **[P2] Add lightweight runtime performance counters/FPS/frame-time diagnostics** behind a developer/debug option.
- [ ] **[P2] Add repeatable performance benchmarks for Fast vs Cycle Accurate** and for heavy UI windows (RAM viewer, I/O Inspector, ASR-33).

---

## P1 — Front-panel timing / LED fidelity

- [ ] **[P1] Replace fixed `PANEL_FRAME` visual integration time with real elapsed render time** where appropriate; current visual persistence should represent wall-clock perception, not an assumed 16 ms frame.
- [ ] **[P1] Review the front-panel activity sample cap/window** so accelerated execution does not always bias LEDs toward the first sampled T-states of a host frame.
- [ ] **[P1] Add deterministic raw-duty diagnostics** for STATUS / ADDRESS / DATA lamps before optical presentation mapping.
- [ ] **[P1] Compare raw lamp duty between Fast and Cycle cores on fixed programs** to distinguish CPU/bus differences from presentation differences.
- [ ] **[P2] Allow Brightness/Aura controls to influence extremely weak visible activity predictably**; today activity below the hard visibility threshold cannot be recovered by Brightness.
- [ ] **[P2] Add named/calibrated LED visual presets only after measurement**, keeping the current live controls available.
- [ ] **[P2] Keep video/camera matching explicitly separate from electrical fidelity** because exposure/rolling shutter/LED optics affect reference footage.

---

## P1 — CPU / machine / backend architecture

- [x] ~~**[P1] Remove the duplicate Fast `Cpu8080` state that remains as a mirror inside the Cycle Accurate machine integration**, making the chassis truly CPU-core agnostic.~~ Completed and locally validated: Cycle now physically owns `AltairChassis + Cpu8080Cycle`; Fast alone owns `AltairMachine + Cpu8080`, with regression guards against reintroducing the dormant Fast CPU, alias or `Deref` wrapper.
- [ ] **[P1] Propagate Cycle Accurate core faults to the application as explicit errors/diagnostics** instead of allowing execution to appear silently stopped.
- [ ] **[P1] Rework Cycle memory reconfiguration so it does not rebuild the backend and accidentally discard unrelated chassis state.**
- [ ] **[P1] Harden `BackendHost` error handling** so backend errors are surfaced rather than converted into application `panic!` paths.
- [ ] **[P1] Gate UI operations using backend capabilities at the common interface**, even while only the two Rust 8080 engines are active.
- [ ] **[P1] Remove/restrict public concrete-backend escape hatches** such as direct `machine()/machine_mut()/into_machine()` access where they undermine the abstraction.
- [ ] **[P1] Add an architectural regression test** that fails if `src/app` starts depending directly on `AltairMachine`/concrete CPU/bus internals again.
- [ ] **[P2] Remove the historical `BackendHost::native()` alias** once all callers use explicit engine naming.

### Future S-100 CPU boards / Z80

> Design contract: a future Z80 enters RusTair as a historically documented physical S-100 CPU board, not as a SIMH/external backend and not as a dormant `Z80State`. See `docs/CPU_BOARD_ARCHITECTURE.md`.

- [x] ~~**[P2] Introduce an explicit physical `CpuBoard` identity distinct from the emulator engine.**~~ Current configuration exposes the MITS 8080 CPU Board, its Intel 8080 processor mapping and its authentic 2 MHz board clock without adding placeholder Z80 runtime state.
- [ ] **[P2] Select and document the exact historical Z80 S-100 CPU board from primary sources before implementation.** Cromemco ZPU is the leading candidate; clocking, signal adaptation and board-specific behaviour must be verified rather than guessed.
- [ ] **[P2] Implement/integrate a real Rust Z80 core independently of the S-100 chassis**, then add `CpuModel::ZilogZ80`; do not reserve a fake enum/state before the core exists.
- [ ] **[P2] Implement the selected Z80 CPU-board adapter against the existing S-100 electrical authority**, preserving chassis/RAM/serial/front-panel ownership and modelling the board’s real signal translation.
- [ ] **[P2] Move runtime scheduling from the global 2 MHz assumption to the installed CPU board’s `clock_hz()` before a second CPU board becomes selectable.**
- [ ] **[P2] Migrate persistence from the legacy processor-only key to an explicit CPU-board key when the second real board exists**, while continuing to load old Intel-8080 configurations safely.
- [ ] **[P2] Add board/core compatibility checks and require POWER OFF for CPU-board replacement.** Never migrate live CPU registers or hidden execution state between different boards.
- [ ] **[P2] Add Z80 CPU state, disassembly/debugger/teaching support only with the real core**, keeping 8080 and Z80 architecture-specific views explicit where their registers/instructions differ.
- [ ] **[P2] Validate the Z80 board electrically and functionally**: reset, memory/I/O cycles, interrupts, wait/bus arbitration behaviour, front-panel observations, 8080-compatible software and Z80-specific instruction suites.

### S-100 / memory fidelity

- [x] ~~**[P1] Add a real S-100 interrupt-request path and interrupt-producing device model** before claiming interrupt-capable peripheral fidelity.~~ Canonical PINT plus 88-SIO/88-2SIO level-sensitive IRQ sources are implemented for both Rust backends.
- [ ] **[P2] Replace the fixed logical 1 KiB protection-board assumption with explicit memory-board modelling** if/when board-level memory fidelity is pursued.
- [ ] **[P2] Revisit unmapped/open-bus memory behavior** instead of always returning deterministic `00h`; make any historical/open-bus policy explicit and testable.

---

## P1 — Serial hardware fidelity

- [ ] **[P1] Move UART transmit/receive timing ownership into the emulated serial board** rather than allowing ASR/Terminal/TCP/COM endpoint pacing to define when hardware TX completes.
- [ ] **[P1] Model RX overrun/error conditions** instead of treating endpoint queues as infinitely forgiving hardware.
- [ ] **[P1] Expand the 88-2SIO / MC6850 model** beyond the BASIC-required subset: control word/framing, parity, stop bits, error flags, overrun and IRQ behavior.
- [ ] **[P1] Audit 88-SIO revision-specific behavior/status semantics** against hardware documentation and make deliberate compatibility choices explicit.
- [x] ~~**[P1] Connect serial IRQ generation to the future S-100 interrupt path.~~ 88-SIO/88-2SIO IRQ conditions now drive canonical S-100 PINT and direct RST 7 acknowledge.
- [ ] **[P2] Add explicit parity/framing configuration where historically meaningful**, separating physical ASR-33 limitations from modern external COM/TCP endpoints.
- [ ] **[P2] Fix External TCP bind retry behavior** so a temporary bind failure is retried without requiring config change/restart.
- [ ] **[P2] Avoid synchronous COM worker `join()` on the UI thread** during close/reconfigure.
- [ ] **[P2] Review whether persisted “External TCP enabled” / COM state should automatically reactivate external services on startup** versus remembering settings but requiring an explicit reconnect/start action.

---

## P1/P2 — ASR-33 remaining fidelity and polish

- [ ] **[P1] Perform a full manual + automated punch test pass.** Reader has been exercised heavily; punch transport still needs equivalent end-to-end validation.
- [ ] **[P1] Restore continuous audio loops correctly after configuration restore/unmute.** Fan and ASR motor should resume when their logical state says they are running.
- [ ] **[P1] Restore ASR-33 LINE/LOCAL persisted mode through the same side-effect path as manual mode changes** so motor/audio state cannot disagree with logical mode.
- [ ] **[P2] Add a dedicated historically sourced paper-reader sound** with correct attribution/licensing instead of the generic click, if a distributable source is available.
- [ ] **[P2] Add/verify a dedicated punch sound** based on a legitimate source; do not label a synthetic substitute as a historical recording.
- [ ] **[P2] Revisit parity/printer-control details from the original Python ASR-33 reference** and implement only behavior supported by hardware documentation.
- [ ] **[P3] Consider richer physical tape-motion visualization** only if it improves understanding without obscuring the controls already implemented.
- [ ] **[P1] Keep native-window drag stutter as an explicit ASR/viewport debt** until the runtime/windowing item above is solved.

---

## P1/P2 — Configuration and persistence

- [x] ~~**[P1] Use atomic configuration replacement instead of direct overwrite, retain failed state and retry after save errors.**~~
- [ ] **[P1] Actually validate `CONFIG_VERSION` and provide schema migration rules.**
- [ ] **[P1] Detect malformed/corrupt configuration and tell the user what was ignored/recovered** instead of silently falling back and potentially overwriting it.
- [ ] **[P1] Add autosave debounce** so dragging sliders or rapidly changing settings does not write the file on every frame/change.
- [ ] **[P2] Move persistence runtime state out of the global `OnceLock<Mutex<...>>` singleton and into application-owned state** for cleaner lifecycle/testing.
- [ ] **[P2] Move `persistence.rs` to a semantically clean app-level module** instead of wiring it through the UI module path.
- [ ] **[P2] Add a visible “configuration path / reset configuration” utility** for troubleshooting without requiring manual `%APPDATA%` knowledge.
- [ ] **[P3] Optional config import/export** after the schema is versioned and stable.

---

## P1/P2 — Loaders, files and dialogs

- [ ] **[P1] Make normal file dialogs non-blocking/asynchronous** so Windows Explorer stalls cannot freeze the emulator UI.
- [ ] **[P1] Redesign raw `Load binary…` semantics.** It currently always loads at `0000h`; allow an explicit base address and clearly define whether the machine must STOP/reset first.
- [ ] **[P1] Prevent accidental live RAM replacement while the CPU is running**, or make “debugger live patch” a separate explicit action.
- [ ] **[P2] Preserve a clear distinction between program loading, debugger patching and paper-tape media mounting.**
- [ ] **[P3] Consider Intel HEX/other address-bearing formats** only after raw binary loading is correct and unambiguous.

---

## P1/P2 — Tests and validation hardening

- [ ] **[P1] Add common `BackendHost` parity tests for Fast and Cycle** instead of testing some front-panel behavior only through concrete `AltairMachine` paths.
- [ ] **[P1] Add end-to-end ASR paper-reader tests** for mount/read/pause/resume/rewind/eject, LINE/OFF/LOCAL, disconnected port and 1×/5×/10×/Unlimited pacing.
- [ ] **[P1] Add end-to-end punch tests** for blank tape, pause/resume, queued bytes, finish/save retry and exact 8-bit output.
- [ ] **[P1] Add deterministic serial-card conformance tests** for status/data/control/overrun once the fuller UART models are implemented.
- [ ] **[P1] Add front-panel integration tests through both backends** for known I/O polling loops and stable duty-cycle expectations.
- [x] ~~**[P1] Add tests for debugger decoder/control-flow/loop detection before enabling educational conclusions in the UI.**~~ Covered by decoder coverage/timing, loop, history, debugger execution, Bus Teacher and architecture regressions.
- [ ] **[P2] Add performance regression benchmarks** for Cycle core, long diagnostics and high-rate UI traces.
- [ ] **[P2] Document the intentionally ignored long-running tests and the exact `--release --ignored` commands/results expected before major releases.**

---

## P1/P2 — Code cleanup / maintainability

- [ ] **[P1] Run a fresh dead-code/unused-field audit after the recent merges** and remove leftovers rather than suppressing warnings.
- [ ] **[P2] Remove obsolete `Tex::tty_keys`/old ASR renderer leftovers** if still unused after a final reference check.
- [ ] **[P1] Split very large UI/controller files where responsibilities are now clearly separable** (`memory_viewer.rs`, `io_inspector.rs`, ASR UI/controller), without gratuitous abstraction.
- [ ] **[P1] Centralize duplicated serial endpoint/UI logic** where it improves correctness without hiding hardware-specific behavior.
- [ ] **[P2] Clean temporary/obsolete development branches and historical integration scaffolding** after confirming no unique work is stranded there. This includes accidental `tmp-*` merge-safety branches currently present on GitHub.
- [ ] **[P2] Audit embedded assets for duplicates/obsolete source artwork** while preserving required build inputs and provenance.

---

## P1/P2 — Documentation / licensing / project hygiene

- [ ] **[P1] Rewrite the root `README.md` current-state section.** It still describes essentially one 8080 core and an 8 KiB model and omits Fast/Cycle engines, configurable RAM, 88-SIO/88-2SIO, routing, TCP/COM, persistence, diagnostics and current ASR tape controls.
- [x] ~~**[P1] Update `src/backend/README.md`** so it describes the architecture that exists now rather than old branch/refactor plans.~~ Updated for the Rust-only Fast/Cycle architecture and CPU-free Cycle chassis.
- [ ] **[P1] Complete `THIRD_PARTY.md` / provenance review** for diagnostic binaries, fonts, images and audio before a public release.
- [ ] **[P1] Review `.github/workflows/*` automatic triggers** against the project rule that GitHub Actions must never be run without explicit permission; prefer manual-only behavior if appropriate.
- [ ] **[P2] Add a concise architecture document** covering UI → `BackendHost/MachineBackend` → chassis/core, serial routing, timing ownership and presentation-vs-electrical front-panel layers.
- [ ] **[P2] Add a historical-fidelity notes document** identifying deliberate approximations, compatibility hacks and optional non-historical conveniences.

---

## P2/P3 — Additional educational features

- [ ] **[P2] Add named explanations for common 8080 idioms** (zeroing A, 16-bit loops, string/memory traversal, polling loops) only when structurally detected with high confidence.
- [ ] **[P2] Recognize serial polling loops and explain them in terms of the installed 88-SIO/88-2SIO and connected ASR/terminal.**
- [ ] **[P2] Add optional common-address annotations** (reset vector, RST vectors, loaded image ranges) without pretending unknown symbols are known source labels.
- [ ] **[P2] Allow copying disassembly/history/trace snippets for teaching/debugging.**
- [ ] **[P3] Session snapshots for debugging/teaching** (machine state + RAM + key debugger metadata), clearly separate from normal power-on configuration persistence.

---

## Completed foundation / progress log

These items pre-date this TODO but are retained as the baseline from which the active list was audited.

- [x] ~~Backend abstraction established so the application primarily talks through `BackendHost/MachineBackend` rather than directly through `AltairMachine`.~~
- [x] ~~Fast Intel 8080 engine integrated.~~
- [x] ~~Cycle Accurate Intel 8080 engine integrated with exact T-state/bus activity support.~~
- [x] ~~Cross-core control-line baseline validates Fast and Cycle semantics, including HOLD freeze after HLDA.~~
- [x] ~~Embedded CPU diagnostic suite and reference cycle/T-state validation integrated.~~
- [x] ~~Configurable RAM sizes / initialization and memory protection support integrated.~~
- [x] ~~MITS 88-SIO and 88-2SIO selectable, including BASIC 3.2 sense-switch behavior.~~
- [x] ~~Explicit serial routing for ASR-33, Text Terminal, External TCP and External COM.~~
- [x] ~~External raw TCP endpoint, data modes, optional multi-client, duplex and trace support.~~
- [x] ~~Real host COM serial endpoint integrated.~~
- [x] ~~I/O Inspector/editor with port map, tracing, UART/TCP/COM inspection and debugger injection tools.~~
- [x] ~~RAM Viewer includes PC/SP, CPU register pairs/flags, current instruction disassembly, memory map/protection, navigation and byte editing.~~
- [x] ~~Front-panel photographic switch sprites, momentary/latching interaction and major panel-control fidelity integrated.~~
- [x] ~~Front-panel LED optical response improved and live Brightness/Aura controls added.~~
- [x] ~~Application configuration persists across restarts in `%APPDATA%\RusTair\config.ini`.~~
- [x] ~~Configuration writes made atomic/retryable after audit.~~
- [x] ~~ASR-33 independent native viewport, LINE/LOCAL/OFF semantics, keyboard/mechanical animation, auto-return and audio integrated.~~
- [x] ~~Paper tape preserves all 8 bits and no longer consumes data while disconnected/OFF/LOCAL/not runnable.~~
- [x] ~~ASR-33 paper reader controls implemented: Put tape, Read/Resume, Pause, Rewind, Eject, state diagnostics and 1×/5×/10×/Unlimited.~~
- [x] ~~ASR-33 punch transport controls implemented with pause/resume/save-retry and 1×/5×/10×/Unlimited.~~
- [x] ~~Reader byte HEX/octal/ASCII display and paper-hole visualization with reversible 8→1 / 1→8 operator view integrated.~~
- [x] ~~ASR-33 header/transport controls made responsive instead of depending on a fixed width breakpoint.~~
- [x] ~~Bundled Microsoft 4K BASIC Quick Load path integrated.~~
- [x] ~~Runtime assets embedded into release executables.~~