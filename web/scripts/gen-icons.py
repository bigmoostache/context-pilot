#!/usr/bin/env python3
"""Generate the Context Pilot home-screen / PWA icon set as real PNGs.

iOS uses an `apple-touch-icon` PNG for the home-screen glyph (it ignores an
SVG there and otherwise falls back to a blurry screenshot of the page — the
exact complaint this fixes, T621). We have no SVG rasteriser on the box
(rsvg/magick/sharp all absent), so the mark is drawn directly with Pillow —
which also keeps it crisp at every size and free of any system-font dependency
(the glyph is a filled shape, not text).

The mark is the app's brand lockup mark: the torch-orange terminal block
cursor `▌` (see the `▌ Context Pilot` wordmark in the app shell) centred on the
dark "Dungeon Forge" background. Colours are the exact theme tokens from
`src/index.css` so the icon matches the running app:
  • background  #222220  (--background, dark)
  • signal      #da7659  (--signal, torch orange)

Run:  python3 scripts/gen-icons.py
Emits into web/public/: apple-touch-icon.png (180), icon-192.png,
icon-512.png, icon-maskable-512.png, favicon-32.png.
"""

from __future__ import annotations

import os
from PIL import Image, ImageDraw

BG = (0x22, 0x22, 0x20, 255)  # --background dark ("Dungeon Forge")
SIGNAL = (0xDA, 0x76, 0x59, 255)  # --signal torch orange
GLOW = (0xDA, 0x76, 0x59, 60)  # faint orange halo behind the bar

HERE = os.path.dirname(os.path.abspath(__file__))
PUBLIC = os.path.normpath(os.path.join(HERE, "..", "public"))


def render(size: int, *, maskable: bool = False) -> Image.Image:
    """Render one square icon at `size` px.

    We draw at 4× then downsample (LANCZOS) for clean anti-aliased edges — the
    cheap supersampling trick, since Pillow's primitives are not themselves
    anti-aliased. A `maskable` icon keeps the mark inside the centre 80% "safe
    zone" (Android crops maskable icons to a circle/squircle) by using a full-
    bleed background and a slightly smaller bar.
    """
    ss = 4
    s = size * ss
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Background: a rounded square (full-bleed for maskable so the crop never
    # bites into a transparent corner; a rounded card otherwise).
    if maskable:
        d.rectangle([0, 0, s, s], fill=BG)
    else:
        radius = int(s * 0.22)  # iOS re-masks anyway, but a rounded source reads better everywhere
        d.rounded_rectangle([0, 0, s - 1, s - 1], radius=radius, fill=BG)

    # The block-cursor bar: a vertical rounded bar, centred. Slightly smaller
    # for maskable so it survives the safe-zone crop.
    bar_h_frac = 0.42 if maskable else 0.50
    bar_w_frac = 0.14 if maskable else 0.155
    bar_h = int(s * bar_h_frac)
    bar_w = int(s * bar_w_frac)
    cx, cy = s // 2, s // 2
    x0, y0 = cx - bar_w // 2, cy - bar_h // 2
    x1, y1 = x0 + bar_w, y0 + bar_h
    bar_r = bar_w // 2

    # Faint halo behind the bar (phosphor glow) — a wider, softer bar.
    pad = int(s * 0.05)
    d.rounded_rectangle(
        [x0 - pad, y0 - pad, x1 + pad, y1 + pad],
        radius=bar_r + pad,
        fill=GLOW,
    )
    d.rounded_rectangle([x0, y0, x1, y1], radius=bar_r, fill=SIGNAL)

    return img.resize((size, size), Image.LANCZOS)


def main() -> None:
    os.makedirs(PUBLIC, exist_ok=True)
    outputs = [
        ("apple-touch-icon.png", 180, False),
        ("icon-192.png", 192, False),
        ("icon-512.png", 512, False),
        ("icon-maskable-512.png", 512, True),
        ("favicon-32.png", 32, False),
    ]
    for name, size, maskable in outputs:
        render(size, maskable=maskable).save(os.path.join(PUBLIC, name))
        print(f"wrote public/{name} ({size}x{size}{' maskable' if maskable else ''})")


if __name__ == "__main__":
    main()
