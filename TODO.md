# RusTair — Living TODO

> Source of truth for remaining project work. Initial audit: 2026-08-26, `main` at `2402bd2` before this file.
>
> **Rule:** when an item is completed, keep it in place and change it to `- [x] ~~completed item~~` (optionally adding the commit). Do not delete completed items; the file is also the project progress log.
>
> **Priorities:** `P0` = next/core goal · `P1` = important debt/correctness · `P2` = worthwhile improvement · `P3` = optional/polish · `PARKED` = do not work on it without explicit instruction.
>
> **Validation rule:** normal changes are validated locally with `cargo test` and `cargo build --release`. Do **not** run GitHub Actions without explicit permission.

## Recommended active order

1. **Authentic paper-tape bootstrap/loader path.**
2. **Didactic RAM viewer + debugger.**
3. **Runtime/UI scheduling and native-window smoothness.**
4. **Serial-card/UART fidelity and interrupt path.**
5. **Architecture, persistence, cleanup, documentation and test hardening.**

---

## P0 — Authentic Altair paper-tape bootstrap / loader

> Active implementation branch: `feature/authentic-paper-tape-bootstrap`. Source implementation currently includes the split Quick/Authentic workflow, manual octal procedure, assisted EXAMINE/DEPOSIT installation, ASR-33/UART guest-paced path, 4 KiB/port/mode/sense validation, loader diagnostics and Fast/Cycle regression tests. These implementation items remain unchecked until local `cargo test` + `cargo build --release` and a real `4K BASIC Ver 3-2.tap` end-to-end run are completed.

- [ ] **[P0] Keep Quick Load and Authentic Load as explicitly separate workflows.** Quick Load may continue copying bytes directly to RAM; Authentic Load must use the emulated machine, serial board and ASR-33 reader.
- [x] ~~**[P0] Establish and document the historically correct bootstrap loader(s)** for the supported MITS 88-SIO / 88-2SIO configurations, including provenance and exact bytes.~~ See `docs/AUTHENTIC_BASIC_BOOTSTRAP.md`.
- [ ] **[P0] Support manual front-panel entry of the bootstrap** as the fully authentic path.
- [ ] **[P0] Add an optional assisted “Install bootstrap” convenience action** that performs the same deposits transparently and shows exactly what was entered; it must not silently bypass the emulated loader.
- [ ] **[P0] Make Authentic Load consume the mounted ASR-33 paper tape through the selected serial port**, so `WAIT GUEST RX` advances because the bootstrap genuinely executes `IN` instructions.
- [ ] **[P0] Preserve reader transport controls and 1× / 5× / 10× / Unlimited speed** during authentic loading; acceleration must alter host/media pacing, not the logical byte stream.
- [ ] **[P0] Add loader progress/status diagnostics**: bootstrap running, waiting for RX, bytes consumed, destination range, end of tape, checksum/validation failure where applicable.
- [ ] **[P0] Make serial-board/sense-switch requirements visible to the operator.** In particular preserve the BASIC 3.2 88-SIO/88-2SIO sense-switch distinction rather than changing switches behind the user’s back.
- [ ] **[P0] Verify Authentic Load with both Rust engines** (`RusTair — Fast 8080` and `RusTair — Cycle Accurate 8080`).
- [ ] **[P0] Regression-test that authentic BASIC loading produces the expected RAM image/state** and reaches the same BASIC entry behavior as Quick Load without direct-RAM shortcuts.
- [ ] **[P1] Add deterministic tests for bootstrap failure modes**: wrong board, wrong port, ASR OFF/LOCAL, STOP state, RX not consumed, premature end-of-tape, insufficient RAM.

---

## P0/P1 — Didactic RAM viewer and debugger

### Shared 8080 decode/control-flow foundation

- [ ] **[P0] Extract the Intel 8080 decoder/disassembler from `memory_viewer.rs` into a shared structured decoder module.** UI, debugger, traces and future tools should use one opcode description source.
- [ ] **[P0] Decoder metadata should include** mnemonic, length, operands, immediate/address targets, flags affected, nominal timing, memory/I/O behavior and control-flow type.
- [ ] **[P1] Add tests covering all 256 opcode byte values**, including undocumented aliases currently accepted by the cores.

### Memory hover / instruction understanding

- [ ] **[P0] Enhance RAM-byte hover with opcode interpretation.** In addition to HEX/decimal/ASCII, show the 8080 instruction that would begin at that address, its bytes and operands.
- [ ] **[P0] Clearly distinguish “this byte can decode as…” from “CPU is executing this instruction”** so data bytes are not misleadingly presented as known code.
- [ ] **[P1] Add an “Explain instruction” view** with plain-language semantics, input/output registers, flags affected, memory/I/O accesses and T-state/machine-cycle information.
- [ ] **[P1] Explain `M` contextually as memory at `[HL]`**, including the current HL address/value when relevant.

### Loop inspector

- [ ] **[P0] Detect simple backward-branch loops around the current PC.**
- [ ] **[P0] Add a closable floating Loop Inspector** showing the whole loop disassembly instead of only the current instruction.
- [ ] **[P0] Highlight the live PC inside the loop** without causing layout movement/flicker.
- [ ] **[P1] Show loop entry, back-edge, exit condition and branch target.**
- [ ] **[P1] Track live iteration count where detection is unambiguous.**
- [ ] **[P1] Explain conditional loop branches as `TAKEN` / `NOT TAKEN` using the actual flags.**
- [ ] **[P2] Support nested/simple adjacent loops without presenting speculative boundaries as certain.**

### “What just happened?” execution history

