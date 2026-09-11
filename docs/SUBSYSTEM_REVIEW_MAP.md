# RusTair Subsystem Review Map

This document answers the practical contributor question: **"I am changing X; which source files, tests and documents must I inspect?"**

It is intentionally redundant with the file-by-file reference. `SOURCE_REFERENCE.md` is organized by file; this document is organized by **subsystem and review task**.

Use this as the first stop before a non-trivial change.

---

## 1. Intel 8080 instruction semantics

### Production code

- `src/cpu8080.rs` — instruction-level semantic executor used by admitted Full windows.
- `src/cpu8080_cycle/mod.rs` — exact state machine and instruction progression.
- `src/cpu8080_cycle/alu.rs` — exact ALU/flag semantics.
- `src/cpu8080_cycle/decode.rs` — exact-core opcode decode.
- `src/cpu8080_cycle/control_flow.rs` — exact call/jump/return behavior.
- `src/cpu8080_cycle/state.rs` — register/state helpers and Full import/export.
- `src/decoder8080.rs` — debugger/teaching decoder metadata; update only when user-facing decode metadata also changes.

### Primary tests

- internal `src/cpu8080_cycle/*_tests.rs` modules;
- `tests/cpu8080_cycle_differential.rs`;
- `tests/cpu8080_cycle_classic_diagnostics.rs`;
- `tests/cpu8080_forced_partial_classic_diagnostics.rs`;
- `tests/cpu8080_adaptive_classic_diagnostics.rs`;
- `tests/decoder8080_coverage.rs`.

### Read first

- `DEVELOPER_GUIDE.md` — 8080 primer;
- `EMULATION_ARCHITECTURE.md` — CPU ownership;
- `ARCHITECTURAL_INVARIANTS.md` — one CPU authority and Full/Partial rules;
- `STATE_SOURCES.md` — canonical state ownership.

### Review focus

Check result bytes/registers **and** flags, PC/SP, instruction timing, stack bus order and any interrupt-delay implications. If the semantic executor changes, prove it remains consistent with the exact core at synchronization boundaries.

---

## 2. T-state, PHI1/PHI2 and machine-cycle timing

### Production code

- `src/cpu8080_cycle/timing.rs`;
- `src/cpu8080_cycle/timing/phase.rs`;
- `src/cpu8080_cycle/mod.rs`;
- `src/cpu8080_cycle/pins.rs`;
- `src/machine/cpu_board.rs`;
- `src/s100_cpu.rs`;
- `src/backend/cycle/partial_impl.rs`.

### Primary tests

- internal exact-core tests;
- `tests/cpu_pin_diagram.rs`;
- `tests/memory_wait_timing.rs`;
- `tests/run_reset_timing.rs`;
- `tests/ei_interrupt_timing_fidelity.rs`;
- `tests/bus_teaching.rs`.

### Read first

- `DEVELOPER_GUIDE.md` sections on T-states/machine cycles;
- `CPU_BOARD_ARCHITECTURE.md`;
- relevant hardware-fidelity record for the signal being changed.

### Review focus

Verify which edge asserts, samples and releases each signal. Avoid moving an event to an instruction boundary merely because it simplifies software.

---

## 3. Adaptive Full execution and Full→Partial rejoin

### Production code

- `src/backend/cycle/full.rs`;
- `src/backend/cycle/full/control_flow_tests.rs`;
- `src/backend/cycle/full/ei_tests.rs`;
- `src/backend/cycle/full/panel_histogram_reference.rs`;
- `src/cpu8080_cycle/state.rs`;
- `src/full_boundary_reconcile.rs`;
- `src/machine/panel_bus.rs`;
- `src/adaptive_metrics.rs`.

### Primary tests/evidence

- `tests/adaptive_cycle_metrics.rs`;
- `tests/cpu8080_adaptive_classic_diagnostics.rs`;
- `tests/cpu8080_forced_partial_classic_diagnostics.rs`;
- `tests/ei_interrupt_timing_fidelity.rs`;
- `tests/panel_lamp_duty.rs`;
- `tests/8080exm_full_barrier_profile.rs`;
- `tests/8080exm_ei_successor_profile.rs`;
- `tests/gui_execution_performance.rs` for host scheduling around Unlimited execution.

### Read first

