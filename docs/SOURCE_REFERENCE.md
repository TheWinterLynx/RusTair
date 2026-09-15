# RusTair Source Reference

This is the file-by-file map of the current Rust source tree. It answers **what each source file owns** and which architectural invariant matters when changing it.

The descriptions below document current production architecture. Test-only Rust files under `src/` are included because `tests/developer_documentation_inventory.rs` requires every `src/**/*.rs` source to remain represented here.

> Start from the subsystem you want to change, then read its implementation, focused tests and the current architecture document that defines its contract.

---

## 1. Crate entry points and cross-cutting source files

| File | Mission and characteristics |
| --- | --- |
| `src/main.rs` | Desktop executable entry point; delegates startup to `rustair::app::run()`. |
| `src/lib.rs` | Crate/module root and crate-private infrastructure/re-exports. |
| `src/adaptive_metrics.rs` | Opt-in Full/Partial metrics. Observational only; metrics never drive execution. |
| `src/audio.rs` | Host presentation audio. Audio failure is non-fatal and never hardware authority. |
| `src/cpu8080.rs` | Instruction-level 8080 semantic executor used transiently inside admitted Full windows and as a semantic/reference core. Never a persistent second CPU. |
| `src/decoder8080.rs` | Stateless debugger/disassembler metadata and control-flow/effect classification. |
| `src/explain8080.rs` | Human-readable teaching explanations derived from decoded/current context. |
| `src/debugger8080.rs` | Conservative debugger analysis, including safe decoding and simple loop inference. |
| `src/debugger_control.rs` | Breakpoint/watchpoint/run-to/step policy. Requests execution stops; does not own a CPU loop. |
| `src/callstack8080.rs` | Bounded-history call-stack inference; never architectural state. |
| `src/trace8080.rs` | Instruction/effect trace model and bounded history. |
| `src/memory_activity8080.rs` | Derived memory-access observations for tools/UI. |
| `src/embedded_assets.rs` | Compile-time built-in asset lookup. |
| `src/full_boundary_reconcile.rs` | Zero-emulated-time Full→Partial CPU-board/S-100 boundary reconciliation. |
| `src/mc6850.rs` | MC6850 ACIA register/status/shift semantics used by 88-2SIO. Board clock/straps live above the chip. |
| `src/s100_cycle_integration_tests.rs` | Crate-internal CPU-cycle/S-100 integration oracles. |

---

## 2. `src/app/` — desktop application and workflow orchestration

The app owns host/UI workflow state. It must never become a second CPU, RAM, UART or serial oscillator.

| File | Mission and characteristics |
| --- | --- |
| `src/app/mod.rs` | `RusTairApp` composition root: configuration, backend host, serial router/endpoints, peripheral controllers, audio, execution clock and UI assets. |
| `src/app/runtime.rs` | `eframe::App::update`; coordinates frame scheduling, backend execution and endpoint/peripheral servicing. Separates host/UI time from modeled hardware domains. |
| `src/app/execution_clock.rs` | Converts host elapsed time into guest CPU T-state debt for throttled CPU speeds. It does not define serial baud or peripheral clocks. |
| `src/app/execution_frame.rs` | CPU frame scheduler. In Authentic/X2/X5/X10 it queries card-owned serial deadlines, leaves quiet UARTs on the normal 4096T service slice, advances the corresponding serial physical time after each CPU interval and replans around serial-`OUT` activation. Unlimited remains host-deadline responsive and uses backend `Instant` serial time. |
| `src/app/commands.rs` | Application-level user command workflows. |
| `src/app/persistence.rs` | Configuration save/load and schema migration. Historical aggregate fields must normalize to one live slot-native topology. |
| `src/app/authentic_loader.rs` | Authentic paper-tape/BASIC bootstrap workflow through emulated peripheral/serial hardware. |
| `src/app/cpu_diagnostics.rs` | External/classic CPU diagnostic workflow. |
| `src/app/embedded_cpu_diagnostics.rs` | Bundled diagnostic selection/run workflow. |
| `src/app/asr33_controller.rs` | Connects ASR-33 peripheral mechanics/input/output to serial routing without replacing card UART state. |
| `src/app/asr33_state.rs` | ASR host/UI transient/presentation state. |
| `src/app/terminal_controller.rs` | Text-terminal controller logic. |
| `src/app/terminal_serial.rs` | Terminal transfer/pacing integration. Endpoint pacing remains distinct from card baud/clock authority. |
| `src/app/terminal_state.rs` | Text-terminal host buffers/preferences/presentation timing. |
| `src/app/serial_hardware.rs` | App coordination between installed serial hardware, backend ports and explicit endpoint/cable choices. Must not silently restrap a card to match an endpoint. |
| `src/app/external_serial.rs` | Host TCP/external serial endpoint lifecycle and servicing. |
| `src/app/external_com.rs` | Host physical COM endpoint lifecycle/config/servicing. |

