# Blue photographic Altair panel

`panel.jpg` is the generated clean front-panel plate used by RusTair. It contains the panel, labels, guide lines, dark LED lenses and **fixed switch mounting hardware**, but no animated lever caps. The metal bezel/mounting hole therefore remains part of the static photograph and never moves when a switch is actuated.

The clean plate source is versioned as Base64 chunks under `source/panel_clean_*.b64`. `scripts/build_blue_panel.py` reconstructs the native **1774×887** runtime image without external downloads.

The approved realistic switch artwork is preserved in `source/sprites_realistic_*.b64`. During the build, `scripts/build_blue_panel.py` derives a higher-resolution **1024×768** 4×3 runtime atlas from that artwork. The runtime switch cells contain only the moving cap and shaft; they deliberately do **not** contain the fixed metal base.

The 4×3 runtime sprite grid contains:

- bright red LED illumination;
- red and ivory bistable lever positions;
- blue three-position spring-centred lever positions (`up / centre / down`);
- black three-position spring-centred AUX lever positions (`up / centre / down`).

The centre blue/black states are generated at double the previous cell resolution and use a foreshortened cap so they do not turn into the low-resolution circular blobs of the previous build.

The lower blue controls are switches, not push buttons. STOP/RUN, SINGLE STEP, EXAMINE, DEPOSIT, RESET/CLR and PROTECT/UNPROTECT are rendered as spring-centred three-position toggle switches. AUX uses the same mechanical model.

POWER follows the captured `altair` behaviour: **UP = OFF** and **DOWN = POWER ON**. On power-up, RusTair also mirrors the reference reset-lamp sequence: all address and data LEDs illuminate for 500 ms, WAIT remains on while stopped, and MEMR/M1/WO are lit whenever the machine is powered.

Runtime coordinates are defined against the panel's native **1774×887** pixel size in `src/app3.rs`.