- `EMULATION_ARCHITECTURE.md`;
- `ARCHITECTURAL_INVARIANTS.md`;
- `DEBUGGING_AND_PERFORMANCE.md`;
- historical Full/performance review documents under `docs/`.

### Review focus

A Full optimization is acceptable only when it is measurably useful **and** exactly equivalent where hardware can observe it. Inspect T-state totals, external cycles, stale-T1/status-latch behavior, panel duty, INTE transitions and the final CPU-board/S-100 boundary.

---

## 4. S-100 connector definitions and generic electrical resolution

### Production code

- `src/s100.rs` — signal/pin vocabulary and contact roles;
- `src/s100_interface.rs` — common card-facing contract;
- `src/s100_backplane.rs` — generic resolver;
- `src/s100_chassis.rs` — physical connector topology.

### Primary tests

- `tests/open_bus_fidelity.rs`;
- `tests/s100_edge_breakdown.rs`;
- `tests/s100_resolver_observer_breakdown.rs`;
- `tests/s100_hotpath_profile.rs`;
- architecture guards under `tests/s100_*authority.rs`.

### Read first

- `EMULATION_ARCHITECTURE.md` S-100 sections;
- `ARCHITECTURAL_INVARIANTS.md` electrical rules;
- hardware-fidelity documentation relevant to the contact/signal.

### Review focus

The backplane must remain card-agnostic. Preserve High-Z, open-collector behavior, multiple drivers, contention and actual connector pin mapping.

---

## 5. Runtime S-100 topology and optimized decode

### Production code

- `src/s100_runtime.rs`;
- `src/config/s100_hardware.rs`;
- `src/s100_io.rs`;
- `src/machine/memory.rs`;
- `src/backend/cycle/partial_impl.rs`.

### Primary tests

- `tests/s100_app_runtime_authority.rs`;
- `tests/s100_backend_hardware.rs`;
- `tests/state_source_architecture.rs`;
- `tests/unified_cycle_architecture.rs`;
- `tests/s100_legacy_cleanup.rs`;
- `tests/s100_hotpath_profile.rs`.

### Read first

- `STATE_SOURCES.md`;
- `EMULATION_ARCHITECTURE.md`;
- `ARCHITECTURAL_INVARIANTS.md`.

### Review focus

Responder tables and slot indices may accelerate known physical decode, but must not create a CPU-visible dispatch abstraction that loses overlaps or bypasses electrical card behavior.

---

## 6. RAM cards, memory mapping, open bus, protection and wait states

### Production code

- `src/s100_memory.rs` — historical board configuration;
- `src/s100_runtime_ram.rs` — live byte storage/card behavior;
- `src/s100_runtime.rs` — address responder compilation and transactions;
- `src/machine/memory.rs` — machine-facing facade;
- `src/machine/panel_bus.rs` — protection/panel state where bus-visible;
- `src/config/s100_hardware.rs` and `src/config/machine.rs` — desired/migration configuration.

### Primary tests

- `tests/open_bus_fidelity.rs`;
- `tests/memory_wait_timing.rs`;
- `tests/s100_memory_viewer_mapping_authority.rs`;
- `tests/s100_debugger_mapping_authority.rs`;
- `tests/s100_memory_activity_mapping_authority.rs`;
- classic diagnostics for broad regression.

### Read first

- RAM/card hardware-fidelity documents under `docs/`;
- `STATE_SOURCES.md`;
- `SUPPORT_AND_LIMITATIONS.md` for remaining dynamic-RAM caveats.

### Review focus

Verify actual installed-card decode, overlap, physical storage ownership, protection and READY/Tw behavior. Do not fall back to a convenient flat 64 KiB host array as guest truth.

---

## 7. MITS 8080 CPU board

### Production code

- `src/s100_cpu.rs`;
- `src/machine/cpu_board.rs`;
- `src/cpu8080_cycle/pins.rs`;
- `src/cpu8080_cycle/timing.rs`;
- `src/full_boundary_reconcile.rs`.

### Primary tests

- `tests/cpu_board_clock_authority.rs`;
- `tests/cpu_board_config.rs`;
- `tests/cpu_pin_diagram.rs`;
- `tests/s100_cpu_board_authority.rs`;
- `tests/run_reset_timing.rs`;
- Full boundary/control-flow tests when the 8212/status latch or retained pins are affected.

### Read first

- `CPU_BOARD_ARCHITECTURE.md`;
- `STATE_SOURCES.md`;
- relevant CPU-board hardware-fidelity records.

