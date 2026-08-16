# RusTair

Native Rust implementation of the **MITS Altair 8800** simulator, built to keep the photographic front panel and machine sounds without requiring a browser.

This repository starts by porting the Intel 8080 behaviour from the `8080.js` used by the existing Altair simulator. Later the CPU module is intentionally replaceable with our own 8080 implementation so both cores can be cross-checked against each other.

## Current state

- Native Windows/Linux desktop window using `eframe/egui` — no web browser or local HTTP server.
- Intel 8080 core in Rust (`src/cpu8080.rs`).
- 8 KiB Altair memory model and the same basic I/O ports used by `sim.html`.
- Photographic front panel with clickable address switches.
- Power, Run/Stop, Single Step, Examine, Deposit and Reset.
- Address/data/WAIT LEDs.
- Binary loader and hook for the existing Microsoft 4K BASIC image.
- Runtime asset loading from `assets/`, so replacing artwork/audio does not require changing emulator code.

## Build

```powershell
cargo run --release
```

The release executable is created at `target/release/rustair.exe` on Windows.

## Assets

The artwork/audio originated in the sibling `TheWinterLynx/altair` project. Put the required files under `assets/`:

- `Altair1.png`
- `LEDon.png`
- `SwitchUp.png`
- `SwitchDown.png`
- `SwitchCentre.png`
- `fan.mp3`
- `click.mp3`
- `powerbtn.mp3`
- `4kbas32.bin`

`scripts/import-assets.ps1` copies them from a local clone of the original repo.

## Architecture

`cpu8080.rs` is deliberately isolated behind a small `Bus` trait. `machine.rs` implements the Altair-specific memory and I/O. The UI only talks to `AltairMachine`. When the home-grown 8080 is ready we can replace the CPU module without rewriting the front panel.

## Accuracy plan

The current CPU is a Rust re-expression of the instruction behaviour and flag rules from the existing `8080.js`. The next validation step is to run the same diagnostic/exerciser against both implementations and compare registers, flags, memory writes, I/O and cycle counts instruction-by-instruction.
