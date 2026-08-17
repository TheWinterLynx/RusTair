#!/usr/bin/env python3
"""Build RusTair's clean Altair panel and moving-lever sprite atlas.

The clean panel and the approved realistic switch artwork are reconstructed
from Base64 chunks versioned under assets/panels/blue/source/.

The runtime switch atlas deliberately contains only the moving lever/cap/shaft.
The fixed metal bezel/mounting hole is already present in panel.jpg, so it
never translates or rotates when a switch is actuated.
"""
from __future__ import annotations

import base64
import hashlib
import io
import math
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageEnhance, ImageFilter

OUT = Path("assets/panels/blue")
SOURCE = OUT / "source"
PANEL_SIZE = (1774, 887)

SOURCE_SPRITE_SIZE = (512, 384)   # 4x3 grid, 128 px cells
SOURCE_SPRITE_SHA256 = "364f6b13c3fb5031643d7b236e2ca93fe7b4ecc14d1f231a125a95efd5158cbf"

RUNTIME_CELL = 256
RUNTIME_SPRITE_SIZE = (RUNTIME_CELL * 4, RUNTIME_CELL * 3)


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


def source_sprite_sheet() -> Image.Image:
    raw = decode_chunks("sprites_realistic_*.b64")
    digest = hashlib.sha256(raw).hexdigest()
    if digest != SOURCE_SPRITE_SHA256:
        raise RuntimeError(
            f"Realistic sprite source SHA-256 mismatch: {digest}; "
            f"expected {SOURCE_SPRITE_SHA256}"
        )
    with Image.open(io.BytesIO(raw)) as image:
        image.load()
        rgba = image.convert("RGBA")
    if rgba.size != SOURCE_SPRITE_SIZE:
        raise RuntimeError(
            f"Unexpected source sprite size {rgba.size}; expected {SOURCE_SPRITE_SIZE}"
        )
    return rgba


def source_cell(sheet: Image.Image, col: int, row: int) -> Image.Image:
    w = sheet.width // 4
    h = sheet.height // 3
    return sheet.crop((col * w, row * h, (col + 1) * w, (row + 1) * h))


def cap_only(full: Image.Image, direction: str) -> Image.Image:
    """Keep only the photographic coloured/ivory cap, never the fixed bezel."""
    alpha = full.getchannel("A")
    mask = Image.new("L", full.size, 0)
    draw = ImageDraw.Draw(mask)

    if direction == "up":
        draw.rectangle((28, 0, 100, 48), fill=255)
    else:
        draw.rectangle((24, 82, 104, 127), fill=255)

    mask = ImageChops.multiply(alpha, mask).filter(ImageFilter.GaussianBlur(0.15))
    out = full.copy()
    out.putalpha(mask)
    return out


def moving_shaft(direction: str, center: bool = False) -> Image.Image:
    """Small deterministic metallic shaft; the panel itself supplies the bezel."""
    scale = 4
    size = 128 * scale
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    cx = 64 * scale

    if center:
        y0, y1, half = 54 * scale, 69 * scale, 5 * scale
    elif direction == "up":
        y0, y1, half = 46 * scale, 69 * scale, 5 * scale
    else:
        y0, y1, half = 59 * scale, 84 * scale, 5 * scale

    draw.rounded_rectangle(
        (cx - half - scale, y0, cx + half + scale, y1),
        radius=3 * scale,
        fill=(12, 10, 8, 80),
    )

    for x in range(cx - half, cx + half + 1):
        t = (x - (cx - half)) / (2 * half)
        value = 70 + 150 * (math.sin(math.pi * t) ** 0.9)
        if t > 0.68:
            value *= 0.68
        color = (
            int(min(255, value + 18)),
            int(min(255, value + 12)),
            int(min(255, value)),
            255,
        )
        draw.line((x, y0, x, y1), fill=color, width=1)

    draw.line(
        (cx - half, y0 + 2 * scale, cx - half, y1 - 2 * scale),
        fill=(45, 38, 30, 220),
        width=scale,
    )
    draw.line(
        (cx + half, y0 + 2 * scale, cx + half, y1 - 2 * scale),
        fill=(38, 32, 28, 220),
        width=scale,
    )
    draw.line(
        (cx - scale, y0 + 2 * scale, cx - scale, y1 - 2 * scale),
        fill=(245, 238, 218, 145),
        width=scale,
    )

    return image.resize((128, 128), Image.Resampling.LANCZOS)


def lever(cap: Image.Image, direction: str, shaft_up: Image.Image, shaft_down: Image.Image) -> Image.Image:
    out = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    out.alpha_composite(shaft_up if direction == "up" else shaft_down)
    out.alpha_composite(cap)
    return out