### Review focus

Keep CPU package pins and board-level S-100 signals conceptually separate. The board translates/latches package behavior; it is not just a transparent alias of CPU registers.

---

## 8. Front panel / Display-Control hardware behavior

### Production code

- `src/machine/front_panel.rs`;
- `src/machine/panel_bus.rs`;
- `src/machine/chassis.rs`;
- `src/backend/cycle/partial_impl.rs`;
- `src/app/ui/front_panel.rs` and related UI files for rendering only.

### Primary tests

- `tests/front_panel_fidelity.rs`;
- `tests/panel_lamp_duty.rs`;
- `tests/s100_front_panel_tool_authority.rs`;
- `tests/s100_front_panel_operator_mapping_authority.rs`;
- `tests/bus_teaching.rs`.

### Read first

- front-panel/base-hardware fidelity documents;
- `ARCHITECTURAL_INVARIANTS.md` front-panel section;
- `RUNTIME_FLOWS.md` EXAMINE/DEPOSIT and RUN/STOP flows.

### Review focus

Separate raw electrical state from optical LED persistence. Panel operations must affect the machine through modeled controls/bus ownership rather than editing CPU state as a shortcut.

---

## 9. MITS 88-SIO

### Production code

- `src/machine/sio.rs`;
- `src/machine/sio_interface.rs`;
- `src/machine/serial_card.rs`;
- `src/machine/serial_devices.rs`;
- `src/machine/serial_bus.rs`;
- `src/s100_io_card.rs`;
- `src/s100_io.rs`;
- `src/config/sio.rs`;
- `src/config/sio_electrical.rs`.

### Primary tests

- `tests/sio88_hardware_fidelity.rs`;
- `tests/sio88_configuration_ui.rs`;
- `tests/sio88_interrupt_configuration.rs`;
- `tests/sio88_endpoint_wiring.rs`;
- `tests/sio88_physical_boundary.rs`;
- `tests/serial_receive_break_fidelity.rs`;
- `tests/s100_physical_serial_authority.rs`.

### Read first

- `88_SIO_HARDWARE_FIDELITY.md`;
- `88_SIO_ABC_ELECTRICAL_INTERFACES.md`;
- `88_SIO_INTERRUPT_ROUTING.md`;
- any revision-specific closeout records.

### Review focus

Check revision-specific status bits, COM2502/UART behavior, external device-ready latches, DATA IN/OUT handshake, interface polarity, address decode and interrupt wiring. Do not make the card call the CPU directly.

---

## 10. MITS 88-2SIO / MC6850

### Production code

- `src/mc6850.rs` — ACIA chip;
- `src/machine/two_sio.rs` — board-level dual-port implementation;
- `src/machine/serial_card.rs` / `serial_devices.rs` / `serial_bus.rs`;
- `src/s100_io_card.rs`;
- `src/s100_io.rs`;
- `src/config/two_sio.rs`;
- `src/config/sio_electrical.rs`.

### Primary tests

- all `tests/two_sio_*.rs` targets;
- `tests/s100_physical_serial_authority.rs`;
- `tests/serial_receive_break_fidelity.rs` where shared serial semantics are affected.

### Read first

- `88_2SIO_MC6850_HARDWARE_FIDELITY.md`;
- `88_2SIO_PHYSICAL_STRAPS.md`;
- `88_2SIO_SIGNAL_INTERFACES.md`;
- `88_2SIO_INTERRUPT_ROUTING.md`;
- `88_2SIO_BREAK_FIDELITY.md`;
- `88_2SIO_EXTERNAL_COM_SIGNALS.md`.

### Review focus

Separate MC6850 chip semantics from board straps/clocking/electrical interfaces. Each physical port can have independent configuration; interrupt and ready behavior must route through the board/S-100 path.

---

## 11. Interrupts, INTE, PINT and future vectored interrupts

### Production code

- `src/cpu8080_cycle/mod.rs` and interrupt-control logic;
- `src/cpu8080_cycle/state.rs`;
- `src/s100.rs` signal definitions;
- `src/s100_runtime.rs` / `src/s100_io.rs` driver masks;
- `src/s100_cpu.rs` / `src/machine/cpu_board.rs`;
- `src/machine/sio.rs` and `two_sio.rs` interrupt sources;
- Full EI/DI logic in `src/backend/cycle/full.rs`.

