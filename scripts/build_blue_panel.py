#!/usr/bin/env python3
"""Build RusTair's clean Altair panel and moving-lever sprite atlas.

The clean panel and the approved realistic switch artwork are reconstructed
from Base64 chunks versioned under assets/panels/blue/source/.

Only the moving lever/cap/shaft is present in the runtime atlas. The fixed metal
bezel/mounting hole lives in panel.jpg and therefore never moves.

There are two mechanically different visual families:
- bistable SENSE/POWER toggles: clearly UP or DOWN;
- spring-centred function/AUX toggles: neutral CENTER at rest, with modest
  momentary UP/DOWN travel.
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

SOURCE_SPRITE_SIZE = (512, 384)  # 4x3 grid, 128 px cells
SOURCE_SPRITE_SHA256 = "364f6b13c3fb5031643d7b236e2ca93fe7b4ecc14d1f231a125a95efd5158cbf"

RUNTIME_CELL = 256
RUNTIME_SPRITE_SIZE = (RUNTIME_CELL * 4, RUNTIME_CELL * 3)

# Geometry in the original 128x128 source-cell coordinate system.
PIVOT_X = 64
PIVOT_Y = 70

# Bistable SENSE/POWER switches. UP intentionally keeps the familiar upright
# photographic look; DOWN is much more foreshortened and close to the bezel so
# the two stable positions read as roughly +/-45 degrees rather than centre/up.
BISTABLE_POSES = {
    "up":     (34, 41, 0.91),
    "down":   (58, 31, 1.08),
}

# Spring-centred lower switches. The neutral pose deliberately matches the
# upright look of the SENSE switches. UP and DOWN move only modestly around it.
CENTERED_POSES = {
    "up":     (27, 44, 0.88),
    "center": (34, 41, 0.91),
    "down":   (43, 36, 0.99),
}


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


def upright_cap(full: Image.Image) -> Image.Image:
    """Extract only the upper photographic cap, excluding shaft and fixed base."""
    alpha = full.getchannel("A")
    mask = Image.new("L", full.size, 0)
    ImageDraw.Draw(mask).rectangle((26, 0, 102, 52), fill=255)
    mask = ImageChops.multiply(alpha, mask).filter(ImageFilter.GaussianBlur(0.15))
    out = full.copy()
    out.putalpha(mask)
    return out


def crop_visible(image: Image.Image) -> Image.Image:
    bbox = image.getchannel("A").getbbox()
    if bbox is None:
        raise RuntimeError("Cannot build lever from an empty photographic cap")
    return image.crop(bbox)


def pose_cap(
    source_cap: Image.Image,
    family: str,
    pose: str,
) -> tuple[Image.Image, tuple[int, int]]:
    """Project one photographic cap into the requested mechanical pose."""
    poses = BISTABLE_POSES if family == "bistable" else CENTERED_POSES
    center_y, target_h, width_scale = poses[pose]

    crop = crop_visible(source_cap)
    aspect = crop.width / max(1, crop.height)
    target_w = max(18, int(target_h * aspect * width_scale))

    cap = crop.resize((target_w, target_h), Image.Resampling.LANCZOS)
    cap = ImageEnhance.Sharpness(cap).enhance(1.45)

    x = PIVOT_X - target_w // 2
    y = center_y - target_h // 2
    return cap, (x, y)


def moving_shaft_to(cap_rect: tuple[int, int, int, int]) -> Image.Image:
    """Draw only the moving metal shaft; its lower endpoint is always fixed."""
    scale = 4
    image = Image.new("RGBA", (128 * scale, 128 * scale), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    x0, _y0, x1, y1 = cap_rect
    cap_bottom_y = min(PIVOT_Y - 2, y1 - 2)
    top_x = ((x0 + x1) // 2) * scale
    top_y = cap_bottom_y * scale
    bottom_x = PIVOT_X * scale
    bottom_y = PIVOT_Y * scale

    draw.line(
        (top_x + 3 * scale, top_y + 2 * scale, bottom_x + 3 * scale, bottom_y + 2 * scale),
        fill=(10, 8, 7, 95),
        width=10 * scale,
    )

    for offset in range(-5 * scale, 6 * scale):
        t = (offset + 5 * scale) / (10 * scale)
        highlight = max(0.0, math.sin(math.pi * t))
        value = 75 + 150 * (highlight ** 0.9)
        if t > 0.7:
            value *= 0.7
        color = (
            int(min(255, value + 18)),
            int(min(255, value + 12)),
            int(min(255, value)),
            255,
        )
        draw.line(
            (top_x + offset, top_y, bottom_x + offset, bottom_y),
            fill=color,
            width=1,
        )

    draw.line(
        (top_x - 2 * scale, top_y + scale, bottom_x - 2 * scale, bottom_y - scale),
        fill=(247, 240, 222, 135),
        width=scale,
    )

    return image.resize((128, 128), Image.Resampling.LANCZOS)


def lever(source_cap: Image.Image, family: str, pose: str) -> Image.Image:
    cap, (x, y) = pose_cap(source_cap, family, pose)
    rect = (x, y, x + cap.width, y + cap.height)
    shaft = moving_shaft_to(rect)

    out = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    out.alpha_composite(shaft)
    out.alpha_composite(cap, (x, y))
    return out


def build_moving_lever_atlas() -> Image.Image:
    source = source_sprite_sheet()

    led_on = source_cell(source, 0, 0)

    # Reuse the approved photographic material. Geometry, not the original
    # extreme photographed pose, determines each runtime state.
    red_cap = upright_cap(source_cell(source, 1, 0))
    white_cap = upright_cap(source_cell(source, 3, 0))
    blue_cap = upright_cap(source_cell(source, 1, 1))
    black_cap = upright_cap(source_cell(source, 0, 2))

    cells = {
        (0, 0): led_on,
        # Bistable SENSE/POWER family: up/down only.
        (1, 0): lever(red_cap, "bistable", "up"),
        (2, 0): lever(red_cap, "bistable", "down"),
        (3, 0): lever(white_cap, "bistable", "up"),
        (0, 1): lever(white_cap, "bistable", "down"),
        # Spring-centred function switches.
        (1, 1): lever(blue_cap, "centered", "up"),
        (2, 1): lever(blue_cap, "centered", "center"),
        (3, 1): lever(blue_cap, "centered", "down"),
        # Spring-centred AUX switches.
        (0, 2): lever(black_cap, "centered", "up"),
        (1, 2): lever(black_cap, "centered", "center"),
        (2, 2): lever(black_cap, "centered", "down"),
    }

    atlas = Image.new("RGBA", RUNTIME_SPRITE_SIZE, (0, 0, 0, 0))
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
        f"Built clean {OUT / 'panel.jpg'} and split-family moving-lever atlas "
        f"{OUT / 'sprites.png'} ({sprites.width}x{sprites.height}, sha256={digest})"
    )


if __name__ == "__main__":
    main()
