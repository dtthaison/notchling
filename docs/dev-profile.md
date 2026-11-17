# Dev profile

A way to run Notchling's daemon and tools, and `claude`, without touching your real `~/.claude` or `~/Library/Application Support/Notchling`. Read "What is not isolated" before you trust it.

## Use

```
tools/dev-profile.sh claude [args]      # runs claude with the profile as its home and config folder
tools/dev-profile.sh shell [args]       # runs $SHELL inside the profile (shell -c 'cmd' works)
```

The profile is `${NOTCHLING_DEV_HOME:-$HOME/.notchling-dev/home}`. It starts empty: no settings, no hooks, no `claude` login of its own. Sign in to `claude` inside it (`tools/dev-profile.sh claude`). `--copy-login` is not implemented, on purpose: copying a login means copying a refresh token, and refresh tokens rotate.

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

`HOME` alone is not enough. On macOS 26 `NSHomeDirectory()` ignores it; it follows `CFFIXED_USER_HOME`.

The script refuses to run when the profile is empty, not an absolute path, `/`, your real home (also through a symlink), a folder that contains your real home, or anything inside your real `~/.claude` or `~/Library`. It changes the mode of the profile folder only when it created it.

## What is not isolated

- **The login Keychain.** The profile uses the same Keychain as the installed app, and the account code goes through `/usr/bin/security`. Account switching, lanes, the Secret Vault and the Codex and Antigravity saved logins can read, replace or delete real items. Don't use them from the profile.
- **Ports and other processes.** A second app or daemon competes with the installed one for the phone bridge's ports. Account switching quits and restarts real apps and runs `codex app-server daemon restart`, and `claude --bg` jobs are real processes.
- **Launch at Login.** Turning it on registers the dev build as a login item that starts without the profile at the next login. Leave it off.
