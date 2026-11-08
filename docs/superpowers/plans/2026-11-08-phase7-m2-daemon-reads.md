# Milestone 2: The daemon reads, the app shows

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans.

**Goal:** `notchlingd` runs as this user's service. It serves the session list and change events over the local API, which has tiers, peer identity and an audit log. The desktop app gets its sessions from the daemon and starts the daemon when it is missing. The daemon only reads files and acts on nothing.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md`, "Architecture" (notchlingd, Local API), Parts E16 and E17, Milestone 2. Decisions 0004, 0007 and 0011.

## Global Constraints

- **The daemon takes over no resource.** In this milestone it owns none of the resources in 0011: no hook entries, no bridge ports, no `vault.sock`, no limit watching, no lanes. The Swift app keeps all of them.
- **No writes to real config.** Nothing writes to `~/.claude`, Keychain or `~/Library/LaunchAgents` during tests.
  - Service install always takes a target folder. Tests use temp folders.
  - The real install runs only when a person types `notchling service install`.
- **Socket and its folder.**
  - The socket lives at `<data dir>/notchlingd.sock`, mode 0600. Its folder is 0700, created with `platform::files::ensure_private_dir`.
  - Windows uses the named pipe `\\.\pipe\notchlingd-<user>`. A stub returning `Unsupported` is acceptable in M2 if it type-checks.
  - `NOTCHLING_DATA_ROOT` (the M1 fixture convention) and an explicit `--socket` override the paths for tests and dev.
  - One daemon per data folder (ruled 2026-11-08). The socket's `flock` sits beside the socket. Over the real folders, a daemon whose `--socket` is elsewhere also locks `<data dir>/notchlingd.lock`, so a second daemon can't read the same folders through another socket. Under `NOTCHLING_DATA_ROOT` only the socket's lock is taken: the root is the test's own, and a test socket elsewhere leaves nothing in it.
- **Wire format.** One JSON object per line (UTF-8, `\n`), each line at most 1 MiB. A longer line closes the connection.
  - Request: `{"id": <u64>, "op": "<name>", "args": {...}}`.
  - Reply: `{"id": <u64>, "ok": <result>}` or `{"id": <u64>, "err": {"code": "<code>", "message": "..."}}`.
  - Event, only after `events.subscribe`: `{"event": "<name>", "data": {...}}`.
  - Error codes: `bad_request`, `unknown_op`, `forbidden`, `needs_approval`, `internal`.
  - The first op on a connection must be `hello` with `{"client": "app"|"cli"|"hook", "version": "<semver>", "api": 1}`. Its reply is `{"api": 1, "daemon": "<version>", "tier": "app"|"agent"}`.
- **Tiers** (0007).
  - The peer's executable path comes from `LOCAL_PEERTOKEN` + `proc_pidpath_audittoken` on macOS, `SO_PEERCRED` + `/proc/<pid>/exe` on Linux, and `GetNamedPipeClientProcessId` on Windows.
  - The peer is read at accept and again at `hello` (and before each app-tier request). The app tier needs one process throughout: the same pid and pid version (macOS) or start time and file (Linux). This closes the connect-fork-exec race on macOS; on Linux it is best effort, since an exec before accept can't be seen (0007's note of 2026-11-08). The code signature joins the check with Developer ID in Phase 6.
  - The peer's uid must equal the daemon's uid; otherwise the connection is closed.
  - The app tier needs an executable path in the daemon's allowlist. That is the installed app bundle's binary, or for dev a path in the env `NOTCHLING_APP_TIER_PATHS` read only at daemon start, and only by a daemon running from a cargo target folder. `ensure_daemon` passes no `NOTCHLING_*` variable on. Everything else is the agent tier.
  - A copy of the app elsewhere is the agent tier.
- **M2 ops.**
  - `hello`.
  - `daemon.status` returns version, pid, uptime and attached clients by kind.
  - `sessions.list` returns `engine::sessions` rows as JSON, through serde, with the field names engine uses.
  - `events.subscribe {"topics": ["sessions"]}` sends `sessions.changed` with the full new list, debounced through `platform::watch`.
  - All of these are agent tier. No op in M2 needs the app tier, but the tier check and its tests exist.
  - `approval.*` ops do not exist yet. An op that would need approval returns `needs_approval` when no app-tier client is attached. A test op behind `#[cfg(test)]` proves this.
