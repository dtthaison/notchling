# Architecture

How Notchling's parts fit, in two layers:

- **Today**: the Swift app for macOS. Everything in that half exists.
- **Target**: a Rust engine running as a service, one GPUI desktop app for macOS, Linux and Windows, and the Flutter phone app. Each part there says whether it exists or is planned, and when.

Why it is this way is in the [decision records](docs/decisions/README.md); changing a decision takes a new record. The plan behind the target is the [design](docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md) (revision 2).

Related: what Notchling is for, [docs/goals.md](docs/goals.md). The rules and their checks, [docs/rules.md](docs/rules.md). The Swift app in detail, [legacy/macos/Sources/AGENTS.md](legacy/macos/Sources/AGENTS.md). Which files make up each feature, [docs/feature-map.md](docs/feature-map.md). The earlier restructuring review of the Swift app, [docs/architecture.md](docs/architecture.md), is kept as history; its phase numbers are its own, not the design's.

## Today: the Swift app

### Processes

```
 Claude Code sessions ── hook event ──▶ Notchling --hook ── files ──┐
        ▲                               (short-lived)               │
        │ claude --bg, attach, stop, rm                             ▼
        └────────────────────────────────────────────────── Notchling.app
                                                             │    ▲    ▲
                                                 hostd.sock  │    │    └── vault.sock ── Notchling --secret
                                                             ▼    │
                                         agent-console-hostd      └── phone app (TLS :47821, tunnel :47822)
                                         (terminal ptys)
```

| Process | What it is | How it talks |
| --- | --- | --- |
| `Notchling.app` | The menu bar app. One process: the notch, Chat, accounts, the vault, pages, search, Machines, keep awake and the phone bridge. | Reads Claude Code's files and runs `claude`, `codex`, `agy`, `ssh` and `/usr/bin/security` itself. |
| The hook | `Notchling --hook`, the same binary, started by Claude Code for each hook event. | **Files only**, in `~/Library/Application Support/Notchling/`: it writes `requests/<id>.json` and waits for `responses/<id>.json`. It passes when the app's `heartbeat` file is older than 15 s. It always exits with an answer or a pass inside Claude Code's 300 s timeout (it waits up to 290 s). |
| `agent-console-hostd` | The session host, shipped in `Contents/Helpers/`. A LaunchAgent, `local.agentconsole.hostd`, installed by `tools/install-hostd.sh` (`./build.sh --install` runs it). | Unix socket `~/Library/Application Support/AgentConsole/hostd.sock`. It owns the ptys of terminal sessions, so restarting or reinstalling the app never ends them. Without the LaunchAgent, the app keeps the ptys itself. |
| Phone app | Flutter, in `apps/flutter/`. Its users build it themselves. | WebSocket with subprotocol `agentconsole.v1`, after pairing: TLS on port 47821, or through the optional Cloudflare tunnel to loopback port 47822, where the Access JWT is checked. |
| Vault command line | `Notchling --secret`, `--secret-request`. | Unix socket `vault.sock` (0600). The app answers only a peer that runs as this user from the app's own executable (`LOCAL_PEERPID` and `proc_pidpath`). The command line never touches Keychain. |
| Other command-line modes | `Notchling --secrets`, `--delegate`, `--page`, `--pages`. | Standalone. They don't need the app running. `--secrets` reads only the vault's list, `vault.json`, which holds no values. |

### Code

| Module | Where | What |
| --- | --- | --- |
| Notchling | `legacy/macos/Sources/Notchling/`, about 41.7k lines | The app, in feature folders. One module, so any file can reach any other; the capability check stands in for module boundaries ([0002](docs/decisions/0002-the-line.md)). |
| ConsoleCore | `legacy/macos/Sources/ConsoleCore/` | A static module that Notchling and hostd import: the phone bridge's engine (`BridgeCore`, `BridgeRouter`), the session host's client and server, and transcript and session readers. It uses no Notchling types and no macOS 14 APIs; the app's minimum is macOS 13. |
| hostd | `legacy/macos/Sources/hostd/` | The session host's entry point, over ConsoleCore. ConsoleCore and hostd are about 7.4k lines together. |
| Tests | `legacy/macos/Tests/NotchlingTests`, `legacy/macos/Tests/ConsoleCoreTests` | Run by `swift test` through `Package.swift`. `build.sh` builds the bundle itself with `swiftc`, for both architectures. |

The feature folders in `legacy/macos/Sources/Notchling/`:

