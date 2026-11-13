#!/bin/bash
set -euo pipefail

APP="${1:?usage: install-hostd.sh <path to Notchling.app>}"
HELPER="${HOSTD_HELPER:-$APP/Contents/Helpers/agent-console-hostd}"
LAUNCHCTL="${LAUNCHCTL:-/bin/launchctl}"
LABEL="local.agentconsole.hostd"
PLIST_DIR="${PLIST_DIR:-$HOME/Library/LaunchAgents}"
PLIST="$PLIST_DIR/$LABEL.plist"
DOMAIN="gui/$(id -u)"
SOCKET_ARGS=()
[[ -n "${HOSTD_SOCKET:-}" ]] && SOCKET_ARGS=(--socket "$HOSTD_SOCKET")

[[ -x "$HELPER" ]] || { echo "install-hostd: helper not found at $HELPER" >&2; exit 1; }

mkdir -p "$PLIST_DIR" "$HOME/Library/Logs"
cat > "$PLIST.new" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>$LABEL</string>
    <key>ProgramArguments</key><array><string>$HELPER</string></array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>ProcessType</key><string>Interactive</string>
    <key>StandardOutPath</key><string>$HOME/Library/Logs/agent-console-hostd.log</string>
    <key>StandardErrorPath</key><string>$HOME/Library/Logs/agent-console-hostd.log</string>
</dict>
</plist>
PLIST
plutil -lint "$PLIST.new" >/dev/null
mv "$PLIST.new" "$PLIST"

if ! "$LAUNCHCTL" print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
  "$LAUNCHCTL" bootstrap "$DOMAIN" "$PLIST"
  echo "install-hostd: loaded $LABEL (the host is starting)"
  exit 0
fi

LOADED_PATH="$("$LAUNCHCTL" print "$DOMAIN/$LABEL" 2>/dev/null | sed -n 's/^[[:space:]]*program = //p' | head -1)"
STATUS="$("$HELPER" --status ${SOCKET_ARGS[@]+"${SOCKET_ARGS[@]}"} 2>/dev/null || echo '{"running":0}')"
RUNNING="$(printf '%s' "$STATUS" | sed -n 's/.*"running":\([0-9]*\).*/\1/p')"
RUNNING="${RUNNING:-0}"
if [[ -n "$LOADED_PATH" && "$LOADED_PATH" != "$HELPER" ]]; then
  if [[ "$RUNNING" -eq 0 ]]; then
    "$LAUNCHCTL" bootout "$DOMAIN/$LABEL" || true
    "$LAUNCHCTL" bootstrap "$DOMAIN" "$PLIST"
    echo "install-hostd: nothing running; moved the host from $LOADED_PATH to the new helper"
  else
    echo "install-hostd: $RUNNING session(s) run in a host started from $LOADED_PATH; left alone. Run tools/migrate-and-install.sh to move them." >&2
  fi
elif [[ "$RUNNING" -eq 0 ]]; then
  "$LAUNCHCTL" kickstart -k "$DOMAIN/$LABEL"
  echo "install-hostd: nothing running; restarted the host onto the new helper"
else
  echo "install-hostd: $RUNNING session(s) running in the host; keeping the old host. The new helper takes over at the next idle moment."
fi
