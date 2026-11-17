# Architecture

How Notchling's parts fit, in two layers:

- **Today**: how the app reaches the tools it shows, its data, the guard rails, CI, releases and the agents. The Swift app these sections once described is retired; its code is in the git history before the commit that removed `legacy/`.
- **Target**: a Rust engine running as a service, one GPUI desktop app for macOS, Linux and Windows, and the Flutter phone app. Each part there says whether it exists or is planned, and when.

Why it is this way is in the [decision records](docs/decisions/README.md); changing a decision takes a new record. The plan behind the target is the [design](docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md) (revision 2).

Related: what Notchling is for, [docs/goals.md](docs/goals.md). The rules and their checks, [docs/rules.md](docs/rules.md). Which files make up each feature, [docs/feature-map.md](docs/feature-map.md). The earlier restructuring review of the Swift app, [docs/architecture.md](docs/architecture.md), is kept as history; its phase numbers are its own, not the design's.

## Today

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
- The Rust app and daemon use the same folder for their own data in SQLite ([0014](docs/decisions/0014-app-data-in-sqlite.md)): `notchling.db` (the settings, one row per key, with the old `settings.json` read in once on the first write; Chat's unsent replies; a `docs` row for each file Notchling alone used to rewrite whole: the saved accounts lists, the daemon's vault list, limits, the cost cache, second opinions and Machines' own registry and offers; and the `inbox` table, one row per inbox item: 200 open items at most, done ones kept 7 days and the newest 300) and `daemon/search.db` (the session index, FTS5 ranked by bm25). `search-index.json` is the Swift app's only.
- `~/Library/Application Support/AgentConsole/`: `bridge/` (the certificate, paired phones, tunnel settings, the audit log), `hostd.sock` and `hostd-state.json`. The folder keeps the old Agent Console name so paired phones survive.
- UserDefaults: bundle id `local.notchling`.

### Guard rails

Built in Phase 2; they exist today.

- `tools/guard/zones.json` names the auto-merge zone. `.github/CODEOWNERS` is generated from it ([0002](docs/decisions/0002-the-line.md)).
- `python3 -m guard` runs in `.github/workflows/guard.yml`: the architecture lint with a shrink-only baseline (the gate sends a PR that grows it to the maintainer), actions pinned and allowlisted, CODEOWNERS in step, rulesets matching the workflows. The same job checks that the generated protocol types match the schema (`tools/protocol/gen.py --check`, `GUARD-PROTOCOL-GEN`) and runs `tools/protocol/tests/`.
- `maintainer-gate` (`.github/workflows/maintainer-gate.yml`, `python3 -m guard gate`): when a PR touches a protected path, uses a capability or weakens a test, it passes only with the maintainer's approval of the head commit.
- `.github/rulesets/` and `tools/github/apply-settings.sh` hold the repository settings as code ([0009](docs/decisions/0009-rulesets-as-code.md)).
- CI also runs the phone's `flutter analyze` and `flutter test` (goldens included).
- `.github/workflows/rust.yml` checks the Cargo workspace (since Phase 4): `rust-test` (`cargo test` on macOS, Linux and Windows), `rust-clippy` (`cargo fmt --all --check`, then clippy with warnings as errors) and `rust-deny` (`cargo deny check`: licenses, advisories, sources).

### Releases

`tools/release/release.py <version>` opens a `release/<version>` PR, and merging it tags and publishes (`.github/workflows/release.yml`): the app is built by `tools/bundle-next.sh` on macOS and the Linux node binaries by `tools/build-node.sh`, then one job checks every file against the hashes the others gave and publishes the release and the update feed. Builds are signed ad hoc.

Phase 6's Beta and Stable pipeline ([0010](docs/decisions/0010-build-then-sign-beta-then-stable.md)) was built for the Swift app (`beta.yml` and `promote.yml`), never turned on, and removed with it; the git history has it. Record 0010 stays as the record of that design.

- `tools/release/` (Python standard library, tested in `tools/release/tests/`, which the `guard` job runs): `release.py` makes the release branch; `fragments.py` reads `changes/` (one fragment per pull request that changes app code; [changes/README.md](changes/README.md)) for the notes and folds them into `CHANGELOG.md`; `version.py` holds the version rules and the feed's build number; `appcast.py` writes and checks the Sparkle 2 feed.
- The feed is `appcast.xml` on the prerelease tagged `appcast`, not on a `gh-pages` branch, so GitHub's immutable releases can't be turned on for that tag. Any account with write access could replace the feed ([the ruling](docs/policies/automation.md#ruling-the-feed-stays-on-the-appcast-release)). The app reads the feed itself (`crates/app/src/welcome/updates.rs`). [docs/policies/automation.md](docs/policies/automation.md#releases-so-far) says what isn't built.

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
| Local API | A Unix socket or Windows named pipe, this user only. Callers are known by executable path. The app tier may do everything; the agent tier gets only its narrow operations. Account switching, vault values, Machines fixes and settings are app tier only. Privileged calls are audit-logged; an action that needs approval is denied when no UI is attached. | Exists on macOS and Linux (part E17, Milestone 2): `crates/local-api`, `<data dir>/notchlingd.sock` (0600, folder 0700), one JSON object per line up to 1 MiB, `hello` first. The tier comes from the peer's executable path (`proc_pidpath_audittoken` on the audit token, `/proc/<pid>/exe`), canonicalized on both sides, against the installed app and, for a daemon in a cargo target folder only, `NOTCHLING_APP_TIER_PATHS`; a copy elsewhere is the agent tier. The peer is read at accept and at `hello`, and the app tier needs one process throughout (the exec race; best effort on Linux). A `flock` beside the socket keeps one daemon per socket, so a stale socket file is removed safely. Refusals and app-tier calls are events the daemon writes to the Audit (`audit/YYYY-MM.jsonl`, sealed lines, 0600) and the Activity store (`daemon/events.db`). Windows' named pipe is a stub that says unsupported. Each request reaches its op with the sending process's pid from the peer credentials, which is how the hook is known (never from its arguments). A topic can be app tier too (`prompts`). Milestone 3's acting ops are app tier, and acting is off at every daemon start: files at rest are not a tier boundary. `vault.approval.answer` and the inbox's ops take the app tier from the app itself. A paired phone reaching the inbox through the bridge (`InboxOps::decide`, with the bridge, acting and vault switches on) may answer a vault item, Allow included; only the Mac's form takes a secret's value. `InboxOps::decide` lets only the Mac or a phone allow a secret. Pairing (`devices.json`) is a file an agent with a shell can write, a risk the maintainer accepted (0007's note of 2026-11-13). | [0007](docs/decisions/0007-local-api-tiers.md) |
| The app reads transcripts | The desktop app reads transcripts itself, read-only, with the engine library, for speed. It never writes or acts except through the daemon. | Milestone 1: the read-only viewer reads files. Milestone 2 (app part): Chat's sidebar and the notch take their session rows from the daemon (`crates/app/src/session_source.rs`: one background thread connects, subscribes to `sessions`, lists, follows `sessions.changed`; the UI threads never touch the socket), start a missing daemon once from the `notchling` beside the app, and fall back to reading the files, with a "Daemon not running" note, while it is unreachable. Transcripts and Codex rollouts are still read by the app; `--data-root`, fixtures and renders never touch a daemon. Proposed: a designer's call, not settled. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| Hook under a daemon | The daemon answers "pass" at once unless a client that can answer (the app or a phone) is attached. The hook keeps its own deadline and passes on any socket error. `claude -p` (`sdk-cli`) sessions stay ignored. Fixture tests for all three. | Planned: part E4, Milestone 3. Today the hook talks to the app through files. | [0004](docs/decisions/0004-notchlingd-engine-service.md) |
| `ptyhost` | A separately restartable pty host, so updating the daemon never ends a terminal session. A port of hostd with the same socket contract. | Planned. hostd keeps the job until the Swift app retires. | [0004](docs/decisions/0004-notchlingd-engine-service.md), [0011](docs/decisions/0011-one-owner-per-resource.md) |
| Secrets per OS | macOS: account logins keep going through `/usr/bin/security` with hex on stdin; the vault item uses the Security framework on purpose. Linux and Windows: Secret Service and Credential Manager have no per-app access control, and [docs/goals.md](docs/goals.md) says so plainly. | Planned: parts E8 and E9, Milestone 6. | |
| Remote computers | The desktop app and the phone connect to other computers' daemons the way the phone connects today, with the same pairing. Each session belongs to exactly one computer, which alone acts on it; session ids are host-qualified. Nothing is replicated: accounts, lanes and the vault never leave their computer. | Cluster v1 (plan B) is built: machines of the maintainer's own join one cluster over Tailscale (pinned TLS 1.3), a primary, Standbys and Workers, one journaled member list, a node tier for what one node may ask of another, remote sessions with their transcript mirrored to the client, dispatch of a session or shell job to a Worker, and the phone through the home. See [the cluster v1 nodes decision](docs/decisions/XXXX-cluster-v1-nodes.md) (Proposed; it takes its number when merged) and [0012](docs/decisions/0012-remote-computers.md) for identity and qualified ids. Accounts, lanes and the vault still never leave their computer. | |
| GPUI app and kit | One GPUI app on a pinned revision (`crates/app`) with our own widget layer (`crates/kit`). Platform pieces behind small traits (`crates/platform`): notch overlay and menu bar on macOS, tray and a top panel on Linux and Windows (the panel is a designer's call). | Planned: parts U0 to U11, from Milestone 1. Linux and Windows show sessions only until `claude attach` is proven there. | [0005](docs/decisions/0005-gpui-desktop-app.md), [0001](docs/decisions/0001-gpui-spike.md) |
| Web content | Pages, artifacts and HTML files open in their own `wry` window, not inside a GPUI view, because GPUI has no web view. Chat's page panel lays a `wry` web view over the panel instead (0013, proposed). | Planned: part U4, Milestone 5. | [0005](docs/decisions/0005-gpui-desktop-app.md) |
| Protocol | `protocol/`: a written spec in JSON Schema, conformance and behavior fixtures, version negotiation, additive only. Dart and Rust types are generated from it. | Exists since Phase 4, without version negotiation or behavior fixtures yet. `protocol/agentconsole.v1.schema.json` describes today's wire, and 74 fixtures hold it there: what the Mac sends is captured from the real Swift bridge (`ProtocolFixturesTests`), and what the phone sends is written from its client and fed to the bridge's router. `tools/protocol/gen.py` generates `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart` (never edited by hand; `GUARD-PROTOCOL-GEN`), and a Rust and a Dart test parse every fixture. The Mac still encodes with `BridgeProtocol.swift` and the phone still reads with its hand-written `models.dart`, pinned to the generated types by that Dart test. See [protocol/README.md](protocol/README.md). | [0006](docs/decisions/0006-flutter-phone-generated-types.md) (Proposed) |
| Licenses | Only permissive crates, enforced by `cargo-deny`. Our own terminal and markdown views on permissive crates. | Exists since Phase 4: `deny.toml` allows only `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Unicode-3.0` and `Zlib`, and only crates.io as a source; `rust-deny` runs it on every pull request. | [0008](docs/decisions/0008-mit-permissive-crates.md) |
| Releases | Beta daily, build then sign apart, Stable by one click, one signed update feed. | `release.py` and `release.yml` exist (see [Releases](#releases)); the Beta and Stable workflows were removed with the Swift app. The Rust app's signed manifest is planned. | [0010](docs/decisions/0010-build-then-sign-beta-then-stable.md) |

Machines stays a separate tool, not inside the daemon, and is ported last (part E10). That is a designer's call, not settled.

### Models and prices

Which models each tool offers, their effort levels, context windows and prices are data, not code ([0016](docs/decisions/0016-provider-interface-in-the-engine.md), Proposed). `notchling_engine::catalog` (`crates/engine/src/catalog/`) lays three sources over each other: the tool's live list (Anthropic's `GET /v1/models` for Claude Code, Codex's `models_cache.json`, the Model and Thinking Level options an `agy acp` session reports), the last live list cached in `live-models.json`, and the compiled-in data file `crates/engine/data/catalog.json`. A `catalog.json` beside the daemon's socket can fix a price, a window or a name, but never adds a model; the daemon reads it once at start, so an edit applies at the next start, and the cost cache then reprices only the models whose price changed (a day whose transcripts are gone is priced again from its cached tokens). The daemon's `CatalogService` (`crates/notchlingd/src/catalog_ops.rs`) owns the one catalog: it serves `catalog.get {tool}` (agent tier) and `catalog.changed` on `tools`, prices `cost.summary`, and decides which models `session.new --model` and the phone's `modelId` may start. Claude Code's list is fetched only while the `accounts` switch is on, with the main login's live token (`ClaudeAccounts::live_token`: read only, never renewed), when a `catalog.get` finds the cached list a day old or missing, and not tried again for 15 minutes after a failure, and a token refused with a 400, 403 or 404 not for a day unless the live token changes (a failure is logged once per run by its status). Until it has been read once, Claude Code's own `additionalModelOptionsCache` in the main `~/.claude.json` (read only) stands in for it. Chat's Claude Code pickers and Context Meter read `catalog.get` at start, every hour and when a picker menu opens, and use the built-in list until it answers. This is phase 1 of 0016; one daemon API and one provider per tool in the engine are planned.

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
| `protocol/` | Spec (`README.md`), JSON Schema, fixtures | Exists |
| `docs/`, `tools/`, `viz/` | Docs; the guard, setup and protocol tools (`tools/protocol/gen.py`, the generator), the agents' scripts (`tools/agents/`: triage, preflight, push wrapper, review) and the release tools (`tools/release/`: version, fragments, appcast); the web page behind the phone's Visualize tab | Exist |
| `changes/` | Changelog fragments, one per pull request that changes app code, folded into `CHANGELOG.md` at each Stable; `README.md` is the guide | Exists (Phase 6) |
| `spike/` | The GPUI spike's results (`RESULTS.md`); its code is in history at `e8c8cd2:spike/gpui/` | Exists |

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
8. Phase 8: retire the Swift app in one release. Done: `legacy/` was removed from the tree.

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
- [0011](docs/decisions/0011-one-owner-per-resource.md) One owner per resource while Swift and Rust ran side by side.
- [0012](docs/decisions/0012-remote-computers.md) Remote computers: identity by certificate, host-qualified session ids (Proposed).
- [0013](docs/decisions/0013-page-panel-web-view.md) A web view in Chat's page panel (Proposed).
- [0014](docs/decisions/0014-app-data-in-sqlite.md) The app's own data in SQLite, search by bm25.
- [0015](docs/decisions/0015-one-chat-interface-per-tool.md) One Chat interface per tool (Proposed, amended by 0016).
- [0016](docs/decisions/0016-provider-interface-in-the-engine.md) One provider interface per tool in the engine; model data from a catalog (Proposed).
