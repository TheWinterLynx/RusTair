# Third-party provenance

The initial CPU behaviour is ported/re-expressed from `site/altair/8080.js` in the original simulator repository.

The source file states:

- Copyright (C) 2013, 2014 Martin Maly.
- Based on BSD-licensed work by Copyright (C) 2008 Chris Double.
- Redistribution in source and binary form is permitted subject to retention of the copyright notice, conditions and disclaimer.

The original `sim.html` identifies the Altair simulator as Copyright Ian Davies, 2016-2025, and credits Martin Maly and Chris Double for the Intel 8080 emulator.

Artwork, audio and ROM/binary assets copied from the original project remain subject to their original provenance/licensing. They are kept separate under `assets/` so their provenance is explicit.

## Rust dependencies added by RusTair

- `serialport` 4.9.x — cross-platform host serial-port access used by the External COM endpoint. Licensed under the Mozilla Public License 2.0 (MPL-2.0). RusTair consumes the upstream crate as a dependency and does not vendor or modify its source.
