#!/bin/bash
# One-time move from "Agent Console.app + Notchling.app" to Notchling alone. Written to be run by hand; the
# build alone never runs it.
#
#   tools/migrate-and-install.sh
#
# WARNING: this ends the Claude sessions that run under the old Agent Console (inside its session host, or as
# children of the app itself) and brings them back through the new host with `claude --resume`. A session you are
# typing in from Agent Console is one of them. Run it from a normal terminal, not from inside Agent Console or Notchling.
#
# 1. builds Notchling (the old apps keep running meanwhile);
# 2. lists the claude sessions that are children of a running AgentConsole app and reads each one's session id and
#    folder from ~/.claude/sessions/<pid>.json (sessions inside the old host need no recording: the host keeps its
#    own records and resumes them on its next start);
# 3. quits Agent Console and Notchling, and stops the old session host (SIGTERM: its records stay);
# 4. records the sessions of step 2 for resume, installs Notchling into /Applications and loads the LaunchAgent
#    local.agentconsole.hostd onto the helper inside Notchling.app; the new host resumes every recorded session;
# 5. moves /Applications/Agent Console.app to the Trash and relaunches Notchling.
#
# The phone stays paired: Notchling uses the same ~/Library/Application Support/AgentConsole folder (certificate,
# paired devices, tunnel settings) and the same ports 47821 and 47822.
set -euo pipefail
cd "$(dirname "$0")/.."

LABEL="local.agentconsole.hostd"
DOMAIN="gui/$(id -u)"
OLD_APP="/Applications/Agent Console.app"
NEW_APP="/Applications/Notchling.app"

echo "== building Notchling"
./build.sh
HELPER="build/Notchling.app/Contents/Helpers/agent-console-hostd"
[[ -x "$HELPER" ]] || { echo "helper missing after build" >&2; exit 1; }

echo "== finding sessions that are children of the running Agent Console"
SESSIONS_DIR="$HOME/.claude/sessions"
RECORDS=()
descendants() {  # pid -> itself and all its descendants
  local pid="$1"
  echo "$pid"
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do descendants "$child"; done
}
for app in $(pgrep -x AgentConsole || true); do
  for child in $(pgrep -P "$app" 2>/dev/null || true); do
    for pid in $(descendants "$child"); do
      FILE="$SESSIONS_DIR/$pid.json"
      [[ -f "$FILE" ]] || continue
      SESSION_ID="$(/usr/bin/plutil -extract sessionId raw -o - "$FILE" 2>/dev/null || true)"
      CWD="$(/usr/bin/plutil -extract cwd raw -o - "$FILE" 2>/dev/null || true)"
      if [[ -n "$SESSION_ID" && -n "$CWD" ]]; then
        echo "  session $SESSION_ID in $CWD (pid $pid)"
        RECORDS+=("$SESSION_ID"$'\t'"$CWD")
        break
      fi
    done
  done
done
echo "  ${#RECORDS[@]} session(s) to record"

echo "== quitting Agent Console and Notchling, stopping the old session host"
pkill -x AgentConsole 2>/dev/null || true
pkill -x Notchling 2>/dev/null || true
sleep 1
if launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; then
  launchctl bootout "$DOMAIN/$LABEL" || true
  sleep 1
fi

echo "== recording sessions for resume"
for record in ${RECORDS[@]+"${RECORDS[@]}"}; do
  "$HELPER" --record-resume "${record%%$'\t'*}" "${record#*$'\t'}"
done

echo "== installing Notchling"
rm -rf "$NEW_APP"
cp -R build/Notchling.app /Applications/
tools/install-hostd.sh "$NEW_APP"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$NEW_APP"

if [[ -d "$OLD_APP" ]]; then
  echo "== moving Agent Console.app to the Trash"
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -u "$OLD_APP" || true
  mv "$OLD_APP" "$HOME/.Trash/Agent Console-$(date +%Y%m%d-%H%M%S).app"
fi

open "$NEW_APP"
echo "== done: Notchling is the only Mac app; the host resumes the recorded sessions"
echo "   check: launchctl print $DOMAIN/$LABEL | grep program; ~/.../agent-console-hostd --status"
