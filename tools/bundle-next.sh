#!/bin/zsh
# Builds the Rust app as build/Notchling Next.app: notchling-app and the notchling daemon side by
# side in Contents/MacOS (the app starts the notchling beside it), signed ad hoc. It sits beside the
# Swift app's Notchling.app until that retires; the daemon trusts /Applications/Notchling Next.app
# as the app tier (notchling_local_api::tier::installed_app_paths).
#
#   tools/bundle-next.sh             build only
#   tools/bundle-next.sh --install   also copy it to /Applications (quits a running Notchling Next)
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release --locked -p notchling-app -p notchling-notchlingd
# Where cargo put it: a shared `build.target-dir` in ~/.cargo/config.toml moves it out of `target/`,
# and copying from there would bundle whatever older build was left behind.
TARGET="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')/release"
NAME="Notchling Next"
APP="build/$NAME.app"
VERSION=$(cargo pkgid -p notchling-app | sed 's/.*[#@]//')

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$TARGET/notchling-app" "$TARGET/notchling" "$APP/Contents/MacOS/"
cp legacy/macos/Icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>local.notchling.next</string>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleExecutable</key><string>notchling-app</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key><array/>
</dict>
</plist>
PLIST
codesign --force --sign - "$APP/Contents/MacOS/notchling"
codesign --force --sign - "$APP"
echo "built $APP ($VERSION)"

if [[ "${1:-}" == "--install" ]]; then
  pkill -f "/Applications/$NAME.app/Contents/MacOS/notchling-app" || true
  rm -rf "/Applications/$NAME.app"
  cp -R "$APP" "/Applications/$NAME.app"
  echo "installed /Applications/$NAME.app"
fi