- **Audit log.** `<data dir>/audit.jsonl`, 0600, one line per privileged call or tier refusal, with time, op, tier, peer path and result. It rotates at 5 MB, keeping one old file. In M2 only refusals are logged.
- **Binary.** One binary, `notchling`, in `crates/notchlingd`. Its subcommands are modes and clients:
  - `notchling daemon` serves.
  - `notchling status` and `notchling sessions [--json]` are clients.
  - `notchling service install|uninstall|print [--dir <folder>]`.
  - Later milestones add `hook`, `secret`, `delegate` and `page`.
- **Service files are pure functions.**
  - macOS: a LaunchAgent plist with label `dev.notchling.daemon`, `KeepAlive` on crash, `RunAtLoad`, and logs in the data dir. `KeepAlive` stays `Crashed` rather than `SuccessfulExit = false` (ruled 2026-11-08, after the review's C2 fix): a stop and "already running" exit 0, a setup error exits non-zero and a restart loop wouldn't fix it, and a daemon a client restarts carries no tier variable, so launchd needn't restart it first.
  - Linux: a systemd user unit.
  - Windows: a Task Scheduler XML file.
  - `install` writes the file into `--dir` (default: the OS's real place) and prints the command that loads it. M2 never runs `launchctl`, `systemctl` or `schtasks` itself.
- **Starting the daemon.** When `hello` fails because the socket is absent, the app and the CLI may spawn `notchling daemon` detached.
  - The spawn is one function in `local-api` (`client::ensure_daemon(binary, socket)`).
  - It waits up to 3 s for the socket.
  - It refuses to spawn a daemon from a path outside the app bundle or cargo target, so tests never start one in the user's real data dir.
- **Deps.** The workspace already allows serde, serde_json and libc. Anything else must pass `cargo deny check`. No async runtime unless it is essential: blocking threads are fine for M2.
- **Checks.** Every crate builds and its tests pass on macOS, and type-checks for `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-msvc`.

## Tasks

1. **E17, local API** (`crates/local-api`): wire types, framing with the 1 MiB cap, server loop (thread per connection), client, peer identity per OS, tiers, audit log, `ensure_daemon`.
   - Tests:
     - framing: an oversize line, garbage, and a partial line;
     - hello required first;
     - tier from a fake allowlist;
     - real peer identity over a socket in a temp dir (the peer is this test process, so its path is the test binary);
     - a uid mismatch can't be faked, so test only the comparison function;
     - `needs_approval` with no app attached;
     - the audit line is written 0600 and rotates.
2. **E16, daemon and command line** (`crates/notchlingd`): the `notchling` binary with the subcommands above.
   - The daemon wires `engine::sessions::Monitor` and `platform::watch` to the ops and events.
   - It shuts down cleanly on SIGTERM and removes the socket.
   - Tests:
     - an end-to-end daemon in a temp data root over the M1 fixture tree, serving `sessions.list` and a `sessions.changed` after a fixture file changes;
     - service file golden tests for all three OSes;
     - `install --dir` writes 0644 into the temp folder and prints the load command.
3. **App on the daemon** (`crates/app`): Chat's sidebar and the notch take their rows from `sessions.list` and `sessions.changed` through the local-api client.
   - If the daemon can't be reached and can't be started, they fall back to reading the files directly, as M1 does, and the sidebar footer says "Daemon not running".
   - Clients check the server before `hello`: the same uid, and on macOS a server file (from its audit token) that `spawn_allowed` accepts. Anything else is `ClientError::UntrustedServer`; the app reads the files and says "Daemon not trusted". Tests that serve from their own binary turn the check off (`ConnectOptions::check_server`); it is on by default.
   - Renders keep using fixtures with no daemon.
   - Tests: the client-backed source against an in-process server, and the fallback.
4. **Docs**: ARCHITECTURE.md, the feature map, AGENTS.md (the `notchling` binary and how to run the daemon in dev), and `docs/dev-profile.md` (running the daemon against the dev profile).

## Review Focus

- A peer that is not this user, or a symlinked or copied app binary, must never get the app tier.
- An oversized, slow or never-ending client line must not stall the daemon or other clients.
- A stale socket file left by a crash must not stop the daemon from starting, and two daemons at once must not happen. Use a lock file with `flock`.
- The app must never block its UI thread on the socket.
- Nothing may touch the real `~/Library/LaunchAgents`, `~/.claude` or Keychain in tests.