### `src/app/ui/`

| File | Mission and characteristics |
| --- | --- |
| `src/app/ui/mod.rs` | UI module root/shared helpers. |
| `src/app/ui/main_menu.rs` | Canonical top-level navigation. |
| `src/app/ui/assets.rs` | egui textures/fonts/visual assets. Presentation only. |
| `src/app/ui/front_panel.rs` | Main photographic Altair panel renderer; consumes backend/machine truth. |
| `src/app/ui/front_panel_3d.rs` | Embedded-GLB WGPU Altair renderer and camera/presentation state. Presentation only: it must consume the same machine/front-panel truth as the 2D renderer and never own emulated hardware state. |
| `src/app/ui/front_panel_3d_switch_runtime.rs` | Parses embedded 3D switch bindings and GLB lever transforms into renderer-only pivots/axes/indices. It derives presentation transforms from the asset contract and never owns switch or machine state. |
| `src/app/ui/front_panel_3d_gpu_tests.rs` | Opt-in headless GPU render/readback regressions for material pipelines, MSAA and sRGB versus UNORM presentation. See `docs/ALTAIR_3D_RENDERING.md`. |
| `src/app/ui/front_panel_assets.rs` | Front-panel visual asset/layout helpers. |
| `src/app/ui/front_panel_switches.rs` | Switch geometry/input/rendering helpers. |
| `src/app/ui/front_panel_operator.rs` | Operator-oriented panel/control window using backend controls. |
| `src/app/ui/cpu_pin_diagram.rs` | Didactic 8080 pin visualization from captured backend state. |
| `src/app/ui/bus_teacher.rs` | Bus/T-state teaching UI; historical snapshots remain observations. |
| `src/app/ui/debugger_controls.rs` | Breakpoint/watchpoint/run-to/step controls. |
| `src/app/ui/execution_position.rs` | Current execution-position presentation helpers. |
| `src/app/ui/instruction_history.rs` | Bounded instruction-history/disassembly UI. |
| `src/app/ui/loop_inspector.rs` | Conservative loop-analysis UI. |
| `src/app/ui/memory_activity.rs` | Recent memory activity visualization. |
| `src/app/ui/memory_viewer.rs` | Physical RAM inspection/debug mutation through backend APIs; no copied RAM authority. |
| `src/app/ui/s100_memory_inspection.rs` | S-100 responder/overlap/protection/mapping inspection. |
| `src/app/ui/s100_hardware.rs` | Physical chassis/card configuration UI; topology changes require POWER OFF. |
| `src/app/ui/s100_hardware_editor.rs` | Dedicated S-100 inventory editor delegating to the same validated hardware authority. |
| `src/app/ui/io_inspector.rs` | Guest I/O plus host endpoint trace observations. |
| `src/app/ui/asr33.rs` | ASR-33 presentation/interaction helpers. |
| `src/app/ui/asr33_window.rs` | ASR-33 dedicated window/operator controls. |
| `src/app/ui/terminal.rs` | Text-terminal renderer/input; host buffers are not UART state. |

---

## 3. `src/backend/` — app-facing contract and Adaptive execution

