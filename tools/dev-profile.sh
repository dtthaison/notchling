#!/bin/bash
set -euo pipefail

usage() {
    sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

REAL_USER=$(/usr/bin/id -un)
REAL_HOME=$(/usr/bin/dscl . -read "/Users/$REAL_USER" NFSHomeDirectory 2>/dev/null | /usr/bin/awk '{print $2}' || true)
REAL_HOME=${REAL_HOME:-${HOME:-}}
[[ -n "$REAL_HOME" && "$REAL_HOME" != "/" ]] || { echo "dev-profile: cannot find the real home" >&2; exit 1; }

case "${1:-}" in
    --copy-login) echo "dev-profile: --copy-login is not implemented. The profile starts empty; sign in inside it." >&2; exit 2 ;;
    claude|app|shell) ;;
    *) usage ;;
esac
CMD=$1
shift

PROFILE=${NOTCHLING_DEV_HOME-$REAL_HOME/.notchling-dev/home}
[[ -n "$PROFILE" ]] || { echo "dev-profile: the profile home is empty" >&2; exit 1; }
[[ "$PROFILE" == /* ]] || { echo "dev-profile: NOTCHLING_DEV_HOME must be an absolute path (got '$PROFILE')" >&2; exit 1; }

refuse() { echo "dev-profile: refusing to use '$PROFILE': $1" >&2; exit 1; }
REAL_PHYSICAL=$(cd -P "$REAL_HOME" && pwd)
[[ "$PROFILE" != "/" && "$PROFILE" != "$REAL_HOME" && "$PROFILE" != "$REAL_PHYSICAL" ]] || refuse "it is your real home or the root"

CREATED=0
if [[ ! -d "$PROFILE" ]]; then mkdir -p "$PROFILE"; CREATED=1; fi
PROFILE=$(cd -P "$PROFILE" && pwd)
[[ "$PROFILE" != "/" && "$PROFILE" != "$REAL_HOME" && "$PROFILE" != "$REAL_PHYSICAL" ]] || refuse "it is your real home or the root"
[[ "$REAL_PHYSICAL/" != "$PROFILE/"* ]] || refuse "it contains your real home"
[[ "$PROFILE/" != "$REAL_PHYSICAL/.claude/"* && "$PROFILE/" != "$REAL_PHYSICAL/Library/"* ]] \
    || refuse "it is inside your real ~/.claude or ~/Library"
if [[ $CREATED == 1 ]]; then chmod 700 "$PROFILE"; fi

mkdir -p "$PROFILE/.claude"

find_claude() { PATH="$PATH:$REAL_HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin" command -v claude || true; }

profile_env() {
    exec env -u ANTHROPIC_API_KEY -u ANTHROPIC_AUTH_TOKEN -u CLAUDE_CODE_OAUTH_TOKEN \
        HOME="$PROFILE" CFFIXED_USER_HOME="$PROFILE" ZDOTDIR="$PROFILE" CLAUDE_CONFIG_DIR="$PROFILE/.claude" NOTCHLING_DEV_PROFILE="$PROFILE" "$@"
}

case "$CMD" in
    claude)
        CLAUDE=$(find_claude)
        [[ -n "$CLAUDE" ]] || { echo "dev-profile: claude not found on PATH" >&2; exit 1; }
        profile_env "$CLAUDE" "$@"
        ;;
    app)
        ROOT=$(cd "$(dirname "$0")/.." && pwd)
        BUNDLE="$ROOT/build/Notchling Dev.app"
        [[ -x "$BUNDLE/Contents/MacOS/Notchling" ]] || { echo "dev-profile: no dev build. Run ./build.sh --dev first." >&2; exit 1; }
        ID=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$BUNDLE/Contents/Info.plist" 2>/dev/null || true)
        [[ "$ID" == "local.notchling.dev" ]] || { echo "dev-profile: '$BUNDLE' has bundle id '$ID', not local.notchling.dev. Refusing." >&2; exit 1; }
        CLAUDE=$(find_claude)
        if [[ -n "$CLAUDE" && ! -e "$PROFILE/.local/bin/claude" ]]; then
            mkdir -p "$PROFILE/.local/bin"
            ln -s "$CLAUDE" "$PROFILE/.local/bin/claude"
        fi
        profile_env "$BUNDLE/Contents/MacOS/Notchling" "$@"
        ;;
    shell)
        profile_env "${SHELL:-/bin/zsh}" "$@"
        ;;
esac
