# Notchling as an agent-run open-source project

Date: 2026-11-07. Status: draft for approval, revision 2 (after three max-effort reviews: Fable 5.1 on
architecture, Opus 5.5 on process, Codex GPT-6.1 Sol on both).

Marks: *(decided)* is the maintainer's decision; *(my call)* is the designer's, open to change;
*(review)* came from the reviews.

## Intent

Notchling is public (`github.com/dtthaison/notchling`, MIT). The maintainer wants it to run like a
well-kept open-source project in which agents do most of the work: they take issues, add features, fix
bugs, open pull requests and ship releases that users' apps install on their own. Guard rails decide what
may land. The maintainer's job moves from reviewing every PR to shaping the rules, checks and skills the
agents work inside, and sampling what landed (poteto's "head chef" model from the Pstack notebook).

At the same time the app is rebuilt: a Rust engine running as a service on every computer, one GPUI
desktop app for macOS, Linux and Windows, and the Flutter phone app.

## Goals

1. An issue can go from filed to merged with no human step when it only touches the auto-merge zone.
2. Quality holds or improves as agent PRs grow: every rule fails a check instead of living in a doc.
3. Another developer or an agent can work correctly from what the repository says, without asking.
4. Mistakes found after merge turn into a check, a test or a skill, so they can't land twice.
5. Users get updates on their own, from Beta to Stable, without a release ever touching a signing key
   from code an agent wrote.

## Non-goals

- App Store or Google Play releases now (the phone app is built by its users for now *(decided)*).
- A website.
- Replacing the sd worker; this design changes how it authenticates and labels, not what it does.

## Anti-goals (a failure even if it works)

- A stranger's issue or PR steering an agent into logins, Keychain, the vault, lanes, Machines, the hook,
  process launching or the release path, and that change landing without the maintainer.
- Agents loosening their own guard rails, by any path: workflows, settings, CODEOWNERS, rulesets, the
  lint baseline, tests, goldens, AGENTS.md, skills, `.claude/`, the kill switch, the App's permissions.
- A secret, token or real login reachable from CI jobs that run repository code, or from a test.
- A green check that means nothing: a check that would pass on its own bad example.
- An agent acting with the maintainer's GitHub identity.

## The line *(decided)*

Agents may merge on their own **only in named safe areas**; everything else waits for the maintainer.
*(review: the first draft listed protected folders, but login switching, OAuth token use, trust prompts
and process launching sat in "safe" folders, and one Swift module lets any file call anything.)*

- `CODEOWNERS` starts with `* @dtthaison`. Only the auto-merge zone is listed without an owner.
- **Auto-merge zone** (today's tree): `Sources/Notchling/Chat/Views/`, `Sources/Notchling/Notch/`
  (views only: `NotchView`, `NotchRows`, `NotchPeek`, `NotchDetail`, `NotchCardParts`, `TabPerch`, `Pet/`),
  `Sources/Notchling/Search/`, `Sources/Notchling/DesignKit/`, `Sources/Notchling/Pages/PagesWindow.swift`,
  transcript readers (`Chat/ConversationReader.swift`, `Agents/TranscriptReader.swift`,
  `Agents/CodexSessions.swift`, `Agents/ACPTranscript.swift`, `ConsoleCore/ChatTranscript.swift`,
  `ConsoleCore/SubagentReader.swift`, `ConsoleCore/TranscriptFacts.swift`), `docs/` except the manual
  layers below, `mobile/lib/features/` except `pairing/`, and new tests (not changes to existing ones).
  In the Rust tree: `crates/app/` views, `crates/kit/`, the transcript and search modules of
  `crates/engine/`.
- **Everything else waits**, including what the reviews found missing: `Chat/MoveAll.swift`,
  `Chat/Model/`, `Pages/OwnerPublish.swift`, `Agents/ReplySender*.swift`, `Agents/AgentActions.swift`,
  `Agents/CodexRunner.swift`, `Agents/ACPRunner.swift`, all of ConsoleCore's pty, hostd, bridge and
  hook code, `.claude/settings.json`, `.claude/skills/`, `CLAUDE.md`, every `AGENTS.md`, `Tests/` and
  `Fixtures/` (changes to existing ones), the manual (`docs/goals.md`, `docs/rules.md`,
  `ARCHITECTURE.md`, `docs/decisions/`), `.gitattributes`, `Icon/make-icon.swift` (the release build runs
  it), `mobile/lib/core/pairing/`, `protocol/`, `.github/`, `build.sh`, `release.sh`, `Package.swift`,
  `Cargo.toml`/`Cargo.lock`, `Info.plist`, `tools/`, `.sd/`.
- **Capability check** *(review)*: a required check that sends a PR to the maintainer, wherever the
  file is, when it adds a new use of `Process`, `forkpty`/`posix_spawn`, `NSAppleScript`/`osascript`,
  `URLSession`/network clients, `SecItem*`/`/usr/bin/security`/`keyring`, `FileManager` writes outside
  the app's folders, or a protected type (`VaultStore`, `AccountStore`, `LaneStore`, `ReplySender`,
  `HookBridge`, `OwnerPublish`) from outside its own files. In Rust, crate boundaries and `cargo-deny`
  take over most of this.
- **`maintainer-gate`** *(review)*: a required check that passes only when the PR touches nothing
  protected and trips no capability, or when the maintainer approved the PR's current head commit.
  Labels are informational only, since anyone with pull-request write can remove a label.

## The flow

1. **Intake.** Issues come through the templates. The triage Action runs a model with **no token**; it
   only outputs JSON (type, missing info, likely duplicate). A fixed script applies labels from an
   allowlist and posts a templated comment; it never adds `notchling:*`, never closes, never echoes the
   issue text, and treats anything that looks like a security report by asking the reporter to use
   private reporting instead *(review)*.
2. **Approval to work.** Only the maintainer adds `notchling:queued` *(decided: labels
   `notchling:queued`, `notchling:ready`, `notchling:needs-you`, `notchling:stuck` replace sd's `sd:*`;
   sd gains a `labels.prefix` setting, default `sd`, in sd's own repo; Morning Actions reads the prefix:
   `MorningItems.labels`, `MorningActions.swift:78,84,96`, `MergePlan`, the marker regex)*. Before
   starting, the worker checks through the GitHub API that the label event's actor is on the
   maintainers list and that the issue body wasn't edited after the label was added *(review)*.
   Issue text is data, quoted as such, never instructions.
3. **Work.** The worker branches `agent/<issue>-<slug>`, implements with tests, runs the checks and
   renders, and opens a pull request *(decided: GitHub pull requests)*. Containment *(review)*:
   - The agent runs as a **separate OS user** on each worker with **no GitHub credential** of any kind.
     Today's personal token from the maintainer's account (`~/.config/agents/github-token`, Issues write)
     is retired.
   - A small push wrapper, run by sd as a different user, takes the agent's finished branch, mints a
     one-hour GitHub App token narrowed to this repository, pushes `agent/**` and opens the PR. The App
     key lives only there (in the Secret Vault on that machine), out of the agent's reach.
   - Every build, test and render runs with bounded time, network limited to package registries, and a
     run budget; failures stop, they don't loop.
   - Machines' "Push is refused" step becomes "Agent user can't push; wrapper can push only agent/**".
     `MergePlan`'s merge is retired; Morning Actions shows what landed, for sampling.
4. **Gates** (required checks on `main`, each bound to the PR's head commit):
   - Swift: `swift test`, `./build.sh` for both architectures. Rust: `cargo test`, `cargo clippy`,
     `cargo-deny` (licenses: MIT/Apache/BSD only *(decided)*, advisories). Flutter: `flutter analyze`,
     `flutter test`, goldens. On macOS, Linux and Windows runners once Rust code exists.
   - The architecture lint and the rules check (every rule has a check that fails on its own bad example).
   - Capability check and `maintainer-gate` (above).
   - Test-weakening check: removed or edited existing tests, new `XCTSkip`/`#[ignore]`, edited goldens,
     a lower coverage ratchet → maintainer. Invisible or bidirectional Unicode is rejected.
   - Cross-model review: a reviewer model picked by rotation, not by what the PR says about its author
     (`Agent-Model:` is informational). Bound to the head commit; fails closed if the review is missing or
     the model API is down. After two failed reviews on one PR, it goes to the maintainer instead of
     another push *(review)*.
5. **Merge.** Agent PRs (authored by the App) in the auto-merge zone merge on GitHub auto-merge
   (squash) once every check is green. Approval must come after the last push
   (`require_last_push_approval`), so only the reviewed commit merges.
6. **Other people's pull requests** *(decided: the review agent starts recommend-only)*:
   - An untrusted `pull_request` job (`permissions: {}`, no secrets) runs the PR's checks and the
     reproduction. A trusted `workflow_run` job reads only the diff and results through the API, never
     files the fork wrote, and posts the review agent's verdict as a check and a comment.
   - The agent checks the PR against the goals, the rules and the architecture; for a bug fix it
     reproduces the bug first (a failing test on `main` that the PR makes pass; for UI or intermittent
     bugs a recorded reproduction is accepted).
   - Verdicts: **right and ours**: recommend merge. **Right, built differently**: request changes with
     concrete pointers; if the author agrees or doesn't respond in 14 days, the worker rebuilds it as an
     App PR with `Co-authored-by:` and a link, and the original stays open until the rebuild merges.
     **Not right**: explain why. Every verdict can be appealed by asking for the maintainer.
   - The maintainer merges. The agent earns auto-merge for outside PRs in the auto-merge zone after it has
     agreed with the maintainer on 20 PRs in a row; one disagreement resets the count.
   - Outside PRs get a model review only after the maintainer labels them `notchling:review`, to cap
     spend from strangers *(review)*.
   - Contributors sign off commits (DCO) *(review)*.
7. **Sample and sharpen.** Each morning the maintainer samples merged PRs. Anything bad becomes a new
   check, test, lint rule or skill change, never just a review comment. If nobody has sampled for 3 days,
   automation pauses: the worker takes no new issues and no promotions happen *(review)*.

## GitHub setup *(review)*

- **Rulesets only**, kept as code in `.github/rulesets/` and applied by a script the maintainer runs;
  the old classic branch protection is removed so the two can't disagree.
- `main`: no direct pushes; required checks pinned to the GitHub Actions source; `maintainer-gate`
  required; approval after last push; maintainer bypass only through a PR.
- `agent/**`: only the App may push. Tags `v*` and releases: only the release workflow.
- **Agent identity** *(decided: a GitHub App)*: contents write (limited by the rulesets above), pull
  requests write, issues write, checks read. Never `workflows`, never admin. Installed on this repository
  only. Its key lives only with the push wrapper on worker machines, never in CI.
- **TrietKun** keeps write access *(decided)*. The rulesets and `maintainer-gate` apply to that account as
  to any other; only `@dtthaison` can satisfy `maintainer-gate`. GOVERNANCE.md names who may queue work
  and approve protected changes.
- Turn on: private vulnerability reporting (SECURITY.md already points there), secret scanning, push
  protection. `allowed_actions`: only GitHub's and SHA-pinned allowlisted actions. Dependabot for Swift,
  pub (`mobile/pubspec.lock`), cargo and Actions, with a cooldown; lockfile updates never auto-merge.
- Model spend: a hard cap at the model provider and a daily cap on agent runs and reviews.

## Releases and updates

*(decided: Stable is one click by the maintainer for now; it becomes automatic once real signals exist.)*

1. **Beta, daily.** On a day `main` has new merges, the release workflow cuts a Beta. The version comes
   from tags at build time (not `Info.plist` edits); notes come from per-PR changelog fragments. A
   breaking protocol or stored-data change needs the maintainer.
2. **Build, then sign apart** *(review)*: the build job runs repository code with no secrets and produces
   artifacts; a separate signing job runs no repository code, signs the exact artifacts, attaches
   provenance attestations and publishes. The same immutable artifact goes from Beta to Stable.
3. **Gates before Stable can be offered:** upgrade and downgrade tests from the last Stable (stored data
   stays readable by the previous release, so rollback is safe), a launch-and-render smoke test on each
   OS, and the maintainer's own machines running the Beta as canaries. Each new Beta resets the soak only
   for the newer candidate; the older one can still be promoted.
4. **Stable.** The maintainer promotes with one click. It becomes automatic (clean 3-day soak on a minimum
   number of Beta installs with opt-in, privacy-respecting crash reports) only after a privacy policy and
   that telemetry exist. A security fix has its own fast path.
5. **Updates.** One signed feed with Beta and Stable channels and phased rollout; updates install only when
   the app is idle. Swift app: Sparkle 2.6.4 or later. Rust app: a static manifest we own, signed with
   ed25519 (minisign-style), installers from Astral's `dist` *(review: Zed's updater talks to zed.dev and
   isn't a reusable format; upstream cargo-dist is unmaintained)*.
6. **Kill switch**: a repository Actions variable that stops every promotion and fails closed when
   missing; not a file, since a red CI could block the PR that flips it *(review)*.
7. **Keys.** The update signing key is backed up offline; losing it strands every install. A Developer
   ID (notarized builds) comes **before** auto-update is turned on, because the vault's Keychain item
   trusts the exact build and ad hoc updates would bring its access prompt back after every Beta
   *(review)*. Notarization moves from non-goal to a Phase 6 prerequisite.

`./release.sh` and the `release/<version>` PR are replaced.

## The operating manual

The repository explains itself in layers. The root `AGENTS.md` is short and points to them; each
folder's `AGENTS.md` adds only that folder's rules.

| Layer | Where | What it answers |
| --- | --- | --- |
| Goals | `docs/goals.md` | What Notchling is, who it's for, what it will never do, what's weaker on Linux and Windows (no per-app secret access) |
| Architecture | `ARCHITECTURE.md`, `docs/decisions/` (one record per decision) | How the parts fit and why; changing a decision takes a new record |
| Rules | `docs/rules.md` | Each rule with an id, why, and its check |
| Flow | `CONTRIBUTING.md` (today says "no test suite yet"; rewritten), PR template | Issue → queued → branch → PR → checks → merge → Beta → Stable, the same for people and agents |
| Governance | `GOVERNANCE.md`, automation policy, privacy policy | Who decides, who may queue, what agents may do, what data leaves a machine |
| Guard rails | CODEOWNERS, `.github/rulesets/`, capability check | What can't happen without the maintainer |
| Validation | fixtures, terminal recordings, goldens, renders, protocol conformance, live-test mode | How to prove a change works |
| Skills | repo skills: port a Swift part to Rust, add a protocol field, add a provider, fix a bug reproduction-first | Step by step for repeated work, each tried once before it's trusted |
| Learning loop | incident → postmortem → rule id → check | So the same mistake can't land twice |
| Dev profile | a sandboxed `CLAUDE_CONFIG_DIR` and data folder for development | Working on the app without writing hooks and skills into one's real `~/.claude/` |

**Every rule has an automatic check** *(decided)*. A rule is added together with the check that enforces
it, the check's failure names the rule id, and the check has a test proving it fails on its own bad
example. A CI job fails if `docs/rules.md` lists a rule with no such check. What can't be checked belongs
in the goals or the architecture.

**Agent instructions** *(decided: AGENTS.md is the source; corrected by review)*. The instructions live
in `AGENTS.md` files, which Codex and Antigravity read. For Claude Code the root keeps a one-line
`CLAUDE.md` containing `@AGENTS.md`, the documented way to share them, which works on every version and
platform. (The first draft's claim that any CLAUDE.md turns AGENTS.md off was wrong.) CI fails on any
other `CLAUDE*.md` in the repo, and the worker's preflight checks that the instructions loaded and pins
the Claude Code version.

## Quality: the first rules

The architecture lint (under `tools/`, run in CI and locally) starts with rules taken from CLAUDE.md:
`ConsoleCore` uses no Notchling types and no macOS 14+ APIs; no force-unwraps outside probes and tests;
views use `Kit*` tokens; every UserDefaults key in code is in the migration list (today 30 keys in code,
24 migrated: the baseline records the gap); clickable views use a kit style or `.pointerCursor()`; no
`NSAppleScript` notifications and no `osascript` (today one `NSAppleScript` use in
`Agents/AgentActions.swift:98`; the `osascript` hit in `LimitWatcher.swift` is a comment).

The lint fails only on new or changed lines; existing violations sit in a generated baseline that may
only shrink. File size: warn over 600 lines, fail over 1,000 for new files.

## Architecture

Decided: **Rust engine, GPUI for every desktop app (the Swift app is replaced), Flutter for the phone,
the engine as an always-on service.** Today's sizes: Swift app 41.7k lines, ConsoleCore and hostd 7.4k,
Flutter 15.6k.

- **`notchlingd`** runs on every computer (LaunchAgent, systemd user service, Windows startup task) and
  owns everything that acts or lives long: replies, session control, the phone bridge, the vault,
  accounts and lanes, limit watching, keep awake. The desktop app, the phone, the hook and the command
  line are its clients.
- **Local API** *(review: tiers)*: a Unix socket or Windows named pipe, this user only, with the caller
  identified by its executable path (`proc_pidpath`, `/proc/<pid>/exe`,
  `GetNamedPipeClientProcessId`), as `VaultServer` does today. Two tiers: the **app tier** (the signed
  desktop app) may do everything; the **agent tier** (the command line agents run: `--secret`,
  `--delegate`, `--page`) gets only its narrow operations, and vault use still goes through the approval
  prompt. Account switching, vault values, Machines fixes and settings are app tier only. The vault is a
  separate service inside the daemon with its own checks. Privileged calls are audit-logged. Approval
  replies are authenticated; an action that needs approval is denied when no UI is attached.
- **The app reads transcripts itself**, read-only, with the engine library, for speed; it never writes
  or acts except through the daemon *(my call)*.
- **The hook under a daemon** *(review)*: the daemon answers "pass" at once unless a client that can
  answer (the app or a phone) is attached; the hook keeps its own deadline and passes on any socket error;
  `claude -p` (`sdk-cli`) sessions stay ignored. Fixture tests for all three.
- **Terminals** stay in a separately restartable pty host (today's hostd, later its Rust port), so
  updating the daemon never ends a running terminal session *(review)*.
- **Secrets per OS** *(review)*: on macOS, account logins keep going through `/usr/bin/security` with hex
  on stdin (never the Security framework, so ad hoc and rebuilt binaries keep access); the vault item uses
  the Security framework on purpose (only this build). On Linux and Windows, Secret Service and Credential
  Manager have no per-app access control; `docs/goals.md` says so plainly.
- **Remote computers** *(decided: Linux and Windows control their own sessions, and computers see each
  other; renamed and narrowed by review)*: the desktop app and the phone connect to other computers'
  daemons the way the phone connects today, with the same pairing. Each session belongs to exactly one
  computer, which alone acts on it; session ids are host-qualified. Nothing is replicated: accounts,
  lanes and the vault never leave their computer. A decision record defines identity, transport (LAN,
  tunnel with Access), reconnect cursors, simultaneous clients and revocation before any milestone uses it.
- **Desktop app**: one GPUI app (pinned revision) for macOS, Linux and Windows, with our own widget layer
  (`crates/kit`). Platform pieces behind small traits: notch overlay and menu bar on macOS, tray and a top
  panel on Linux and Windows *(my call)*.
- **License** *(decided: stay MIT)*: only MIT/Apache/BSD crates, enforced by `cargo-deny`. `gpui` and
  `alacritty_terminal` are fine; Zed's `terminal`, `terminal_view` and `markdown` are GPL-3 and are not
  used, so we write our terminal view and markdown view on permissive crates (`alacritty_terminal`,
  `pulldown-cmark`, `tree-sitter`).
- **Web content** *(review: GPUI has no web view)*: pages, artifacts and HTML files open in their own
  `wry` window, not inside a GPUI view.
- **Protocol**: written spec (JSON Schema) plus conformance and behavior fixtures, version negotiation,
  additive only. Dart and Rust types are generated from it.

Repo layout:

```
crates/engine/      transcripts, sessions, accounts, lanes, vault, bridge (library)
crates/notchlingd/  the engine service
crates/ptyhost/     the separately restartable terminal host (port of hostd)
crates/local-api/   app and CLI ↔ daemon API, with tiers
crates/app/         the GPUI desktop app
crates/platform/    notch and menu bar (macOS), tray and panel (Linux, Windows)
crates/kit/         our widget layer over GPUI
apps/flutter/       the phone app (today's mobile/)
legacy/macos/       today's Swift app and ConsoleCore, fixes only, until retired
protocol/           spec, JSON Schema, fixtures
docs/  tools/  viz/
```

## Swift and Rust side by side *(review)*

Both run on the maintainer's Mac during Milestones 1–5 and would fight over single-owner resources. One
owner per resource per milestone; the Swift app reads a stand-down setting per area (the bridge switch
already works this way):

| Resource | Owner until | Then |
| --- | --- | --- |
| Hook entries in `~/.claude/settings.json` | M2 | the daemon; retirement rewrites the hook command to the new binary |
| hostd LaunchAgent `local.agentconsole.hostd`, `hostd.sock` | Swift retirement | ported pty host, same socket contract |
| Phone bridge ports 47821/47822, `AgentConsole/bridge/` | M3 | the daemon, carrying `cert.pem`/`key.pem` byte for byte so phones don't re-pair |
| `vault.sock`, vault Keychain item | M5 | the daemon (one Keychain prompt on handover) |
| Limit watching and auto-switch | M5 | the daemon; never both |
| Lanes, account switching | M5 | the daemon; lane Keychain names `Claude Code-credentials-<sha256(path)[:8]>` reproduced exactly |

Retirement unloads the Swift LaunchAgents, rewrites the hook commands and leaves a backup.

## Parts

A part is done when its **ported tests** pass (not just the old fixtures) and its behavior fixtures match
the Swift app *(review)*. Before E3, a **recording phase** captures terminal byte fixtures for every reply
behavior in CLAUDE.md's "Things that bit before" (clear-input key order, pauses, forkpty, silent
device-attribute queries, `NO_COLOR`, the dialog check before typing, `takeBackQueued`, Esc only while
working), while the Swift app can still produce them, plus a Rust live-test mode like `ZLiveReplyTests`.

### Engine (Rust)

| # | Part | What moves | Protected |
| --- | --- | --- | --- |
| E1 | Transcripts | Claude, Codex and Antigravity readers, subagents, background shells, search matching (bounded: huge lines skipped, tail reads) | |
| E2 | Sessions | Jobs across lanes, `state.json`, `/clear` and worktrees (`resumeSessionId`) | |
| E3 | Terminal and replies | pty host, `claude attach`, typing replies, reading question dialogs | yes |
| E4 | Hook | The `--hook` process, the daemon's pass rule | yes |
| E5 | Phone bridge | TLS, pairing, tunnel, protocol, existing certificates kept | yes |
| E6 | Other tools | Codex runner, Antigravity (ACP), Delegate, second opinions | yes |
| E7 | Pages and search index | Artifact library, owner publish (protected), search index | owner publish |
| E8 | Accounts | Providers, limits, auto-switch, lanes, cost tracking | yes |
| E9 | Vault | Keychain item, vault service, redactor, CLI | yes |
| E10 | Machines | Kept as a separate tool, not inside the daemon; ported last *(review)* | yes |
| E11 | Session control | New Session, Trust Folder, Stop, Remove, moves, takeover, commands, resume after wake, git status | yes |
| E12 | Keep awake and system | Awake, sleep and wake, notifications, file watching, 0600 files | |
| E13 | Installing into other tools | Hooks in `settings.json` (backup, others kept), skills | yes |
| E14 | Settings and data | Settings store, migration enumerated from code (all 30 keys), data folders per OS | yes |
| E15 | Phone extras | Visualize, photos, web snapshots, streaming | |
| E16 | Command line and services | One binary, its modes as clients; service install per OS; installers | yes |
| E17 | Local API | Tiers, peer identity, audit log, events, starting the daemon | yes |

### Desktop app (GPUI)

| # | Part | What it covers |
| --- | --- | --- |
| U0 | Kit and shell | Widget layer, window, tray and menu bar, Dock presence, light and dark |
| U1 | Chat | Sidebar, conversation, our markdown view, composer, inspector, cards, file panel, terminal sheet |
| U2 | Notch | Overlay, rows, cards, Peek, Tab Perch, pet; top panel on Linux and Windows |
| U3 | Search | Global hot key panel |
| U4 | Pages | Library window; pages in their own `wry` window |
| U5 | Accounts, Vault, Machines | Windows and sheets |
| U6 | Phone pairing | QR and code, paired phones, bridge switch, tunnel status |
| U7 | Pop-ups outside Chat | Vault approval (not fakeable), request form, banners, Trust Folder |
| U8 | Sheets | New Session, Add Account, Settings, Second Opinion |
| U9 | Usage and cost | Limit bars, cost |
| U10 | Rendering for agents | Offscreen PNG; if GPUI has no offscreen path, window capture as the fallback *(review)* |
| U11 | First run and updates | Hooks on first launch, migration from Swift, About, updates |

Also to build: Vietnamese input (Telex/VNI), selection across a reply, drag and drop, file previews per
OS, keyboard, the `?` part names, pointer cursors, and screen reader support at least equal to the Swift
app's before it retires *(review)*.

### Phone app (Flutter)

| # | Part | What it covers | When |
| --- | --- | --- | --- |
| P0 | Phone guard rails | `flutter analyze`, `flutter test`, goldens, folder AGENTS.md | Phase 2 |
| P1 | Generated protocol types | Dart and Rust types from the schema; no Rust in the phone *(my call)* | Phase 4 |
| P2 | Remote computers | One phone, several computers, sessions shown per computer | Milestone 3 |
| P3 | Rename and docs | Notchling name, build-it-yourself guide with no maintainer-specific signing or tunnel | Any time |

### Milestones

1. **Read-only viewer**: E1, E2, E12, E14, U0, U10, read-only Chat and a read-only notch. Only reads files.
2. **Daemon reads, app shows**: E16's service, E17, the daemon serving sessions to the app.
3. **Daemon acts**: E3, E4, E11, U2, U7, U8 (after the recording phase).
4. **Phone**: E5, E15, U6, P2.
5. **The rest that's safe**: E6, E7, U3, U4.
6. **Logins and secrets**: E8, E9, E13, U5, U9, U11; E10 last. Then the Swift app retires.

Claude Code background sessions exist on Linux and Windows (`--bg`, jobs under `~/.claude/jobs/`;
Windows uses named pipes) *(review)*; only `attach` over Windows ConPTY needs the spike.

## Phases

1. **GPUI spike** (throwaway, first, before anything moves) *(review)*: notch overlay, `wry` window
   beside GPUI, Vietnamese input, offscreen render or capture, `claude attach` over ConPTY. Any "no" →
   the maintainer decides again.
2. **Guard rails and quality**: rulesets as code, CODEOWNERS default-protected, `maintainer-gate`,
   capability check, test-weakening check, the lint with its baseline, phone checks (P0), security
   settings, Dependabot, SHA-pinned actions, spend caps.
3. **Operating manual**: AGENTS.md (+ one-line CLAUDE.md), goals, architecture and decision records,
   rules catalog, GOVERNANCE, automation and privacy policies, dev profile, first skills, feature map.
4. **Repo layout and protocol**: `legacy/macos/`, `apps/flutter/`, Cargo workspace, protocol spec,
   generated types (P1).
5. **Agents on**: create the GitHub App; separate agent user and push wrapper on workers; sd label prefix;
   triage Action; the worker for `notchling:queued` in the auto-merge zone; the review agent
   recommend-only. A week of sampling before widening.
6. **Releases**: Developer ID and notarization, build/sign split, Beta feed, Sparkle 2.6.4+, kill switch
   variable, upgrade/downgrade and smoke tests, Stable by one click.
7. **Recording phase**, then **Milestones 1–6**, with Linux and Windows builds as each lands.
8. **Retire the Swift app** in one release.

Turned on last, when their conditions hold: automatic Stable (telemetry and minimum installs), review
agent auto-merge for outside PRs (20 agreed PRs).

## Constraints

- Tests never touch Keychain, real logins, ssh or `gh` (existing rule).
- CI jobs that run repository code have no secrets; the model review key and signing keys live in jobs
  that run no repository code.
- macOS runners: free for public repos but about 5 at once; no merge queue on a user-owned repo, so
  "require up to date" stays off and the head-commit binding does that job.

## Left to the builder

Workflow file layout, the lint's language, the review prompt and its threshold, the capability check's
exact patterns, label colors, the order of parts inside a milestone.

## Calls I made (check these)

- Exactly which files are in the auto-merge zone (listed above).
- 14 days before an outside PR that isn't ours is rebuilt; 20 agreed PRs before the review agent may
  auto-merge; 3 unsampled days before automation pauses.
- Phone stays free of Rust; protocol types are generated.
- Machines stays a separate tool, not part of the daemon.
- Notch becomes a top panel on Linux and Windows.
- The app reads transcripts directly, read-only.
