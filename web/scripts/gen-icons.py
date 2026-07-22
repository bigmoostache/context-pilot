#!/usr/bin/env python3
"""Generate the Context Pilot home-screen / PWA icon set from a source image.

iOS uses an `apple-touch-icon` PNG for the home-screen glyph (it ignores an
SVG there and otherwise falls back to a blurry screenshot of the page — the
exact complaint this fixes, T621). This script rasterises the brand icon at
every size the home-screen / PWA slots need, straight from a single square
source PNG (`scripts/assets/app-icon-source.png`) — so swapping the app icon is
just replacing that one file and re-running.

The source is composited onto the app's dark "Dungeon Forge" background
(`--background` #222220, from `src/index.css`) for the OPAQUE slots: iOS renders
any alpha in an `apple-touch-icon` as BLACK, so a transparent source would get
an ugly black backing — compositing onto the theme colour keeps it on-brand.
The maskable icon keeps the artwork inside the centre 80% "safe zone" (Android
crops maskable icons to a circle/squircle) on a full-bleed background.

Run:  python3 scripts/gen-icons.py
Emits into web/public/: apple-touch-icon.png (180), icon-192.png,
icon-512.png, icon-maskable-512.png, favicon-32.png.
"""

from __future__ import annotations

import os
from PIL import Image

BG = (0x22, 0x22, 0x20, 255)  # --background dark ("Dungeon Forge")

HERE = os.path.dirname(os.path.abspath(__file__))
PUBLIC = os.path.normpath(os.path.join(HERE, "..", "public"))
SOURCE = os.path.join(HERE, "assets", "app-icon-source.png")


def load_source() -> Image.Image:
    """Load the source icon as RGBA, centre-cropped to a square if needed."""
    img = Image.open(SOURCE).convert("RGBA")
    w, h = img.size
    if w != h:
        # Centre-crop to the largest square the source contains.
        side = min(w, h)
        left = (w - side) // 2
        top = (h - side) // 2
        img = img.crop((left, top, left + side, top + side))
    return img


def flatten(img: Image.Image) -> Image.Image:
    """Composite the (possibly transparent) source onto the opaque theme bg —
    iOS renders alpha in an apple-touch-icon as black, so opaque slots must be
    flattened onto the brand background rather than left transparent."""
    bg = Image.new("RGBA", img.size, BG)
    bg.alpha_composite(img)
    return bg


def render(src: Image.Image, size: int, *, maskable: bool = False) -> Image.Image:
    """Render one square icon at `size` px from the source.

    Opaque slots (apple-touch, favicon, 192/512) flatten the source onto the
    theme background. The `maskable` icon insets the artwork into the centre 80%
    safe zone on a full-bleed background so Android's circle/squircle crop never
    bites into it. All resampling is LANCZOS for clean edges.
    """
    flat = flatten(src)
    if not maskable:
        return flat.resize((size, size), Image.LANCZOS)

    # Maskable: full-bleed bg + artwork shrunk into the centre 80% safe zone.
    canvas = Image.new("RGBA", (size, size), BG)
    inner = int(size * 0.8)
    art = flat.resize((inner, inner), Image.LANCZOS)
    off = (size - inner) // 2
    canvas.alpha_composite(art, (off, off))
    return canvas


def main() -> None:
    os.makedirs(PUBLIC, exist_ok=True)
    src = load_source()
    outputs = [
        ("apple-touch-icon.png", 180, False),
        ("icon-192.png", 192, False),
        ("icon-512.png", 512, False),
        ("icon-maskable-512.png", 512, True),
        ("favicon-32.png", 32, False),
    ]
    for name, size, maskable in outputs:
        render(src, size, maskable=maskable).save(os.path.join(PUBLIC, name))
        print(f"wrote public/{name} ({size}x{size}{' maskable' if maskable else ''})")


if __name__ == "__main__":
    main()
