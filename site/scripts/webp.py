#!/usr/bin/env python3
"""Writes a WebP copy of every screenshot in site/assets/screens (offline; outputs are committed).

    python3 site/scripts/webp.py

Needs Pillow (MIT-CMU licence). The site build itself stays standard-library only: it just
emits <source type="image/webp"> for screenshots that have a .webp next to the .png.
"""
import pathlib

from PIL import Image

SCREENS = pathlib.Path(__file__).resolve().parent.parent / "assets" / "screens"

total_png = total_webp = 0
for png in sorted(SCREENS.glob("*.png")):
    webp = png.with_suffix(".webp")
    with Image.open(png) as im:
        im = im.convert("RGBA" if im.mode in ("RGBA", "LA", "P") else "RGB")
        im.save(webp, "WEBP", quality=82, method=6)
    total_png += png.stat().st_size
    total_webp += webp.stat().st_size
    print(f"{png.name}: {png.stat().st_size // 1024} KB -> {webp.stat().st_size // 1024} KB")
print(f"total: {total_png // 1024} KB PNG -> {total_webp // 1024} KB WebP")
