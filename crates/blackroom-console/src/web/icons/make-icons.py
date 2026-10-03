#!/usr/bin/env python3
"""Renders the app icons (run by hand when the logo changes; the PNGs are committed): python3 make-icons.py"""
import os
from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
BLUE, WHITE = (29, 95, 180, 255), (255, 255, 255, 255)

def draw(size, maskable):
    scale = 4  # draw large, shrink for smooth edges
    s = size * scale
    image = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(image)
    if maskable:
        d.rectangle((0, 0, s, s), fill=BLUE)  # the launcher crops it; keep the glyph in the middle 60%
        inner = 0.6
    else:
        d.rounded_rectangle((0, 0, s - 1, s - 1), radius=int(s * 0.25), fill=BLUE)
        inner = 0.72
    pad = s * (1 - inner) / 2
    x0, y0, x1, y1 = pad, pad + s * 0.02, s - pad, s - pad - s * 0.12 * inner
    width = max(2, int(s * 0.05 * inner))
    d.rounded_rectangle((x0, y0, x1, y1), radius=int(s * 0.04), outline=WHITE, width=width)
    cx, base = s / 2, s - pad
    d.line((cx - s * 0.12 * inner, base - width, cx + s * 0.12 * inner, base - width), fill=WHITE, width=width)
    d.line((cx, y1, cx, base - width), fill=WHITE, width=width)
    return image.resize((size, size), Image.LANCZOS)

for name, size, maskable in [("icon-192.png", 192, False), ("icon-512.png", 512, False), ("icon-maskable-512.png", 512, True)]:
    draw(size, maskable).save(os.path.join(HERE, name), optimize=True)
    print("wrote", name)
