# Altair 8800 3D asset package

This directory contains the source, runtime and reference files for the optional interactive 3D representation of the RusTair Altair 8800 front panel.

## Layout

- `runtime/Altair8800_1975.glb` — binary glTF consumed by the native WGPU renderer.
- `runtime/bindings.json` — stable switch/LED node binding contract consumed by RusTair.
- `source/Altair8800_1975.blend` — editable Blender master; never loaded at runtime.
- `reference/VALIDATION.md` — provenance, assumptions and validation record.
- `reference/frontpanel.mjs` — reference adapter from the original package; not used by RusTair at runtime.

## Runtime packaging

RusTair is intentionally self-contained. The runtime GLB and binding manifest are compiled into `rustair.exe` through `src/embedded_assets.rs`; the application must not require an external `assets/` directory after build.

## Presentation invariant

The existing 2D front panel remains a supported presentation. The 3D panel is an additional view over the same authoritative emulated front-panel state and operations; it must not create a second hardware authority.

## Asset accuracy

See `VALIDATION.md` for the detailed accuracy status, assumptions and remaining metrology gaps of the supplied reconstruction.
