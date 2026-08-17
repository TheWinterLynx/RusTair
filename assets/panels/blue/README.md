# Blue photographic Altair panel

`panel.jpg` is the clean front-panel plate used by RusTair. It intentionally contains the panel, labels, guide lines, dark LED lenses and empty switch mounting holes, but **no switch levers**. This prevents the animated controls from being drawn over photographed controls.

`sprites.png` is generated reproducibly by `scripts/build_blue_panel.py` and contains transparent overlays for:

- bright red LED illumination;
- red and ivory bistable sense/power switch positions;
- blue three-position spring-centred function switches (`up / centre / down`);
- black three-position spring-centred AUX switches (`up / centre / down`).

The lower blue controls on an Altair are switches, not push buttons. RusTair therefore renders STOP/RUN, SINGLE STEP, EXAMINE, DEPOSIT, RESET/CLR and PROTECT/UNPROTECT as spring-centred toggle switches.

The clean plate was prepared from the user-selected blue Altair front-panel reference specifically for the RusTair skin. Runtime coordinates are defined against its native **1774×887** pixel size in `src/app3.rs`.
