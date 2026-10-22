#!/bin/bash
# Builds Notchling.app into ./build. Pass --install to copy it to /Applications.
set -euo pipefail
cd "$(dirname "$0")"

APP=build/Notchling.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"

swiftc -O -target arm64-apple-macos13.0 -o build/Notchling-arm64 Sources/*.swift
swiftc -O -target x86_64-apple-macos13.0 -o build/Notchling-x86_64 Sources/*.swift
lipo -create build/Notchling-arm64 build/Notchling-x86_64 -output "$APP/Contents/MacOS/Notchling"
rm build/Notchling-arm64 build/Notchling-x86_64

# Re-render the icon only when its drawing script changes.
if [[ ! -f Icon/AppIcon.icns || Icon/make-icon.swift -nt Icon/AppIcon.icns ]]; then
    swift Icon/make-icon.swift Icon/icon-1024.png
    ICONSET=build/AppIcon.iconset
    rm -rf "$ICONSET" && mkdir -p "$ICONSET"
    for px in 16 32 128 256 512; do
        sips -z $px $px Icon/icon-1024.png --out "$ICONSET/icon_${px}x${px}.png" >/dev/null
        sips -z $((px * 2)) $((px * 2)) Icon/icon-1024.png --out "$ICONSET/icon_${px}x${px}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o Icon/AppIcon.icns
    rm -rf "$ICONSET"
fi

mkdir -p "$APP/Contents/Resources"
cp Icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp Info.plist "$APP/Contents/Info.plist"
codesign --force --sign - "$APP"

echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
    pkill -x Notchling 2>/dev/null && sleep 1 || true
    rm -rf /Applications/Notchling.app
    cp -R "$APP" /Applications/
    open /Applications/Notchling.app
    echo "Installed to /Applications/Notchling.app"
fi
