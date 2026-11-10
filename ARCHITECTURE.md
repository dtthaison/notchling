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
- `python3 -m guard` runs in `.github/workflows/guard.yml`: the architecture lint with a shrink-only baseline (the gate sends a PR that grows it to the maintainer), actions pinned and allowlisted, CODEOWNERS in step, rulesets matching the workflows. The same job checks that the generated protocol types match the schema (`tools/protocol/gen.py --check`, `GUARD-PROTOCOL-GEN`) and runs `tools/protocol/tests/`.
- `maintainer-gate` (`.github/workflows/maintainer-gate.yml`, `python3 -m guard gate`): when a PR touches a protected path, uses a capability or weakens a test, it passes only with the maintainer's approval of the head commit.
- `.github/rulesets/` and `tools/github/apply-settings.sh` hold the repository settings as code ([0009](docs/decisions/0009-rulesets-as-code.md)).
- CI also runs `swift test`, `./build.sh`, and the phone's `flutter analyze` and `flutter test` (goldens included).
- `.github/workflows/rust.yml` checks the Cargo workspace (since Phase 4): `rust-test` (`cargo test` on macOS, Linux and Windows), `rust-clippy` (`cargo fmt --all --check`, then clippy with warnings as errors) and `rust-deny` (`cargo deny check`: licenses, advisories, sources).

### Releases

Releases today: `legacy/macos/release.sh <version>` opens a `release/<version>` PR, and merging it tags and publishes (`.github/workflows/release.yml`). Builds are signed ad hoc.

Phase 6's pipeline replaces this ([0010](docs/decisions/0010-build-then-sign-beta-then-stable.md)). It is built and **off**: every job stops at its first step until the repository variable `NOTCHLING_RELEASES` is `on`, and the signing secrets and the update key don't exist yet. Record 0010 says "None of this exists yet"; it was written before the build, and records aren't edited, so this section is where the current state is kept.

| Workflow | Job | What it does | Code it runs | What it holds |
| --- | --- | --- | --- | --- |
| `beta.yml` (daily, or a click) | `gate` | Kill switch, the next tag | The default branch's | A read-only token |
| | `build` | `./build.sh`, the version into the built copy, the unsigned zip and its SHA-256 | The gated commit's | A read-only token, no secrets |
| | `sign` | Checks the hash; Developer ID with the hardened runtime and inline entitlements (Apple Events); notarize, staple; `sign_update`; attests | None: no checkout | The signing secrets (the only job in the `release` environment), no repository write |
| | `publish` | The prerelease `vX.Y.Z-beta.N`, the feed item | The default branch's `appcast.py` | A write token, no secrets, no environment |
| `promote.yml` (a maintainer's click) | `authorize` | Kill switch, `MAINTAINERS`, the Stable tag, the notes | The default branch's | A read-only token |
| | `release` | The Beta's files, same SHA-256 and attestations (bound to the default branch and the Beta's commit), published as `vX.Y.Z` | None: no checkout | A write token, no environment |
| | `appcast` | Checks the item is this Beta's, then it loses its `beta` channel and gets the Stable notes link, a fresh `pubDate` and a one-day phased rollout | The default branch's `appcast.py` | A write token, no environment |
| | `fold` | A pull request folding the fragments present at the Beta's commit into `CHANGELOG.md` | The default branch's `fragments.py` | Contents and pull requests write |

- `tools/release/` (Python standard library, tested in `tools/release/tests/`, which the `guard` job runs): `version.py` picks the next tag from the `v*` tags and the fragments' kinds and writes it into the built app's `Info.plist` copy, never the source; `fragments.py` reads `changes/` (one fragment per pull request that changes app code; [changes/README.md](changes/README.md)) for the notes and folds them into `CHANGELOG.md`, counting only fragments not already in the last Stable tag; `appcast.py` writes and checks the Sparkle 2 feed. It signs nothing: Sparkle's `sign_update`, fetched by a pinned URL and SHA-256, signs in the sign job.
- The feed is `appcast.xml` on the prerelease tagged `appcast`, not on a `gh-pages` branch, so GitHub's immutable releases can't be turned on for that tag. Any account with write access could replace the feed, but Sparkle's EdDSA signature protects what installs ([the ruling](docs/policies/automation.md#ruling-the-feed-stays-on-the-appcast-release)). A Beta item has the `beta` channel; promotion drops it and gives the item the Stable notes link and a phased rollout, so a Stable item keeps the Beta's download and signature.
- Beta and Promote share one concurrency group, `notchling-release`; GitHub keeps one pending run per group, so a newer pending run cancels an older pending one.
- The app doesn't read the feed yet: Sparkle 2.6.4 or later comes in its own protected pull request, after the Developer ID. [docs/policies/automation.md](docs/policies/automation.md#releases-so-far) has the turn-on steps and what isn't built.

