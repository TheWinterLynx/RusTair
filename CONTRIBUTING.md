# Contributing to RusTair

RusTair is a hardware-fidelity emulator of the MITS Altair 8800. Contributions are welcome, but changes to the emulator must preserve a central rule: **the software architecture follows the physical machine architecture**.

This guide assumes no previous knowledge of RusTair, Rust, the Intel 8080, the S-100 bus, or emulator development. Start here, then follow the links under **Developer documentation**.

## 1. What you are contributing to

The emulated machine is organized like the real computer:

```text
Altair 8800
├── Chassis
│   └── S-100 backplane
│       ├── MITS 8080 CPU board
│       ├── RAM cards
│       ├── MITS 88-SIO / 88-2SIO serial cards
│       └── future S-100 cards
└── Display/Control front panel
    └── electrically connected to the system bus
```

A card must not call another card directly. CPU, RAM and I/O cards observe and drive the S-100 bus. The front panel derives its visible state from the same physical bus model.

RusTair has one production execution engine, **Adaptive Cycle 8080**, with two internal execution strategies:

- **Partial**: exact Intel 8080 T-state and PHI1/PHI2 stepping through the live S-100 fabric. This is the physical fidelity oracle.
- **Full**: accelerated instruction or multi-instruction execution used only when the machine proves that omitted intermediate host work cannot alter observable hardware behavior.

Full and Partial are not two machines. They use the same CPU authority, RAM, cards, bus and front panel.

## 2. Required tools

You need:

- Git.
- A recent Rust toolchain compatible with Rust edition 2024.
- Cargo, installed with Rust.
- A desktop environment capable of running the `eframe`/`wgpu` GUI for interactive work.
- On Windows, normal desktop development has the broadest test coverage because host COM support is exercised there.

Check the toolchain:

```powershell
rustc --version
cargo --version
git --version
```

## 3. Clone, build and run

```powershell
git clone https://github.com/TheWinterLynx/RusTair.git
cd RusTair
cargo run --release
```

Use a release build for normal GUI execution. Some end-to-end emulator paths are intentionally expensive, and the application also uses large ASR-33 artwork.

Run the normal test suite:

```powershell
cargo test --locked
```

For a pre-merge validation of production changes:

```powershell
$env:RUSTFLAGS='-Dwarnings'; cargo test --locked --all-targets; if ($LASTEXITCODE -ne 0) { throw "TESTS FAILED" }; cargo build --locked --release; if ($LASTEXITCODE -ne 0) { throw "RELEASE BUILD FAILED" }
```

`-Dwarnings` turns every compiler warning into an error. New warnings should be fixed, not hidden with lint suppressions unless there is a documented reason.

## 4. Rust concepts you need to recognize

You do not need to be a Rust expert before reading the project. These concepts appear frequently:

- **crate**: a Rust package/library. RusTair's library root is `src/lib.rs`; the desktop executable starts at `src/main.rs`.
- **module**: a namespace usually backed by a file or directory. `mod foo;` includes module `foo`; `pub mod foo;` exports it.
- **struct**: a data type with named fields, similar to a class containing data.
- **enum**: a type that can be one of several variants. Hardware states, T-states and configuration choices often use enums.
- **trait**: a behavior contract, similar to an interface. `cpu8080::Bus` and the S-100 card contracts are important examples.
- **impl**: methods implemented for a struct, enum or trait.
- **Option<T>**: either `Some(value)` or `None`; used when a value may legitimately be absent.
- **Result<T, E>**: either success (`Ok`) or failure (`Err`); used for recoverable errors and validation.
- **borrowing (`&T`, `&mut T`)**: temporary access to a value without transferring ownership. Rust uses this to prevent accidental simultaneous mutable access.
- **`pub(crate)`**: visible inside the RusTair crate but not part of the public external API.
- **`#[cfg(test)]`**: code compiled only for tests.
- **generics**: code parameterized by a type. The semantic 8080 core uses `B: Bus` so it can execute against a bus implementation without knowing concrete hardware classes.

See `docs/DEVELOPER_GUIDE.md` for a project-specific Rust primer.

## 5. Before changing code: find the authority

The most important contributor habit is to identify **which object owns the real emulated state** before editing anything.

