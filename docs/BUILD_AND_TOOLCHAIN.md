# RusTair Build, Toolchain and Dependency Guide

This guide explains how the project is built and what each direct dependency is used for. It is aimed at contributors who may be new to Rust/Cargo.

---

## 1. Rust package basics

RusTair is one Rust package/crate named `rustair`.

Important root files:

- `Cargo.toml` — package metadata, direct dependencies and build profiles.
- `Cargo.lock` — exact resolved dependency versions. It is committed so developer/CI builds resolve the same graph.
- `src/lib.rs` — library crate root and module graph.
- `src/main.rs` — desktop binary entry point.

Current package metadata:

```text
name: rustair
version: 0.1.0
Rust edition: 2024
license: BSD-2-Clause
```

---

## 2. Install Rust

Install Rust through rustup, then verify:

```powershell
rustc --version
cargo --version
```

The repository uses edition 2024 syntax/features, so use a current stable toolchain capable of compiling Rust 2024 projects.

Useful optional components:

```powershell
rustup component add rustfmt
rustup component add clippy
```

---

## 3. Build and run

Normal desktop execution:

```powershell
cargo run --release
```

Build only:

```powershell
cargo build --locked --release
```

On Windows, the executable is typically:

```text
target/release/rustair.exe
```

Release mode is the supported normal GUI mode because the application includes large image assets and several emulation workloads are intentionally computationally heavy.

---

## 4. Test builds are intentionally optimized

`Cargo.toml` sets:

```toml
[profile.test]
opt-level = 2
```

This is not a benchmark trick. Many correctness tests execute tens of millions or billions of guest timing steps through real emulator infrastructure. At optimization level 0, a correct hardware regression can look hung.

Assertions remain enabled in the test profile.

Run tests:

```powershell
cargo test --locked
```

Full validation:

```powershell
$env:RUSTFLAGS='-Dwarnings'; cargo test --locked --all-targets; if ($LASTEXITCODE -ne 0) { throw "TESTS FAILED" }; cargo build --locked --release
```

---

## 5. Release profile

Current release profile:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = true
```

Why this matters:

- **Thin LTO** lets LLVM optimize across module/codegen boundaries without the full cost of fat LTO.
- **one codegen unit** improves cross-function optimization and has historically mattered to the hot emulator path.
- **strip** removes normal debug symbols from the production artifact.

Do not compare a symbolized profiler build to the normal stripped release when reporting production MHz.

---

## 6. Direct dependencies

### `eframe` 0.32 with WGPU

Desktop application framework around egui.

Used for:

- native window/event loop;
- `eframe::App` lifecycle;
- egui integration;
- WGPU renderer.

Emulation hardware modules should not depend on eframe.

### `egui_extras` 0.32 (`image` feature)

Additional egui helpers, principally image-loading integration used by artwork-heavy UI such as the front panel and ASR-33.

### `image` 0.25 (`png`, `jpeg`)

Image decoding/processing support for bundled/UI assets. Default features are disabled to keep the dependency surface intentional.

### `rodio` 0.21 (`mp3`, `playback`)

Native audio playback for panel/ASR-33 sounds. `AudioEngine` treats missing audio hardware as non-fatal.

### `rfd` 0.15

Native file dialogs used for loading binaries/diagnostics/tapes or other user-selected files.

### `rand` 0.9

Randomization used where the emulator intentionally models undefined/randomized power-on or memory state.

Do not replace historical reset semantics with randomization unless the actual state is undefined at that lifecycle point.

### `serialport` 4.9 (default features disabled)

Host physical COM-port transport. It belongs to host I/O (`src/io/com_serial.rs`), not to the emulated 88-SIO/88-2SIO UART implementation.

---

## 7. Embedded assets

Built-in assets are compiled into the executable through `src/embedded_assets.rs` and referenced by stable paths.

They include categories such as:

- front-panel artwork;
- ASR-33 artwork;
- fonts;
- audio;
- bundled BASIC;
- classic CPU diagnostics.

User-selected external files remain ordinary filesystem inputs.

When adding an asset:

1. place it under the appropriate `assets/` subtree;
2. update embedded lookup/consumers;
3. document third-party attribution/license in `THIRD_PARTY.md` when required;
4. do not commit generated caches/editor exports unintentionally.

---

## 8. Platform boundaries

The core CPU/S-100/machine architecture is intended to remain host-platform independent.

Host-specific concerns belong near the edge:

- physical COM support — `serialport`/`src/io/com_serial.rs`;
- native file dialogs — `rfd`;
- graphics — eframe/WGPU;
- audio — rodio.

A new low-level emulator feature should not require Windows APIs merely because the main development machine runs Windows.

---

## 9. Formatting and lint discipline

Format Rust:

```powershell
cargo fmt
```

Inspect formatting changes before committing; avoid unrelated whole-file churn.

Compiler warnings are treated as errors for final validation:

```powershell
$env:RUSTFLAGS='-Dwarnings'; cargo test --locked --all-targets
```

Use `#[allow(...)]` only when the code is intentional and the reason is documented. A suppression should not be used to hide abandoned code.

Clippy can be useful during refactors:

```powershell
cargo clippy --all-targets
```

However, do not mechanically apply a Clippy suggestion if it obscures a hardware invariant or changes hot-path performance without measurement.

---

## 10. `--locked`

Use `--locked` for validation/benchmark commands. It tells Cargo to use the committed `Cargo.lock` exactly instead of resolving a different dependency graph.

Dependency upgrades should be isolated tasks, not incidental side effects of a hardware change.

---

## 11. Cleaning builds

Normal incremental builds should be preferred.

If diagnosing a build-cache issue:

```powershell
cargo clean
cargo build --locked --release
```

Be aware that a clean build can take significantly longer and is normally unnecessary for source changes.

Everything under `target/` is generated build/profiling output and should not be committed.

---

## 12. Symbolized release builds for profiling

The ordinary release strips symbols. For the sampling profiler, override package profile settings without changing `Cargo.toml`:

```powershell
cargo --config profile.release.package.rustair.debug=1 --config profile.release.package.rustair.strip=false test --locked --release --test cpu8080_adaptive_classic_diagnostics --no-run
```

This build is for attribution, not for headline throughput comparisons.

---

## 13. Common build problems

### GUI debug build trips an image/texture assertion

Use `cargo run --release`; release is the supported normal desktop mode for the large ASR-33 artwork.

### Tests appear to run for a long time

Check whether you launched a deliberately long/ignored diagnostic. Normal test profile is already optimized. Use `--nocapture` if the test emits progress.

### `-Dwarnings` fails after a cleanup

Fix the underlying unused import/dead symbol/stale assignment. Do not add a broad `allow` unless the code is intentionally retained and you can explain why.

### Profiler benchmark is much slower than normal release

Expected: symbolization and sampling add overhead. Use the profiler for hotspot distribution and a normal release build for production throughput.

---

## 14. Dependency-change policy

When changing dependencies:

- make the dependency change its own reviewable task where possible;
- explain why the new crate/feature is needed;
- keep emulation-core layers free from GUI/host dependencies;
- inspect license implications;
- run the complete all-target/release validation;
- check binary size/performance if the dependency enters a hot or ubiquitous layer;
- avoid Cargo.lock churn unrelated to the requested change.