| File | Mission and characteristics |
| --- | --- |
| `src/backend/mod.rs` | Public single-engine backend contract, neutral snapshots, serial physical-time/deadline API and `BackendHost` facade. |
| `src/backend/cycle_host.rs` | Host/debugger/scheduling policy facade. Owns managed-vs-`Instant` serial physical-time handoff and elapsed-time conversion, but never oscillator/UART state or an exact T-state loop. |
| `src/backend/bus_teaching.rs` | Stable didactic bus/pin/machine-cycle snapshots. Observer only. |
| `src/backend/cycle.rs` | Concrete Adaptive Cycle composition plus exact managed serial-barrier primitive. Exact T-state progression for the causal serial `OUT` barrier stays here rather than in the host facade. |
| `src/backend/cycle/partial_impl.rs` | Authoritative exact edge/T-state machine backend, lifecycle/control and debugger integration. Partial remains the physical oracle. |
| `src/backend/cycle/full.rs` | Adaptive Full executor/`FullInstructionBus`, safety/admission, caches, panel duty, control-flow exactness and boundary reconstruction. Detects pending `OUT` to installed serial hardware and can stop before it for managed causal replanning. |
| `src/backend/cycle/full/control_flow_tests.rs` | Full-versus-exact control-flow timing/boundary oracles. |
| `src/backend/cycle/full/ei_tests.rs` | Conservative Full delayed-EI/DI and interrupt-boundary oracles. |
| `src/backend/cycle/full/panel_histogram_reference.rs` | Independent/reference panel-duty oracle for the optimized Full representation. |

Read `src/backend/README.md` before editing this directory; it is the current ownership contract.

---

## 4. `src/cpu8080_cycle/` — exact Intel 8080 timing core

| File | Mission and characteristics |
| --- | --- |
| `src/cpu8080_cycle/mod.rs` | `Cpu8080Cycle` state/tick engine and synchronization-boundary CPU authority. |
| `src/cpu8080_cycle/alu.rs` | Exact-core ALU/flag semantics. |
| `src/cpu8080_cycle/decode.rs` | Execution-oriented exact-core opcode decode. |
| `src/cpu8080_cycle/control_flow.rs` | Exact call/jump/return/restart schedules. |
| `src/cpu8080_cycle/pins.rs` | Intel 8080 package input/output pin structures. |
| `src/cpu8080_cycle/state.rs` | CPU state helpers and Full window export/import. |
| `src/cpu8080_cycle/timing.rs` | Machine-cycle, status-word, T-state and PHI vocabulary. |
| `src/cpu8080_cycle/timing/phase.rs` | Edge-level PHI1/PHI2 transition/sequencing logic. |
| `src/cpu8080_cycle/alu_tests.rs` | Exact ALU/flag tests. |
| `src/cpu8080_cycle/call_return_tests.rs` | CALL/RET/RST exact behavior/timing tests. |
| `src/cpu8080_cycle/control_flow_tests.rs` | Exact jump/conditional/control-flow tests. |
| `src/cpu8080_cycle/core_tests.rs` | Core instruction/tick/reset invariants. |
| `src/cpu8080_cycle/hold_tests.rs` | HOLD/HLDA entry/dwell/resume tests. |
| `src/cpu8080_cycle/interrupt_control_tests.rs` | Interrupt/DI/EI timing tests. |
| `src/cpu8080_cycle/io_tests.rs` | Exact IN/OUT machine-cycle tests. |
| `src/cpu8080_cycle/special_transfer_tests.rs` | Dedicated multi-cycle transfer timing tests. |

---

## 5. `src/machine/` — chassis, panel, disk and serial integration

| File | Mission and characteristics |
| --- | --- |
| `src/machine/mod.rs` | `AltairBus` composition, panel/control state, diagnostic metering and separation of CPU/chassis-time from serial physical-time advancement. |
| `src/machine/chassis.rs` | CPU-free `AltairChassis` lifecycle/control wrapper; RUN derives from the physical bus latch. |
| `src/machine/cpu_board.rs` | `Cpu8080Cycle` package ↔ MITS 8080 S-100 CPU-board adapter. |
| `src/machine/dcdd.rs` | MITS 88-DCDD two-board controller/harness and source-backed guest I/O/read-stream routing. Uses CPU/chassis virtual time, not serial physical time. |
| `src/machine/dcdd_read.rs` | DCDD Board #1 read-data latch/NRDA electronics. |
| `src/machine/fd400.rs` | FD-400 mechanics/media owner with O(1) CPU/chassis virtual-time epoch/deadline model. |
| `src/machine/fd400_media.rs` | Physical 77×32×137-byte removable-medium geometry/payload/write-protect state. |
| `src/machine/fd400_read.rs` | Source-backed first-byte/byte-cadence physical read-stream timing and O(1) catch-up. |
| `src/machine/front_panel.rs` | Front-panel operator/control state and physical operations. |
| `src/machine/panel_bus.rs` | Canonical raw `S100BusState` plus panel duty/persistence integration. Raw state is authority; brightness is derived. |
| `src/machine/memory.rs` | Machine facade over live `S100RuntimeFabric`. Separately forwards serial physical time/deadlines and DCDD/chassis virtual time; owns no second memory or generic merged peripheral clock. |
| `src/machine/serial.rs` | Historical/shared 88-SIO-facing types/logic exposed by the machine module. |
| `src/machine/serial_bus.rs` | Serial-only machine/card/endpoint bridge and serial physical-time/deadline forwarding. It no longer clocks DCDD mechanics; disk/chassis time has a separate path. |
| `src/machine/serial_card.rs` | Runtime serial-card handle/device boundary. Exposes controlled host access plus the deadline derived from that same card-owned state. |
| `src/machine/serial_devices.rs` | Installed serial device/card-family facade. Propagates/aggregates card deadlines, ignores quiet ports and keeps compatibility routing from becoming another UART authority. |
| `src/machine/sio.rs` | MITS 88-SIO/COM2502 card state, revisions, data/status, handshake/interrupt and timing. Owns effective next-clock deadline and returns no scheduler deadline while timing is quiet. |
| `src/machine/sio_interface.rs` | 88-SIO external electrical-interface conversion/connector rules. |
| `src/machine/two_sio.rs` | 88-2SIO/MC6850 board. Owns two ports, free-running external 16× baud-tap phase, `/1`/`/16`/`/64` divider phase/effective deadlines, straps, data/status/control and interrupt wiring. |

