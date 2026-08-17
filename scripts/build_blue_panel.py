#!/usr/bin/env python3
"""Build the photographic Altair 8800 panel assets used by RusTair.

The base photograph is downloaded from Wikimedia Commons and is CC BY-SA 4.0:
  "MITS Altair 8800 Front Panel.jpg" by Cromemco
  https://commons.wikimedia.org/wiki/File:MITS_Altair_8800_Front_Panel.jpg

The control sprites are drawn procedurally so the switch states can be animated
without baking another photographed switch position into the front panel.
"""
from __future__ import annotations

import hashlib
import io
import urllib.request
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

OUT = Path("assets/panels/blue")
SOURCE_URL = "https://upload.wikimedia.org/wikipedia/commons/b/b2/MITS_Altair_8800_Front_Panel.jpg"
SOURCE_SHA1 = "f496df0232032f7d0d46e9bbb306691b380d8c36"
PANEL_SIZE = (2048, 869)
CELL = 128
SCALE = 4


def download_panel() -> Image.Image:
    req = urllib.request.Request(SOURCE_URL, headers={"User-Agent": "RusTair asset builder/1.0"})
    with urllib.request.urlopen(req, timeout=60) as response:
        data = response.read()
    digest = hashlib.sha1(data).hexdigest()
    if digest != SOURCE_SHA1:
        raise RuntimeError(f"Unexpected source image SHA-1: {digest}")
    image = Image.open(io.BytesIO(data)).convert("RGB")
    return image.resize(PANEL_SIZE, Image.Resampling.LANCZOS)


def rounded_line(draw: ImageDraw.ImageDraw, xy, width, fill):
    draw.line(xy, fill=fill, width=width)
    r = width // 2
    for x, y in (xy[0], xy[-1]):
        draw.ellipse((x-r, y-r, x+r, y+r), fill=fill)


def toggle(color: tuple[int, int, int], up: bool) -> Image.Image:
    s = CELL * SCALE
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    cx = cy = s // 2
    shadow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    sd.ellipse((cx-92, cy-82, cx+92, cy+102), fill=(0, 0, 0, 100))
    shadow = shadow.filter(ImageFilter.GaussianBlur(18))
    im.alpha_composite(shadow)
    d = ImageDraw.Draw(im)
    d.ellipse((cx-82, cy-82, cx+82, cy+82), fill=(92, 91, 84, 255), outline=(190, 180, 155, 255), width=10)
    d.ellipse((cx-62, cy-62, cx+62, cy+62), fill=(18, 19, 18, 255), outline=(7, 7, 7, 255), width=8)
    end_y = cy - 88 if up else cy + 88
    rounded_line(d, [(cx, cy), (cx, end_y)], 38, (154, 148, 132, 255))
    hy = cy - 118 if up else cy + 118
    handle = (cx-48, hy-58, cx+48, hy+58)
    d.rounded_rectangle(handle, radius=42, fill=(*color, 255), outline=(45, 40, 35, 220), width=7)
    d.rounded_rectangle((cx-30, hy-43, cx-5, hy+20), radius=12, fill=(255, 255, 255, 55))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def push_button(color=(38, 164, 213)) -> Image.Image:
    s = CELL * SCALE
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    cx = cy = s // 2
    shadow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    sd.ellipse((cx-90, cy-70, cx+90, cy+105), fill=(0, 0, 0, 100))
    shadow = shadow.filter(ImageFilter.GaussianBlur(16))
    im.alpha_composite(shadow)
    d = ImageDraw.Draw(im)
    d.ellipse((cx-78, cy-78, cx+78, cy+78), fill=(80, 78, 71, 255), outline=(185, 176, 155, 255), width=10)
    d.ellipse((cx-58, cy-58, cx+58, cy+58), fill=(10, 13, 14, 255))
    d.ellipse((cx-49, cy-49, cx+49, cy+49), fill=(*color, 255), outline=(12, 45, 58, 255), width=6)
    d.ellipse((cx-28, cy-34, cx+5, cy-4), fill=(255, 255, 255, 65))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def led_on() -> Image.Image:
    s = CELL * SCALE
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    cx = cy = s // 2
    glow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse((cx-150, cy-150, cx+150, cy+150), fill=(255, 16, 0, 95))
    glow = glow.filter(ImageFilter.GaussianBlur(52))
    im.alpha_composite(glow)
    d = ImageDraw.Draw(im)
    d.ellipse((cx-75, cy-75, cx+75, cy+75), fill=(23, 18, 17, 255), outline=(105, 96, 82, 255), width=8)
    d.ellipse((cx-59, cy-59, cx+59, cy+59), fill=(255, 18, 3, 255), outline=(125, 0, 0, 255), width=5)
    d.ellipse((cx-28, cy-28, cx+28, cy+28), fill=(255, 75, 10, 255))
    d.ellipse((cx-10, cy-10, cx+10, cy+10), fill=(255, 245, 188, 255))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def led_off() -> Image.Image:
    s = CELL * SCALE
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    cx = cy = s // 2
    d.ellipse((cx-75, cy-75, cx+75, cy+75), fill=(26, 22, 20, 255), outline=(92, 87, 76, 255), width=8)
    d.ellipse((cx-58, cy-58, cx+58, cy+58), fill=(73, 5, 5, 255), outline=(35, 0, 0, 255), width=5)
    d.ellipse((cx-33, cy-39, cx+5, cy-9), fill=(255, 120, 105, 25))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def make_sprites() -> Image.Image:
    sheet = Image.new("RGBA", (CELL * 4, CELL * 3), (0, 0, 0, 0))
    sprites = {
        (0, 0): led_on(),
        (1, 0): toggle((215, 33, 73), True),
        (2, 0): toggle((215, 33, 73), False),
        (3, 0): toggle((226, 218, 190), True),
        (0, 1): toggle((226, 218, 190), False),
        (1, 1): toggle((30, 30, 28), True),
        (2, 1): toggle((30, 30, 28), False),
        (3, 1): push_button((35, 164, 214)),
        (0, 2): push_button((28, 29, 27)),
        (1, 2): led_off(),
    }
    for (col, row), sprite in sprites.items():
        sheet.alpha_composite(sprite, (col * CELL, row * CELL))
    return sheet


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    panel = download_panel()
    panel.save(OUT / "panel.jpg", quality=92, optimize=True, progressive=True)
    make_sprites().save(OUT / "sprites.png", optimize=True)
    print(f"Built {OUT / 'panel.jpg'} and {OUT / 'sprites.png'}")


if __name__ == "__main__":
    main()
