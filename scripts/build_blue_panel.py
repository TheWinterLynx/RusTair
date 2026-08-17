#!/usr/bin/env python3
"""Build RusTair's clean Altair panel and validate the realistic sprite sheet.

The clean panel is reconstructed from Base64 chunks stored under
assets/panels/blue/source/. The animated control sprite sheet is versioned
separately because it contains the approved realistic switch artwork.
CI must never replace it with procedural/vector stand-ins.
"""
from __future__ import annotations

import base64
import io
from pathlib import Path

from PIL import Image

OUT = Path("assets/panels/blue")
SOURCE = OUT / "source"
PANEL_SIZE = (1774, 887)
SPRITE_SIZE = (512, 384)  # 4 columns x 3 rows, 128 px per cell


def build_clean_panel() -> Image.Image:
    parts = sorted(SOURCE.glob("panel_clean_*.b64"))
    if not parts:
        raise RuntimeError(f"No clean panel source chunks found in {SOURCE}")

    encoded = "".join(p.read_text(encoding="ascii").strip() for p in parts)
    raw = base64.b64decode(encoded, validate=True)
    panel = Image.open(io.BytesIO(raw)).convert("RGB")
    if panel.size != PANEL_SIZE:
        raise RuntimeError(
            f"Unexpected clean panel size {panel.size}; expected {PANEL_SIZE}"
        )
    return panel


def validate_sprites() -> None:
    path = OUT / "sprites.png"
    if not path.exists():
        raise RuntimeError(f"Missing versioned realistic sprite sheet: {path}")

    with Image.open(path) as sprites:
        if sprites.size != SPRITE_SIZE:
            raise RuntimeError(
                f"Unexpected sprite sheet size {sprites.size}; expected {SPRITE_SIZE}"
            )
        if sprites.width % 4 or sprites.height % 3:
            raise RuntimeError("Sprite sheet must be an exact 4x3 grid")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    panel = build_clean_panel()
    panel.save(OUT / "panel.jpg", quality=94, optimize=True, progressive=True)
    validate_sprites()
    print(f"Built clean {OUT / 'panel.jpg'} and validated realistic {OUT / 'sprites.png'}")


if __name__ == "__main__":
    main()
