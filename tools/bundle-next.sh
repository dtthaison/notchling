#!/bin/zsh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

INSTALL=0
NODES=0
for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    --nodes) NODES=1 ;;
  esac
done

IDENTITY="${NOTCHLING_SIGN_IDENTITY:--}"
KEYCHAIN=()
if [[ -n "${NOTCHLING_SIGN_KEYCHAIN:-}" ]]; then
  KEYCHAIN=(--keychain "$NOTCHLING_SIGN_KEYCHAIN")
fi
if [[ "$IDENTITY" != "-" ]]; then
  IDENTITIES="$(security find-identity -v -p codesigning ${NOTCHLING_SIGN_KEYCHAIN:-})"
  if [[ "$IDENTITIES" != *"$IDENTITY"* ]]; then
    echo "no code signing identity matching \"$IDENTITY\" (security find-identity -v -p codesigning lists them)" >&2
    exit 1
  fi
fi

NOTCHLING_BUILD="$(git rev-parse --short=12 HEAD)$(git diff --quiet HEAD || echo -dirty)"
export NOTCHLING_BUILD
cargo build --release --locked -p notchling-app -p notchling-notchlingd -p notchling-ptyhost -p notchling-embed
TARGET="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')/release"
NAME="Notchling"
APP="${NOTCHLING_BUNDLE_OUT:-build/next}/$NAME.app"
VERSION=$(cargo pkgid -p notchling-app | sed 's/.*[#@]//')

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Helpers"
cp "$TARGET/notchling-app" "$TARGET/notchling" "$APP/Contents/MacOS/"
cp "$TARGET/notchling-ptyhost" "$TARGET/notchling-embed" "$APP/Contents/Helpers/"
cp assets/brand/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
python3 tools/gen_sound.py "$APP/Contents/Resources/stopped.aiff"
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
sign() {
  codesign --force --timestamp=none "${KEYCHAIN[@]}" --sign "$IDENTITY" "$@"
}
sign --identifier dev.notchling.daemon "$APP/Contents/MacOS/notchling"
sign --identifier dev.notchling.ptyhost "$APP/Contents/Helpers/notchling-ptyhost"
sign --identifier dev.notchling.embed "$APP/Contents/Helpers/notchling-embed"
sign "$APP"
echo "built $APP ($VERSION)"
if [[ "$IDENTITY" == "-" ]]; then
  echo "signed ad hoc: the Keychain asks again for Notchling's items after this build is installed (docs/local-signing.md)"
else
  TEAM="$(codesign -dv "$APP/Contents/MacOS/notchling" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
  echo "signed with \"$IDENTITY\", team ${TEAM:-not set}"
  APPLE_ISSUED='anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] and certificate leaf[field.1.2.840.113635.100.6.1.13] or anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.1] and certificate leaf[field.1.2.840.113635.100.6.1.12] or anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.1] and certificate leaf[field.1.2.840.113635.100.6.1.7]'
  if ! codesign --verify -R="$APPLE_ISSUED" "$APP/Contents/MacOS/notchling" 2>/dev/null; then
    echo "warning: not an Apple-issued development or Developer ID certificate, so the Keychain still tells builds apart by hash and asks again after each install (docs/local-signing.md)" >&2
  fi
fi

if [[ $NODES -eq 1 ]]; then
  tools/build-node.sh
fi

if [[ $INSTALL -eq 1 ]]; then
  if grep -q "<string>local.notchling</string>" "/Applications/$NAME.app/Contents/Info.plist" 2>/dev/null; then
    echo "/Applications/$NAME.app is still the Swift app; retire it first (it is never replaced here)" >&2
    exit 1
  fi
  pkill -f "/Applications/$NAME.app/Contents/MacOS/notchling-app" || true
  rm -rf "/Applications/$NAME.app"
  cp -R "$APP" "/Applications/$NAME.app"
  echo "installed /Applications/$NAME.app"
fi
