# RusTair Repository Reference

This document maps the repository outside the Rust source tree. For every Rust source file, see [SOURCE_REFERENCE.md](SOURCE_REFERENCE.md).

The purpose is to help a new contributor answer questions such as:

- Which files define the Rust package/build?
- Where are tests, assets and licenses?
- Which documentation is current architecture versus historical research?
- Which files are generated and must not be committed?
- What do the GitHub workflow files do?

---

## 1. Repository root

| Path | Purpose |
| --- | --- |
| `Cargo.toml` | Rust package manifest: project metadata, direct dependencies and test/release profiles. See [BUILD_AND_TOOLCHAIN.md](BUILD_AND_TOOLCHAIN.md). |
| `Cargo.lock` | Exact dependency-resolution lockfile. Commit changes only when the dependency graph intentionally changes. Validation normally uses `--locked`. |
| `README.md` | Public project landing page: what RusTair is, architecture summary, implemented hardware and links into contributor docs. |
| `CONTRIBUTING.md` | Contributor onboarding, fidelity rules, development workflow and where to make common changes. |
| `TODO.md` | Active engineering roadmap. This is a planning document, not a hardware specification; completed/history claims should not override current architecture/source/tests. |
| `THIRD_PARTY.md` | Third-party asset/software attributions and license references. Update it when adding externally sourced material that requires attribution. |
| `.gitignore` | Generated/local paths Git must ignore. `target/` and other build scratch must remain outside version control. |

There is deliberately no active `build.rs`: obsolete SIMH native-link scaffolding was retired. If a future native build script is introduced, it should have a current, unavoidable build responsibility rather than resurrect retired integration.

---

## 2. `src/`

All production/library Rust source. It is exhaustively documented in [SOURCE_REFERENCE.md](SOURCE_REFERENCE.md).

High-level groups:

```text
src/
├── app/                 GUI/application/controllers/persistence
├── backend/             app-facing backend + Adaptive Cycle execution
├── config/              validated desired configuration
├── cpu8080_cycle/       exact 8080 timing state machine
├── io/                  host TCP/COM/cable routing
├── machine/             Altair chassis/front panel/card integration
├── peripherals/asr33/   external ASR-33 model
└── top-level *.rs       semantic CPU, S-100 fabric, debugger helpers, etc.
```

A documentation-inventory regression test ensures every `src/**/*.rs` file is represented in `docs/SOURCE_REFERENCE.md`. Context-relative entries are allowed only when the filename is unambiguous within the scanned source inventory.

---

## 3. `tests/`

Integration/regression tests compiled as separate Cargo test targets.

The directory is intentionally large because RusTair tests physical invariants at multiple levels instead of relying only on unit tests.

Major categories include:

- 8080 semantic/cycle diagnostics;
- Full-versus-Partial equivalence;
- S-100 electrical/card topology;
- RAM decode/waits/protection;
- CPU-board/pin/front-panel timing;
- READY/HOLD/interrupt/reset behavior;
- 88-SIO/88-2SIO hardware/config/electrical interfaces;
- serial endpoint and idle-clock integration;
- persistence/migration;
- debugger/teacher/UI architecture;
- authentic loader/paper-tape flows;
- architecture authority guards;
- ignored long performance/profiling workloads.

Use [TEST_REFERENCE.md](TEST_REFERENCE.md) when you need to know **why a particular test file exists** and [TESTING_GUIDE.md](TESTING_GUIDE.md) for test strategy/commands.

`tests/developer_documentation_inventory.rs` is itself an architecture guard. It keeps the source/test file inventories and core contributor-document index from silently drifting out of date.

Do not infer that a test is obsolete merely because it is ignored; many long diagnostics are intentionally manual.

---

## 4. `docs/`

Contains both current developer documentation and historical fidelity/performance records.

### Current contributor documentation

Start from [`docs/README.md`](README.md). The core current set includes:

- `DEVELOPER_GUIDE.md` — from-zero Rust/emulation/Altair onboarding;
- `GLOSSARY.md` — Rust, 8080, S-100 and project terminology;
- `EMULATION_ARCHITECTURE.md` — current machine/execution architecture;
- `ARCHITECTURAL_INVARIANTS.md` — non-negotiable ownership/timing/fidelity rules;
- `RUNTIME_FLOWS.md` — end-to-end runtime/control/data flows;
- `SUPPORT_AND_LIMITATIONS.md` — implemented scope, deliberate non-claims and known limitations;
- `SUBSYSTEM_REVIEW_MAP.md` — subsystem → source/tests/docs review map;
- `SOURCE_REFERENCE.md` — every Rust production/test-module source under `src/`;
- `TEST_REFERENCE.md` — every Rust integration-test source under `tests/`;
- `REPOSITORY_REFERENCE.md` — this repository/infrastructure map;
- `BUILD_AND_TOOLCHAIN.md` — Cargo/Rust/dependency/build details;
- `CODING_CONVENTIONS.md` — project-specific Rust and architecture conventions;
- `TESTING_GUIDE.md` — test/evidence strategy and validation commands;
- `EXTENDING_RUSTAIR.md` — safe extension recipes;
- `DEBUGGING_AND_PERFORMANCE.md` — debugger, tracing, metrics and profiling;
- `STATE_SOURCES.md` — authoritative vs derived runtime state;
- `CPU_BOARD_ARCHITECTURE.md` — CPU-board ownership/model;
- `HARDWARE_FIDELITY_DOCUMENTATION_STANDARD.md` — standard for low-level fidelity claims.

These are living current documents. If production architecture changes, update the relevant current document in the same logical branch.

### Hardware fidelity records

Many other documents record detailed research/validation for particular cards and signals. They are valuable because they preserve source evidence and regression intent.

