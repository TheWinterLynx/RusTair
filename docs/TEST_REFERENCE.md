# RusTair Integration Test Reference

This file documents every Rust integration-test source under `tests/`. The broader philosophy and commands are in [TESTING_GUIDE.md](TESTING_GUIDE.md).

Each `tests/<name>.rs` file is compiled by Cargo as a separate integration-test target. Many tests are architecture or hardware-fidelity guards rather than ordinary application unit tests. Long profiling/classic diagnostic cases may be intentionally ignored by default.

> The short descriptions below tell you **what invariant to investigate first** when a test fails. Always read the actual test before changing behavior.

---

## 1. Adaptive Full/Partial, classic diagnostics and performance evidence

| Test file | Mission |
| --- | --- |
| `tests/8080exm_ei_successor_profile.rs` | Manual/ignored 8080EXM workload profiler that records which opcode follows EI. Preserves the empirical evidence supporting the conservative EI→LHLD Full specialization. Profiling evidence, not a general CPU correctness oracle. |
| `tests/8080exm_full_barrier_profile.rs` | Manual/ignored profile of Full-window barrier/fallback behavior during 8080EXM. Used to identify remaining fragmentation/coverage opportunities without weakening Full admission rules. |
| `tests/adaptive_cycle_metrics.rs` | Verifies opt-in Adaptive metrics count Full/Partial execution/transitions/reasons correctly and do not alter normal execution semantics. |
| `tests/cpu8080_adaptive_classic_diagnostics.rs` | Full production-machine classic 8080 diagnostic harness using Adaptive Cycle, live MITS CPU board, S-100 RAM/serial hardware and panel behavior. Contains the canonical CPUTEST/8080EXM reference-total tests. |
| `tests/cpu8080_cycle_classic_diagnostics.rs` | Runs classic diagnostics through the exact cycle-oriented CPU path to validate CPU semantics/timing independently of Adaptive Full acceleration. |
| `tests/cpu8080_cycle_differential.rs` | Differential comparison between exact cycle execution and instruction-level semantic/reference behavior at valid comparison boundaries. Finds semantic/timing-state divergences. |
| `tests/cpu8080_forced_partial_classic_diagnostics.rs` | Runs classic diagnostic workloads while forcing the exact Partial path, providing an end-to-end reference against the accelerated Adaptive behavior. |
| `tests/emulation_speed_benchmark.rs` | Controlled speed-mode/throughput benchmark infrastructure. Performance evidence rather than a hardware timing specification. |
| `tests/emulation_speed_ui.rs` | Guards the UI/configuration semantics of Authentic/accelerated/Unlimited speed choices and keeps host speed separate from the installed CPU board clock. |
| `tests/gui_execution_performance.rs` | Guards host GUI scheduling behavior, especially that Unlimited execution is not accidentally repaint-bound and throttled modes retain their intended scheduling semantics. |

---

## 2. CPU board, clock, pins, timing and interrupts

| Test file | Mission |
| --- | --- |
| `tests/cpu_board_clock_authority.rs` | Ensures authentic CPU clock ownership comes from the installed CPU board/configuration rather than duplicated global/UI assumptions; host speed multipliers must not redefine the board. |
| `tests/cpu_board_config.rs` | Validates CPU-board identity/configuration rules and supported current board selection. |
| `tests/cpu_pin_diagram.rs` | Cross-checks CPU pin/teaching/UI-facing pin observations against the exact backend so the diagram does not fabricate signal state. |
| `tests/ei_interrupt_timing_fidelity.rs` | External exact oracle for delayed EI enable and EI/DI/interrupt interaction timing at the real machine boundary. Critical when changing EI, INTE or interrupt admission. |
| `tests/memory_wait_timing.rs` | Proves RAM/card READY wait configuration produces the expected CPU wait-state placement/timing through the physical path. |
| `tests/run_reset_timing.rs` | Validates RUN/STOP and RESET timing/lifecycle behavior and their relationship with exact CPU/chassis state. |

---

## 3. Backend/state/chassis architecture authority

| Test file | Mission |
| --- | --- |
| `tests/backend_authority.rs` | Verifies the production backend is the authoritative machine execution path and compares Adaptive execution against exact/forced behavior for selected invariants. |
| `tests/backend_lifecycle_authority.rs` | Guards power/reset/run/lifecycle ownership so app/backend/chassis do not maintain conflicting state authorities. |
| `tests/chassis_architecture.rs` | Source/architecture guard for the CPU-free physical `AltairChassis` design and intended composition boundary. |
| `tests/no_simh.rs` | Retirement guard ensuring removed SIMH backend/FFI/build scaffolding and artifacts do not return accidentally. |
| `tests/state_source_architecture.rs` | Guards the documented single sources of truth for CPU/chassis/S-100/runtime state and prevents duplicate/mirrored architectural state. |
| `tests/unified_cycle_architecture.rs` | Guards the one-engine Adaptive Cycle architecture and prevents resurrection of the old separate Fast/Cycle backend model. |
| `tests/developer_documentation_inventory.rs` | Documentation architecture guard added for contributors: every `src/**/*.rs` and `tests/*.rs` source must be named in the appropriate source/test reference, and the core developer-document set must exist and be indexed. |

