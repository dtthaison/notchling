#!/bin/bash
# Runs claude, the dev build of Notchling, or a shell with HOME pointed at a throwaway profile, so nothing a
# dev run writes lands in your real ~/.claude or ~/Library/Application Support/Notchling. Read docs/dev-profile.md
# first: the Keychain and the phone ports are NOT isolated.
#
#   tools/dev-profile.sh claude [args…]   run claude with HOME and CLAUDE_CONFIG_DIR inside the profile
#   tools/dev-profile.sh app              run "build/Notchling Dev.app" (made by ./build.sh --dev), binary called directly
#   tools/dev-profile.sh shell [args…]    run $SHELL inside the profile (args go to it, e.g. shell -c 'echo $HOME')
#
# The profile is ${NOTCHLING_DEV_HOME:-$HOME/.notchling-dev/home} and starts empty. --copy-login is not implemented:
# sign in inside the profile instead. The script refuses to run when the profile would be your real home.
set -euo pipefail

usage() {
    sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

# The real home, from the account database, not from $HOME (which may already be a profile).
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

# `-` not `:-`: a variable set to the empty string is refused, not replaced by the default.
PROFILE=${NOTCHLING_DEV_HOME-$REAL_HOME/.notchling-dev/home}
[[ -n "$PROFILE" ]] || { echo "dev-profile: the profile home is empty" >&2; exit 1; }
[[ "$PROFILE" == /* ]] || { echo "dev-profile: NOTCHLING_DEV_HOME must be an absolute path (got '$PROFILE')" >&2; exit 1; }

refuse() { echo "dev-profile: refusing to use '$PROFILE': $1" >&2; exit 1; }
REAL_PHYSICAL=$(cd -P "$REAL_HOME" && pwd)
# Check the literal path before creating anything, then again on the physical path.
[[ "$PROFILE" != "/" && "$PROFILE" != "$REAL_HOME" && "$PROFILE" != "$REAL_PHYSICAL" ]] || refuse "it is your real home or the root"

CREATED=0
if [[ ! -d "$PROFILE" ]]; then mkdir -p "$PROFILE"; CREATED=1; fi
# The physical path, so a symlink to the real home or a trailing slash can't slip past the checks.
PROFILE=$(cd -P "$PROFILE" && pwd)
[[ "$PROFILE" != "/" && "$PROFILE" != "$REAL_HOME" && "$PROFILE" != "$REAL_PHYSICAL" ]] || refuse "it is your real home or the root"
[[ "$REAL_PHYSICAL/" != "$PROFILE/"* ]] || refuse "it contains your real home"
[[ "$PROFILE/" != "$REAL_PHYSICAL/.claude/"* && "$PROFILE/" != "$REAL_PHYSICAL/Library/"* ]] \
    || refuse "it is inside your real ~/.claude or ~/Library"
# Only a folder this script made gets its mode changed; an existing one is used as it is.
if [[ $CREATED == 1 ]]; then chmod 700 "$PROFILE"; fi

mkdir -p "$PROFILE/.claude"

# Find claude with the real PATH before HOME changes: it usually lives in the real ~/.local/bin.
find_claude() { PATH="$PATH:$REAL_HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin" command -v claude || true; }

# A login in the environment would make the profile use your real account.
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
        # The app looks for claude in $HOME/.local/bin first (ReplySender.claudePath); point that at the real binary.
        CLAUDE=$(find_claude)
        if [[ -n "$CLAUDE" && ! -e "$PROFILE/.local/bin/claude" ]]; then
            mkdir -p "$PROFILE/.local/bin"
            ln -s "$CLAUDE" "$PROFILE/.local/bin/claude"
        fi
        # Called directly: `open` would start it from launchd with the real environment.
        profile_env "$BUNDLE/Contents/MacOS/Notchling" "$@"
        ;;
    shell)
        profile_env "${SHELL:-/bin/zsh}" "$@"
        ;;
esac
