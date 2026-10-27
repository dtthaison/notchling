#!/bin/bash
# Renders a README demo from the real views, driven by made-up data, as a GIF in docs/ and an
# MP4 in build/demo/. Nothing on this Mac is read or shown.
#   Demo/make-demo.sh            the notch (Demo/render.swift → docs/demo.gif)
#   Demo/make-demo.sh accounts   account switching (Demo/accounts.swift → docs/accounts.gif)
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-notch}" in
    notch) SCENE=Demo/render.swift; GIF=docs/demo.gif; NAME=demo ;;
    accounts) SCENE=Demo/accounts.swift; GIF=docs/accounts.gif; NAME=accounts ;;
    *) echo "Usage: Demo/make-demo.sh [notch|accounts]" >&2; exit 1 ;;
esac

FPS=20  # keep in step with `fps` in the scene
OUT=build/demo
APP=$OUT/Render.app
rm -rf "$APP" "$OUT/src" "$OUT/frames" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$OUT/src"

# The pet loads its drawings from the bundle, so the renderer runs as a tiny app.
cp Pet/*.png "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>local.notchling.demo-render</string>
<key>CFBundleExecutable</key><string>render</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
</dict></plist>
PLIST

# Top-level code has to live in a file called main.swift.
cp "$SCENE" "$OUT/src/main.swift"
SOURCES=$(ls Sources/*.swift | grep -vE '/(main|AppDelegate)\.swift$')
# shellcheck disable=SC2086
swiftc -O -D DEMO -o "$APP/Contents/MacOS/render" $SOURCES "$OUT/src/main.swift"
swiftc -O -o "$OUT/encode" Demo/encode.swift
"$APP/Contents/MacOS/render" "$OUT/frames"
"$OUT/encode" "$OUT/frames" "$GIF" "$OUT/$NAME.mp4" "$FPS" 960

echo "Wrote $GIF and $OUT/$NAME.mp4"