Some are explicitly marked **historical validation record**. Their old API/branch names are preserved intentionally; do not rewrite historical results to match current naming.

### Performance research

Documents such as `GLOBAL_PERFORMANCE_REVIEW.md`, `FULL_PANEL_MARGINAL_REVIEW.md` and `S100_BUS_DRIVE_PERFORMANCE.md` preserve the evidence behind major optimization decisions. They should be read before repeating an old experiment, but their benchmark numbers are historical rather than current promises.

---

## 5. `assets/`

Build inputs embedded or consumed by the application.

Notable categories include:

- bundled Microsoft BASIC image(s);
- classic Intel 8080 diagnostic COM files under `assets/cpu-tests/`;
- ASR-33 artwork;
- front-panel artwork under `assets/panels/`;
- fonts;
- UI/panel/peripheral audio.

These are source assets, not generated `target/` output.

When adding an asset:

1. verify redistribution/license status;
2. add attribution to `THIRD_PARTY.md` when required;
3. use `embedded_assets` or the existing asset-loading path;
4. avoid committing editor/project/cache side files.

---

## 6. `sounds/`

Additional licensed/original audio source assets used by the ASR-33/panel sound set and attribution workflow.

Do not deduplicate files by name similarity alone. Housekeeping already found similarly named WAV assets with different content hashes. Preserve distinct recordings unless equivalence/license evidence proves they are duplicates.

---

## 7. `licenses/`

License texts associated with bundled third-party assets/software.

`THIRD_PARTY.md` is the human-readable attribution index; `licenses/` contains the corresponding license material where required.

Do not remove a license merely because code no longer imports a Rust crate: it may cover a bundled ROM/audio/font/image asset.

---

## 8. `tools/`

Developer-only scripts.

### `tools/profile_full_cpu.ps1`

Windows sampling-profiler helper for optimized Full/Adaptive workloads. It launches the selected test executable, samples instruction pointers and resolves symbols to produce hotspot counts.

Use it with a symbolized release test artifact as described in [DEBUGGING_AND_PERFORMANCE.md](DEBUGGING_AND_PERFORMANCE.md).

Generated logs/executables belong under ignored `target/` or other local scratch locations and must not be committed.

---

## 9. `.github/workflows/`

Repository automation definitions. They are infrastructure, not emulator runtime code.

### `.github/workflows/build.yml`

Manual (`workflow_dispatch`) Windows validation workflow. It:

- checks out the repository;
- installs stable Rust;
- caches Cargo/target data;
- runs a canonical S-100 authority source guard against reintroducing an old `cpu_inte` mirror;
- runs selected hardware/architecture regressions;
- runs `cargo test --all-targets`.

### `.github/workflows/refactor-validate.yml`

Pull-request workflow targeting `main`. On Windows it runs:

- `cargo test --all-targets`;
- `cargo build --release`.

Local contributor validation remains documented independently in `CONTRIBUTING.md`/`TESTING_GUIDE.md`. A developer does not need to trigger workflows to understand or work on the project.

Repository policy for this project is to **not launch GitHub Actions merely as a substitute for local validation**. Actions should only be run when explicitly requested/appropriate to the project workflow.

---

## 10. Generated/local-only paths

### `target/`

Cargo build output and local profiler scratch. Never source-controlled.

May contain:

- debug/release executables;
- test binaries;
- incremental compilation state;
- symbolized profiler builds;
- temporary logs/disassembly/profiling output.

Deleting `target/` does not delete source state, though it forces a full rebuild.

### IDE/editor files

Keep personal IDE caches/settings out of version control unless the project deliberately standardizes a shared configuration.

### Local loaded software/data

User-selected binaries, paper tapes, serial captures and terminal files are external runtime inputs unless intentionally added as licensed/test assets.

---

## 11. Documentation versus code authority

When documentation and source appear to disagree, classify the document before changing anything:

1. **Current architecture docs** should describe production and should be fixed if stale.
2. **Historical validation/performance records** may intentionally describe an older branch/API. Preserve history and add a banner/cross-link rather than rewriting the result.
3. **Tests** often encode the strongest machine invariant and should be examined alongside source.
4. **Primary hardware source references** cited by fidelity documents can override an incorrect implementation assumption; in that case fix implementation + tests + current docs together.

Do not blindly "make the docs match the code" if the code may be the bug.

The preferred evidence chain for a hardware claim is documented in `SUPPORT_AND_LIMITATIONS.md` and the hardware-fidelity documentation standard.

---

## 12. What should be in a clean commit

A normal source commit should not include:

- `target/` contents;
- profiler logs;
- disassembly dumps;
- editor scratch;
- downloaded archives;
- accidental test output;
- generated patch files;
- dependency lockfile churn unrelated to the task.

Before committing:

```powershell
git status --short
git diff --stat
git diff
```

For architecture/hardware changes, include the relevant documentation/test update in the same logical series so future collaborators can reconstruct the reasoning.

---

## 13. Where to start when you do not know the repository

Use this sequence:

1. `README.md` — what the project is.
2. `CONTRIBUTING.md` — how contributors work.
3. `docs/DEVELOPER_GUIDE.md` — terminology/Rust/emulation foundation.
4. `docs/EMULATION_ARCHITECTURE.md` + `ARCHITECTURAL_INVARIANTS.md` — how the machine is actually represented.
5. `docs/SUBSYSTEM_REVIEW_MAP.md` — which files/tests/docs belong to your task.
6. `docs/SOURCE_REFERENCE.md` / `TEST_REFERENCE.md` — detailed file ownership.
7. component-specific fidelity documents — historical/hardware evidence for the exact thing you plan to change.

If you still cannot answer **which object owns the real state you are about to modify**, do not start coding yet.
