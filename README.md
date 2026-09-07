# RusTair

RusTair is a native Rust emulator of the **MITS Altair 8800** focused on hardware fidelity: an Intel 8080 CPU board, physical S-100 cards, the Display/Control front panel, serial interfaces and peripherals are modeled as parts of one machine rather than as independent shortcuts.

## Architecture

```text
Altair 8800
├── Chassis
│   └── S-100 backplane
│       ├── MITS 8080 CPU board
│       ├── RAM cards
│       ├── MITS 88-SIO / 88-2SIO
│       └── additional S-100 cards as they are implemented
└── Display/Control front panel
    └── connected to the system bus
```

Cards do not call one another directly. The CPU board, memory and I/O cards observe and drive the S-100 bus. The front panel is part of the same physical system.

There is one production execution engine: **Adaptive Cycle 8080**. It uses two internal strategies over the same authoritative CPU/chassis/card state:

- **Partial** — exact Intel 8080 T-state/PHI1/PHI2 execution through the live S-100 fabric.
- **Full** — accelerated whole-instruction execution only where the same externally visible machine-cycle schedule can be reconstructed safely.

Full and Partial are implementation strategies, not separate emulated machines or user-selectable CPU cores.

## Implemented system

- Intel 8080 cycle core with T1/T2/Tw/T3/T4/T5, READY, HOLD/HLDA, HLT, RESET and interrupts.
- MITS 8080 CPU board on the live S-100 backplane.
- Configurable Altair 8800 / 8800a / 8800b chassis connector populations.
- Slot-native MITS RAM boards with per-card address/population/timing configuration.
- MITS 88-SIO including revision, A/B/C electrical interface, baud/format and interrupt routing.
- MITS 88-2SIO including address, baud/interface straps and interrupt wiring.
- Photographic Display/Control front panel with exact switch controls, LEDs, EXAMINE/DEPOSIT, RUN/STOP, RESET, PROTECT and related bus behavior.
- ASR-33 teletype with keyboard, paper tape, reader/punch mechanics and audio.
- Text terminal plus optional TCP and host COM endpoints.
- Explicit serial cabling: endpoints can be disconnected or attached to an available emulated port; incompatible direct electrical connections are rejected rather than hidden behind an implicit level converter.
- RAM viewer, debugger, execution history, I/O inspector, T-state teacher and panel-operator tools.
- Quick/direct program loading and authentic paper-tape loading are separate workflows.
- Embedded Microsoft BASIC and classic Intel 8080 diagnostic support.
- Persistent hardware, peripheral, wiring and UI preferences.

## Build and test

Normal desktop execution should use a release build:

```powershell
cargo run --release
```

Run the complete automated test suite with:

```powershell
cargo test
```

The Windows release executable is written to `target/release/rustair.exe`.

The ASR-33 artwork is larger than 2048 pixels on a side. Some graphics backends can trip a debug-only `egui` texture assertion, so `--release` is the supported normal desktop build.

## Configuration model

Physical hardware is configured under **Configuration → S-100 Chassis / Cards** while POWER is off. Chassis, slot occupancy, RAM card straps and serial-card straps are properties of that inventory.

Host emulation speed is deliberately separate from the installed CPU board. The MITS 8080 board remains a 2 MHz historical device; 5×, 10× and Unlimited change only how quickly virtual machine time is advanced on the host.

ASR-33 and Text Terminal each have their own cable selector. BASIC auto-open is a UI preference only: it may reveal the console already connected to the relevant port, but it must never rewire the machine.

Compatibility workarounds are explicit and opt-in. Historical bugs or awkward hardware behavior are not silently corrected merely to make software run.

## Source layout

- `src/app/` — desktop application, controllers, persistence and UI.
- `src/backend/` — host-facing Adaptive Cycle execution boundary.
- `src/cpu8080_cycle/` — authoritative T-state-accurate Intel 8080 core.
- `src/cpu8080.rs` — validated instruction-level semantic executor used by internal Full windows.
- `src/machine/` — CPU-independent Altair chassis, front-panel state and runtime card façades.
- `src/s100_*` / `src/s100/` — S-100 contacts, backplane, CPU/I/O/RAM cards and live runtime fabric.
- `src/config/` — physical hardware and application configuration.
- `src/io/` — external serial transports and cable routing.
- `src/peripherals/asr33/` — ASR-33 model.
- `tests/` — architecture, fidelity, UI-structure, differential and classic diagnostic regression tests.

## Runtime assets

Active front-panel artwork lives under `assets/panels/white-pivot/`. Shared build inputs under `assets/` include ASR-33 artwork/audio, fonts, panel audio, Microsoft BASIC and embedded CPU diagnostics.

Normal release execution uses bytes embedded into the executable. User-selected binaries, paper tapes and terminal files remain ordinary external files.

## Project direction

The active roadmap is kept in [`TODO.md`](TODO.md). The priority is to finish and verify one coherent Altair implementation before adding speculative hardware: clean physical ownership, historical fidelity, usable performance and regression coverage come first.
