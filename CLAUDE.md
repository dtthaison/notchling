# Notchling: notes for Claude

A macOS menu bar app (Swift, AppKit + SwiftUI) that shows Claude Code agents in the camera notch and keeps the Mac awake. See README.md for what it does from the user's side.

## Build, install, run

- `./build.sh` builds `build/Notchling.app`; `./build.sh --install` replaces `/Applications/Notchling.app` and relaunches it. Always install after a change the user should see; they test on their own screen.
- There is no Xcode project, Package.swift or test target. Everything in `Sources/` is compiled together by `swiftc` (macOS 13 target, arm64 + x86_64).
- Bundle id `local.notchling`; settings live in its UserDefaults. `AppDelegate.migrateFromAwake()` copies keys from the old `local.awake.menubar` domain (the app used to be called Awake). When adding a setting, add its key to that list.

## Checking UI changes

This environment can't take screenshots, and screenshots the user pastes from the thumbnail live in a temp folder that is usually unreadable or already deleted. Ask for them to be saved to the Desktop instead.

To see a view, render it offscreen: write a probe `main.swift` in its own folder under `/tmp`, compile it with every file in `Sources/` except `main.swift` and `AppDelegate.swift`, put `NotchView(model:)` in a borderless window at `NotchController.canvas` size, drive the model (`go(.list)`, `peek(_:)`, `go(.detail(id))`, `requests`, `width`), wait for animations to settle (2–3 s; longer if transcripts are big), then `cacheDisplay` to a PNG and read it. `AgentMonitor().poll()` gives the real agents on this Mac.

NSMenu contents can't be captured as an image (glass is blank), but walking the popped-up menu window's view tree gives exact frames.

## Architecture in one pass

- `AppDelegate` polls every 3 s: `AgentMonitor.poll()` → apply hidden/cleared filters → mark agents with a pending hook request as needs-input → `NotchModel.agents`, snapshots via `TranscriptReader`, peeks on transitions, `AwakeController.apply`.
- `NotchModel.size(for:)` is the single source of truth for the black shape's size per state. `NotchController` keeps a fixed transparent canvas (never resized) centred on the camera and toggles `ignoresMouseEvents` by testing the mouse against that shape, so clicks outside fall through.
- Question and permission cards report their real height through `CardHeightKey` → `model.cardHeight`; don't go back to estimating heights.
- `PetView` keeps its position in a `Walk` object between frames; its path is computed from the shape, not from the clock.
- The hook (`Notchling --hook`) is a separate short-lived process. It talks to the app only through files in `~/Library/Application Support/Notchling/` and must always exit with a valid answer or a pass, never hang Claude Code beyond its 300 s timeout.

## Things that bit before

- macOS 26 adds an icon to menu items with standard actions such as `terminate:`, which shifts every item in that group right. Use our own selectors.
- Transcripts can hold huge base64 image lines. `TranscriptReader` reads only the last 2 MB and skips lines over 100 KB.
- `claude -p` sessions (entrypoint `sdk-cli`) are headless: the hook must ignore them, and they can't be replied to.
- Background job text replies go through `claude attach` in a pty with bracketed paste; question answers are typed as digits, with options.count + 1 as the free-text option.
- Editing `~/.claude/settings.json` is the user's real config. Change only our own hook entries (match on the `--hook` command), keep a backup, and keep their other hooks.

## Style

- Match the surrounding code: short doc comments that explain why, `guard` early, no force-unwraps outside probes.
- User-facing text is short and plain. Menu titles use Title Case.
- The user prefers short replies and "just build it"; ask only when a decision is genuinely theirs (accounts, tokens, anything outward-facing).
