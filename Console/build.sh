#!/bin/bash
# Builds "Agent Console.app" into Console/build. Separate from the repo-root build.sh, which builds Notchling.
# Pass --install to also replace /Applications/Agent Console.app (the old one is kept in Console/build/backup/).
# --install-only installs the app already built in Console/build without building again (used by tools/migrate-and-install.sh).
#
# The session host (agent-console-hostd) is built into Contents/Helpers and installed as a LaunchAgent. Installing
# replaces the helper file but only restarts the running host when no session is running in it (see
# tools/install-hostd.sh); Agent Console itself can be replaced at any time without ending sessions.
set -euo pipefail
cd "$(dirname "$0")"

if [[ "${1:-}" != "--install-only" ]]; then
swift build -c release --package-path .
BIN_DIR="$(swift build -c release --package-path . --show-bin-path)"
BIN="$BIN_DIR/AgentConsole"
HOSTD="$BIN_DIR/agent-console-hostd"

APP="build/Agent Console.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/AgentConsole"
cp Info.plist "$APP/Contents/Info.plist"
mkdir -p "$APP/Contents/Helpers"
cp "$HOSTD" "$APP/Contents/Helpers/agent-console-hostd"
# Geist and Geist Mono (OFL) are registered at launch from Contents/Resources/Fonts.
mkdir -p "$APP/Contents/Resources/Fonts"
cp Sources/AgentConsole/Resources/Fonts/* "$APP/Contents/Resources/Fonts/"
# Same icon as Notchling; the root build.sh makes Icon/AppIcon.icns.
cp ../Icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
# The shared viz bundle (HTML, JS, vendored mermaid and Chart.js) loads from Contents/Resources/viz.
rm -rf "$APP/Contents/Resources/viz"
cp -R ../viz "$APP/Contents/Resources/viz"
codesign --force --sign - "$APP/Contents/Helpers/agent-console-hostd"
codesign --force --sign - "$APP"

echo "Built Console/$APP"
fi
APP="build/Agent Console.app"

if [[ "${1:-}" == "--install" || "${1:-}" == "--install-only" ]]; then
    DEST="/Applications/Agent Console.app"
    WAS_RUNNING=0
    if pgrep -x AgentConsole >/dev/null; then WAS_RUNNING=1; pkill -x AgentConsole || true; sleep 1; fi
    if [[ -d "$DEST" ]]; then
        mkdir -p build/backup
        cp -R "$DEST" "build/backup/Agent Console-$(date +%Y%m%d-%H%M%S).app"
    fi
    rm -rf "$DEST"
    cp -R "$APP" /Applications/
    ./tools/install-hostd.sh "$DEST"
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST"
    if [[ "$WAS_RUNNING" == "1" ]]; then open "$DEST"; fi
    echo "Installed to $DEST"
fi