---

## 6. S-100 files — physical bus and live card runtime

| File | Mission and characteristics |
| --- | --- |
| `src/s100.rs` | Historical S-100 signals/pins/contact roles/card contract vocabulary. |
| `src/s100_interface.rs` | Common card/backplane electrical interfaces and host-inspection boundary. |
| `src/s100_backplane.rs` | Card-agnostic High-Z/strong/open-collector/contention resolver with cached/delta hot path. |
| `src/s100_chassis.rs` | 8800/8800a/8800b physical connector populations/topology. |
| `src/s100_runtime.rs` | Live physical fabric/topology authority. Materializes cards, owns backplane/decode tables/harnesses, keeps `advance_serial_time` and `advance_dcdd_time` as separate domains and aggregates the earliest installed serial-card deadline. |
| `src/s100_cpu.rs` | Live MITS 8080 CPU-board S-100 card and reconciliation/inspection hooks. |
| `src/s100_memory.rs` | Historical RAM-board configuration/decode/timing/protection properties. |
| `src/s100_runtime_ram.rs` | Live RAM card and authoritative guest byte storage. |
| `src/s100_runtime_ram/full_timing.rs` | Full transactional timing acceleration for supported dynamic RAM refresh/WAIT state while preserving authoritative RAM bytes. |
| `src/s100_io.rs` | Precompiled I/O/interrupt responder masks from physical straps; acceleration metadata only. |
| `src/s100_io_card.rs` | S-100 electrical adapter connecting runtime serial/I/O cards to generic bus cycles. |

---

## 7. `src/config/` — desired hardware and host preferences

| File | Mission and characteristics |
| --- | --- |
| `src/config/mod.rs` | Configuration root/re-exports. |
| `src/config/machine.rs` | Machine/CPU speed/migration configuration plus ASR/terminal endpoint pacing choices. Host CPU speed and endpoint pacing are not serial-card clock authority. |
| `src/config/s100_hardware.rs` | Slot-native desired S-100 inventory and topology validation. |
| `src/config/s100_codec.rs` | Slot-native hardware persistence codec. |
| `src/config/sio.rs` | 88-SIO address/revision/format/baud/interface/interrupt physical configuration. |
| `src/config/sio_electrical.rs` | Shared serial physical-interface configuration. |
| `src/config/two_sio.rs` | 88-2SIO address/baud-tap/interface/interrupt straps. Card configuration remains independent of endpoint pacing. |
| `src/config/external_serial.rs` | Host TCP serial endpoint settings. |
| `src/config/external_com.rs` | Host COM endpoint settings. |
| `src/config/terminal.rs` | Text-terminal host configuration/pacing settings. |

---

## 8. `src/io/` — host transports and cable routing

| File | Mission and characteristics |
| --- | --- |
| `src/io/mod.rs` | Host I/O transport module root. |
| `src/io/serial_router.rs` | Explicit endpoint↔emulated-port cable routing. It does not auto-synchronize card baud to endpoint settings. |
| `src/io/tcp_serial.rs` | Host TCP transport/buffering/trace; not a UART. |
| `src/io/com_serial.rs` | Host physical COM transport/config/control lines; separate from emulated UART/card state. |