def centered_lever(cap_up: Image.Image, shaft_center: Image.Image) -> Image.Image:
    """Foreshorten the cap toward the viewer without scaling the fixed bezel."""
    bbox = cap_up.getchannel("A").getbbox()
    if bbox is None:
        raise RuntimeError("Cannot build centre lever from empty cap")
    crop = cap_up.crop(bbox)

    target_w = max(1, int(crop.width * 0.94))
    target_h = max(22, int(crop.height * 0.58))
    cap = crop.resize((target_w, target_h), Image.Resampling.LANCZOS)
    cap = ImageEnhance.Sharpness(cap).enhance(1.6)

    out = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    out.alpha_composite(shaft_center)
    out.alpha_composite(cap, ((128 - target_w) // 2, 58))
    return out


def build_moving_lever_atlas() -> Image.Image:
    source = source_sprite_sheet()

    led_on = source_cell(source, 0, 0)

    red_up_full = source_cell(source, 1, 0)
    red_down_full = source_cell(source, 2, 0)
    white_up_full = source_cell(source, 3, 0)
    white_down_full = source_cell(source, 0, 1)
    blue_up_full = source_cell(source, 1, 1)
    blue_down_full = source_cell(source, 3, 1)
    black_up_full = source_cell(source, 0, 2)
    black_down_full = source_cell(source, 2, 2)

    shaft_up = moving_shaft("up")
    shaft_down = moving_shaft("down")
    shaft_center = moving_shaft("up", center=True)

    red_up = lever(cap_only(red_up_full, "up"), "up", shaft_up, shaft_down)
    red_down = lever(cap_only(red_down_full, "down"), "down", shaft_up, shaft_down)
    white_up = lever(cap_only(white_up_full, "up"), "up", shaft_up, shaft_down)
    white_down = lever(cap_only(white_down_full, "down"), "down", shaft_up, shaft_down)
    blue_up_cap = cap_only(blue_up_full, "up")
    blue_down = lever(cap_only(blue_down_full, "down"), "down", shaft_up, shaft_down)
    blue_up = lever(blue_up_cap, "up", shaft_up, shaft_down)
    blue_center = centered_lever(blue_up_cap, shaft_center)
    black_up_cap = cap_only(black_up_full, "up")
    black_down = lever(cap_only(black_down_full, "down"), "down", shaft_up, shaft_down)
    black_up = lever(black_up_cap, "up", shaft_up, shaft_down)
    black_center = centered_lever(black_up_cap, shaft_center)

    atlas = Image.new("RGBA", RUNTIME_SPRITE_SIZE, (0, 0, 0, 0))
    cells = {
        (0, 0): led_on,
        (1, 0): red_up,
        (2, 0): red_down,
        (3, 0): white_up,
        (0, 1): white_down,
        (1, 1): blue_up,
        (2, 1): blue_center,
        (3, 1): blue_down,
        (0, 2): black_up,
        (1, 2): black_center,
        (2, 2): black_down,
    }

    for (col, row), image in cells.items():
        hi = image.resize((RUNTIME_CELL, RUNTIME_CELL), Image.Resampling.LANCZOS)
        atlas.alpha_composite(hi, (col * RUNTIME_CELL, row * RUNTIME_CELL))

    return atlas


def validate_runtime_sprites(sprites: Image.Image) -> None:
    sprites.load()
    if sprites.size != RUNTIME_SPRITE_SIZE:
        raise RuntimeError(
            f"Unexpected runtime sprite size {sprites.size}; expected {RUNTIME_SPRITE_SIZE}"
        )

    alpha = sprites.getchannel("A")
    if alpha.getbbox() is None:
        raise RuntimeError("Runtime sprite atlas is completely transparent")

    cell_w = sprites.width // 4
    cell_h = sprites.height // 3
    for row in range(3):
        for col in range(4):
            if (col, row) == (3, 2):
                continue
            cell = alpha.crop(
                (col * cell_w, row * cell_h, (col + 1) * cell_w, (row + 1) * cell_h)
            )
            if cell.getbbox() is None:
                raise RuntimeError(f"Runtime sprite cell ({col}, {row}) is empty")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    panel = build_clean_panel()
    panel.save(OUT / "panel.jpg", quality=94, optimize=True, progressive=True)

    sprites = build_moving_lever_atlas()
    validate_runtime_sprites(sprites)
    sprites.save(OUT / "sprites.png", optimize=True)

    digest = hashlib.sha256((OUT / "sprites.png").read_bytes()).hexdigest()
    print(
        f"Built clean {OUT / 'panel.jpg'} and moving-lever atlas "
        f"{OUT / 'sprites.png'} ({sprites.width}x{sprites.height}, sha256={digest})"
    )


if __name__ == "__main__":
    main()
