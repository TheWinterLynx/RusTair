# RusTair

Native Rust implementation of the **MITS Altair 8800** simulator, with a photographic front panel, machine audio and an ASR-33 teletype.

## Current state

- Native Windows/Linux desktop UI using `eframe/egui`.
- Intel 8080 CPU core in Rust.
- 8 KiB Altair memory model and front-panel operations.
- Address/data/status LEDs.
- Configurable two-position and spring-centred three-position front-panel switches.
- Per-switch, per-pose sprite selection, X/Y micro-adjustment and scale.
- ASR-33 teletype with keyboard, paper tape and audio.
- Bundled Microsoft 4K BASIC image.
- Embedded runtime assets: release executables are self-contained and do not require an adjacent `assets/` directory.

## Build

```powershell
cargo run --release
```

The release executable is created at `target/release/rustair.exe` on Windows.

> The ASR-33 source artwork is larger than 2048 pixels on a side. With the current `egui` version a debug build can trip a debug-only texture-size assertion on some backends, while the release build used by GitHub Actions runs correctly on the same machine. Use `--release` for normal local testing.

## Source layout

- `src/main.rs` — executable entry point.
- `src/app/` — application composition, controllers and UI.
- `src/embedded_assets.rs` — compile-time registry for bundled runtime assets.
- `src/audio.rs` — audio playback engine using embedded MP3 data.
- `src/machine/` — Altair memory, I/O bus and machine state.
- `src/cpu8080.rs` — Intel 8080 core.
- `src/peripherals/asr33/` — reusable ASR-33 data model.

## Front-panel switch configuration

Every physical switch uses the same `SwitchConfig` structure in the front-panel UI modules.

Two-position switches use `SwitchKind::TwoPosition` and spring-centred controls use `SwitchKind::ThreePosition`.

Each available pose has its own sprite reference, X/Y offset and scale. This means, for example, A15 UP can use a different sprite while other positions continue using the default artwork.

## Runtime assets

The active front-panel artwork is under `assets/panels/white-pivot/`.

Shared source assets under `assets/` include the ASR-33 artwork/audio, `teletype.ttf`, `fan.mp3`, `click.mp3`, `powerbtn.mp3`, Microsoft 4K BASIC and the embedded CPU diagnostic images.

These files remain in the repository as build inputs, but normal release execution reads them from bytes compiled into the executable. User-selected files such as external binaries, paper tapes and terminal text files continue to be loaded from disk normally.

Legacy/unused artwork is intentionally not kept in the active asset set.
