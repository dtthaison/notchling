# Notchling as an agent-run open-source project

Date: 2026-11-07. Status: draft for approval.

## Intent

Notchling is public (`github.com/dtthaison/notchling`). The maintainer wants it to run like a well-kept
open-source project in which agents do most of the work: they take issues, add features, fix bugs and open
pull requests, and guard rails decide whether a change may land. The maintainer's job moves from reviewing
every PR to shaping the rules, checks and skills the agents work inside, and sampling what landed
(poteto's "head chef" model from the Pstack notebook).

Why now: the repo just went public, so issues will come from strangers, and the sd worker on `m1` and
`vps-sing-1` can already pick up GitHub issues overnight.

## Goals

1. An issue can go from filed to merged with no human step, when it only touches safe areas.
2. Quality holds or improves as agent PRs grow: every rule that matters fails CI instead of living in a doc.
3. The codebase is shaped so an agent working in one feature reads and changes only that feature.
4. Mistakes found after merge turn into a check, a test or a skill, so the same mistake can't land twice.

## Non-goals

- Notarization, App Store, or a website.
- Shipping Linux or Windows apps before the Rust engine and the GPUI app exist.
- Replacing the sd worker or the Machines window; this design uses them.
- A full rewrite. The re-architecture is incremental and every step ships green.

## Anti-goals (a failure even if it works)

- An issue from a stranger steering an agent into accounts, Keychain, the vault, lanes, Machines or the
  hook, and that change merging without the maintainer.
- Agents weakening their own guard rails (editing CI, CODEOWNERS, the protected-path list, or the agent
  instructions) and merging that.
- A secret, token or real login touched by CI or by a test.
- A green CI that means nothing: checks that pass because they test too little.

## The line (decided by the maintainer)

Agents may merge on their own **except** in the login and secrets areas, which always wait for the
maintainer. Concretely, the protected paths are:

| Area | Paths |
| --- | --- |
| Accounts and logins | `Sources/Notchling/Accounts/`, `Sources/Notchling/Providers/`, `Sources/Notchling/Lanes/`, `Sources/Notchling/Limits/` |
| Secrets | `Sources/Notchling/Vault/` |
| Machines (ssh, sudo, deploy keys) | `Sources/Notchling/Machines/` |
| Hook (runs inside every Claude Code prompt) | `Sources/Notchling/Hook/`, `Sources/ConsoleCore/HookResponder.swift` |
| Phone bridge (TLS, pairing, tokens) | `Sources/Notchling/Phone/`, `Sources/ConsoleCore/Bridge/` *(my call: it holds tokens and a network listener)* |
| Process launching | `Sources/Notchling/Delegate/`, `Sources/Notchling/Terminal/`, `Sources/hostd/`, `Sources/ConsoleCore/Hostd/`, and any file that reads or writes `~/.claude/settings.json` *(added after the builder check)* |
| The guard rails themselves | `.github/`, `CODEOWNERS`, `build.sh`, `release.sh`, `Package.swift`, `Info.plist`, every `AGENTS.md`, `tools/`, `.sd/` *(my call: an agent must not loosen its own rules)* |

Everything else (Chat, Notch, Search, Pages, Terminal, DesignKit, Delegate, Agents, the rest of
ConsoleCore, docs, tests, fixtures) is the auto-merge zone.

## The flow

1. **Intake.** A stranger files an issue through the existing templates. A triage agent (GitHub Action,
   read-only token) labels it `bug`/`enhancement`, asks for missing repro info, closes duplicates, and
   marks it `needs-triage`. It never starts work.
2. **Approval to work.** Only the maintainer adds `sd:queued`, the label the sd worker already takes
   (its other labels: `sd:ready`, `sd:needs-you`, `sd:stuck`; this design adds no new work labels). Issue text from
   anyone else is treated as data, not instructions: the worker's prompt quotes it and says so.
3. **Work.** The sd worker takes `sd:queued` issues, branches `agent/<issue>-<slug>`, implements with
   tests, runs the checks and renders locally, pushes the branch and opens a GitHub pull request that
   links the issue and lists what it verified. *(Maintainer's decision: work lands through GitHub pull
   requests.)* This replaces the worker's read-only key and the MacBook merge: the worker may push
   `agent/*` branches only, never `main` (branch protection enforces it), and the Machines setup's
   "Push is refused" step becomes "Push only to agent/*". Morning Actions' `MergePlan` merge is
   retired; Morning Actions keeps showing what landed, for sampling.
4. **Gates (all required checks on `main`):**
   - `swift test` (exists today).
   - Build both architectures with `./build.sh` (the app, not just tests).
   - Architecture rules (see below) as a lint step.
   - Cross-model review: a reviewer on a different model than the author, as a GitHub Action calling a
     model API with a key in Actions secrets (never a real Codex/Antigravity login in CI). The author model
     is named in a PR trailer `Agent-Model:`. Blocking findings fail the check.
   - Protected-path check: if the diff touches a protected path, the PR is labeled
     `needs-maintainer` and auto-merge is refused. CODEOWNERS makes the maintainer a required reviewer
     for those paths, so this holds even if the label step is bypassed.
5. **Merge.** GitHub auto-merge (squash) for the agent's own PRs when every required check is green and
   no protected path is touched. Otherwise it waits for the maintainer.
   **Pull requests from other people** *(maintainer's decision)* are welcome and get the same checks and
   the same agent review, but never merge on their own: the maintainer reviews each one against the
   agent setup (AGENTS.md, the lint, the protected paths) and merges it. Fork PRs get no secrets, so the
   agent review of a fork PR runs from a trusted workflow that reads the diff and never checks out or
   runs the fork's code *(my call)*.
6. **Sample and sharpen.** Each morning the maintainer samples merged PRs. Anything bad becomes a new
   check, test, lint rule or skill change (pstack `reflect`/`recall`), never just a review comment.

Release stays as today: `./release.sh` opens `release/<version>`, a protected path, so releases are
always the maintainer's.

## Quality: rules that fail the build

Move rules from CLAUDE.md into an architecture lint (a script under `tools/`, run in CI and locally):

- `ConsoleCore` imports no Notchling types and uses no macOS 14+ APIs.
- No force-unwraps outside probes and tests.
- Views use `Kit*` tokens, not raw colors or sizes, in `Chat/` and `Notch/`.
- Every new UserDefaults key is in `AppDelegate.migrateFromAwake()`'s list.
- Clickable SwiftUI views use a kit style or `.pointerCursor()`.
- No `osascript`, no `NSAppleScript` notifications.
- Feature-folder import rules (once modules exist, the compiler enforces them; until then, the lint).

Each rule has a fixture test showing it catches a bad example. *(My call after the builder check:)* the
lint fails only on new or changed lines; existing violations (for example `osascript` in
`Limits/LimitWatcher.swift` and `Agents/AgentActions.swift`) go in a checked-in baseline file that may
only shrink. The lint, its fixtures and the baseline are protected paths. A PR that deletes or weakens an
existing test (fewer test functions, removed assertions) is labeled `needs-maintainer`. A rule added after an incident names the
incident in its message.

## Agent-friendly architecture

Findings: all of `Sources/Notchling` is one module, so any folder can reach into any other; CLAUDE.md is
48 KB (~12k tokens) loaded into every session; Chat (9.1k lines) and Machines (8.3k lines) are the heavy
areas, with files up to 1,200 lines.

Target:

1. **AGENTS.md, not CLAUDE.md** *(maintainer's decision)*. One set of instructions every tool reads:
   Claude Code (2.1.319's default `instructionFiles` loads AGENTS.md where a project has no CLAUDE.md,
   exactly where and how CLAUDE.md would load), Codex and Antigravity. So CLAUDE.md is renamed, not kept
   beside it: any CLAUDE.md in the repo turns the AGENTS.md loading off for Claude Code. The root
   AGENTS.md is short (what the app is, build/test, the protected-path line, where notes live); each
   feature folder gets its own `AGENTS.md` with today's per-area paragraphs, which every tool loads only
   when it works in that folder. Things that point at CLAUDE.md (README, CONTRIBUTING, docs, skills such
   as sd's, which look for a CLAUDE.md block) are updated in the same change.
2. **Feature map.** Generated from the 124 `.part("Name")` tags: part name → file and line, checked in
   CI to be current. This is the "where is the thing the user named" index.
3. **Modules per feature, gradually.** Turn feature folders into SwiftPM targets in `Package.swift` and
   `build.sh`, leaf-first, so the compiler enforces boundaries. Order is left to the builder, but the
   protected areas (Accounts, Vault, Machines) should become modules early so their surface is explicit.
4. **File size budget.** Lint warns over 600 lines, fails over 1,000 for new files; existing big files
   are split when next touched.
5. **Verification skill.** `render-notch-view` plus `swift test` is the agent's "eyes"; extend it to
   render the Chat parts a PR touched and attach the PNGs to the PR.

## Multi-platform architecture

Today: the macOS app (Swift, ~55k lines), its engine `ConsoleCore` + `hostd` (Swift, ~7.4k lines), and a
Flutter app for iOS and Android (`mobile/`, ~15.6k lines) that knows the Mac only through the
`agentconsole.v1` protocol.

Decided by the maintainer: **Rust for the engine, GPUI (Zed's UI framework) for every desktop app,
the Mac included.** The Swift app is replaced, not kept beside it.

- **Engine** (`notchlingd` and library crates): finds Claude Code, Codex and Antigravity sessions, reads
  transcripts, hosts terminals, keeps the vault and logins, serves the protocol to the phone. Same code on
  macOS, Linux and Windows (`portable-pty`, `keyring`, `notify`, `tokio`/`rustls`, `sysinfo`,
  `keepawake`).
- **The engine is a background service on every computer** *(maintainer's decision)*. `notchlingd`
  runs always (LaunchAgent on macOS, systemd user service on Linux, a startup task on Windows) and owns
  everything that acts or lives long: terminals and replies, session control, the phone bridge, the
  vault, accounts and lanes, limit watching, keep awake. The desktop app, the phone, the hook and the
  command line are all its clients. So the phone and running terminals survive the app quitting,
  updating or crashing (which matters while the GPUI app is young), and a server runs the same binary as
  a laptop. It absorbs today's hostd.
- **How the desktop app talks to it**: a local API over a Unix socket (a named pipe on Windows), only
  for this user (peer check, as the vault socket does today), with the same Rust types on both ends.
  It is richer than the phone protocol (accounts, vault, Machines, settings) and versioned the same way.
  For speed, the app reads transcripts and session files itself with the engine library, read-only; it
  never writes or acts except through `notchlingd`. *(My call: read-only direct, everything else over the
  socket.)* If `notchlingd` isn't running, the app starts it and says so.
- **Desktop app**: one GPUI app for macOS, Linux and Windows, in the same Cargo workspace as the engine.
  Platform pieces live behind small traits with one implementation per
  OS: the notch overlay and menu bar on macOS (through `objc2`), a tray and a top-of-screen panel on Linux
  and Windows *(my call: no notch there, so the notch views become a small always-on-top panel)*.
- **Phone app**: stays Flutter (GPUI has no iOS or Android). It keeps speaking the protocol to
  `notchlingd`.
- **Protocol** is the contract between engine and phone: a written spec (JSON Schema) plus conformance
  fixtures. It stays additive, as today.
- **On Linux and Windows the app controls that computer's own sessions** *(maintainer's decision)*,
  and **computers sync with each other** the way the phone and the Mac do today: each `notchlingd` can
  pair with others over the same protocol, so any desktop app or phone shows and drives the sessions of
  every paired computer. *(Was: my call, still to confirm: on Linux and Windows the app watches and controls the sessions on that same
  computer.)*

GPUI facts that shape the plan:
- Pre-1.0 and developed for Zed: pin an exact revision, upgrade on purpose, and keep our own thin widget
  layer (the ChatKit equivalent) so a GPUI change touches one place.
- Desktop only: two UI codebases from now on (GPUI desktop, Flutter phone).
- Zed's own crates can be reused where they fit, for example its terminal view built on
  `alacritty_terminal` in place of our `TerminalScreen`.

Risks to settle with a throwaway spike before any port (each a yes/no):
1. The notch: a borderless, transparent, click-through window placed around the camera, with the
   mouse-test toggling of `ignoresMouseEvents` we do today. GPUI has popup and transparent windows; a
   notch overlay is untested.
2. Web content: Pages, artifact previews and the HTML file viewer need a web view. GPUI has none;
   the options are embedding `wry` (WKWebView, WebView2, WebKitGTK) in a GPUI window or opening pages in
   their own window.
3. Previews the Mac gets from the OS today (PDFKit, AVKit, Quick Look) and native notifications: what
   replaces each, per OS.
4. Rules that carry over unchanged: Keychain through `/usr/bin/security`, never the Security framework,
   for account logins (ad hoc builds lose access on rebuild), and the vault's opposite rule (trusts only
   this build); the hook stays a short-lived process that never hangs Claude Code.

Repo layout (one repo, one Cargo workspace):

```
crates/engine/      sessions, transcripts, accounts, lanes, vault, pty, bridge (library)
crates/notchlingd/  the engine service, on every computer (absorbs hostd)
crates/local-api/   the app ↔ notchlingd API types and client
crates/app/         the GPUI desktop app (macOS, Linux, Windows)
crates/platform/    per-OS pieces: notch and menu bar (macOS), tray and panel (Linux, Windows)
crates/kit/         our widget layer over GPUI (today's ChatKit)
apps/flutter/       the phone app (today's mobile/)
legacy/macos/       today's Swift app and ConsoleCore, frozen to fixes until retired
protocol/           spec, JSON Schema, conformance fixtures
docs/  tools/  viz/
```

The port goes area by area, each one done when it matches the Swift app's behavior on the existing test
fixtures (transcripts, terminal recordings, `Fixtures/`). The Swift app keeps shipping releases until the
GPUI app covers every area; then it is retired in one release. Accounts, lanes and the vault are ported
last and stay protected paths in Rust.

Protected paths gain `protocol/`, `crates/engine` modules for accounts, lanes, vault and pty, and
`crates/platform`.

## Parts

Each part is done when the Rust version passes the same fixtures the Swift version passes today
(transcripts, terminal recordings, protocol messages in `Fixtures/`). Each part gets its own detailed
plan when its turn comes, since what the early parts teach about GPUI changes the later ones. Sizes are
today's Swift lines. "Protected" parts wait for the maintainer.

### Engine (Rust)

| # | Part | What moves | Size | Protected |
| --- | --- | --- | --- | --- |
| E1 | Transcripts | Claude, Codex and Antigravity readers, subagents, background shells, search matching | ~4k, pure | |
| E2 | Sessions | Finding jobs across lanes, `state.json`, following `/clear` and worktrees (`resumeSessionId`) | ~2k | |
| E3 | Terminal and replies | pty, `claude attach`, typing replies, reading question dialogs, hostd; Zed's terminal replaces `TerminalScreen` | ~3k | yes |
| E4 | Hook | The `--hook` process; must never hang Claude Code | ~0.5k | yes |
| E5 | Phone bridge | TLS, pairing, tunnel, the protocol, served by `notchlingd` | ~4k | yes |
| E6 | Other tools | Codex runner, Antigravity (ACP), Delegate, second opinions | ~2.5k | Delegate |
| E7 | Pages and search index | Artifact library, owner publish, the search index | ~2k | |
| E8 | Accounts | Four providers, limits, auto-switch at a limit (acts on its own, changes the live login), lanes, cost tracking | ~5k | yes |
| E9 | Vault | Keychain item, socket server, redactor, CLI | ~2.3k | yes |
| E10 | Machines | ssh setup steps, deploy keys, Morning Actions | ~8.3k | yes |
| E11 | Session control | New Session, Trust Folder, Stop, Remove, moves between accounts and Move All, takeover, slash and `!` commands, resume after wake, git status | ~3k | yes |
| E12 | Keep awake and system | Keeping the computer awake, sleep and wake, notifications with the banner fallback, file watching, 0600 files | ~1k | |
| E13 | Installing into other tools | Our hooks in `~/.claude/settings.json` (backup, others' hooks kept), skills for Claude Code, Codex and Antigravity | ~1k | yes |
| E14 | Settings and data | One settings store for all OSes, moving today's UserDefaults (and the old Awake keys), data folders per OS, the AgentConsole folder kept so paired phones survive | ~0.5k | yes |
| E15 | Phone extras | Visualize tab (`viz/`), photos from the phone, web snapshots, screen and window streaming | ~2k | |
| E16 | Command line and services | One binary with all its modes (`--hook`, `--page`, `--secret`, `--delegate`), as clients of `notchlingd`; `notchlingd` as LaunchAgent, systemd user service or Windows startup task (absorbs hostd); installers and updates | ~1k | yes |
| E17 | Local API | The app ↔ `notchlingd` socket: per-user peer check, versioned messages, events streamed to the app, starting the service when it isn't running | new | yes |

### Desktop app (GPUI)

| # | Part | What it covers |
| --- | --- | --- |
| U0 | Kit and shell | Our widget layer over GPUI (today's ChatKit), app window, tray and menu bar, Dock presence per OS, light and dark |
| U1 | Chat | Sidebar, conversation, markdown, composer, inspector, question and permission cards, file panel, terminal sheet |
| U2 | Notch | Overlay, rows, cards, Peek, Tab Perch, the pet; a top panel on Linux and Windows |
| U3 | Search | Global hot key panel |
| U4 | Pages | Library window and web view |
| U5 | Accounts, Vault, Machines | Their windows and sheets |
| U6 | Phone pairing | QR and one-time code, paired phones, bridge switch, tunnel status |
| U7 | Pop-ups outside Chat | Vault approval (must not be fakeable by another app), the vault request form, notice banners, Trust Folder |
| U8 | Sheets | New Session, Add Account, Settings, Second Opinion views |
| U9 | Usage and cost | Limit bars, cost by account and model |
| U10 | Rendering for agents | Offscreen render of any view to PNG (today's `render-notch-view`, KitGallery, README GIFs). Agents need it to check their own UI work, so it lands in Milestone 1 |
| U11 | First run and updates | Installing hooks on first launch, moving over from the Swift app (settings, paired phones, vault), About, updates |

What macOS gives the Swift app for free and the GPUI app must build or reuse:
- Input methods (Vietnamese Telex/VNI) in the reply box; Zed handles them on Mac, weaker on Linux and Windows.
- Selecting text across a whole reply (today a dedicated NSTextView per prose run).
- Markdown and code highlighting (reuse Zed's markdown and tree-sitter).
- Drag and drop of files and images, attachment thumbnails.
- File previews per OS (PDF, video, Quick Look, images).
- Keyboard: copy and paste, Esc, ⌘[, Tab in cards, the global hot key.
- Screen readers: GPUI has little support today; decide how much we own.
- The `?` button that names every part, from a GPUI equivalent of `.part("Name")`; the feature map
  depends on it.
- Pointer cursor on everything clickable.

### Phone app (Flutter)

Today: `mobile/`, ~15.6k lines (sessions, chat, pairing by QR, connection, Macs list, visualize, voice
with on-device Whisper, settings), 25 test files, English and Vietnamese. It knows the computer only
through the protocol, so it keeps working through the engine port as long as the protocol holds.

| # | Part | What it covers | When |
| --- | --- | --- | --- |
| P0 | Phone guard rails | `flutter analyze` and `flutter test` as required checks (CI tests none of it today), golden tests (screens to PNG) as the phone's rendering for agents, an `AGENTS.md` for the folder | Phase 2 |
| P1 | Generated protocol types | Dart and Rust types generated from the protocol's JSON Schema, checked by the shared fixtures; no Rust inside the phone (`flutter_rust_bridge` would complicate iOS and Android builds) *(my call)* | Phase 3 |
| P2 | Many computers | "Macs" becomes "computers": one phone paired with several engines on macOS, Linux and Windows | Milestone 3 |
| P3 | Rename and docs | "Agent Console" / `agent_console` becomes Notchling; a real README in place of Flutter's template, with the build-it-yourself guide | Any time, agents can take it |

Later, when people ask: a simpler way to reach a computer from outside home than one's own Cloudflare
tunnel and Access setup; the visualize tab, images, Stop and new sessions for Codex and Antigravity
sessions on the phone.

Distribution *(maintainer's decision)*: for now people build the phone app themselves; store releases
(App Store, Google Play) come later. So P3 includes a build guide that works from a fresh clone: Flutter
version, `flutter build ios` with one's own Apple team and bundle id, `flutter build apk` for Android,
and pairing with the computer. Nothing in the repo may assume the maintainer's signing team, bundle id
or Cloudflare setup.

### Milestones (each one a usable app)

1. **Read-only viewer**: E1, E2, E12, E14, U0, U10, Chat read-only. Lists sessions and shows
   conversations on Mac, Linux and Windows; only reads files, through the engine library, so no service
   is needed yet. Agents can take most of it.
2. **Reply**: E3, E4, E11, E16's service, E17, U2, U7, U8. `notchlingd` runs as a service; answer and
   drive sessions through it. Maintainer reviews.
3. **Phone**: E5, E15, U6, P2. `notchlingd` serves the phone; the Swift bridge is
   switched off. Maintainer reviews.
4. **The rest that's safe**: E6, E7, U3, U4. Agents can take most of it.
5. **Logins and secrets**: E8, E9, E10, E13, U5, U9, U11. Then the Swift app is retired. Maintainer
   reviews.

Open question for the spike: how Claude Code background sessions (`claude --bg`, `attach`,
`state.json`) behave on Linux and Windows and where their files live. If they don't exist there, the
Linux and Windows apps can show sessions but not drive them.

## Phases

1. Guard rails first: CODEOWNERS, branch protection with required checks, protected-path check,
   `sd:queued`/`needs-maintainer` labels, auto-merge on. Nothing agent-run turns on before this.
2. Quality checks: build job, architecture lint with its fixtures, cross-model review job, and the
   phone's guard rails (P0).
3. Repo layout and protocol: move into the layout above (Swift to `legacy/macos/`, Flutter to
   `apps/flutter/`), write the protocol spec and its conformance fixtures, and generate the Dart types
   from it (P1).
4. Agent context: CLAUDE.md becomes a short root AGENTS.md plus one per folder; feature map.
5. Turn on the worker for `sd:queued` issues in the auto-merge zone only; watch a week of sampling.
6. GPUI spike (throwaway): the notch overlay, a web view in a GPUI window, Vietnamese input in a text
   field, offscreen render to PNG, and Claude Code background sessions on Linux and Windows. If any
   fails, the maintainer decides again before phase 7.
7. Milestones 1 to 5 above, in order, engine and app parts together. Linux and Windows builds from the
   same app as each milestone lands.
8. Retire the Swift app in one release once the GPUI app covers every part.

Dropped from the earlier draft: splitting the Swift app into Swift modules (it is being replaced; it
gets fixes only).

## Constraints

- CI runs on GitHub's macOS runners; no secrets in CI beyond `GITHUB_TOKEN` and the review model's API
  key. Tests never touch Keychain, real logins, ssh or `gh` (existing rule).
- The reviewer key, if any, lives in GitHub Actions secrets, set by the maintainer.
- Changes to the user's real config (`~/.claude/settings.json`, Keychain) stay out of scope for agents.

## Agent identity

*(Maintainer's decision.)* Agents act on GitHub as a **GitHub App** installed on this repository only,
never as the maintainer's account. Agent PRs are authored by the App, so the maintainer can approve
them and CODEOWNERS holds protected PRs.

- Permissions: contents read and write (branch protection limits pushes to `agent/*`), pull requests
  read and write, issues read and write (labels, comments), checks read. Nothing else, no admin.
- Each worker mints a short-lived installation token (one hour) from the App's private key; the key
  lives in the Secret Vault on each worker machine (a new vault name, set up by the Machines window like
  today's `*-github-issues` tokens) and never in the repo or in CI.
- Branch protection on `main`: required checks, required review from code owners on protected paths,
  only the App may push `agent/*`, nobody pushes `main` directly, auto-merge allowed.
- The triage Action and the review Action use the workflow's own `GITHUB_TOKEN`, not the App.

## Left to the builder

Exact workflow file layout, the lint's implementation language, the review prompt and its blocking
threshold, the module split order outside protected areas, and label colors.

## Calls I made (check these)

- Phone bridge counts as protected.
- The guard rails themselves (CI, CODEOWNERS, build/release scripts, agent instructions) are protected.
- `agent-ready` is added only by the maintainer, not by the triage agent.
- Squash merge for agent PRs.