| Concern | Authoritative owner |
| --- | --- |
| 8080 registers, flags, PC, SP, INTE and exact execution state | `Cpu8080Cycle` |
| Physical chassis power | `AltairChassis` |
| RUN/STOP latch | `S100BusState::signals.run` |
| Raw S-100 electrical state | `S100BusState` / live S-100 resolver |
| Installed RAM and S-100 cards | `S100RuntimeFabric` and the installed card instances |
| UART/serial-card state | the installed live serial-card instance |
| LED persistence/brightness | presentation-only integrator; never an emulation authority |
| Full semantic CPU state | transient only while a Full window is active; committed back at the synchronization boundary |

If a proposed change creates a second source of truth for one of these, stop and redesign it.

## 6. Non-negotiable fidelity rules

Do not introduce any of the following:

1. **Direct card-to-card shortcuts.** A serial card must not call the CPU; RAM must not call the front panel. Interactions belong on the S-100 bus.
2. **A second CPU authority.** `Cpu8080Cycle` is the authoritative CPU state at synchronization boundaries.
3. **A hidden second RAM.** Debugger, Full execution and UI inspection must reach the same physical runtime card storage used by guest execution.
4. **Fake front-panel state.** Do not synthesize ADDRESS/DATA/STATUS lamps from PC/registers when the physical bus says something else.
5. **Instruction-boundary approximations for hardware that samples mid-instruction.** READY, HOLD, interrupt timing and bus ownership must be sampled where the real hardware samples them.
6. **Silent hardware repair.** Compatibility workarounds must be explicit and opt-in; historical behavior is not silently changed to make software convenient.
7. **Host-speed changes to guest hardware.** 5×, 10× and Unlimited change how quickly virtual time is executed, not the installed CPU board's historical clock or peripheral timing model.
8. **Unsafe Full expansion.** A new Full-supported instruction or block requires an equivalence argument and exact oracle tests against Partial.

The complete reviewer-oriented form of these rules is in `docs/ARCHITECTURAL_INVARIANTS.md`.

## 7. Where should I make a change?

| Goal | Start here |
| --- | --- |
| Change 8080 instruction semantics | `src/cpu8080.rs`, then validate against `src/cpu8080_cycle/` |
| Change exact T-state / PHI timing | `src/cpu8080_cycle/`, especially `timing.rs`, `timing/phase.rs`, `mod.rs` |
| Change Full acceleration | `src/backend/cycle/full.rs` |
| Change Adaptive dispatch / exact backend | `src/backend/cycle.rs`, `src/backend/cycle/partial_impl.rs` |
| Change app-facing machine API | `src/backend/mod.rs`, `src/backend/cycle_host.rs` |
| Change physical S-100 signals or card contract | `src/s100.rs`, `src/s100_interface.rs` |
| Change electrical bus resolution | `src/s100_backplane.rs` |
| Change mounted-card runtime / physical topology | `src/s100_runtime.rs`, `src/config/s100_hardware.rs` |
| Change MITS 8080 CPU-board electrical behavior | `src/s100_cpu.rs`, `src/machine/cpu_board.rs` |
| Change RAM board behavior | `src/s100_memory.rs`, `src/s100_runtime_ram.rs` |
| Change 88-SIO / 88-2SIO hardware | `src/machine/sio.rs`, `src/machine/two_sio.rs`, `src/s100_io_card.rs` |
| Change front-panel physical behavior | `src/machine/front_panel.rs`, `src/machine/panel_bus.rs`, `src/machine/chassis.rs` |
| Change front-panel drawing/UI | `src/app/ui/front_panel*.rs` |
| Change host TCP/COM routing | `src/io/`, `src/app/external_*.rs` |
| Change ASR-33 hardware model | `src/peripherals/asr33/` |
| Change ASR-33 UI/controller | `src/app/asr33_*.rs`, `src/app/ui/asr33*.rs` |
| Change persistence/migrations | `src/app/persistence.rs`, `src/config/` |
| Change debugger breakpoints/watchpoints | `src/debugger_control.rs`, backend debugger integration, `src/app/ui/debugger_controls.rs` |
| Change instruction decoding/explanation tools | `src/decoder8080.rs`, `src/explain8080.rs`, `src/debugger8080.rs` |
| Change tracing/history | `src/trace8080.rs`, `src/memory_activity8080.rs`, UI history files |
| Profile Adaptive Full/Partial behavior | `src/adaptive_metrics.rs`, `tools/profile_full_cpu.ps1` |

The exhaustive file-by-file map is in `docs/SOURCE_REFERENCE.md`. For a change that crosses several files, `docs/SUBSYSTEM_REVIEW_MAP.md` lists the production files, focused tests and hardware documents that should be reviewed together.

