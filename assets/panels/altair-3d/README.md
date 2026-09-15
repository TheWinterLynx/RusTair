# Altair 8800 3D front-panel assets

This directory is the asset boundary for the optional interactive 3D presentation of the original Altair 8800 front panel.

## Architectural rule

The existing 2D front panel remains a first-class supported presentation and must not be deleted or replaced. The 2D and 3D views are two presentations of the same authoritative emulated front-panel state.

Neither the GLB scene nor its renderer owns hardware state. LED values come from the existing `front_panel_state()` / panel-activity integration, and switch input is forwarded to the existing front-panel machine API. No CPU, S-100, RAM, serial, DCDD or terminal state may be duplicated in the 3D subsystem.

## Upload layout

Place the supplied files as follows:

```text
assets/panels/altair-3d/
├── runtime/
│   ├── Altair8800_1975.glb
│   └── bindings.json
├── source/
│   └── Altair8800_1975.blend
└── reference/
    ├── ASSET_README.md
    ├── VALIDATION.md
    └── frontpanel.mjs
```

### Required for the first runtime checkpoint

- `runtime/Altair8800_1975.glb`
- `runtime/bindings.json`

### Keep in the repository for editable-source/provenance work

- `source/Altair8800_1975.blend`
- `reference/ASSET_README.md` — copy of the asset package `README.md` under a non-conflicting name.
- `reference/VALIDATION.md`
- `reference/frontpanel.mjs` — reference adapter only; RusTair will implement the binding natively in Rust.

The other package screenshots, Blender rebuild scripts and validation JSON files are not required for the initial emulator integration. They can be added later if we decide to make the 3D model rebuild/validation pipeline part of RusTair itself.

## First implementation checkpoints

1. Preserve the current 2D front panel unchanged and make it the default.
2. Add an explicit 2D / Interactive 3D presentation selector.
3. Load and render the GLB through the existing `eframe` / WGPU device without a second window or game engine.
4. Bind all 36 3D LEDs to the existing time-integrated lamp intensities.
5. Bind A15..A00 and POWER using 3D picking while retaining the existing machine state as authority.
6. Bind the momentary controls to the existing press/release APIs (RUN/STOP, SINGLE STEP, EXAMINE, DEPOSIT, RESET/CLR, PROTECT, AUX1/AUX2).
7. Keep 2D and 3D behavior regression-testable against the same front-panel state.

## Asset naming contract

The supplied manifest is authoritative for scene-node names, switch axes/pivots/states and LED material bindings. RusTair should parse `bindings.json`; production code should not duplicate all component geometry positions as hand-maintained constants.

The 3D view is presentation/input only. A renderer or asset failure must never mutate emulated hardware state and must not make the classic 2D panel unavailable.
