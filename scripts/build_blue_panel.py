#!/usr/bin/env python3
"""Build RusTair's clean Altair panel and moving-lever sprite atlas.

The clean panel and the approved realistic switch artwork are reconstructed
from Base64 chunks versioned under assets/panels/blue/source/.

The runtime switch atlas deliberately contains only the moving lever/cap/shaft.
The fixed metal bezel/mounting hole is already present in panel.jpg, so it
never translates or rotates when a switch is actuated.

The source photographs contain rather extreme up/down poses. They are used here
only for their photographic material/colour. Runtime lever geometry is rebuilt
around one fixed pivot with deliberately short travel so a toggle appears to
rock through a modest angle rather than fold through almost 90 degrees.
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

# Geometry is expressed in the original 128x128 source-cell coordinate system.
# The fixed photographed bezel is centred around this pivot in panel.jpg.
PIVOT_X = 64
PIVOT_Y = 70

# The old atlas effectively moved the cap by roughly 30-40 source pixels between
# extreme states. These positions keep the visible cap centres only 7 px apart.
# At runtime that produces a much more plausible small toggle movement.
POSE_CAP_CENTER_Y = {
    "up": 51,
    "center": 58,
    "down": 65,
}
POSE_CAP_HEIGHT = {
    "up": 34,
    "center": 31,
    "down": 34,
}
POSE_CAP_WIDTH_SCALE = {
    "up": 0.90,
    "center": 0.94,
    "down": 0.90,
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


def cap_only(full: Image.Image, direction: str) -> Image.Image:
    """Keep only the photographic coloured/ivory cap, never the fixed bezel."""
    alpha = full.getchannel("A")
    mask = Image.new("L", full.size, 0)
    draw = ImageDraw.Draw(mask)

    if direction == "up":
        draw.rectangle((28, 0, 100, 50), fill=255)
    else:
        draw.rectangle((24, 80, 104, 127), fill=255)

    mask = ImageChops.multiply(alpha, mask).filter(ImageFilter.GaussianBlur(0.15))
    out = full.copy()
    out.putalpha(mask)
    return out


def crop_visible(image: Image.Image) -> Image.Image:
    bbox = image.getchannel("A").getbbox()
    if bbox is None:
        raise RuntimeError("Cannot build lever from an empty photographic cap")
    return image.crop(bbox)


def pose_cap(source_cap: Image.Image, pose: str) -> tuple[Image.Image, tuple[int, int]]:
    """Reproject a photographic cap into a compact runtime toggle pose.

    We deliberately do not reuse the source cap's extreme screen position.
    Instead all poses share the same X axis and move only a few pixels around
    the fixed bezel pivot. A slight height change supplies perspective without
    making the lever look as if it rotates through a huge angle.
    """
    crop = crop_visible(source_cap)
    target_h = POSE_CAP_HEIGHT[pose]
    aspect = crop.width / max(1, crop.height)
    target_w = max(18, int(target_h * aspect * POSE_CAP_WIDTH_SCALE[pose]))

    cap = crop.resize((target_w, target_h), Image.Resampling.LANCZOS)
    cap = ImageEnhance.Sharpness(cap).enhance(1.45)

    x = PIVOT_X - target_w // 2
    y = POSE_CAP_CENTER_Y[pose] - target_h // 2
    return cap, (x, y)


def moving_shaft_to(cap_rect: tuple[int, int, int, int]) -> Image.Image:
    """Draw only the small moving metal shaft between fixed pivot and cap.

    Its bottom end is always PIVOT_X/PIVOT_Y, so the bezel never moves. The top
    end follows the cap by only a few pixels between poses.
    """
    scale = 4
    size = 128 * scale
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    x0, y0, x1, y1 = cap_rect
    cap_bottom_y = min(PIVOT_Y - 2, y1 - 2)
    top_x = ((x0 + x1) // 2) * scale
    top_y = cap_bottom_y * scale
    bottom_x = PIVOT_X * scale
    bottom_y = PIVOT_Y * scale

    # Small shadow behind the moving shaft.
    draw.line(
        (top_x + 3 * scale, top_y + 2 * scale, bottom_x + 3 * scale, bottom_y + 2 * scale),
        fill=(10, 8, 7, 95),
        width=10 * scale,
    )

    # Cylindrical metallic shaft. Because the travel is small, the shaft only
    # changes length subtly; it never drags the fixed ring with it.
    for offset in range(-5 * scale, 6 * scale):
        t = (offset + 5 * scale) / (10 * scale)
        value = 75 + 150 * (math.sin(math.pi * t) ** 0.9)
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


def compact_lever(source_cap: Image.Image, pose: str) -> Image.Image:
    cap, (x, y) = pose_cap(source_cap, pose)
    rect = (x, y, x + cap.width, y + cap.height)
    shaft = moving_shaft_to(rect)

    out = Image.new("RGBA", (128, 128), (0, 0, 0, 0))
    out.alpha_composite(shaft)
    out.alpha_composite(cap, (x, y))
    return out


def centre_source(up_cap: Image.Image, down_cap: Image.Image) -> Image.Image:
    """Blend both photographic views for a high-resolution neutral material."""
    up = crop_visible(up_cap)
    down = crop_visible(down_cap)
    target_w = max(up.width, down.width)
    target_h = max(up.height, down.height)
    up = up.resize((target_w, target_h), Image.Resampling.LANCZOS)
    down = down.resize((target_w, target_h), Image.Resampling.LANCZOS)
    return Image.blend(up, down, 0.35)


def build_moving_lever_atlas() -> Image.Image:
    source = source_sprite_sheet()

    led_on = source_cell(source, 0, 0)

    red_up_cap = cap_only(source_cell(source, 1, 0), "up")
    red_down_cap = cap_only(source_cell(source, 2, 0), "down")
    white_up_cap = cap_only(source_cell(source, 3, 0), "up")
    white_down_cap = cap_only(source_cell(source, 0, 1), "down")
    blue_up_cap = cap_only(source_cell(source, 1, 1), "up")
    blue_down_cap = cap_only(source_cell(source, 3, 1), "down")
    black_up_cap = cap_only(source_cell(source, 0, 2), "up")
    black_down_cap = cap_only(source_cell(source, 2, 2), "down")

    # Bistable sense/power switches use compact up/down poses too.
    red_up = compact_lever(red_up_cap, "up")
    red_down = compact_lever(red_down_cap, "down")
    white_up = compact_lever(white_up_cap, "up")
    white_down = compact_lever(white_down_cap, "down")

    # Spring-centred function/AUX switches use the same fixed pivot and only a
    # seven-pixel pose delta either side of centre.
    blue_up = compact_lever(blue_up_cap, "up")
    blue_center = compact_lever(centre_source(blue_up_cap, blue_down_cap), "center")
    blue_down = compact_lever(blue_down_cap, "down")

    black_up = compact_lever(black_up_cap, "up")
    black_center = compact_lever(centre_source(black_up_cap, black_down_cap), "center")
    black_down = compact_lever(black_down_cap, "down")

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
        f"Built clean {OUT / 'panel.jpg'} and compact moving-lever atlas "
        f"{OUT / 'sprites.png'} ({sprites.width}x{sprites.height}, sha256={digest})"
    )


if __name__ == "__main__":
    main()