### Agents

Phase 5's parts that need no credential exist, in `tools/agents/` (Python standard library, tested against fakes in `tools/agents/tests/`, which the `guard` job runs). Each does nothing, or fails closed, until its credential exists; [docs/policies/automation.md](docs/policies/automation.md#phase-5-so-far) says what turns each on.

- Triage: `.github/workflows/triage.yml` runs `triage.py` on each new issue in two jobs, one with the model key and no GitHub token, one with an `issues: write` token and no model key. Labels come from `triage_labels.json`.
- Review agent, recommend-only: `.github/workflows/review-run.yml` runs on each pull request with no permissions and runs nothing from it; `.github/workflows/review-post.yml` follows it (`workflow_run`) with the default branch's code and runs `review.py`, which reads the diff and check results through the API and posts the `review-agent` status and a comment. `review-agent` is not a required check.
- The worker's side: `preflight.py` checks a queued issue before sd starts it, and `push_wrapper.py` pushes the agent's `agent/<issue>-<slug>` branch and opens the pull request as the GitHub App. Neither runs anywhere yet; [docs/agents/worker-setup.md](docs/agents/worker-setup.md) has the setup on each worker.

## Target

Decided: a Rust engine, GPUI for every desktop app (the Swift app is replaced), Flutter for the phone, and the engine as an always-on service.

