# Dev profile

A way to run a dev build of Notchling, and `claude`, without touching your real `~/.claude` or `~/Library/Application Support/Notchling`. Read "What is not isolated" before you trust it.

## Use

```
./build.sh --dev                        # builds build/Notchling Dev.app (bundle id local.notchling.dev)
tools/dev-profile.sh app                # runs that build, with the profile as its home
tools/dev-profile.sh claude [args]      # runs claude with the profile as its home and config folder
tools/dev-profile.sh shell [args]       # runs $SHELL inside the profile (shell -c 'cmd' works)
```

The profile is `${NOTCHLING_DEV_HOME:-$HOME/.notchling-dev/home}`. It starts empty: no settings, no hooks, no `claude` login of its own. The app still sees your real Claude Code login; see "What is not isolated". Sign in to `claude` inside it (`tools/dev-profile.sh claude`). `--copy-login` is not implemented, on purpose: copying a login means copying a refresh token, and refresh tokens rotate.

`./build.sh --dev` takes no other flag and is never installed. It changes only the built copy of `legacy/macos/Info.plist`: bundle id `local.notchling.dev`, name "Notchling Dev", and it drops the `agentconsole://` URL scheme so it doesn't compete with the installed app. Without `--dev`, `build.sh` does what it always did.

## The daemon against the profile

The Rust daemon (`notchling daemon`, `crates/notchlingd`) takes its folders from `HOME` (`platform::paths`), so inside the profile it reads the profile's `.claude` and puts `notchlingd.sock`, `notchlingd.lock` and `audit.jsonl` in the profile's `Library/Application Support/Notchling`:

```
cargo build -p notchling-notchlingd
tools/dev-profile.sh shell -c 'target/debug/notchling daemon'      # serves until Ctrl-C (or SIGTERM), then removes its socket
tools/dev-profile.sh shell -c 'target/debug/notchling sessions'    # from another terminal
```

Use a relative `target/debug/notchling` from the repository root or an absolute path (`CARGO_TARGET_DIR` if you set one). Two other ways keep it off your real folders without the profile: `NOTCHLING_DATA_ROOT=<dir>` reads `<dir>/claude` and `<dir>/notchling` (copy a fixture there first, such as `crates/app/fixtures/notch`, since the daemon writes its socket and lock into `<dir>/notchling`), and `--socket <path>` puts the socket, lock and audit log in that path's folder (the sessions are still read from `HOME`, read-only). The daemon only reads unless its `acting` switch is on. It is off at every start, whatever any file says (an old `notchlingd-settings.json` saying on is ignored and audit-logged), and only an app-tier `settings.set_acting` during that run turns it on. The app never sends it from a file: its own `settings.json` (`"actingOn": true`) only decides whether, after a daemon restart, it shows "Notchling restarted: … are off" with Turn Back On, and only that click (then its confirmation) sends it. Acting is turned on that way or from the handover dialog: it touches no hook, Keychain item, port or LaunchAgent. Don't run `notchling service install` without `--dir` inside the profile: its default folder is `$HOME/Library/LaunchAgents`, which is the profile's, but loading it with `launchctl` would start the daemon at your next login without the profile.

## How it works

The script sets `HOME`, `CFFIXED_USER_HOME`, `ZDOTDIR` and `CLAUDE_CONFIG_DIR` to the profile and clears `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` and `CLAUDE_CODE_OAUTH_TOKEN`.

`HOME` alone is not enough. On macOS 26 `NSHomeDirectory()` and `FileManager.default.homeDirectoryForCurrentUser` ignore it; they follow `CFFIXED_USER_HOME`. Checked with a small probe: with only `HOME` set both returned the real home, with both set they returned the profile.

`app` runs `build/Notchling Dev.app/Contents/MacOS/Notchling` directly, and only if that bundle's id is `local.notchling.dev`. Never start the dev build with `open` or by double-clicking: it then runs with your real home and writes to your real folders.

The script refuses to run when the profile is empty, not an absolute path, `/`, your real home (also through a symlink), a folder that contains your real home, or anything inside your real `~/.claude` or `~/Library`. It changes the mode of the profile folder only when it created it.

