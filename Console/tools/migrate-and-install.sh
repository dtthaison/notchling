#!/bin/bash
# One-time move to the session host, for sessions that are alive inside a running (old) Agent Console right now.
#
#   Console/tools/migrate-and-install.sh
#
# WARNING: this ends the Claude sessions running inside the old Agent Console (including a session you are typing
# in if it was opened from there) and brings them back through the host with `claude --resume`. Run it from a
# normal terminal, not from inside Agent Console.
#
# 1. builds the new app + helper (the old app keeps running meanwhile);
# 2. lists the claude processes that are children of the running AgentConsole, finds each one's session id and
#    folder in ~/.claude/sessions/<pid>.json, and records them in the host's state as "resume on start";
# 3. installs the helper and the app (the old app is stopped here) and loads the LaunchAgent, so the host starts and
#    resumes the recorded sessions;
# 4. relaunches Agent Console.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== building"
./build.sh
HELPER="build/Agent Console.app/Contents/Helpers/agent-console-hostd"
[[ -x "$HELPER" ]] || { echo "helper missing after build" >&2; exit 1; }

echo "== finding sessions inside the running Agent Console"
APPPIDS="$(pgrep -x AgentConsole || true)"
SESSIONS_DIR="$HOME/.claude/sessions"
FOUND=0
descendants() {  # pid -> itself and all its descendants
  local pid="$1"
  echo "$pid"
  for child in $(pgrep -P "$pid" 2>/dev/null || true); do descendants "$child"; done
}
for app in $APPPIDS; do
  for child in $(pgrep -P "$app" 2>/dev/null || true); do
    PIDS="$(descendants "$child")"
    for pid in $PIDS; do
      FILE="$SESSIONS_DIR/$pid.json"
      [[ -f "$FILE" ]] || continue
      SESSION_ID="$(/usr/bin/plutil -extract sessionId raw -o - "$FILE" 2>/dev/null || true)"
      CWD="$(/usr/bin/plutil -extract cwd raw -o - "$FILE" 2>/dev/null || true)"
      if [[ -n "$SESSION_ID" && -n "$CWD" ]]; then
        echo "  session $SESSION_ID in $CWD (pid $pid)"
        "$HELPER" --record-resume "$SESSION_ID" "$CWD"
        FOUND=$((FOUND + 1))
        break
      fi
    done
  done
done
echo "  $FOUND session(s) recorded"

echo "== installing (the old Agent Console stops here)"
./build.sh --install-only
echo "== done: the host resumes the recorded sessions and Agent Console is relaunched"
