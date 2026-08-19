# White-pivot Altair front panel

This branch intentionally uses only the four image assets supplied for this iteration. They must not be regenerated, recoloured, resized or otherwise rewritten on disk; all crop, resize, alpha handling and pivot placement is performed at runtime by Rust.

Expected files:

- `panel.png` — supplied `front_clean(1).png`, 1935x813 RGBA, SHA-256 `9c103b9199d9cfea6a542bb7381e64a960c51310dbd0adb3ed2498321c966eff`
- `switch_up.png` — supplied `1f53c7ac-a45b-497d-94e3-3d40a00910f4(1).png`, 1254x1254 RGBA, SHA-256 `1fa727f5dc60cd17799b29415b7ef3a8aad6496903ff4d00e4d4849442bdd063`
- `switch_center.png` — supplied `d8d2faf1-206b-4f46-8eb9-3ba16eef7fbd.png`, 1254x1254 RGB, SHA-256 `4ef416230c47443c660b2c67c73cb8d770c9598ba78853614dcb04a4a545c566`
- `switch_down.png` — supplied `d9111009-5b93-4fed-bc53-964e29ff4833(1).png`, 1254x1254 RGBA, SHA-256 `154a6a9b886cb03d57f78365fe5b13c8f0a4af7b7bedb86e04c3eeddf521a164`

Switch rules implemented in code:

- SENSE A15-A0: white UP/DOWN only.
- POWER: white UP/DOWN only (`UP = OFF`, `DOWN = ON`).
- STOP/RUN, SINGLE STEP, EXAMINE, DEPOSIT, RESET, PROTECT, AUX1 and AUX2: white UP/CENTER/DOWN, spring-centred.
- Fixed metal nuts/sockets are part of `panel.png` only and never move.
- Every switch pose is anchored by its own physical source pivot to the fixed socket centre.
- All poses share one source-pixel scale (`59/1254` panel pixels per source pixel), approximately half the previous linear size, so changing pose cannot resize the switch.
- `switch_center.png` has its black canvas removed only in the decoded runtime texture; its file bytes remain untouched.
- LEDs are dark in `panel.png`; the runtime lit state has no halo or cast-shadow layer, and no LED overlay is drawn while POWER is OFF.
