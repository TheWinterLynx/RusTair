# RusTair Documentation Index

This directory contains two kinds of documentation:

1. **Current developer/architecture documentation** — how the production emulator is organized today.
2. **Hardware-fidelity research and historical validation records** — detailed investigations tied to particular cards, signals, revisions or earlier implementation stages.

New contributors should start with the current developer set below before reading subsystem-specific historical records.

## New contributor path

| Order | Document | Purpose |
| ---: | --- | --- |
| 1 | [`../README.md`](../README.md) | Project purpose, implemented system, build/run overview. |
| 2 | [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Contribution rules, setup, where to edit, fidelity constraints. |
| 3 | [`DEVELOPER_GUIDE.md`](DEVELOPER_GUIDE.md) | From-zero onboarding to Rust, emulation, Intel 8080, S-100 and RusTair. |
| 4 | [`GLOSSARY.md`](GLOSSARY.md) | Definitions of Rust/8080/S-100/RusTair terminology. |
| 5 | [`EMULATION_ARCHITECTURE.md`](EMULATION_ARCHITECTURE.md) | Current runtime architecture, state ownership, Full/Partial and bus/card relationships. |
| 6 | [`ARCHITECTURAL_INVARIANTS.md`](ARCHITECTURAL_INVARIANTS.md) | Non-negotiable rules and reviewer checklist for protecting the physical-machine model. |
| 7 | [`DESIGN_RATIONALE.md`](DESIGN_RATIONALE.md) | Why the architecture deliberately uses S-100 cards, one CPU authority, Adaptive Full/Partial, physical RAM ownership and explicit host/device boundaries. |
| 8 | [`RUNTIME_FLOWS.md`](RUNTIME_FLOWS.md) | End-to-end control/data flows: startup, CPU cycles, Full windows, memory, serial, interrupts, panel and loading. |
| 9 | [`SUPPORT_AND_LIMITATIONS.md`](SUPPORT_AND_LIMITATIONS.md) | Current support scope, deliberate non-claims, known limitations and active structural debt. |
| 10 | [`SUBSYSTEM_REVIEW_MAP.md`](SUBSYSTEM_REVIEW_MAP.md) | For each subsystem, exactly which source files, tests and documents to inspect before changing it. |
| 11 | [`SOURCE_REFERENCE.md`](SOURCE_REFERENCE.md) | File-by-file mission and responsibilities for every Rust source file under `src/`. |
| 12 | [`TEST_REFERENCE.md`](TEST_REFERENCE.md) | File-by-file mission of every Rust integration-test source under `tests/`. |
| 13 | [`REPOSITORY_REFERENCE.md`](REPOSITORY_REFERENCE.md) | Root files, assets, tools, workflows, licenses and generated/local-only paths. |
| 14 | [`BUILD_AND_TOOLCHAIN.md`](BUILD_AND_TOOLCHAIN.md) | Rust/Cargo build model, dependencies, profiles, assets and platform/tooling notes. |
| 15 | [`CODING_CONVENTIONS.md`](CODING_CONVENTIONS.md) | Rust conventions, architectural rules, naming, error/assert policy, hot-path discipline and review checklist used specifically by RusTair. |
| 16 | [`TESTING_GUIDE.md`](TESTING_GUIDE.md) | Test taxonomy, commands, fidelity oracles, classic diagnostics and performance validation. |
| 17 | [`EXTENDING_RUSTAIR.md`](EXTENDING_RUSTAIR.md) | Recipes for adding CPU behavior, Full support, cards, endpoints, peripherals and tools safely. |
| 18 | [`DEBUGGING_AND_PERFORMANCE.md`](DEBUGGING_AND_PERFORMANCE.md) | Debugger/tracing architecture, profiling, metrics and optimization discipline. |

## Suggested reading by task

| I want to... | Read first |
| --- | --- |
| Understand the project from zero | `DEVELOPER_GUIDE.md` → `GLOSSARY.md` |
| Understand the runtime architecture | `EMULATION_ARCHITECTURE.md` → `ARCHITECTURAL_INVARIANTS.md` → `DESIGN_RATIONALE.md` → `RUNTIME_FLOWS.md` |
| Understand why a seemingly simpler architecture was rejected | `DESIGN_RATIONALE.md` |
| Know what is supported and what is not claimed | `SUPPORT_AND_LIMITATIONS.md` |
| Change a specific subsystem | `SUBSYSTEM_REVIEW_MAP.md` → relevant source/tests/hardware record |
| Find which production file owns a behavior | `SOURCE_REFERENCE.md` |
| Understand why a specific integration test exists | `TEST_REFERENCE.md` → actual test source |
| Understand a root/tool/asset/infrastructure file | `REPOSITORY_REFERENCE.md` |
| Build/run/debug the Rust project | `BUILD_AND_TOOLCHAIN.md` |
| Make a normal Rust/code change | `CODING_CONVENTIONS.md` → relevant source/test references |
| Fix a failing fidelity test | `TESTING_GUIDE.md` + `TEST_REFERENCE.md` + relevant hardware record |
| Add a new S-100 card/peripheral | `EXTENDING_RUSTAIR.md` + `SUBSYSTEM_REVIEW_MAP.md` |
| Work on Full/Partial performance | `DEBUGGING_AND_PERFORMANCE.md` + `EMULATION_ARCHITECTURE.md` |
| Change state ownership | `STATE_SOURCES.md` + `ARCHITECTURAL_INVARIANTS.md` + `src/backend/README.md` |

## Current architecture authorities

These pre-existing documents are also current and important:

- [`STATE_SOURCES.md`](STATE_SOURCES.md) — authoritative versus derived runtime state.
- [`CPU_BOARD_ARCHITECTURE.md`](CPU_BOARD_ARCHITECTURE.md) — physical CPU-board model and future-board constraints.
- [`../src/backend/README.md`](../src/backend/README.md) — backend ownership/execution contract.
- [`HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md`](HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md) — standard for claiming/documenting hardware fidelity.

When changing an architectural invariant, update the relevant authority document as well as the higher-level contributor guide.

## Hardware-specific documentation

Before changing a board or electrical subsystem, search this directory for its hardware-fidelity record. Current material includes detailed work on:

- MITS 88-SIO revisions/electrical interfaces/interrupt routing;
- MITS 88-2SIO / MC6850 straps, interfaces, interrupts and BREAK behavior;
- CPU-board/S-100 behavior;
- base-hardware fidelity closeout;
- front-panel and S-100 performance/fidelity investigations.

Some documents are explicitly marked **historical validation record** or **historical performance investigation**. Preserve their original measurements and claims as history; use the current architecture documents above for today's ownership/model.

## Performance/history records

Performance documents preserve the evidence behind major design choices, including experiments that were rejected. Their numbers are not a promise of current throughput and may refer to old branches/compilers/hardware load.

Before repeating a performance idea, search these records to see whether the same representation or fast path has already been measured.

## Documentation maintenance rules

When architecture changes:

- update the current developer documentation in the same branch;
- do not rewrite historical validation results to make them look current;
- add a historical banner when an old document could otherwise mislead a reader about current production;
- document state ownership and physical timing assumptions, not only function names;
- document every new `src/**/*.rs` file in `SOURCE_REFERENCE.md`;
- document every new `tests/**/*.rs` file in `TEST_REFERENCE.md`;
- keep both inventories enforced by `tests/developer_documentation_inventory.rs`;
- link new test categories/oracles from `TESTING_GUIDE.md`;
- update `GLOSSARY.md` when introducing project-specific terminology;
- update `RUNTIME_FLOWS.md` when a major end-to-end path changes;
- update `REPOSITORY_REFERENCE.md` when adding new root/tool/infrastructure areas;
- update `CODING_CONVENTIONS.md` when a new project-wide Rust or architectural convention is established;
- update `ARCHITECTURAL_INVARIANTS.md` when a non-negotiable ownership/timing rule changes;
- update `DESIGN_RATIONALE.md` when a project-wide architectural decision changes or its rationale materially changes;
- update `SUPPORT_AND_LIMITATIONS.md` when a known limitation closes or a new non-claim appears;
- update `SUBSYSTEM_REVIEW_MAP.md` when ownership or the relevant review/test bundle for a subsystem changes.

The goal is that a new developer can answer **"where is the truth for this behavior?"** before making a change.