**The target's Rust tree is a skeleton.** Since Phase 4 the Cargo workspace (`Cargo.toml`, Rust 1.99.1 pinned in `rust-toolchain.toml`, `deny.toml`) holds the eight crates in the layout below. Seven are stubs: each builds, has one test, and does nothing else. `crates/protocol` has the bridge protocol's generated types. GPUI 0.2.2 is a macOS-only dependency of `crates/kit` and `crates/app` (`LINT-GPUI-MACOS-ONLY`; Linux needs system libraries CI doesn't install yet, Windows is unverified), with the `runtime_shaders` feature because this Mac has no Metal Toolchain. The GPUI spike is done ([0001](docs/decisions/0001-gpui-spike.md)). Its results are in `spike/RESULTS.md`; the throwaway code is only in git history (`git show e8c8cd2:spike/gpui/`; its branch `spike/gpui-phase1` was deleted on 2026-11-08, so records 0001 and 0008 point at a branch that no longer exists) and never goes into `main`.

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
| `notchlingd` | The engine as a service on every computer: a LaunchAgent, a systemd user service, a Windows startup task. It owns everything that acts or lives long. The app, the phone, the hook and the command line are its clients. | Serves sessions (Milestone 2, part E16): `notchling daemon` from `crates/notchlingd` reads `engine::sessions` on `platform::watch` changes and every 5 s, and serves `sessions.list` and `sessions.changed`; it only reads and owns none of 0011's resources yet. `notchling service install|uninstall|print` writes the LaunchAgent (`dev.notchling.daemon`), systemd user unit or Task Scheduler XML and prints the command that loads it, never running it. Acts from Milestone 3. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| Local API | A Unix socket or Windows named pipe, this user only. Callers are known by executable path. The app tier may do everything; the agent tier gets only its narrow operations. Account switching, vault values, Machines fixes and settings are app tier only. Privileged calls are audit-logged; an action that needs approval is denied when no UI is attached. | Exists on macOS and Linux (part E17, Milestone 2): `crates/local-api`, `<data dir>/notchlingd.sock` (0600, folder 0700), one JSON object per line up to 1 MiB, `hello` first. The tier comes from the peer's executable path (`proc_pidpath_audittoken` on the audit token, `/proc/<pid>/exe`), canonicalized on both sides, against the installed app and, for a daemon in a cargo target folder only, `NOTCHLING_APP_TIER_PATHS`; a copy elsewhere is the agent tier. The peer is read at accept and at `hello`, and the app tier needs one process throughout (the exec race; best effort on Linux). A `flock` beside the socket keeps one daemon per socket, so a stale socket file is removed safely. Refusals go to `audit.jsonl` (0600, rotates at 5 MB). Windows' named pipe is a stub that says unsupported. Each request reaches its op with the sending process's pid from the peer credentials, which is how the hook is known (never from its arguments). A topic can be app tier too (`prompts`). Milestone 3's acting ops are app tier, and acting is off at every daemon start: files at rest are not a tier boundary. | [0007](docs/decisions/0007-local-api-tiers.md) |
| The app reads transcripts | The desktop app reads transcripts itself, read-only, with the engine library, for speed. It never writes or acts except through the daemon. | Milestone 1: the read-only viewer reads files. Milestone 2 (app part): Chat's sidebar and the notch take their session rows from the daemon (`crates/app/src/session_source.rs`: one background thread connects, subscribes to `sessions`, lists, follows `sessions.changed`; the UI threads never touch the socket), start a missing daemon once from the `notchling` beside the app, and fall back to reading the files, with a "Daemon not running" note, while it is unreachable. Transcripts and Codex rollouts are still read by the app; `--data-root`, fixtures and renders never touch a daemon. Proposed: a designer's call, not settled. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| Hook under a daemon | The daemon answers "pass" at once unless a client that can answer (the app or a phone) is attached. The hook keeps its own deadline and passes on any socket error. `claude -p` (`sdk-cli`) sessions stay ignored. Fixture tests for all three. | Planned: part E4, Milestone 3. Today the hook talks to the app through files. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| `ptyhost` | A separately restartable pty host, so updating the daemon never ends a terminal session. A port of hostd with the same socket contract. | Planned. hostd keeps the job until the Swift app retires. | [0004](docs/decisions/0004-notchlingd-engine-service.md), [0011](docs/decisions/0011-one-owner-per-resource.md) |
| Secrets per OS | macOS: account logins keep going through `/usr/bin/security` with hex on stdin; the vault item uses the Security framework on purpose. Linux and Windows: Secret Service and Credential Manager have no per-app access control, and [docs/goals.md](docs/goals.md) says so plainly. | Planned: parts E8 and E9, Milestone 6. | |
| Remote computers | The desktop app and the phone connect to other computers' daemons the way the phone connects today, with the same pairing. Each session belongs to exactly one computer, which alone acts on it; session ids are host-qualified. Nothing is replicated: accounts, lanes and the vault never leave their computer. | Planned, with the phone work (part P2). A decision record must define identity, transport (LAN, tunnel with Access), reconnect cursors, simultaneous clients and revocation before any milestone uses it. Not written yet. | |
| GPUI app and kit | One GPUI app on a pinned revision (`crates/app`) with our own widget layer (`crates/kit`). Platform pieces behind small traits (`crates/platform`): notch overlay and menu bar on macOS, tray and a top panel on Linux and Windows (the panel is a designer's call). | Planned: parts U0 to U11, from Milestone 1. Linux and Windows show sessions only until `claude attach` is proven there. | [0005](docs/decisions/0005-gpui-desktop-app.md), [0001](docs/decisions/0001-gpui-spike.md) |
| Web content | Pages, artifacts and HTML files open in their own `wry` window, not inside a GPUI view, because GPUI has no web view. Chat's page panel lays a `wry` web view over the panel instead (0013, proposed). | Planned: part U4, Milestone 5. | [0005](docs/decisions/0005-gpui-desktop-app.md) |
| Protocol | `protocol/`: a written spec in JSON Schema, conformance and behavior fixtures, version negotiation, additive only. Dart and Rust types are generated from it. | Exists since Phase 4, without version negotiation or behavior fixtures yet. `protocol/agentconsole.v1.schema.json` describes today's wire, and 74 fixtures hold it there: what the Mac sends is captured from the real Swift bridge (`ProtocolFixturesTests`), and what the phone sends is written from its client and fed to the bridge's router. `tools/protocol/gen.py` generates `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart` (never edited by hand; `GUARD-PROTOCOL-GEN`), and a Rust and a Dart test parse every fixture. The Mac still encodes with `BridgeProtocol.swift` and the phone still reads with its hand-written `models.dart`, pinned to the generated types by that Dart test. See [protocol/README.md](protocol/README.md). | [0006](docs/decisions/0006-flutter-phone-generated-types.md) (Proposed) |
| Licenses | Only permissive crates, enforced by `cargo-deny`. Our own terminal and markdown views on permissive crates. | Exists since Phase 4: `deny.toml` allows only `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Unicode-3.0` and `Zlib`, and only crates.io as a source; `rust-deny` runs it on every pull request. | [0008](docs/decisions/0008-mit-permissive-crates.md) |
| Releases | Beta daily, build then sign apart, Stable by one click, one signed update feed. | Built in Phase 6 for the Swift app and off until the maintainer turns it on (see [Releases](#releases)); the app has no updater yet. The Rust app's signed manifest is planned. | [0010](docs/decisions/0010-build-then-sign-beta-then-stable.md) |

Machines stays a separate tool, not inside the daemon, and is ported last (part E10). That is a designer's call, not settled.

### Repo layout

| Folder | What | Today |
| --- | --- | --- |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `deny.toml` | The Cargo workspace (every folder in `crates/`), the pinned toolchain, the license and source rules | Exist |
| `crates/engine/` | Transcripts, sessions, accounts, lanes, vault, bridge (a library) | Exists as a stub |
| `crates/notchlingd/` | The engine service: the `notchling` binary | The daemon serves sessions (M2); `status`, `sessions`, `service` |
| `crates/ptyhost/` | The separately restartable terminal host (port of hostd) | Exists as a stub that prints its version |
| `crates/local-api/` | App and command line to daemon API, with tiers | Server, client, tiers, audit log and `ensure_daemon` exist (M2; Windows a stub) |
| `crates/app/` | The GPUI desktop app | U0 shell (menu bar item, Dock presence, main window placeholder) and U10 `--render <view> --out <png>` exist, macOS only; other systems get a stub `main` |
| `crates/platform/` | Notch and menu bar (macOS), tray and panel (Linux, Windows) | Exists as a stub |
| `crates/kit/` | Our widget layer over GPUI | Tokens (every system) and the first GPUI widgets with `KitGallery` (macOS) exist, U0 |
| `crates/protocol/` | The bridge protocol's Rust types, generated from `protocol/` | Exists |
| `apps/flutter/` | The phone app | Exists (moved here from the old top-level phone folder in Phase 4) |
| `legacy/macos/` | Today's Swift app and ConsoleCore, fixes only, until retired | Exists (moved from the repo root in Phase 4): `legacy/macos/Sources/`, `Tests/`, `Demo/`, `Icon/`, `Pet/`, `build.sh`, `release.sh`. `Package.swift` and a one-line `build.sh` wrapper stay at the root |
| `protocol/` | Spec (`README.md`), JSON Schema, fixtures | Exists |
| `docs/`, `tools/`, `viz/` | Docs; the guard, setup and protocol tools (`tools/protocol/gen.py`, the generator), the agents' scripts (`tools/agents/`: triage, preflight, push wrapper, review) and the release tools (`tools/release/`: version, fragments, appcast); the web page behind the phone's Visualize tab | Exist |
| `changes/` | Changelog fragments, one per pull request that changes app code, folded into `CHANGELOG.md` at each Stable; `README.md` is the guide | Exists (Phase 6) |
| `spike/` | The GPUI spike's results (`RESULTS.md`); its code is in history at `e8c8cd2:spike/gpui/` | Exists |

### Swift and Rust side by side

Both run on the maintainer's Mac during Milestones 1 to 5. Some resources can have only one owner: the hook entries, hostd and its socket, the phone bridge's ports and certificate, `vault.sock` and the vault item, limit watching and auto-switch, lanes and account switching. Each has one owner per milestone, and the Swift app reads a stand-down setting per area. The table of who owns what, until when, is in [0011](docs/decisions/0011-one-owner-per-resource.md).

Today the Swift app owns all of them.

### Order of work

1. Phase 1, the GPUI spike: done ([0001](docs/decisions/0001-gpui-spike.md)).
2. Phase 2, guard rails and quality: merged. The maintainer applies the repository settings with `tools/github/apply-settings.sh`.
3. Phase 3, the operating manual: this document, the decision records, goals, rules, governance and policies.
4. Phase 4: the repo layout above, the Cargo workspace, the protocol spec and generated types: done. Switching the phone to the generated types is a later change (part P1).
5. Phase 5: agents on (the GitHub App, a separate agent user and push wrapper on workers, triage, the worker, the review agent). The parts that need no credential are built and off (see [Agents](#agents)); the rest waits for the maintainer.
6. Phase 6: releases (notarization, build and sign apart, the Beta feed, Stable by one click). The parts that need no signing identity are built and off (see [Releases](#releases)); the rest waits for the maintainer.
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