- **App**: launch, menu bar, keep awake, Dock presence, resume after wake.
- **Notch**: the overlay at the camera notch, its rows, cards, Peek and the pet.
- **Agents**: finding sessions (`AgentMonitor`), transcript readers, replies (`ReplySender`), the Codex and Antigravity runners.
- **Hook**: `HookBridge`, the app's end of the hook's files, and installing the hook entries.
- **Chat**: the Chat window, its model and views.
- **Terminal**: the terminal sheet (`claude attach` in a pty and a small emulator).
- **Accounts**, **Providers**, **Limits**, **Lanes**: logins, the provider registry, limit watching and switching, lanes.
- **Phone**: the bridge service, pairing and the sessions the phone sees.
- **Pages**, **Search**: the artifact library and the search panel.
- **Vault**: the Secret Vault, its server and its command line.
- **Delegate**: handing a job to Codex or Antigravity.
- **Machines**: setting up and watching the overnight worker's machines over ssh.
- **DesignKit**: Chat's design kit.
- **System**: file events.

### How the app reaches the tools it shows

- **Claude Code.** The app reads each config folder: `~/.claude` and every lane under `~/.claude-lanes/`. Background jobs are `jobs/<id>/state.json`; transcripts are under `projects/`. It acts by running `claude` with that folder's `CLAUDE_CONFIG_DIR`: `--bg` to start a session, `attach` in a pty to type a reply, `stop` and `rm`. Its hook entries in `~/.claude/settings.json` are the only ones it touches.
- **Codex.** It reads rollouts under `~/.codex/sessions/`. Each turn is one `codex exec --json` process.
- **Antigravity CLI.** It drives `agy acp` (the Agent Client Protocol, JSON-RPC on stdio), one per opened session. Its session list comes from a copy of Antigravity's summaries database, never the original.
- **Machines.** It runs ssh to the worker machines. Checks are read-only; a fix runs only on a click, after a sheet shows its commands.

### Logins and secrets

- Account logins are Keychain items read and written through `/usr/bin/security`, with hex on stdin. Never the Security framework: an ad hoc signed build would lose access on every rebuild.
- The vault's values are one Keychain item written through the Security framework on purpose, so only this build of the app can read it.
- Notchling never renews a token that is in use. Refresh tokens rotate, and renewing would sign the tool out.

### Data on disk

- `~/Library/Application Support/Notchling/`: the hook's `requests/`, `responses/` and `heartbeat`; `lanes.json`; `pages/` with `pages.json`; `machines.json`; `vault.json` (names only, no values) and `vault.sock`; `search-index.json`.
- `~/Library/Application Support/AgentConsole/`: `bridge/` (the certificate, paired phones, tunnel settings, the audit log), `hostd.sock` and `hostd-state.json`. The folder keeps the old Agent Console name so paired phones survive.
- UserDefaults: bundle id `local.notchling`.

### Guard rails

Built in Phase 2; they exist today.

- `tools/guard/zones.json` names the auto-merge zone. `.github/CODEOWNERS` is generated from it ([0002](docs/decisions/0002-the-line.md)).
- `python3 -m guard` runs in `.github/workflows/guard.yml`: the architecture lint with a shrink-only baseline (the gate sends a PR that grows it to the maintainer), actions pinned and allowlisted, CODEOWNERS in step, rulesets matching the workflows.
- `maintainer-gate` (`.github/workflows/maintainer-gate.yml`, `python3 -m guard gate`): when a PR touches a protected path, uses a capability or weakens a test, it passes only with the maintainer's approval of the head commit.
- `.github/rulesets/` and `tools/github/apply-settings.sh` hold the repository settings as code ([0009](docs/decisions/0009-rulesets-as-code.md)).
- CI also runs `swift test`, `./build.sh`, and the phone's `flutter analyze` and `flutter test` (goldens included).

Releases today: `legacy/macos/release.sh <version>` opens a `release/<version>` PR, and merging it tags and publishes. Builds are signed ad hoc. Phase 6 replaces this ([0010](docs/decisions/0010-build-then-sign-beta-then-stable.md)).

## Target

Decided: a Rust engine, GPUI for every desktop app (the Swift app is replaced), Flutter for the phone, and the engine as an always-on service.

**None of the target's Rust tree (`crates/`) exists yet.** The GPUI spike is done ([0001](docs/decisions/0001-gpui-spike.md)). Its results are in `spike/RESULTS.md`; the throwaway code is on branch `spike/gpui-phase1` (and in git history) and never goes into `main`.

