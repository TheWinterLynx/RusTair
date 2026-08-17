#!/usr/bin/env python3
"""Build RusTair's clean Altair panel and realistic switch sprite sheet.

Both runtime images are reconstructed from Base64 chunks versioned under
assets/panels/blue/source/. This avoids binary-upload truncation and makes the
assets reproducible in GitHub Actions.
"""
from __future__ import annotations

import base64
import hashlib
import io
from pathlib import Path

from PIL import Image

OUT = Path("assets/panels/blue")
SOURCE = OUT / "source"
PANEL_SIZE = (1774, 887)
SPRITE_SIZE = (512, 384)  # 4 columns x 3 rows, 128 px per cell
SPRITE_SHA256 = "364f6b13c3fb5031643d7b236e2ca93fe7b4ecc14d1f231a125a95efd5158cbf"


def decode_chunks(pattern: str) -> bytes:
    parts = sorted(SOURCE.glob(pattern))
    if not parts:
        raise RuntimeError(f"No source chunks matching {pattern!r} in {SOURCE}")
    encoded = "".join(p.read_text(encoding="ascii").strip() for p in parts)
    return base64.b64decode(encoded, validate=True)


def build_clean_panel() -> Image.Image:
    raw = decode_chunks("panel_clean_*.b64")
    panel = Image.open(io.BytesIO(raw)).convert("RGB")
    panel.load()
    if panel.size != PANEL_SIZE:
        raise RuntimeError(
            f"Unexpected clean panel size {panel.size}; expected {PANEL_SIZE}"
        )
    return panel


def build_realistic_sprites() -> bytes:
    raw = decode_chunks("sprites_realistic_*.b64")
    digest = hashlib.sha256(raw).hexdigest()
    if digest != SPRITE_SHA256:
        raise RuntimeError(
            f"Realistic sprite SHA-256 mismatch: {digest}; expected {SPRITE_SHA256}"
        )

    # Force Pillow to decode every scanline. Image.open() alone only validates
    # enough of the PNG header to know its dimensions and previously allowed a
    # truncated sprites.png to pass CI.
    with Image.open(io.BytesIO(raw)) as sprites:
        sprites.load()
        if sprites.size != SPRITE_SIZE:
            raise RuntimeError(
                f"Unexpected sprite sheet size {sprites.size}; expected {SPRITE_SIZE}"
            )
        rgba = sprites.convert("RGBA")
        alpha = rgba.getchannel("A")
        if alpha.getbbox() is None:
            raise RuntimeError("Realistic sprite sheet is completely transparent")

        cell_w = sprites.width // 4
        cell_h = sprites.height // 3
        # Every control cell except the intentionally unused final cell must
        # contain visible pixels.
        for row in range(3):
            for col in range(4):
                if (col, row) == (3, 2):
                    continue
                cell = alpha.crop(
                    (col * cell_w, row * cell_h, (col + 1) * cell_w, (row + 1) * cell_h)
                )
                if cell.getbbox() is None:
                    raise RuntimeError(f"Sprite cell ({col}, {row}) is empty")

    return raw


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    panel = build_clean_panel()
    panel.save(OUT / "panel.jpg", quality=94, optimize=True, progressive=True)

    sprite_raw = build_realistic_sprites()
    (OUT / "sprites.png").write_bytes(sprite_raw)

    print(
        f"Built clean {OUT / 'panel.jpg'} and fully validated realistic "
        f"{OUT / 'sprites.png'} ({len(sprite_raw)} bytes)"
    )


if __name__ == "__main__":
    main()