## What is isolated

Everything the app builds from the home folder, because the data folder name is written into the code and not taken from the bundle id (see "Hard-coded names"):

- `~/.claude`: hooks in `settings.json`, the `notchling-*` skills, sessions, transcripts, `~/.claude-lanes`.
- `~/Library/Application Support/Notchling` (hook files, `pages.json`, `vault.json`, `vault.sock`, search index, Machines) and `~/Library/Application Support/AgentConsole` (phone certificate, tokens, `hostd.sock`).
- `~/.codex`, `~/.gemini/antigravity-cli`, the Claude Desktop folders: they are looked up under the profile too, so they are empty there.

## What is not isolated

- **The login Keychain.** The dev build uses the same Keychain as the installed app, and the account code goes through `/usr/bin/security`. Account switching, lanes, the Secret Vault and the Codex and Antigravity saved logins can read, replace or delete real items. Don't use them in a dev build. We did not test whether `security` finds your keychain with a different `HOME`; don't count on either answer. `claude` with `CLAUDE_CONFIG_DIR` set keeps its login in a separate item (`Claude Code-credentials-<hash of the path>`), but that item still lands in your real keychain.
- **Your real Claude Code login, on launch.** The live login item is the fixed `Claude Code-credentials` (`AccountStore.liveService`). `capture()` runs at launch and every 60 s, reads that real login, asks `/api/oauth/profile` with it and writes it into the saved-login Keychain service the installed app uses too.
- **Jobs on the main folder drop `CLAUDE_CONFIG_DIR`.** `AgentMonitor.environment(configDir: nil)` removes it, so `claude attach`, `rm`, `stop` and moves the dev app starts for such jobs run with the profile's `HOME` but the real, unhashed login item. Keeping an inherited `CLAUDE_CONFIG_DIR` as the main folder is a protected code change, not made yet.
- **Launch at Login.** Turning it on registers `build/Notchling Dev.app` as a login item. At the next login it starts without the profile and writes its hooks into your real `~/.claude/settings.json`. Leave it off in a dev build.
- **Ports 47821 and 47822.** The phone bridge binds them at launch. If the installed app runs, the dev build can't, and if the dev build wins, your phone may pair with it. Turn the phone bridge off in the dev build.
- **UserDefaults.** The dev bundle id gives it its own settings domain, but the plist still sits in your real `~/Library/Preferences` (checked: a defaults write from inside the profile landed there). On first launch `migrateFromAwake` can copy keys from the old `local.awake.menubar` domain into it.
- **Other apps and processes.** Account switching quits and restarts real apps (ChatGPT/Codex, Antigravity) and runs `codex app-server daemon restart`. `claude --bg` jobs are real processes. The global Search hot key (⌃⌥P unless changed in Settings) and the notch are shared with the installed app. Notification permission is per bundle id, so the dev build asks again.
- **The session host.** `agent-console-hostd` is a per-user LaunchAgent made by `tools/install-hostd.sh`. The profile has none, so terminal sessions that live in the host are not available in it, and nothing here should install one.

## Hard-coded names (found by grep, not changed)

- `Library/Application Support/Notchling` and `…/AgentConsole` are literal strings joined to `FileManager.default.homeDirectoryForCurrentUser` in about 65 places (for example `HookBridge`, `SecretVault`, `VaultServer`, `SessionIndex`, `BridgeIdentity`, `hostd`). The bundle id plays no part, so a dev build run without the script writes to the real folders.
- `.claude`, `.codex`, `.gemini`, `.local/bin` are joined to the same home value. `ReplySender.claudePath` looks for `$HOME/.local/bin/claude`, then `/usr/local/bin/claude`; `dev-profile.sh app` links the real `claude` into the profile for that.
- `local.notchling` appears only as dispatch queue labels and one notification name. `local.awake.menubar` is read by `migrateFromAwake`. The app's own bundle id is never compared in code, checked for `nil` (`Bundle.main.bundleIdentifier != nil`) before notifications.
- Hooks and skills use `Bundle.main.bundlePath`, so inside the profile they point at `build/Notchling Dev.app`, with a space in the path.