```
  GPUI desktop app      hook            command line             phone, and apps on
  (app tier)        (short-lived)    (agent tier: --secret,      other computers
       │                 │            --delegate, --page)        (paired: LAN or tunnel)
       └─────────────────┼──────────────────┘                          │
     local API: Unix socket or named pipe, this user only              │ phone bridge
                         ▼                                             ▼
  ┌──────────────────────────────── notchlingd ──────────────────────────────────┐
  │ replies, session control, phone bridge, vault service, accounts and lanes,   │
  │ limit watching, keep awake, over the engine library (crates/engine)          │
  └─────────────────────────────────────┬────────────────────────────────────────┘
                                        │ same socket contract as hostd.sock
                             ptyhost: terminal ptys (claude attach)

  The desktop app also reads transcripts itself, read-only, with the engine library.
```

### Parts

| Part | What | Status | Record |
| --- | --- | --- | --- |
| `notchlingd` | The engine as a service on every computer: a LaunchAgent, a systemd user service, a Windows startup task. It owns everything that acts or lives long. The app, the phone, the hook and the command line are its clients. | Planned. It serves sessions in Milestone 2 and acts in Milestone 3. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| Local API | A Unix socket or Windows named pipe, this user only. Callers are known by executable path. The app tier may do everything; the agent tier gets only its narrow operations. Account switching, vault values, Machines fixes and settings are app tier only. Privileged calls are audit-logged; an action that needs approval is denied when no UI is attached. | Planned: part E17, Milestone 2. Today's `VaultServer` is the model. | [0007](docs/decisions/0007-local-api-tiers.md) |
| The app reads transcripts | The desktop app reads transcripts itself, read-only, with the engine library, for speed. It never writes or acts except through the daemon. | Planned: Milestone 1, the read-only viewer. Proposed: a designer's call, not settled. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| Hook under a daemon | The daemon answers "pass" at once unless a client that can answer (the app or a phone) is attached. The hook keeps its own deadline and passes on any socket error. `claude -p` (`sdk-cli`) sessions stay ignored. Fixture tests for all three. | Planned: part E4, Milestone 3. Today the hook talks to the app through files. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| `ptyhost` | A separately restartable pty host, so updating the daemon never ends a terminal session. A port of hostd with the same socket contract. | Planned. hostd keeps the job until the Swift app retires. | [0004](docs/decisions/0004-notchlingd-engine-service.md), [0011](docs/decisions/0011-one-owner-per-resource.md) |
| Secrets per OS | macOS: account logins keep going through `/usr/bin/security` with hex on stdin; the vault item uses the Security framework on purpose. Linux and Windows: Secret Service and Credential Manager have no per-app access control, and [docs/goals.md](docs/goals.md) says so plainly. | Planned: parts E8 and E9, Milestone 6. | |
| Remote computers | The desktop app and the phone connect to other computers' daemons the way the phone connects today, with the same pairing. Each session belongs to exactly one computer, which alone acts on it; session ids are host-qualified. Nothing is replicated: accounts, lanes and the vault never leave their computer. | Planned, with the phone work (part P2). A decision record must define identity, transport (LAN, tunnel with Access), reconnect cursors, simultaneous clients and revocation before any milestone uses it. Not written yet. | |
| GPUI app and kit | One GPUI app on a pinned revision (`crates/app`) with our own widget layer (`crates/kit`). Platform pieces behind small traits (`crates/platform`): notch overlay and menu bar on macOS, tray and a top panel on Linux and Windows (the panel is a designer's call). | Planned: parts U0 to U11, from Milestone 1. Linux and Windows show sessions only until `claude attach` is proven there. | [0005](docs/decisions/0005-gpui-desktop-app.md), [0001](docs/decisions/0001-gpui-spike.md) |
| Web content | Pages, artifacts and HTML files open in their own `wry` window, not inside a GPUI view, because GPUI has no web view. | Planned: part U4, Milestone 5. | [0005](docs/decisions/0005-gpui-desktop-app.md) |
| Protocol | `protocol/`: a written spec in JSON Schema, conformance and behavior fixtures, version negotiation, additive only. Dart and Rust types are generated from it. | Planned: Phase 4. Today `agentconsole.v1` is written by hand twice, in `legacy/macos/Sources/ConsoleCore/Bridge/BridgeProtocol.swift` and `apps/flutter/lib/core/protocol/models.dart`. | [0006](docs/decisions/0006-flutter-phone-generated-types.md) (Proposed) |
| Licenses | Only MIT, Apache and BSD crates, enforced by `cargo-deny`. Our own terminal and markdown views on permissive crates. | Planned, with the Cargo workspace in Phase 4. | [0008](docs/decisions/0008-mit-permissive-crates.md) |
| Releases | Beta daily, build then sign apart, Stable by one click, one signed update feed. | Planned: Phase 6. | [0010](docs/decisions/0010-build-then-sign-beta-then-stable.md) |

Machines stays a separate tool, not inside the daemon, and is ported last (part E10). That is a designer's call, not settled.

### Repo layout

| Folder | What | Today |
| --- | --- | --- |
| `crates/engine/` | Transcripts, sessions, accounts, lanes, vault, bridge (a library) | Planned |
| `crates/notchlingd/` | The engine service | Planned |
| `crates/ptyhost/` | The separately restartable terminal host (port of hostd) | Planned |
| `crates/local-api/` | App and command line to daemon API, with tiers | Planned |
| `crates/app/` | The GPUI desktop app | Planned |
| `crates/platform/` | Notch and menu bar (macOS), tray and panel (Linux, Windows) | Planned |
| `crates/kit/` | Our widget layer over GPUI | Planned |
| `apps/flutter/` | The phone app | Exists (moved here from the old top-level phone folder in Phase 4) |
| `legacy/macos/` | Today's Swift app and ConsoleCore, fixes only, until retired | Exists (moved from the repo root in Phase 4): `legacy/macos/Sources/`, `Tests/`, `Demo/`, `Icon/`, `Pet/`, `build.sh`, `release.sh`. `Package.swift` and a one-line `build.sh` wrapper stay at the root |
| `protocol/` | Spec, JSON Schema, fixtures | Planned: Phase 4 |
| `docs/`, `tools/`, `viz/` | Docs; the guard and setup tools; the web page behind the phone's Visualize tab | Exist |
| `spike/` | The GPUI spike's results (`RESULTS.md`); its code stays on branch `spike/gpui-phase1` | Exists |

### Swift and Rust side by side

Both run on the maintainer's Mac during Milestones 1 to 5. Some resources can have only one owner: the hook entries, hostd and its socket, the phone bridge's ports and certificate, `vault.sock` and the vault item, limit watching and auto-switch, lanes and account switching. Each has one owner per milestone, and the Swift app reads a stand-down setting per area. The table of who owns what, until when, is in [0011](docs/decisions/0011-one-owner-per-resource.md).

Today the Swift app owns all of them.

### Order of work

1. Phase 1, the GPUI spike: done ([0001](docs/decisions/0001-gpui-spike.md)).
2. Phase 2, guard rails and quality: merged. The maintainer applies the repository settings with `tools/github/apply-settings.sh`.
3. Phase 3, the operating manual: this document, the decision records, goals, rules, governance and policies.
4. Phase 4: the repo layout above, the Cargo workspace, the protocol spec and generated types.
5. Phase 5: agents on (the GitHub App, a separate agent user and push wrapper on workers, triage, the worker, the review agent).
6. Phase 6: releases (notarization, build and sign apart, the Beta feed, Stable by one click).
7. Phase 7: a recording phase that captures terminal byte fixtures for every reply behavior while the Swift app can still produce them, then Milestones 1 to 6:
   1. read-only viewer;
   2. the daemon reads, the app shows;
   3. the daemon acts;
   4. the phone;
   5. the rest that's safe;
   6. logins and secrets, then the Swift app retires.
8. Phase 8: retire the Swift app in one release.

## Decision records

- [0001](docs/decisions/0001-gpui-spike.md) GPUI spike: the rebuild goes ahead.
- [0002](docs/decisions/0002-the-line.md) The line: named safe areas, `maintainer-gate`.
- [0003](docs/decisions/0003-agent-instructions-in-agents-md.md) Agent instructions in `AGENTS.md`.
- [0004](docs/decisions/0004-notchlingd-engine-service.md) The Rust engine runs as the `notchlingd` service.
- [0005](docs/decisions/0005-gpui-desktop-app.md) GPUI for every desktop app.
- [0006](docs/decisions/0006-flutter-phone-generated-types.md) Flutter phone, generated protocol types, no Rust (Proposed).
- [0007](docs/decisions/0007-local-api-tiers.md) Local API tiers and peer identity.
- [0008](docs/decisions/0008-mit-permissive-crates.md) Stay MIT, permissive crates only.
- [0009](docs/decisions/0009-rulesets-as-code.md) Rulesets as code, applied by the maintainer.
- [0010](docs/decisions/0010-build-then-sign-beta-then-stable.md) Build, then sign apart; Beta, then Stable.
- [0011](docs/decisions/0011-one-owner-per-resource.md) One owner per resource while Swift and Rust run side by side.