---

## 4. Front panel and teaching/debugger presentation

| Test file | Mission |
| --- | --- |
| `tests/front_panel_fidelity.rs` | Validates raw front-panel state/control behavior is derived from the physical machine/bus rather than convenient CPU/UI projections. |
| `tests/panel_lamp_duty.rs` | Verifies panel lamp duty/persistence inputs preserve expected T-state-weighted activity rather than just final bit values. |
| `tests/bus_teaching.rs` | Validates bus-teaching snapshots against exact CPU/S-100 observations and accuracy labels. |
| `tests/bus_teacher_layout.rs` | Structural/UI guard for important Bus Teacher layout/fields so educational signal information remains present and correctly grouped. |
| `tests/bus_teacher_lifecycle.rs` | Guards Bus Teacher behavior across run/stop/reset/lifecycle transitions and historical snapshot semantics. |
| `tests/main_toolbar_navigation.rs` | UI structural test ensuring the main toolbar/menu exposes expected developer/operator tools/navigation. |
| `tests/ui_collapsible_sections.rs` | UI structure/layout guard for collapsible sections used in complex diagnostic/configuration tools. |

---

## 5. Debugger, trace and history

| Test file | Mission |
| --- | --- |
| `tests/debugger_architecture.rs` | Guards separation between debugger observers/controllers and authoritative machine state; debugger features must use backend contracts rather than own a shadow CPU/memory. |
| `tests/debugger_execution.rs` | End-to-end debugger control tests for stepping/run-to/breakpoint/watchpoint semantics against live execution. |
| `tests/debugger_live_breakpoint.rs` | Focused regression for breakpoints during actual backend execution/resume so a stopped PC does not immediately retrigger incorrectly. |
| `tests/decoder8080_coverage.rs` | Ensures the teaching/debugger decoder covers the complete 8080 opcode space/metadata expectations needed by tools. |
| `tests/instruction_history.rs` | Validates retained instruction-history sequencing/effects used by disassembly, call-stack and debugger views. |

---

## 6. Quick loader, authentic loader and ASR-33 reader

| Test file | Mission |
| --- | --- |
| `tests/basic_quick_load_regression.rs` | Guards the convenience BASIC/direct-load workflow, including the intended physical RAM target/state preparation, independently from authentic tape loading. |
| `tests/authentic_basic_tape.rs` | End-to-end authentic BASIC/paper-tape loading regression: historical bootstrap/software must receive bytes through the emulated reader/serial hardware path rather than a hidden direct memory load. |
| `tests/authentic_reader_transport_guards.rs` | Architecture/transport guards that keep authentic reader input on the intended peripheral/serial path and reject shortcut routing that would bypass hardware. |
| `tests/asr33_reader_control_architecture.rs` | Guards separation/ownership between ASR-33 reader controls, peripheral model, app controller and serial hardware. |

---

## 7. S-100 runtime, mapping and authority

| Test file | Mission |
| --- | --- |
| `tests/s100_app_runtime_authority.rs` | Ensures app configuration is mounted into one live S-100 runtime and the UI does not maintain an alternate guest-visible card/RAM authority. |
| `tests/s100_backend_hardware.rs` | Validates backend-visible hardware inspection/configuration corresponds to the actual mounted S-100 hardware. |
| `tests/s100_cpu_board_authority.rs` | Guards the MITS 8080 CPU board as the S-100 CPU electrical authority and prevents bypass/duplicate board state. |
| `tests/s100_debugger_mapping_authority.rs` | Ensures debugger mapping/inspection uses the mounted physical S-100 mapping rather than a synthetic flat memory map. |
| `tests/s100_front_panel_operator_mapping_authority.rs` | Guards front-panel operator tooling against inventing a separate memory/card mapping; it must use live physical topology. |
| `tests/s100_front_panel_tool_authority.rs` | General front-panel developer-tool authority guard: raw hardware state must come from backend/S-100 truth. |
| `tests/s100_memory_activity_mapping_authority.rs` | Ensures memory-activity visualization interprets addresses against canonical S-100 mapping rather than owning mapping state. |
| `tests/s100_memory_viewer_mapping_authority.rs` | Ensures the RAM viewer resolves/edits the same physical mounted RAM cards used by guest execution. |
| `tests/s100_physical_serial_authority.rs` | Proves guest CPU I/O and host endpoint/debug access refer to the same installed physical serial-card instance, not duplicate UARTs. |
| `tests/s100_serial_inventory_ui.rs` | Guards serial-card inventory/configuration UI against diverging from slot-native S-100 hardware configuration. |
| `tests/s100_serial_reader_authority.rs` | Guards authentic reader/serial selection against alternate aggregate/legacy serial state; reader reaches the installed physical serial hardware. |
| `tests/s100_legacy_cleanup.rs` | Regression/source guard that retired legacy S-100/machine compatibility surfaces stay removed while required migration/compatibility contracts remain. |

