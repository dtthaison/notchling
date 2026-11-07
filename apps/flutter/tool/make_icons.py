#!/usr/bin/env python3
"""Builds the launcher icons from assets/icon/icon-1024.png (Notchling's coffee cup).

iOS needs a full-bleed square with no transparency, so the rounded square is flattened onto its own
background. Android gets legacy icons (the rounded square as is), plus an adaptive icon: a background
layer (the icon's own gradient) and a foreground layer (the cup and steam, inside the 66% safe zone).

Needs Pillow:  python3 -m pip install pillow     Run from apps/flutter/:  python3 tool/make_icons.py
"""
from __future__ import annotations

import json
from pathlib import Path

from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "assets/icon/icon-1024.png"
ANDROID_RES = ROOT / "android/app/src/main/res"
IOS_SET = ROOT / "ios/Runner/Assets.xcassets/AppIcon.appiconset"


def load_square() -> Image.Image:
    """The rounded square without the transparent margin and its soft shadow."""
    image = Image.open(SOURCE).convert("RGBA")
    solid = image.split()[3].point(lambda a: 255 if a > 200 else 0)
    box = solid.getbbox()
    assert box, "icon has no opaque pixels"
    side = min(box[2] - box[0], box[3] - box[1])
    return image.crop((box[0], box[1], box[0] + side, box[1] + side))


def background_gradient(square: Image.Image) -> Image.Image:
    """A full-bleed copy of the icon's own background, from its colour down the left side.

    The left edge is clear of the cup and saucer in the middle rows; in the top and bottom rows (rounded
    corners) a column a little further in is used, still clear of the steam and the cup.
    """
    size = square.width
    column = Image.new("RGB", (1, size))
    for y in range(size):
        t = y / size
        centre = 0.06 if 0.38 <= t <= 0.85 else 0.2
        x = int(size * centre)
        pixels = [square.getpixel((x + dx, y)) for dx in range(-2, 3)]
        opaque = [p for p in pixels if p[3] > 250] or pixels
        column.putpixel((0, y), tuple(sum(p[i] for p in opaque) // len(opaque) for i in range(3)))
    return column.resize((size, size), Image.BILINEAR).filter(ImageFilter.GaussianBlur(6))


def flattened(square: Image.Image, background: Image.Image) -> Image.Image:
    full = background.convert("RGBA")
    full.alpha_composite(square)
    return full.convert("RGB")


def cup_layer(flat: Image.Image, background: Image.Image) -> Image.Image:
    """Cup, steam and stars: whatever differs from the plain background, with a soft edge."""
    diff = Image.new("L", flat.size)
    pixels = flat.load()
    base = background.convert("RGB").load()
    out = diff.load()
    for y in range(flat.height):
        for x in range(flat.width):
            a, b = pixels[x, y], base[x, y]
            out[x, y] = max(0, min(255, (max(abs(a[i] - b[i]) for i in range(3)) - 14) * 8))
    layer = flat.convert("RGBA")
    layer.putalpha(diff)
    return layer


def on_canvas(layer: Image.Image, canvas_px: int, content_fraction: float) -> Image.Image:
    """The layer's visible part, fitted inside the middle `content_fraction` of a transparent canvas."""
    box = layer.split()[3].point(lambda a: 255 if a > 40 else 0).getbbox() or (0, 0, layer.width, layer.height)
    content = layer.crop(box)
    target = int(canvas_px * content_fraction)
    scale = min(target / content.width, target / content.height)
    content = content.resize((max(1, round(content.width * scale)), max(1, round(content.height * scale))), Image.LANCZOS)
    canvas = Image.new("RGBA", (canvas_px, canvas_px), (0, 0, 0, 0))
    canvas.alpha_composite(content, ((canvas_px - content.width) // 2, (canvas_px - content.height) // 2))
    return canvas


def main() -> None:
    square = load_square()
    background = background_gradient(square)
    flat = flattened(square, background)
    foreground = cup_layer(flat, background)

    # iOS: full-bleed, opaque.
    contents = json.loads((IOS_SET / "Contents.json").read_text())
    for entry in contents["images"]:
        points = float(entry["size"].split("x")[0])
        pixels = round(points * float(entry["scale"].rstrip("x")))
        name = f"Icon-{entry['idiom']}-{entry['size']}@{entry['scale']}.png"
        flat.resize((pixels, pixels), Image.LANCZOS).save(IOS_SET / name)
        entry["filename"] = name
    (IOS_SET / "Contents.json").write_text(json.dumps(contents, indent=2) + "\n")

    # Android: legacy (rounded square with its transparent corners) and adaptive layers.
    legacy = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
    layers = {"mdpi": 108, "hdpi": 162, "xhdpi": 216, "xxhdpi": 324, "xxxhdpi": 432}
    for density, px in legacy.items():
        folder = ANDROID_RES / f"mipmap-{density}"
        folder.mkdir(parents=True, exist_ok=True)
        square.resize((px, px), Image.LANCZOS).save(folder / "ic_launcher.png")
    for density, px in layers.items():
        folder = ANDROID_RES / f"mipmap-{density}"
        background.resize((px, px), Image.LANCZOS).convert("RGB").save(folder / "ic_launcher_background.png")
        # 66 % is the adaptive-icon safe zone: the cup is never clipped by any mask shape.
        on_canvas(foreground, px, 0.66).save(folder / "ic_launcher_foreground.png")
    adaptive = ANDROID_RES / "mipmap-anydpi-v26"
    adaptive.mkdir(parents=True, exist_ok=True)
    (adaptive / "ic_launcher.xml").write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
        '    <background android:drawable="@mipmap/ic_launcher_background"/>\n'
        '    <foreground android:drawable="@mipmap/ic_launcher_foreground"/>\n'
        "</adaptive-icon>\n"
    )
    print("icons written")


if __name__ == "__main__":
    main()