- [ ] **[P0] Add a bounded instruction trace/history buffer** independent of the I/O trace.
- [ ] **[P0] For each executed instruction record before/after deltas** for PC, registers and flags.
- [ ] **[P1] Show memory reads/writes caused by the instruction.**
- [ ] **[P1] Show I/O operations caused by the instruction and link them to the configured MITS serial board/port.**
- [ ] **[P1] Add a “What just happened?” panel** explaining the last instruction in human terms.
- [ ] **[P1] Allow pausing/following history without stopping capture unintentionally.**

### Stack / calls / control flow

- [ ] **[P1] Add CALL/RET/RST stack visualization** around SP, including pushed return addresses.
- [ ] **[P1] Detect likely call frames conservatively** and label uncertainty instead of inventing symbols.
- [ ] **[P1] Add debugger `Step over`, `Step out` and `Run to cursor/address`.**
- [ ] **[P1] Add execute breakpoints.**
- [ ] **[P1] Add memory read/write watchpoints.**
- [ ] **[P2] Add conditional breakpoints/watchpoints** over registers/flags/address/value.

### Memory activity visualization

- [ ] **[P1] Track READ / WRITE / EXECUTE activity separately** and provide an optional overlay/heatmap in the RAM viewer.
- [ ] **[P1] Add explicit STACK / PC / HL/M markers** without moving surrounding layout as addresses change.
- [ ] **[P2] Add per-address recent access counters/timestamps with a clear/reset action.**
- [ ] **[P2] Link memory activity back to the instruction-history entry that caused it.**

### Bus / front-panel teaching

- [ ] **[P1] Add an educational machine-cycle/T-state view for the Cycle Accurate engine**, showing address, data, S-100 status/control lines and current machine cycle.
- [ ] **[P1] Explain why the corresponding front-panel LEDs are lit for the selected/current cycle.**
- [ ] **[P2] Provide a side-by-side “instruction → machine cycles → T-states → panel LEDs” explanation.**
- [ ] **[P2] In Fast mode, clearly label reconstructed/synthesized bus activity as approximate.**

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

- [ ] **[P1] Remove the duplicate Fast `Cpu8080` state that remains as a mirror inside the Cycle Accurate machine integration**, making the chassis truly CPU-core agnostic.
- [ ] **[P1] Propagate Cycle Accurate core faults to the application as explicit errors/diagnostics** instead of allowing execution to appear silently stopped.
- [ ] **[P1] Rework Cycle memory reconfiguration so it does not rebuild the backend and accidentally discard unrelated chassis state.**
- [ ] **[P1] Harden `BackendHost` error handling** so backend errors are surfaced rather than converted into application `panic!` paths.
- [ ] **[P1] Gate UI operations using backend capabilities at the common interface**, even while only the two Rust 8080 engines are active.
- [ ] **[P1] Remove/restrict public concrete-backend escape hatches** such as direct `machine()/machine_mut()/into_machine()` access where they undermine the abstraction.
- [ ] **[P1] Add an architectural regression test** that fails if `src/app` starts depending directly on `AltairMachine`/concrete CPU/bus internals again.
- [ ] **[P2] Remove the historical `BackendHost::native()` alias** once all callers use explicit engine naming.

### S-100 / memory fidelity

- [ ] **[P1] Add a real S-100 interrupt-request path and interrupt-producing device model** before claiming interrupt-capable peripheral fidelity.
- [ ] **[P2] Replace the fixed logical 1 KiB protection-board assumption with explicit memory-board modelling** if/when board-level memory fidelity is pursued.
- [ ] **[P2] Revisit unmapped/open-bus memory behavior** instead of always returning deterministic `00h`; make any historical/open-bus policy explicit and testable.

---

## P1 — Serial hardware fidelity

- [ ] **[P1] Move UART transmit/receive timing ownership into the emulated serial board** rather than allowing ASR/Terminal/TCP/COM endpoint pacing to define when hardware TX completes.
- [ ] **[P1] Model RX overrun/error conditions** instead of treating endpoint queues as infinitely forgiving hardware.
- [ ] **[P1] Expand the 88-2SIO / MC6850 model** beyond the BASIC-required subset: control word/framing, parity, stop bits, error flags, overrun and IRQ behavior.
- [ ] **[P1] Audit 88-SIO revision-specific behavior/status semantics** against hardware documentation and make deliberate compatibility choices explicit.
- [ ] **[P1] Connect serial IRQ generation to the future S-100 interrupt path.**
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
- [ ] **[P1] Add tests for debugger decoder/control-flow/loop detection before enabling educational conclusions in the UI.**
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
- [ ] **[P1] Update `src/backend/README.md`** so it describes the architecture that exists now rather than old branch/refactor plans.
- [ ] **[P1] Complete `THIRD_PARTY.md` / provenance review** for Open SIMH material already present, diagnostic binaries, fonts, images and audio before a public release.
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

## PARKED — SIMH / Z80 (do not work without explicit instruction)

> The existing files remain exactly as they are. These are recorded so the work is not forgotten, but they are **outside the active backlog until the user explicitly reactivates them**.

- [ ] **[PARKED] Open SIMH classic Altair backend integration/factory activation.**
- [ ] **[PARKED] Open SIMH serial/TMXR routing into the common ASR/Terminal/TCP/COM endpoint model.**
- [ ] **[PARKED] Open SIMH front-panel lamp/activity integration.**
- [ ] **[PARKED] Open SIMH memory/profile/configuration negotiation and disk operations through the common backend contract.**
- [ ] **[PARKED] AltairZ80 backend activation and Z80-aware CPU state/UI/debugger/disassembly.**
- [ ] **[PARKED] SIMH/Z80 smoke/integration testing and capabilities cleanup.**

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