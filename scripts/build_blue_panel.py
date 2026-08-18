#!/usr/bin/env python3
"""Build RusTair's clean Altair panel and moving-lever sprite atlas.

The clean panel and approved realistic switch artwork are reconstructed from
Base64 chunks versioned under assets/panels/blue/source/.

Only the moving lever/cap/shaft is present in the runtime atlas. The fixed metal
bezel/mounting hole lives in panel.jpg and therefore never moves.

There are two mechanically different visual families:
- bistable SENSE/POWER toggles: a real DOWN pose below the pivot and a compact
  neutral/slightly-UP pose above it;
- spring-centred function/AUX toggles: their REST/CENTER pose deliberately
  matches the physical DOWN/rest pose of the SENSE switches, exactly as on the
  reference front panel. UP/DOWN are only shown while the switch is actuated.
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

# Compact upper pose used when a bistable switch is raised. Keeping this close
# to the bezel prevents the cap from covering the printed number/label above it.
RAISED_POSE = (53, 31, 1.04)  # centre_y, height, width scale

# Bistable SENSE/POWER switches start DOWN. DOWN is genuinely below the fixed
# pivot. This is also the visual reference for the resting lower switches.
BISTABLE_POSES = {
    "up": RAISED_POSE,
    "down": (88, 34, 1.02),
}

# The lower blue/grey switches are spring-centred logically, but in the real
# front-panel perspective their unactuated lever should look like the resting
# A15-A0 toggles. Therefore CENTER is EXACTLY the SENSE DOWN geometry. When held
# upward it moves only to RAISED_POSE; when held downward it travels a little
# farther below the pivot.
FUNCTION_REST_POSE = BISTABLE_POSES["down"]
CENTERED_POSES = {
    "up": RAISED_POSE,
    "center": FUNCTION_REST_POSE,
    "down": (101, 31, 1.06),
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
    """Extract cap material while excluding the photographed fixed bezel.

    The realistic source contains complete UP and DOWN switch photographs. The
    runtime panel already owns the metal mounting bezel, so only cap material is
    retained here and a new moving shaft is drawn from the fixed pivot.
    """
    alpha = full.getchannel("A")
    mask = Image.new("L", full.size, 0)
    draw = ImageDraw.Draw(mask)

    if direction == "up":
        draw.rectangle((24, 0, 104, 53), fill=255)
    else:
        # The down-facing cap occupies the narrow lower centre of the source
        # photograph. Keeping this region avoids reintroducing the metal bezel.
        draw.rectangle((48, 76, 80, 120), fill=255)

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
    """Draw the moving shaft from one fixed bezel pivot to the nearest cap edge."""
    scale = 4
    image = Image.new("RGBA", (128 * scale, 128 * scale), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    x0, y0, x1, y1 = cap_rect
    cap_center_y = (y0 + y1) / 2
    target_x = ((x0 + x1) // 2) * scale

    if cap_center_y < PIVOT_Y:
        target_y = min(PIVOT_Y - 2, y1 - 2) * scale
    else:
        target_y = max(PIVOT_Y + 2, y0 + 2) * scale

    bottom_x = PIVOT_X * scale
    bottom_y = PIVOT_Y * scale

    draw.line(
        (
            target_x + 3 * scale,
            target_y + 2 * scale,
            bottom_x + 3 * scale,
            bottom_y + 2 * scale,
        ),
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
            (target_x + offset, target_y, bottom_x + offset, bottom_y),
            fill=color,
            width=1,
        )

    draw.line(
        (
            target_x - 2 * scale,
            target_y + scale,
            bottom_x - 2 * scale,
            bottom_y - scale,
        ),
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

    red_up = cap_only(source_cell(source, 1, 0), "up")
    red_down = cap_only(source_cell(source, 2, 0), "down")
    white_up = cap_only(source_cell(source, 3, 0), "up")
    white_down = cap_only(source_cell(source, 0, 1), "down")
    blue_up = cap_only(source_cell(source, 1, 1), "up")
    blue_down = cap_only(source_cell(source, 3, 1), "down")
    black_up = cap_only(source_cell(source, 0, 2), "up")
    black_down = cap_only(source_cell(source, 2, 2), "down")

    cells = {
        (0, 0): led_on,
        # Bistable SENSE/POWER family: true physical UP/DOWN.
        (1, 0): lever(red_up, "bistable", "up"),
        (2, 0): lever(red_down, "bistable", "down"),
        (3, 0): lever(white_up, "bistable", "up"),
        (0, 1): lever(white_down, "bistable", "down"),
        # Spring-centred function switches. CENTER deliberately uses the same
        # down-facing cap material and geometry as the resting SENSE switches.
        (1, 1): lever(blue_up, "centered", "up"),
        (2, 1): lever(blue_down, "centered", "center"),
        (3, 1): lever(blue_down, "centered", "down"),
        # AUX follows the exact same mechanical convention.
        (0, 2): lever(black_up, "centered", "up"),
        (1, 2): lever(black_down, "centered", "center"),
        (2, 2): lever(black_down, "centered", "down"),
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

    # Semantic sanity checks so the lower controls cannot silently drift back to
    # the old raised-looking rest pose.
    if not (BISTABLE_POSES["up"][0] < PIVOT_Y < BISTABLE_POSES["down"][0]):
        raise RuntimeError("Bistable UP/DOWN poses do not straddle the fixed pivot")
    if CENTERED_POSES["center"] != BISTABLE_POSES["down"]:
        raise RuntimeError("Function-switch CENTER no longer matches SENSE rest pose")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    panel = build_clean_panel()
    panel.save(OUT / "panel.jpg", quality=94, optimize=True, progressive=True)

    sprites = build_moving_lever_atlas()
    validate_runtime_sprites(sprites)
    sprites.save(OUT / "sprites.png", optimize=True)

    digest = hashlib.sha256((OUT / "sprites.png").read_bytes()).hexdigest()
    print(
        f"Built clean {OUT / 'panel.jpg'} and physical-pose moving-lever atlas "
        f"{OUT / 'sprites.png'} ({sprites.width}x{sprites.height}, sha256={digest})"
    )


if __name__ == "__main__":
    main()
