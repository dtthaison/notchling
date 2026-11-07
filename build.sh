#!/bin/bash
# Builds Notchling.app into ./build. Pass --install to copy it to /Applications,
# or --dmg to also make build/Notchling-<version>.dmg for other Macs.
#
# Three pieces, all swiftc, none with package dependencies:
#   ConsoleCore  Sources/ConsoleCore: the phone bridge's engine and the session host client, built per
#                architecture as a static module that Notchling and the helper import
#   Notchling    Sources/Notchling
#   hostd        Sources/hostd: the session host, shipped as Contents/Helpers/agent-console-hostd
set -euo pipefail
cd "$(dirname "$0")"

APP=build/Notchling.app
SOURCES=$(find Sources/Notchling -name '*.swift' | sort)
CORE_SOURCES=$(find Sources/ConsoleCore -name '*.swift' | sort)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers"

for arch in arm64 x86_64; do
    CORE=build/core-$arch
    rm -rf "$CORE" && mkdir -p "$CORE"
    swiftc -O -swift-version 5 -target $arch-apple-macos13.0 -parse-as-library -module-name ConsoleCore \
        -emit-module -emit-module-path "$CORE/ConsoleCore.swiftmodule" -static -emit-library -o "$CORE/libConsoleCore.a" $CORE_SOURCES
    swiftc -O -target $arch-apple-macos13.0 -I "$CORE" -L "$CORE" -lConsoleCore -o build/Notchling-$arch $SOURCES
    swiftc -O -swift-version 5 -target $arch-apple-macos13.0 -I "$CORE" -L "$CORE" -lConsoleCore -o build/hostd-$arch Sources/hostd/main.swift
done
lipo -create build/Notchling-arm64 build/Notchling-x86_64 -output "$APP/Contents/MacOS/Notchling"
lipo -create build/hostd-arm64 build/hostd-x86_64 -output "$APP/Contents/Helpers/agent-console-hostd"
rm -rf build/Notchling-arm64 build/Notchling-x86_64 build/hostd-arm64 build/hostd-x86_64 build/core-arm64 build/core-x86_64

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
cp Pet/*.png "$APP/Contents/Resources/"
cp Info.plist "$APP/Contents/Info.plist"
codesign --force --sign - "$APP/Contents/Helpers/agent-console-hostd"
codesign --force --sign - "$APP"

echo "Built $APP"

if [[ "${1:-}" == "--dmg" ]]; then
    # Drag-to-install disk image: the app next to a link to /Applications.
    VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" Info.plist)
    DMG=build/Notchling-$VERSION.dmg
    STAGE=build/dmg
    rm -rf "$STAGE" "$DMG" && mkdir -p "$STAGE"
    cp -R "$APP" "$STAGE/"
    ln -s /Applications "$STAGE/Applications"
    hdiutil create -volname Notchling -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
    rm -rf "$STAGE"
    echo "Built $DMG"
fi

if [[ "${1:-}" == "--install" ]]; then
    pkill -x Notchling 2>/dev/null && sleep 1 || true
    rm -rf /Applications/Notchling.app
    cp -R "$APP" /Applications/
    # The session host (LaunchAgent) points at the helper inside the app. Installing replaces the helper file but only
    # restarts the host when no session runs in it. First time, or moving from Agent Console: tools/migrate-and-install.sh.
    tools/install-hostd.sh /Applications/Notchling.app
    open /Applications/Notchling.app
    echo "Installed to /Applications/Notchling.app"
fi