---

## 9. `src/peripherals/asr33/` — ASR-33 teletype

| File | Mission and characteristics |
| --- | --- |
| `src/peripherals/mod.rs` | Peripheral module root. |
| `src/peripherals/asr33/mod.rs` | ASR-33 module root/re-exports. |
| `src/peripherals/asr33/model.rs` | Core teletype state/printing/input coordination. |
| `src/peripherals/asr33/keyboard.rs` | ASR-33 key definitions/mapping/encoding. |
| `src/peripherals/asr33/paper_tape.rs` | Paper-tape reader/punch data/transport model. |
| `src/peripherals/asr33/answerback.rs` | Answerback mechanism/state. |
| `src/peripherals/asr33/mechanics.rs` | Mechanical carriage/paper/print timing. Endpoint mechanics are downstream from card baud. |

---

## 10. Current cross-cutting serial-clock ownership

For any change involving baud, serial progress or host scheduling, inspect this chain together:

```text
src/app/execution_clock.rs             CPU host-time debt only
src/app/execution_frame.rs             card-deadline-aware managed scheduling
src/backend/mod.rs                     serial physical-time/deadline contract
src/backend/cycle_host.rs              managed/Instant handoff and elapsed-time bridge
src/backend/cycle.rs                   exact managed serial barrier T-state primitive
src/backend/cycle/full.rs              stop-before-installed-serial-OUT barrier
src/machine/memory.rs                  separates serial and DCDD/chassis forwarding
src/machine/serial_bus.rs              serial-only machine/card boundary
src/machine/serial_card.rs             card state + card-owned deadline handle
src/machine/serial_devices.rs          family propagation/earliest active deadline
src/machine/sio.rs                     COM2502 effective deadline
src/machine/two_sio.rs                 16× tap phase + effective MC6850 divider deadline
src/s100_runtime.rs                    independent serial/DCDD time advancement + aggregation
```

The current contract is documented in `docs/SERIAL_CLOCK_DOMAINS.md`.

---

## 11. Files that are especially dangerous to change casually

- `src/backend/cycle/partial_impl.rs` — exact physical oracle.
- `src/backend/cycle/full.rs` — optimized path and serial `OUT` barrier.
- `src/backend/cycle.rs` — exact managed barrier ownership.
- `src/backend/cycle_host.rs` — host scheduling must not acquire a T-state loop or duplicate device state.
- `src/cpu8080_cycle/mod.rs`, `src/cpu8080_cycle/timing.rs`, `src/cpu8080_cycle/timing/phase.rs` — exact processor timing.
- `src/machine/panel_bus.rs` — raw bus/panel authority.
- `src/s100_backplane.rs` — electrical resolution.
- `src/s100_runtime.rs` — live topology plus time-domain/decode hot paths.
- `src/s100_runtime_ram.rs` — authoritative guest RAM.
- `src/machine/dcdd.rs`, `src/machine/dcdd_read.rs`, `src/machine/fd400.rs`, `src/machine/fd400_read.rs`, `src/machine/fd400_media.rs` — source-backed disk controller/drive/media and CPU/chassis-time behavior.
- `src/machine/sio.rs`, `src/machine/two_sio.rs`, `src/mc6850.rs` — guest-visible serial state/clocking.
- `src/full_boundary_reconcile.rs` — Full→Partial physical re-entry.
- `src/app/execution_clock.rs`, `src/app/execution_frame.rs` — host scheduling versus hardware time domains.

A change in these normally needs focused fidelity tests, not only manual UI validation.

---

## 12. Intentionally derived state

Decoder/explanation/debug analysis, trace/history, memory activity, most UI viewers, Bus Teacher presentation and Adaptive metrics are derived observations. They may be stale or bounded by design and must never feed back as hardware authority.

---

## 13. Integration-test reference

Integration tests under `tests/` are catalogued separately in [`TEST_REFERENCE.md`](TEST_REFERENCE.md). Important groups cover Adaptive authority, classic diagnostics, exact timing, front panel, S-100 topology/electrical behavior, 88-SIO/88-2SIO hardware, serial clock-domain independence, debugger architecture, authentic loading and manual performance evidence.
