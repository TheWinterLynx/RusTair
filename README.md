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

## Build

```powershell
cargo run --release
```

The release executable is created at `target/release/rustair.exe` on Windows.

> The ASR-33 source artwork is larger than 2048 pixels on a side. With the current `egui` version a debug build can trip a debug-only texture-size assertion on some backends, while the release build used by GitHub Actions runs correctly on the same machine. Use `--release` for normal local testing.

## Source layout

- `src/main.rs` — executable entry point.
- `src/application.rs` — application state, texture loading and shared UI helpers.
- `src/front_panel.rs` — front-panel switch model/configuration and rendering.
- `src/application_loop.rs` — `eframe::App` update loop and main menu.
- `src/teletype_controller.rs` — ASR-33 input, serial and mechanical behaviour.
- `src/teletype_renderer.rs` — ASR-33 drawing and keyboard animation.
- `src/teletype_io.rs` — paper-tape and teletype-window I/O.
- `src/altair_machine.rs` — Altair memory, I/O bus and machine state.
- `src/cpu8080.rs` — Intel 8080 core.
- `src/teletype.rs` — reusable ASR-33 data model.
- `src/audio.rs` — audio playback engine.

## Front-panel switch configuration

Every physical switch uses the same `SwitchConfig` structure in `src/front_panel.rs`.

Two-position switches use `SwitchKind::TwoPosition` and `center: None`. Spring-centred switches use `SwitchKind::ThreePosition` and `center: Some(...)`.

Each available pose has its own sprite reference, X/Y offset and scale. This means, for example, A15 UP can use a red sprite while A15 DOWN and every other switch continue using the default white artwork.

## Runtime assets

The active front-panel artwork is under `assets/panels/white-pivot/`.

Shared runtime assets under `assets/` include the ASR-33 artwork/audio, `teletype.ttf`, `fan.mp3`, `click.mp3`, `powerbtn.mp3` and `4kbas32.bin`.

Legacy panel artwork and abandoned generated-panel experiments are intentionally not kept in the active branch.