### Primary tests

- internal `interrupt_control_tests.rs`;
- `tests/ei_interrupt_timing_fidelity.rs`;
- `tests/sio88_interrupt_configuration.rs`;
- `tests/two_sio_interrupt_ui.rs` plus relevant board tests.

### Read first

- serial-card interrupt routing documents;
- `SUPPORT_AND_LIMITATIONS.md` for current 88-VI status;
- Full EI records if changing delayed enable.

### Review focus

Do not turn an S-100 interrupt line directly into a fabricated RST opcode in a peripheral. Interrupt acknowledge/vector behavior belongs in the appropriate CPU-board/bus/interrupt-controller model.

---

## 12. ASR-33 and authentic paper-tape loading

### Production code

- `src/peripherals/asr33/*` — peripheral model;
- `src/app/asr33_controller.rs` and `asr33_state.rs` — app coordination;
- `src/app/ui/asr33.rs`, `asr33_window.rs` — UI;
- `src/app/authentic_loader.rs` — authentic loading workflow;
- serial router/card path used by the configured connection.

### Primary tests

- `tests/authentic_basic_tape.rs`;
- `tests/authentic_reader_transport_guards.rs`;
- `tests/asr33_reader_control_architecture.rs`;
- serial-card hardware tests when reader timing reaches them.

### Read first

- `AUTHENTIC_BASIC_BOOTSTRAP.md`;
- `AUTHENTIC_BASIC_VALIDATION.md`;
- `RUNTIME_FLOWS.md` loading sections.

### Review focus

Authentic loading must transport bytes through the emulated reader/serial hardware. Mechanical/presentation timing and CPU/serial timing are distinct layers and should remain explicit.

---

## 13. Text terminal, TCP and host COM endpoints

### Production code

- `src/io/serial_router.rs`;
- `src/io/tcp_serial.rs`;
- `src/io/com_serial.rs`;
- `src/app/external_serial.rs`;
- `src/app/external_com.rs`;
- `src/app/terminal_controller.rs`;
- `src/app/terminal_serial.rs`;
- `src/app/terminal_state.rs`;
- `src/app/serial_hardware.rs`.

### Primary tests

- `tests/sio88_endpoint_wiring.rs`;
- `tests/two_sio_external_com_signals.rs`;
- `tests/two_sio_modem_pins.rs`;
- `tests/s100_physical_serial_authority.rs`;
- `tests/s100_serial_inventory_ui.rs`.

### Read first

- electrical-interface documents for the selected card;
- `SUPPORT_AND_LIMITATIONS.md` for current cable-routing constraints.

### Review focus

Host transports move endpoint data/signals. They must not emulate or override UART register state. Wiring must identify a physical installed card/port unambiguously.

---

## 14. Configuration, migrations and persistence

### Production code

- `src/config/*`;
- `src/app/persistence.rs`;
- `src/app/serial_hardware.rs`;
- `src/app/ui/s100_hardware.rs`;
- `src/s100_runtime.rs` when configuration becomes live hardware.

### Primary tests

- `tests/s100_app_runtime_authority.rs`;
- `tests/s100_backend_hardware.rs`;
- `tests/s100_serial_inventory_ui.rs`;
- `tests/s100_legacy_cleanup.rs`;
- backend lifecycle/authority tests;
- configuration UI tests for the affected board.

### Read first

- `STATE_SOURCES.md`;
- `RUNTIME_FLOWS.md` configuration/persistence flow;
- `SUPPORT_AND_LIMITATIONS.md` for remaining migration cleanup.

### Review focus

Legacy keys are migration inputs, not a second runtime model. Save current physical configuration atomically and reject invalid/ambiguous hardware explicitly.

---

## 15. Debugger, trace, call stack, loop inspector and memory tools

### Production code

- `src/debugger_control.rs`;
- `src/debugger8080.rs`;
- `src/decoder8080.rs`;
- `src/explain8080.rs`;
- `src/trace8080.rs`;
- `src/callstack8080.rs`;
- `src/memory_activity8080.rs`;
- relevant files under `src/app/ui/`;
- backend debugger/control APIs in `src/backend/`.

### Primary tests

- `tests/debugger_architecture.rs`;
- `tests/debugger_execution.rs`;
- `tests/debugger_live_breakpoint.rs`;
- `tests/instruction_history.rs`;
- `tests/decoder8080_coverage.rs`;
- S-100 debugger/memory mapping authority tests.

