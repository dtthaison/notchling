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
- Shipping Linux or Windows apps in this plan's first phases; the layout only makes room for them.
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
3. **Work.** The sd worker takes `agent-ready` issues, branches `agent/<issue>-<slug>`, implements with
   tests, runs `swift test` and the probe renders locally, and opens a PR that links the issue and
   lists what it verified.
4. **Gates (all required checks on `main`):**
   - `swift test` (exists today).
   - Build both architectures with `./build.sh` (the app, not just tests).
   - Architecture rules (see below) as a lint step.
   - Cross-model review: a reviewer on a different model than the author. *(Where it runs depends on
     open question 1; it never needs a real Codex/Antigravity login in CI. In CI it is an API call with a
     key in Actions secrets; on the MacBook it is `notchling-delegate`.)* The author model is named in a
     PR trailer `Agent-Model:`. Blocking findings fail the check.
   - Protected-path check: if the diff touches a protected path, the PR is labeled
     `needs-maintainer` and auto-merge is refused. CODEOWNERS makes the maintainer a required reviewer
     for those paths, so this holds even if the label step is bypassed.
5. **Merge.** Auto-merge (squash) when every required check is green and no protected path is touched.
   Otherwise it waits for the maintainer.
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
`agentconsole.v1` protocol. Linux and Windows may come later.

Shape: **one engine per computer, many apps talking to it.**

- **Engine** (`notchlingd`): finds Claude Code (and Codex, Antigravity) sessions, reads transcripts,
  hosts terminals, keeps the vault and logins, serves the protocol to local and remote apps. **Rust**
  *(maintainer's decision, over Go)*, so one engine runs on macOS, Linux and Windows (`portable-pty`, `keyring`, `notify`, `tokio`/`rustls`,
  `sysinfo`, `keepawake`, `tray-icon`).
- **Apps**: the native macOS app stays Swift (the notch is Mac-only) and calls the engine through
  UniFFI or the protocol; one Flutter app covers iOS, Android, and later Linux and Windows desktops
  *(my call: Flutter over Tauri or a Rust UI, since the phone app is already Flutter; it calls the engine
  through `flutter_rust_bridge` or the protocol)*.
- **Protocol** is the contract between them: a written spec (JSON Schema) plus conformance fixtures
  every engine and app must pass. It stays additive (older apps ignore new fields), as today.

*(My call, not yet confirmed: on Linux and Windows the app watches and controls the sessions running on
that same computer, which is why the engine moves to Rust. If those desktops only need to view the
Mac's sessions, the Flutter desktop app needs no engine there and the Rust port can wait.)*

Repo layout (one repo):

```
apps/macos/        Swift app: notch, menu bar, Chat (today's Sources/Notchling)
apps/flutter/      one Flutter app: iOS, Android, then Linux and Windows (today's mobile/)
engine/            Rust workspace: crates per job (sessions, transcripts, pty, bridge, vault,
                   accounts) and the notchlingd binary
engine-swift/      today's ConsoleCore + hostd, kept until the Rust engine replaces each part
protocol/          agentconsole spec, JSON Schema, conformance fixtures
docs/  tools/  viz/
```

The Rust port goes piece by piece behind the protocol and never as one switch: first a headless
`notchlingd` on Linux (sessions, transcripts, bridge), which the Mac doesn't depend on; then the Mac app
moves one piece at a time, with the Swift piece deleted once its Rust replacement passes the same
fixtures. Vault and account code is ported last and stays a protected path in both languages.

Protected paths gain: `protocol/` (a careless change breaks paired phones), and `engine/` crates for
vault, accounts and pty.

## Phases

1. Guard rails first: CODEOWNERS, branch protection with required checks, protected-path check,
   `sd:queued`/`needs-maintainer` labels, auto-merge on. Nothing agent-run turns on before this.
2. Quality checks: build job, architecture lint with its fixtures, cross-model review job.
3. Repo layout: move into `apps/`, `engine-swift/`, `protocol/`; write the protocol spec and its
   conformance fixtures from today's Swift and Dart code. Done before the AGENTS.md split, since folders
   move.
4. Agent context: CLAUDE.md becomes a short root AGENTS.md plus one per feature folder; feature map.
5. Turn on the worker for `sd:queued` issues in the auto-merge zone only; watch a week of sampling.
6. macOS app into Swift modules, one folder at a time (protected ones need the maintainer).
7. Rust engine: headless `notchlingd` on Linux first, then piece by piece on the Mac.
8. Flutter desktop builds for Linux and Windows against `notchlingd`.

## Constraints

- CI runs on GitHub's macOS runners; no secrets in CI beyond `GITHUB_TOKEN` and the review model's API
  key. Tests never touch Keychain, real logins, ssh or `gh` (existing rule).
- The reviewer key, if any, lives in GitHub Actions secrets, set by the maintainer.
- Changes to the user's real config (`~/.claude/settings.json`, Keychain) stay out of scope for agents.

## Open questions (the maintainer's)

1. **Where the work lands.** Today the worker has a read-only deploy key ("Push is refused" must pass)
   and the MacBook merges in Morning Actions (`MergePlan` pushes `main` itself). Either keep that and
   add the gates to `MergePlan`, or give the worker push rights to `agent/*` branches and merge through
   GitHub PRs and auto-merge. Phases 1, 2 and 4 depend on this.
2. **Whose GitHub identity** opens and merges agent PRs: the maintainer's or a bot account. A PR by the
   maintainer's own account can't satisfy a CODEOWNERS review.
3. **Stranger PRs.** Whether a fork PR that stays in the safe zone may auto-merge, or only the agent's own.

## Left to the builder

Exact workflow file layout, the lint's implementation language, the review prompt and its blocking
threshold, the module split order outside protected areas, and label colors.

## Calls I made (check these)

- Phone bridge counts as protected.
- The guard rails themselves (CI, CODEOWNERS, build/release scripts, agent instructions) are protected.
- `agent-ready` is added only by the maintainer, not by the triage agent.
- Squash merge for agent PRs.