---

## 8. S-100 electrical/performance investigation tests

| Test file | Mission |
| --- | --- |
| `tests/s100_edge_breakdown.rs` | Profiling/measurement harness that decomposes host cost of representative S-100 signal/edge operations. Used for performance analysis, not as a replacement for electrical fidelity tests. |
| `tests/s100_hotpath_profile.rs` | Microprofile of important S-100 runtime/backplane hot paths, such as stable settles, address/DBIN/DO edges and memory access. Historical/current measurements must be interpreted under matching builds. |
| `tests/s100_resolver_observer_breakdown.rs` | Separates resolver and card-observer costs to identify whether backplane electrical resolution or card observation dominates a measured path. |

---

## 9. Open bus and memory physical behavior

| Test file | Mission |
| --- | --- |
| `tests/open_bus_fidelity.rs` | Validates unmapped/high-impedance memory reads and overlap/open-bus semantics. Protects against convenient hidden flat-memory behavior. |

Memory wait timing is listed in the CPU timing section because it specifically validates READY/Tw placement.

---

## 10. MITS 88-SIO tests

| Test file | Mission |
| --- | --- |
| `tests/sio88_configuration_ui.rs` | Guards 88-SIO configuration UI fields/options and their mapping to typed physical card configuration. |
| `tests/sio88_endpoint_wiring.rs` | Validates endpoint/cable compatibility and wiring for 88-SIO physical interfaces; host devices must not be connected through impossible hidden conversions. |
| `tests/sio88_hardware_fidelity.rs` | Main 88-SIO hardware-fidelity regressions across revisions/status/data/ready/handshake behavior. |
| `tests/sio88_interrupt_configuration.rs` | Verifies configured 88-SIO interrupt source/target/wiring affects the physical interrupt path as intended. |
| `tests/sio88_physical_boundary.rs` | Guards the boundary between S-100 electrical card behavior, serial connector state and host inspection so no alternate UART state is introduced. |
| `tests/serial_receive_break_fidelity.rs` | Focused receive-BREAK regression for serial hardware/electrical behavior (including no fabricated ordinary byte where BREAK semantics apply). |

---

## 11. MITS 88-2SIO / MC6850 tests

| Test file | Mission |
| --- | --- |
| `tests/two_sio_break_fidelity.rs` | Validates transmit/receive BREAK behavior of the 88-2SIO/MC6850 board path and associated electrical overrides. |
| `tests/two_sio_debugger_wait_isolation.rs` | Ensures debugger/inspection access does not incorrectly consume or perturb guest-visible 88-2SIO READY/wait behavior. |
| `tests/two_sio_external_com_signals.rs` | Validates host COM modem/control signals are projected to/from the emulated 88-2SIO connector with correct semantics/polarity. |
| `tests/two_sio_idle_chassis_clock.rs` | Proves independent serial-card time continues correctly while CPU instruction execution is STOPped, RESET-held or HOLD/HLDA parked, without double-counting panel/host time. |
| `tests/two_sio_interrupt_ui.rs` | Guards configuration UI representation of 88-2SIO interrupt wiring/targets. |
| `tests/two_sio_modem_pins.rs` | Focused 88-2SIO modem/handshake pin semantics (e.g. CTS/DCD/RTS as applicable to interface/config). |
| `tests/two_sio_prdy_timing.rs` | Validates physical port-ready/PRDY timing and its interaction with READY/wait behavior at the S-100 boundary. |
| `tests/two_sio_signal_interfaces.rs` | Validates independent per-port electrical interface choices and signal mapping for supported RS-232/TTL/TTY-style connections. |
| `tests/two_sio_strap_ui.rs` | Guards address/baud/physical strap configuration exposed by the S-100 hardware UI. |

---

## 12. How to maintain this file

Whenever a new `tests/*.rs` file is added:

1. Give the file a name describing the invariant, not an implementation helper.
2. Add it to this reference with a one-paragraph purpose statement.
3. Add it to the right test category in `TESTING_GUIDE.md` if it introduces a new class of evidence.
4. If it is intentionally ignored/long-running, document why near the test and/or here.
5. Do not delete an old hardware oracle merely because a newer broader test also passes; prove that the invariant is genuinely duplicated first.

`tests/developer_documentation_inventory.rs` enforces that every integration-test source remains listed here.
