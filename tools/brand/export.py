#!/usr/bin/env python3
from __future__ import annotations

import html
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
BRAND = ROOT / "assets/brand"
KIT = BRAND / "kit"
FLUTTER_ICON = ROOT / "apps/flutter/assets/icon"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
RENDERS = {
    "notchling-icon.svg": "icon-1024.png",
    "notchling-icon-square.svg": "icon-square-1024.png",
    "notchling-adaptive-background.svg": "adaptive-background-1024.png",
    "notchling-adaptive-foreground.svg": "adaptive-foreground-1024.png",
}


def chrome(*args: str) -> str:
    out = subprocess.run([CHROME, "--headless=new", "--disable-gpu", *args], capture_output=True, text=True, check=True)
    return out.stdout


def square_variant(icon: str) -> str:
    full = icon.replace('viewBox="0 0 1024 1024"', 'viewBox="100 100 824 824"')
    return re.sub(r'(<clipPath id="nbc\d+"><rect [^>]*?) rx="185"', r"\1", full)


def write_svgs() -> None:
    dom = chrome("--dump-dom", (KIT / "export.html").as_uri())
    found = re.search(r'<pre id="out">(.*?)</pre>', dom, re.S)
    if not found:
        sys.exit("export.html gave no output")
    svgs = json.loads(html.unescape(found.group(1)))
    svgs["notchling-icon-square.svg"] = square_variant(svgs["notchling-icon.svg"])
    for name, text in svgs.items():
        (BRAND / name).write_text(text + "\n")


def render_pngs() -> None:
    for svg, png in RENDERS.items():
        chrome("--hide-scrollbars", "--default-background-color=00000000", "--window-size=1024,1024",
               f"--screenshot={BRAND / png}", (BRAND / svg).as_uri())
    for stem, name in (("notchling-menubar-template", "menubarTemplate"), ("notchling-menubar-template-filled", "menubarTemplateFilled")):
        glyph = (BRAND / f"{stem}.svg").read_text().replace('width="22" height="16"', 'width="1100" height="800"')
        with tempfile.TemporaryDirectory() as tmp:
            big_svg, big_png = Path(tmp) / "glyph.svg", Path(tmp) / "glyph.png"
            big_svg.write_text(glyph)
            chrome("--hide-scrollbars", "--default-background-color=00000000", "--window-size=1100,800",
                   f"--screenshot={big_png}", big_svg.as_uri())
            big = Image.open(big_png).convert("RGBA")
            big.resize((22, 16), Image.LANCZOS).save(BRAND / f"{name}.png")
            big.resize((44, 32), Image.LANCZOS).save(BRAND / f"{name}@2x.png")


def build_icns() -> None:
    icon = Image.open(BRAND / "icon-1024.png").convert("RGBA")
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "AppIcon.iconset"
        iconset.mkdir()
        for side in (16, 32, 128, 256, 512):
            icon.resize((side, side), Image.LANCZOS).save(iconset / f"icon_{side}x{side}.png")
            icon.resize((side * 2, side * 2), Image.LANCZOS).save(iconset / f"icon_{side}x{side}@2x.png")
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(BRAND / "AppIcon.icns")], check=True)


def copy_to_flutter() -> None:
    for png in RENDERS.values():
        shutil.copyfile(BRAND / png, FLUTTER_ICON / png)


def main() -> None:
    write_svgs()
    render_pngs()
    build_icns()
    copy_to_flutter()
    print("brand assets written; now run: cd apps/flutter && python3 tool/make_icons.py")


if __name__ == "__main__":
    main()
