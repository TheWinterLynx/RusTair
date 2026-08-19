# White-pivot Altair front panel

This directory contains the active front-panel artwork used by RusTair:

- `panel.png` — fixed photographic panel, including the metal switch sockets.
- `switch_up.png` — default white UP pose.
- `switch_center.png` — default white CENTER pose.
- `switch_down.png` — default white DOWN pose.

The switch artwork is configured in `src/front_panel.rs`.

## Switch model

Every physical switch uses the same `SwitchConfig` structure. A two-position switch has UP and DOWN poses and `center: None`. A spring-centred switch has UP, CENTER and DOWN poses.

Every available pose can independently select:

- a sprite ID,
- an X/Y micro-offset in panel pixels,
- a scale factor.

The socket position is also stored per physical switch, so individual controls can be aligned without changing any other switch.

## Adding another sprite

For example, to add a red UP lever:

1. Add the PNG to this directory (or another asset directory).
2. Add a `SwitchSpriteId` variant in `src/front_panel.rs`.
3. Add its `SwitchSpriteAsset` metadata (path, canvas, crop, pivot and scale).
4. Reference that sprite ID from the UP pose of whichever switch should use it.

The application automatically caches every sprite referenced by the switch configuration; there is no separate hard-coded texture list.

`switch_center.png` may be supplied on a black RGB canvas. Its asset metadata currently uses `SwitchAlphaMode::RemoveBlack`, so black-background removal is performed only in the decoded runtime texture.
