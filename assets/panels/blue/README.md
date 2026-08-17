# Blue photographic Altair panel

`panel.jpg` is the generated clean front-panel plate used by RusTair. It intentionally contains the panel, labels, guide lines, dark LED lenses and empty switch mounting holes, but **no switch levers**. This prevents the animated controls from being drawn over photographed controls.

The clean plate source is versioned losslessly with respect to its compressed WebP representation as Base64 chunks under `source/panel_clean_*.b64`. `scripts/build_blue_panel.py` reconstructs the 1774×887 image and writes the runtime `panel.jpg`; no browser, Photoshop or external download is needed during the build.

`sprites.png` is generated reproducibly by the same script and contains transparent overlays for:

- bright red LED illumination;
- red and ivory bistable sense/power switch positions;
- blue three-position spring-centred function switches (`up / centre / down`);
- black three-position spring-centred AUX switches (`up / centre / down`).

The lower blue controls on an Altair are switches, not push buttons. RusTair therefore renders STOP/RUN, SINGLE STEP, EXAMINE, DEPOSIT, RESET/CLR and PROTECT/UNPROTECT as spring-centred toggle switches. The switch overlays are deliberately larger than the previous skin so their proportions match the clean photographic panel better.

Runtime coordinates are defined against the panel's native **1774×887** pixel size in `src/app3.rs`.
