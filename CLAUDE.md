# Notchling: notes for Claude

A macOS menu bar app (Swift, AppKit + SwiftUI) that shows Claude Code agents in the camera notch and keeps the Mac awake. See README.md for what it does from the user's side.

## Build, install, run

- `./build.sh` builds `build/Notchling.app`; `./build.sh --install` replaces `/Applications/Notchling.app` and relaunches it. Always install after a change the user should see; they test on their own screen.
- Bundle id `local.notchling`; settings live in its UserDefaults. `AppDelegate.migrateFromAwake()` copies keys from the old `local.awake.menubar` domain (the app used to be called Awake). When adding a setting, add its key to that list.

## Checking UI changes

This environment can't take screenshots, and screenshots the user pastes from the thumbnail live in a temp folder that is usually unreadable or already deleted. Ask for them to be saved to the Desktop instead. To see a view or menu yourself, use the `render-notch-view` skill.

## Architecture in one pass

- `NotchModel.size(for:)` is the single source of truth for the black shape's size per state. `NotchController` keeps a transparent canvas (screen-wide, resized only when the display changes) centred on the camera and toggles `ignoresMouseEvents` by testing the mouse against that shape, so clicks outside fall through.
- Question and permission cards report their real height through `CardHeightKey` → `model.cardHeight`; don't go back to estimating heights.
- `PetView` keeps its position in a `Walk` object between frames; its path is computed from the shape, not from the clock.
- The hook (`Notchling --hook`) is a separate short-lived process. It talks to the app only through files in `~/Library/Application Support/Notchling/` and must always exit with a valid answer or a pass, never hang Claude Code beyond its 300 s timeout.

## Things that bit before

- macOS 26 adds an icon to menu items with standard actions such as `terminate:`, which shifts every item in that group right. Use our own selectors.
- Transcripts can hold huge base64 image lines. `TranscriptReader` reads only the last 2 MB and skips lines over 100 KB.
- `claude -p` sessions (entrypoint `sdk-cli`) are headless: the hook must ignore them, and they can't be replied to.
- Background job text replies go through `claude attach` in a pty with bracketed paste; question answers are typed as digits, with options.count + 1 as the free-text option.
- Account switching touches the user's real login. Replace only `claudeAiOauth` in the `Claude Code-credentials` item (it also holds MCP logins) and only `oauthAccount` in `~/.claude.json`. Never refresh the token of the active account: refresh tokens rotate, so it would sign Claude Code out. Keychain goes through `/usr/bin/security` (hex on stdin), never the Security framework: an ad hoc signed build loses access on every rebuild.
- Antigravity switching replaces `~/.gemini/jetski-standalone-oauth-token` (shared by Antigravity and Antigravity IDE). Quit the apps before writing it, and only remove it once the Keychain copy reads back equal.
- Antigravity limits come from its local language server, not Google's APIs (those refuse consumer accounts). Port is random (`lsof`), CSRF token is `--csrf_token` in its args, header `x-codeium-csrf-token`, Connect JSON over self-signed HTTPS. Only works while Antigravity runs.
- Editing `~/.claude/settings.json` is the user's real config. Change only our own hook entries (match on the `--hook` command), keep a backup, and keep their other hooks.

## Style

- Match the surrounding code: short doc comments that explain why, `guard` early, no force-unwraps outside probes.
- User-facing text is short and plain. Menu titles use Title Case.
- The user prefers short replies and "just build it"; ask only when a decision is genuinely theirs (accounts, tokens, anything outward-facing).
