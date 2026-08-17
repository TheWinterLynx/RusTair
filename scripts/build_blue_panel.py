#!/usr/bin/env python3
"""Build animated switch/LED sprites for RusTair's clean blue Altair panel.

The clean panel background is versioned separately as assets/panels/blue/panel.jpg.
This script only regenerates the transparent sprite sheet. Every lower blue control
on the Altair is a toggle switch: the function switches are three-position,
spring-centred controls (up / centre / down), not push buttons.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter

OUT = Path("assets/panels/blue")
CELL = 160
SCALE = 4

RED = (218, 35, 68)
WHITE = (231, 222, 193)
BLUE = (37, 151, 205)
BLACK = (35, 35, 31)


def rounded_line(draw, xy, width, fill):
    draw.line(xy, fill=fill, width=width)
    r = width // 2
    for x, y in (xy[0], xy[-1]):
        draw.ellipse((x-r, y-r, x+r, y+r), fill=fill)


def toggle(color, position):
    """Transparent toggle overlay. position: -1 up, 0 centre, +1 down."""
    s = CELL * SCALE
    im = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    cx = cy = s // 2

    # The clean panel already contains the mounting ring, so only draw the
    # lever, its pivot and a small contact shadow.
    shadow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    sd.ellipse((cx-54, cy-42, cx+54, cy+62), fill=(0, 0, 0, 105))
    shadow = shadow.filter(ImageFilter.GaussianBlur(16))
    im.alpha_composite(shadow)
    d = ImageDraw.Draw(im)

    d.ellipse((cx-28, cy-28, cx+28, cy+28), fill=(108, 105, 96, 230), outline=(210, 201, 179, 220), width=5)

    if position == 0:
        # In the spring-centred state the lever points out of the panel, so its
        # coloured end cap is seen almost head-on.
        d.ellipse((cx-50, cy-38, cx+50, cy+43), fill=(*color, 255), outline=(39, 36, 32, 245), width=7)
        d.ellipse((cx-28, cy-24, cx+4, cy+3), fill=(255, 255, 255, 75))
    else:
        end_y = cy + position * 112
        rounded_line(d, [(cx, cy), (cx, end_y)], 34, (170, 164, 148, 255))
        hy = cy + position * 132
        box = (cx-48, hy-62, cx+48, hy+62)
        d.rounded_rectangle(box, radius=40, fill=(*color, 255), outline=(42, 38, 34, 240), width=7)
        highlight_y0 = hy-46 if position < 0 else hy-30
        d.rounded_rectangle((cx-30, highlight_y0, cx-7, highlight_y0+58), radius=11, fill=(255,255,255,55))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def led_on():
    s = CELL * SCALE
    im = Image.new("RGBA", (s, s), (0,0,0,0))
    cx = cy = s//2

    glow = Image.new("RGBA", (s,s), (0,0,0,0))
    gd = ImageDraw.Draw(glow)
    gd.ellipse((cx-190, cy-190, cx+190, cy+190), fill=(255, 18, 0, 112))
    glow = glow.filter(ImageFilter.GaussianBlur(68))
    im.alpha_composite(glow)

    glow2 = Image.new("RGBA", (s,s), (0,0,0,0))
    g2 = ImageDraw.Draw(glow2)
    g2.ellipse((cx-105, cy-105, cx+105, cy+105), fill=(255, 42, 4, 150))
    glow2 = glow2.filter(ImageFilter.GaussianBlur(30))
    im.alpha_composite(glow2)

    d = ImageDraw.Draw(im)
    d.ellipse((cx-66, cy-66, cx+66, cy+66), fill=(255, 20, 3, 250))
    d.ellipse((cx-39, cy-39, cx+39, cy+39), fill=(255, 78, 12, 255))
    d.ellipse((cx-14, cy-14, cx+14, cy+14), fill=(255, 251, 215, 255))
    d.ellipse((cx-22, cy-30, cx-4, cy-12), fill=(255,255,255,210))
    return im.resize((CELL, CELL), Image.Resampling.LANCZOS)


def led_off():
    # Kept for future skins; the current clean plate already contains off lenses.
    s = CELL * SCALE
    im = Image.new("RGBA", (s,s), (0,0,0,0))
    d = ImageDraw.Draw(im)
    cx = cy = s//2
    d.ellipse((cx-62,cy-62,cx+62,cy+62), fill=(64,5,5,245), outline=(25,0,0,240), width=6)
    d.ellipse((cx-32,cy-38,cx+2,cy-10), fill=(255,130,115,28))
    return im.resize((CELL,CELL), Image.Resampling.LANCZOS)


def make_sprites():
    sheet = Image.new("RGBA", (CELL*4, CELL*3), (0,0,0,0))
    sprites = {
        (0,0): led_on(),
        (1,0): toggle(RED, -1),
        (2,0): toggle(RED, +1),
        (3,0): toggle(WHITE, -1),
        (0,1): toggle(WHITE, +1),
        (1,1): toggle(BLUE, -1),
        (2,1): toggle(BLUE, 0),
        (3,1): toggle(BLUE, +1),
        (0,2): toggle(BLACK, -1),
        (1,2): toggle(BLACK, 0),
        (2,2): toggle(BLACK, +1),
        (3,2): led_off(),
    }
    for (col,row), sprite in sprites.items():
        sheet.alpha_composite(sprite, (col*CELL,row*CELL))
    return sheet


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    panel = OUT / "panel.jpg"
    if not panel.exists():
        raise RuntimeError(f"Missing clean panel base: {panel}")
    make_sprites().save(OUT / "sprites.png", optimize=True)
    print(f"Built {OUT/'sprites.png'} from clean panel skin")


if __name__ == '__main__':
    main()
