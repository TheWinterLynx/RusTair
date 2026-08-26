# Third-party provenance

The initial CPU behaviour is ported/re-expressed from `site/altair/8080.js` in the original simulator repository.

The source file states:

- Copyright (C) 2013, 2014 Martin Maly.
- Based on BSD-licensed work by Copyright (C) 2008 Chris Double.
- Redistribution in source and binary form is permitted subject to retention of the copyright notice, conditions and disclaimer.

The original `sim.html` identifies the Altair simulator as Copyright Ian Davies, 2016-2025, and credits Martin Maly and Chris Double for the Intel 8080 emulator.

Artwork, audio and ROM/binary assets copied from the original project remain subject to their original provenance/licensing. They are kept separate under `assets/` so their provenance is explicit.

## Open-SIMH embedded backend

RusTair embeds a validated Windows x64 build of Open-SIMH from upstream commit `a1f57fa3738ed31148d31126ba1a7278ff845c6d`.

Embedded runtime files are kept under `SIMH-backend/` and compiled into `rustair.exe` with `include_bytes!`. At runtime they are materialized in a versioned private directory under `%LOCALAPPDATA%\RusTair\simh\` and `simh_frontpanel.dll` is loaded dynamically. No Open-SIMH installation or import library is required to compile or run RusTair.

RusTair applies two build-tree-only compatibility patches to this pinned revision: a FrontPanel EXAMINE parser correction and an AltairZ80 timer-stop guard. No CPU, MITS 88-2SIO, or TMXR serial source is modified. Full provenance, validation results, and update procedure are documented in `SIMH-backend/BUILDINFO.md`.

Open-SIMH is distributed under its MIT-style license; the required license text is included as `SIMH-backend/LICENSE-OPEN-SIMH.txt`.

## Rust dependencies added by RusTair

- `serialport` 4.9.x — cross-platform host serial-port access used by the External COM endpoint. Licensed under the Mozilla Public License 2.0 (MPL-2.0). RusTair consumes the upstream crate as a dependency and does not vendor or modify its source.
