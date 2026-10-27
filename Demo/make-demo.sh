#!/bin/bash
# Renders docs/demo.gif (for the README) and build/demo/demo.mp4 from the real notch views,
# driven by made-up agents in Demo/render.swift. Nothing on this Mac is read or shown.
set -euo pipefail
cd "$(dirname "$0")/.."

FPS=20  # keep in step with `fps` in Demo/render.swift
OUT=build/demo
APP=$OUT/Render.app
rm -rf "$OUT" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$OUT/src"

# The pet loads its drawings from the bundle, so the renderer runs as a tiny app.
cp Pet/*.png "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>local.notchling.demo-render</string>
<key>CFBundleExecutable</key><string>render</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
</dict></plist>
EOF

# Top-level code has to live in a file called main.swift.
cp Demo/render.swift "$OUT/src/main.swift"
SOURCES=$(ls Sources/*.swift | grep -vE '/(main|AppDelegate)\.swift$')
# shellcheck disable=SC2086
swiftc -O -o "$APP/Contents/MacOS/render" $SOURCES "$OUT/src/main.swift"
swiftc -O -o "$OUT/encode" Demo/encode.swift
"$APP/Contents/MacOS/render" "$OUT/frames"
"$OUT/encode" "$OUT/frames" docs/demo.gif "$OUT/demo.mp4" "$FPS" 960

echo "Wrote docs/demo.gif and $OUT/demo.mp4"