## 8. Development workflow

Use a dedicated branch. Keep a change conceptually narrow.

Recommended sequence:

1. Read the relevant architecture document and source reference.
2. Find the current tests for the invariant you are changing.
3. Add or update a focused failing test when practical.
4. Make the smallest implementation change that satisfies the physical model.
5. Run focused tests while iterating.
6. Run formatting only on touched Rust code: `cargo fmt`.
7. Run `cargo test --locked --all-targets` with warnings denied before requesting merge.
8. Run a release build.
9. For hot-path changes, benchmark before and after with the same executable profile and host conditions.
10. Explain the hardware invariant in the commit/PR, not only the software change.

Avoid broad refactors mixed with hardware changes. They make fidelity review and regression diagnosis unnecessarily difficult.

## 9. Tests are part of the architecture

RusTair intentionally has many architecture and hardware-fidelity tests. They are not merely unit tests.

A change may need several layers of evidence:

- pure CPU semantic tests;
- exact cycle/T-state tests;
- S-100 electrical tests;
- Full-versus-Partial differential tests;
- front-panel duty/state tests;
- serial timing and interrupt-routing tests;
- architecture guards that prevent duplicate state authorities;
- classic 8080 diagnostics such as CPUTEST and 8080EXM.

Long-running diagnostic/profiling tests may be intentionally `#[ignore]`. Do not unignore, delete or weaken them casually. See `docs/TESTING_GUIDE.md` and `docs/TEST_REFERENCE.md`.

## 10. Performance work

Performance is important, but hardware fidelity has priority.

Safe optimization usually means **eliminating redundant host computation while preserving the same physical result**. Examples include cached static address decoding, event-driven recomputation and compiler specialization of a proven Full path.

Unsafe optimization includes moving hardware events to convenient instruction boundaries, bypassing the S-100 fabric, inventing panel values, or suppressing overlap/high-impedance behavior.

Always compare optimized code against the exact Partial oracle when the optimization changes execution granularity. See `docs/DEBUGGING_AND_PERFORMANCE.md`.

## 11. Documentation expectations

A contribution that changes architecture, ownership, hardware behavior or a public workflow should update the relevant documentation in the same branch.

At minimum document:

- what physical component or host feature changed;
- the source of truth for the affected state;
- timing/ordering assumptions;
- known non-claims or simplifications;
- tests that prove the intended behavior;
- compatibility or migration consequences.

New Rust source or integration-test files must also be added to `docs/SOURCE_REFERENCE.md` or `docs/TEST_REFERENCE.md`. `tests/developer_documentation_inventory.rs` guards these inventories.

## 12. Developer documentation

If you are new to the project, the recommended reading order is:

1. `docs/DEVELOPER_GUIDE.md` — from-zero project, Rust and emulation onboarding.
2. `docs/GLOSSARY.md` — terminology used by the code and hardware documents.
3. `docs/EMULATION_ARCHITECTURE.md` — current machine architecture and execution model.
4. `docs/ARCHITECTURAL_INVARIANTS.md` — rules that must remain true during refactors/optimization.
5. `docs/RUNTIME_FLOWS.md` — how real operations travel through the code end to end.
6. `docs/SUPPORT_AND_LIMITATIONS.md` — what the emulator currently supports and what it does not claim.
7. `docs/SUBSYSTEM_REVIEW_MAP.md` — source files, tests and documents to inspect for the subsystem you want to change.
8. `docs/SOURCE_REFERENCE.md` — mission/characteristics of every Rust source file.
9. `docs/TEST_REFERENCE.md` — purpose of every integration-test source file.
10. `docs/REPOSITORY_REFERENCE.md` — root files, assets, tools, workflows and generated paths.
11. `docs/BUILD_AND_TOOLCHAIN.md` — Cargo, profiles, dependencies and build details.
12. `docs/CODING_CONVENTIONS.md` — project-specific Rust and review conventions.
13. `docs/TESTING_GUIDE.md` — test strategy and validation matrix.
14. `docs/EXTENDING_RUSTAIR.md` — how to add CPU behavior, S-100 cards, peripherals and UI features safely.
15. `docs/DEBUGGING_AND_PERFORMANCE.md` — debugger architecture, tracing, metrics and profiling.

The complete index is `docs/README.md`. Existing hardware-fidelity records under `docs/` contain detailed historical research and validation for specific boards/signals. Read the relevant record before changing those components.
