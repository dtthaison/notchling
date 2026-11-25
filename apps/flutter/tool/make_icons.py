#!/usr/bin/env python3
from __future__ import annotations

import json
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "assets/icon"
ANDROID_RES = ROOT / "android/app/src/main/res"
IOS_SET = ROOT / "ios/Runner/Assets.xcassets/AppIcon.appiconset"
SAFE_ZONE = 0.46


def load(name: str) -> Image.Image:
    return Image.open(ICONS / name).convert("RGBA")


def on_canvas(layer: Image.Image, canvas_px: int, content_fraction: float) -> Image.Image:
    box = layer.split()[3].point(lambda a: 255 if a > 40 else 0).getbbox() or (0, 0, layer.width, layer.height)
    content = layer.crop(box)
    target = int(canvas_px * content_fraction)
    scale = min(target / content.width, target / content.height)
    content = content.resize((max(1, round(content.width * scale)), max(1, round(content.height * scale))), Image.LANCZOS)
    canvas = Image.new("RGBA", (canvas_px, canvas_px), (0, 0, 0, 0))
    canvas.alpha_composite(content, ((canvas_px - content.width) // 2, (canvas_px - content.height) // 2))
    return canvas


def main() -> None:
    rounded = load("icon-1024.png")
    flat = load("icon-square-1024.png").convert("RGB")
    background = load("adaptive-background-1024.png").convert("RGB")
    foreground = load("adaptive-foreground-1024.png")

    contents = json.loads((IOS_SET / "Contents.json").read_text())
    for entry in contents["images"]:
        points = float(entry["size"].split("x")[0])
        pixels = round(points * float(entry["scale"].rstrip("x")))
        name = f"Icon-{entry['idiom']}-{entry['size']}@{entry['scale']}.png"
        flat.resize((pixels, pixels), Image.LANCZOS).save(IOS_SET / name)
        entry["filename"] = name
    (IOS_SET / "Contents.json").write_text(json.dumps(contents, indent=2) + "\n")

    box = rounded.split()[3].point(lambda a: 255 if a > 200 else 0).getbbox()
    square = rounded.crop(box) if box else rounded
    legacy = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
    layers = {"mdpi": 108, "hdpi": 162, "xhdpi": 216, "xxhdpi": 324, "xxxhdpi": 432}
    for density, px in legacy.items():
        folder = ANDROID_RES / f"mipmap-{density}"
        folder.mkdir(parents=True, exist_ok=True)
        square.resize((px, px), Image.LANCZOS).save(folder / "ic_launcher.png")
    for density, px in layers.items():
        folder = ANDROID_RES / f"mipmap-{density}"
        background.resize((px, px), Image.LANCZOS).save(folder / "ic_launcher_background.png")
        on_canvas(foreground, px, SAFE_ZONE).save(folder / "ic_launcher_foreground.png")
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
