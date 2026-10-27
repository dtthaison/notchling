# Contributing to Notchling

Thanks for wanting to help. Notchling is maintained in spare time by one person, so this page is mostly about making your issue or pull request easy to say yes to.

## Before you start

- **Bugs:** open an issue with the bug template. macOS version, Mac model (notch or not), Claude Code version and steps to reproduce make it much faster to fix.
- **Ideas:** open a feature request first, before writing code. Notchling tries to stay small: a notch view of Claude Code agents, keeping the Mac awake, and switching accounts. Things outside that may get a polite no, and that's about scope, not about you.
- **Security problems:** don't open an issue. See [SECURITY.md](SECURITY.md).
- **Small fixes** (typos, an obvious bug with a one-line fix) can go straight to a pull request.

Expect a first reply within about a week.

## Building

You need macOS 13 or later and the Xcode Command Line Tools (`xcode-select --install`). There's no Xcode project.

```sh
./build.sh            # builds build/Notchling.app (arm64 + x86_64, signed ad hoc)
./build.sh --install  # replaces /Applications/Notchling.app and relaunches it
./build.sh --dmg      # also makes build/Notchling-<version>.dmg
```

There's no test suite yet, so check changes by running the app. A notched MacBook is the main target; also check that nothing breaks on a screen without a notch, where the panel hangs under the menu bar.

## Be careful with real logins

Notchling works on your actual Claude Code, Claude Desktop and Antigravity logins. While developing:

- Never refresh the token of the account Claude Code is using; refresh tokens rotate, so it signs Claude Code out.
- In `Claude Code-credentials`, replace only `claudeAiOauth`; it also holds MCP logins. In `~/.claude.json`, replace only `oauthAccount`.
- Keychain access goes through `/usr/bin/security`, not the Security framework, because an ad hoc signed build loses access every time it's rebuilt.
- In `~/.claude/settings.json`, touch only Notchling's own hook entries.
- Never paste real tokens, cookies or `~/.claude.json` contents into issues or pull requests.

[CLAUDE.md](CLAUDE.md) has the architecture in one pass and a list of things that bit before. It's written for Claude Code but reads fine for people too.

## Code style

- Match the surrounding code: short doc comments that say why, `guard` early, no force-unwraps.
- User-facing text is short and plain. Menu titles use Title Case.
- When you add a setting, add its key to `AppDelegate.migrateFromAwake()`.
- Clickable SwiftUI controls get `.pointerCursor()`.

## Pull requests

- Keep each one to a single change, and say in the description what you checked by hand.
- Add a line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md) if users would notice the change.
- Screenshots or a short recording help a lot for anything visual.

By contributing you agree that your work is released under the [MIT License](LICENSE), and to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
