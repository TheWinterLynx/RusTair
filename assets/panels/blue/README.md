# Blue photographic Altair panel

`panel.jpg` is the generated clean front-panel plate used by RusTair. It intentionally contains the panel, labels, guide lines, dark LED lenses and empty switch mounting holes, but **no switch levers**. This prevents animated controls from being drawn over photographed controls.

The clean plate source is versioned as Base64 chunks under `source/panel_clean_*.b64`. `scripts/build_blue_panel.py` reconstructs the native **1774×887** runtime image without external downloads.

`sprites.png` is a **versioned realistic sprite sheet**, not a procedural/vector drawing. It uses the approved photographic/generated switch artwork so the levers retain metal texture, volume, perspective, shadows and realistic coloured caps. CI validates this file but deliberately does **not** regenerate or replace it.

The 4×3 sprite grid contains:

- bright red LED illumination;
- red and ivory bistable sense/power switch positions;
- blue three-position spring-centred function switches (`up / centre / down`);
- black three-position spring-centred AUX switches (`up / centre / down`).

The lower blue controls are switches, not push buttons. STOP/RUN, SINGLE STEP, EXAMINE, DEPOSIT, RESET/CLR and PROTECT/UNPROTECT are therefore rendered as spring-centred three-position toggle switches. AUX uses the same mechanical model.

Runtime coordinates are defined against the panel's native **1774×887** pixel size in `src/app3.rs`.