### Read first

- `DEBUGGING_AND_PERFORMANCE.md`;
- `ARCHITECTURAL_INVARIANTS.md` observer rules;
- `STATE_SOURCES.md` derived-state section.

### Review focus

Debugger history/inference may be incomplete and must remain derived. Watchpoints/breakpoints may control execution, but they must not alter the architectural result of an instruction or create a shadow memory/CPU.

---

## 16. Bus Teacher and didactic tools

### Production code

- `src/backend/bus_teaching.rs`;
- `src/app/ui/bus_teacher.rs`;
- `src/app/ui/cpu_pin_diagram.rs`;
- exact CPU/S-100 snapshot producers.

### Primary tests

- `tests/bus_teaching.rs`;
- `tests/bus_teacher_layout.rs`;
- `tests/bus_teacher_lifecycle.rs`;
- `tests/cpu_pin_diagram.rs`.

### Read first

- `STATE_SOURCES.md` teaching/debugger rules;
- `DEVELOPER_GUIDE.md` for terminology.

### Review focus

Clearly distinguish exact current electrical samples, control-state observations and historical/frozen snapshots. A didactic simplification must never feed back into execution.

---

## 17. GUI execution scheduling and emulation speed

### Production code

- `src/app/execution_clock.rs`;
- `src/app/execution_frame.rs`;
- `src/app/runtime.rs`;
- speed configuration in `src/config/machine.rs`.

### Primary tests

- `tests/emulation_speed_ui.rs`;
- `tests/emulation_speed_benchmark.rs`;
- `tests/gui_execution_performance.rs`;
- classic diagnostics for final machine-time invariants when scheduling code changes.

### Read first

- `RUNTIME_FLOWS.md` GUI-frame flow;
- `ARCHITECTURAL_INVARIANTS.md` host-time rule;
- `DEBUGGING_AND_PERFORMANCE.md`.

### Review focus

Host responsiveness/yielding can change, guest hardware timing cannot. Unlimited should execute as fast as practical without becoming repaint-bound.

---

## 18. Assets, audio and visual presentation

### Production code/resources

- `src/embedded_assets.rs`;
- `src/audio.rs`;
- `assets/` and `sounds/`;
- UI asset loaders under `src/app/ui/`.

### Read first

- `REPOSITORY_REFERENCE.md`;
- `BUILD_AND_TOOLCHAIN.md`;
- `THIRD_PARTY.md` and `licenses/` for attribution.

### Review focus

Keep presentation assets outside hardware authority. Check licensing/provenance for third-party assets and avoid committing generated editor artifacts.

---

## 19. Documentation-only changes

### Files to update as applicable

- `README.md` — user/contributor entry point;
- `CONTRIBUTING.md` — contribution workflow;
- `docs/README.md` — documentation index;
- `SOURCE_REFERENCE.md` — source ownership;
- `TEST_REFERENCE.md` — integration-test ownership;
- `RUNTIME_FLOWS.md` — end-to-end behavior;
- subsystem-specific hardware fidelity records;
- `GLOSSARY.md` — new terminology.

### Guard

`tests/developer_documentation_inventory.rs` verifies that Rust source/test inventories and the core contributor-document set remain represented.

---

## 20. Minimum review bundle by change type

| Change type | Minimum review bundle |
| --- | --- |
| Pure UI layout | affected UI source + UI structural test + release visual smoke test |
| CPU semantic change | semantic core + exact core + focused unit tests + differential + classic diagnostics |
| Exact timing change | exact CPU + CPU-board/S-100 path + exact timing tests + broad diagnostic regression |
| Full optimization | Full code + Partial oracle + Full/Partial differential + panel/boundary tests + before/after benchmark |
| RAM/card decode change | card config/runtime + backplane/runtime + overlap/open-bus/wait tests + mapping authority tests |
| Serial hardware change | card/chip/interface/config + S-100 adapter + board-specific hardware tests + endpoint tests |
| Persistence/config change | config types + persistence + UI + migration/authority tests + round-trip check |
| Debugger/tooling change | observer/control code + authority tests + no-hot-path-cost check when closed |
| Documentation change | affected docs + documentation inventory test + link/name review |

A broad `cargo test` passing is necessary, but for hardware/core work it is not a substitute for reading the focused oracle that defines the invariant being changed.
