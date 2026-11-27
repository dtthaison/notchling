# Version sync: every machine on the Mac's versions

- **Date:** 2026-11-13
- **Status:** Draft, for the maintainer. Nothing here is built. Revised on 2026-11-14 with the coordinator's calls after a builder's review ([Calls settled after review](#calls-settled-after-review)) and the user's answers to every open question ([Decided by the user](#decided-by-the-user)). No question is open.
- **Approach:** A, the Mac is the reference, chosen by the maintainer on 2026-11-13.
- **Base:** branch `agent/move-auto-trust` (8e73a97).
- **Touches:** decisions 0004 (Machines is a separate tool), 0007 (tiers), 0010 (releases and updates), 0011 (one owner per resource), 0012 and the [cluster design](2026-11-13-notchling-cluster-design.md) (review issue 10, version skew), 0019 on `agent/agents-setup-decision` (Settings > Agents), 0020 on `agent/agent-settings-cli` (setting classes), and the [logging design](https://github.com/dtthaison/notchling/blob/agent/logging-design/docs/superpowers/specs/2026-11-13-logging-design.md) on `agent/logging-design` (event kinds). It needs a decision record of its own and one line changed in `docs/goals.md` (Phase 0).

## In short

1. Each provider CLI (Claude Code, Codex, Antigravity's `agy`) and Notchling itself runs the **same version on the Mac and on every worker** Machines manages.
2. **The Mac is the reference.** The user updates there, with an Update button per tool in Settings > Agents or through the CLI's own auto-update, which stays on on the Mac.
3. Notchling **notices** the Mac's version changed by reading files (a symlink, a `package.json`), never by starting the tool. Machines asks the daemon for it, and uses it only while its check matches the binary the Mac has installed now.
4. It runs a **smoke check** on the Mac that starts no session and writes nothing *(user's decision 2)*: each tool's `--version`, `claude doctor`, Codex's `exec` flags, and Notchling's readers in strict mode over the transcripts already there. It can't see screen or dialog changes, so every result is **Partly Checked**: enough for a Roll Out click, never for Automatic. A scratch-session check and a paid turn are possible later additions.
5. On a pass, Notchling **rolls the exact version out**, one worker at a time, through Machines' runner. One **Roll Out click in Machines** approves a version for every worker. Automatic arrives only when the daemon runs rollouts (Phase 6), and never for agy or npm installs. No agent can change that setting (class Never).
6. On a worker it **waits until none of that tool's processes run** (Claude Code's supervised background sessions are moved, not waited for), checks that the worker's plan is still the one shown, installs, checks the exact version, and moves idle sessions over. It never stops a running turn.
7. A worker that fails is **rolled back** to its previous version, best effort, else marked Needs You. The rollout **halts**, the rest keep the old version, and a notice says what happened. Offline workers **catch up** when they come back. A worker ahead of the Mac is shown as drift and moved down only by its own click.
8. On workers each CLI's **self-update is off**, one key per tool written with a backup. The Mac's own config is never changed.
9. **Notchling on workers** follows the Mac's Notchling build from the same GitHub release. That needs per-target daemon builds and a signed manifest, which are release-path work for the maintainer.
10. **Approach B** (known-good versions per Notchling release) has a slot: a `Gate` checked before the Mac step, which is a pass-through in A.

## Calls settled after review

A builder read the first draft, and the coordinator settled the points below on 2026-11-14. Each is marked *coordinator's call* here and in the sections it changes. The review's question 5, how the smoke check is set up, went to the user with the other open questions; the user's answers are in [Decided by the user](#decided-by-the-user).

- **1. Machines reads the reference from the daemon** *(coordinator's call)*. In Phases 1 to 4, Machines asks the daemon with `versions.get` (read only, agent tier). If the daemon is down or doesn't answer, Machines shows "Notchling isn't running" and starts no rollout. A reference is usable only when its check ran against the version the Mac has installed now: the same version string and the same binary (its SHA-256). Otherwise it is stale and is checked again before any rollout. Sections [7](#7-reading-a-version), [11.4](#114-results), [18](#18-where-the-code-goes).
- **2. One Roll Out click approves** *(coordinator's call)*. The Roll Out click in the Machines window approves that version for every worker. Before each worker's install, Machines rebuilds that worker's plan and compares it with the plan it showed; any difference halts the rollout at that worker as Needs You, and workers already done stay done. In Phases 1 to 4 Inbox items are notices with Open Machines; Automatic exists only from Phase 6, when the daemon runs rollouts. Sections [12](#12-the-rollout), [17.3](#173-inbox), [20](#20-settings-keys).
- **3. First run, workers ahead, partial checks** *(coordinator's call)*. Turning the feature on checks the Mac's current version and offers a rollout; the first one is never automatic. A worker ahead of the Mac is drift: shown, never moved down automatically. Match the Mac downward takes its own click, with a warning that `minimumVersion` or a managed `requiredMinimumVersion` can stop the older version. "Passed, partly" (acting off) never qualifies for Automatic; it allows a Roll Out click, labelled Partly Checked. Sections [10](#10-the-mac-step), [11.4](#114-results), [14](#14-offline-workers-and-drift).
- **4. Each worker has its own tool list** *(coordinator's call)*. A worker's tools are the ones it has installed. A tool it lacks shows Not Installed and is skipped: the rollout never installs it, and installing Codex or agy on a worker is a separate Machines step, outside this design. Done and drift ignore absent tools. A version check never needs a login. Sections [7](#7-reading-a-version), [8](#8-installing-an-exact-version).
- **6. The knob goes in every place** *(coordinator's call)*. Claude Code's self-update knob is written in every Claude Code config folder Machines knows for that worker (the ssh user's `~/.claude` and each lane's `CLAUDE_CONFIG_DIR`), and set as an environment variable in the sd service files Machines writes. The probe reports each place and flags any it can't read. Section [9](#9-self-update-off-on-workers).
- **7. Only supervised sessions are moved** *(coordinator's call)*. Only Claude Code's background sessions under its supervisor are moved to the new binary. Anything else (a plain `claude` in tmux, `claude -p` jobs, cluster jobs) counts as busy: that tool's install on that worker waits until none of its processes run, shown as "Waiting for 3 processes" with a Cancel. Section [13](#13-running-sessions-during-an-update).
- **8. Rollback is best effort** *(coordinator's call)*, then Needs You. agy and npm installs are never Automatic. Sections [15](#15-failure-and-rollback), [20](#20-settings-keys).
- **9. One rollout at a time, by a lock** *(coordinator's call)*. A lock row in the rollout journal in `notchling.db` holds the owner's pid and a heartbeat; a stale lock is taken over. A rollout stopped by closing the window resumes from the journal the next time Machines opens, through a Resume Rollout prompt, never silently. Section [12](#12-the-rollout).
- **10. A mismatch check in Phase 1** *(coordinator's call)*. Phase 1 ships a read-only version-mismatch check. The cluster's step-1 version handshake and `delegate --machine` use it to warn; refusing dispatch stays in Phase 6. Sections [17.5](#175-dispatch), [23](#23-phases).

## Decided by the user

The user answered every open question on 2026-11-14, each as recommended. Each answer is marked *user's decision* here and in the sections it changes. No question is open ([27](#27-open-questions)).

- **1. Vendor installers on workers** *(user's decision)*. Notchling manages only the vendors' own installers on workers: Claude Code's native installer, and Codex's and agy's standalone ones. A worker whose tool came from Homebrew gets a Fix that switches it. npm stays allowed for Claude Code and Codex, installed only while no process of that tool runs, and never Automatic. Sections [8](#8-installing-an-exact-version), [17.2](#172-machines).
- **2. The smoke check is files and flags only** *(user's decision)*: each tool's `--version`, `claude doctor`, `codex exec --help`, and Notchling's readers in strict mode over the transcripts already on the Mac. Nothing is started and nothing is written to the user's config, and every result is labelled Partly Checked. A scratch pty session and a paid turn are not built now; they stay described as possible later additions, each behind its own setting. Sections [11](#11-the-smoke-check), [12](#12-the-rollout).
- **3. The soak is shown, never gating** *(user's decision)*: in Settings > Agents and on the rollout item. Section [11.3](#113-strict-mode-and-the-soak).
- **4. Sparkle for the Rust app** *(user's decision)*, with the existing appcast and EdDSA key. The same key signs the worker manifest: one key, one offline backup. A new decision record amends 0010, and it waits for the Developer ID. Section [16](#16-notchling-itself).

**What decision 2 means for approval.** The built check never gives a full pass, so every reference is Partly Checked. Ask, the setting once the feature is on, works with that: the Roll Out sheet says Partly Checked, with what wasn't checked, and one click rolls out. Automatic (Phase 6) stays unavailable while the check is partial, per call 3: Settings shows it off with "Needs a full check" until a later addition ([11.2](#112-possible-later-additions)) makes full checks possible.

## Contents

1. [Why](#1-why)
2. [Goals, non-goals, anti-goals](#2-goals-non-goals-anti-goals)
3. [Constraints](#3-constraints)
4. [Approaches](#4-approaches)
5. [What exists today](#5-what-exists-today)
6. [Terms](#6-terms)
7. [Reading a version](#7-reading-a-version)
8. [Installing an exact version](#8-installing-an-exact-version)
9. [Self-update off on workers](#9-self-update-off-on-workers)
10. [The Mac step](#10-the-mac-step)
11. [The smoke check](#11-the-smoke-check)
12. [The rollout](#12-the-rollout)
13. [Running sessions during an update](#13-running-sessions-during-an-update)
14. [Offline workers and drift](#14-offline-workers-and-drift)
15. [Failure and rollback](#15-failure-and-rollback)
16. [Notchling itself](#16-notchling-itself)
17. [What the user sees](#17-what-the-user-sees)
18. [Where the code goes](#18-where-the-code-goes)
19. [Events](#19-events)
20. [Settings keys](#20-settings-keys)
21. [Security and privacy](#21-security-and-privacy)
22. [Tests](#22-tests)
23. [Phases](#23-phases)
24. [The slot for approach B](#24-the-slot-for-approach-b)
25. [Left to the builder](#25-left-to-the-builder)
26. [What was checked on the web](#26-what-was-checked-on-the-web)
27. [Open questions](#27-open-questions)

## 1. Why

Notchling reads what each CLI writes and draws, and a new CLI version can change any of it:

- **Transcripts and queue lines.** Chat, the notch and the phone parse Claude Code's transcript lines, Codex's rollouts and ACP updates. Claude Code 2.1.322 changed how it takes queued entries (it took a notification before older ones; `crates/AGENTS.md`, "Things that bit before"), and Chat showed messages stuck in Queued until the parser learned the new shape.
- **Dialogs on the screen.** Replies, question answers and permission answers read Claude Code's terminal: the dialog markers, the question tabs, the permission dialog's numbered options, the trust prompt (`docs/recording.md`). The reply keys and timings are pinned to what the Swift app saw (`crates/engine/src/reply/pin_tests.rs`), and the recordings name the version they were made with.
- **Flags and protocols.** Codex turns are `codex exec --json` with flags Notchling builds (`tools/codex/plan.rs`); Antigravity sessions are ACP over `agy acp`. A renamed flag or a new event type breaks a turn.
- **Work across machines.** The cluster design hands sessions and jobs to workers (`notchling delegate --machine`, Run on). The same prompt should give output in the same shape on any machine, a model id the Mac's CLI accepts must also be accepted on the worker, and its review issue 10 asks for a handshake that refuses dispatch on a tool-version mismatch.

Today nothing keeps versions together. The Mac's Claude Code updates itself whenever Anthropic ships. Each worker updates itself on its own schedule, or never. Machines shows a worker's `claude --version` as a pass or fail, never against the Mac. Codex 0.157.0 started a background updater that updates hourly and, per a user report (openai/codex #40969), can stop a running turn when it does.

## 2. Goals, non-goals, anti-goals

**Goals**

1. Every machine in Machines runs the Mac's version of each provider CLI and of Notchling, except during a rollout and while a worker is offline.
2. The user updates once, on the Mac. Workers follow without the user running anything on them.
3. A version that fails the Mac's check never reaches a worker.
4. An update never stops a running turn, a waiting question or an attached terminal.
5. One failed worker stops the rollout. The others keep a working version, and the failed one goes back to its previous version, or is marked Needs You when it can't.
6. The user can see, per tool and per machine: installed, latest, the Mac's version, drift, the last check and the rollout's progress.
7. All of it is tested with fakes. No test runs ssh, an installer or a real CLI.

**Non-goals**

- Choosing versions for the user. In approach A the user (or the CLI's auto-update on the Mac) chooses; Notchling follows.
- Holding the Mac back. The Mac's own auto-update stays on, and Notchling never writes the Mac's tool configs.
- Managing tools other than the three CLIs and Notchling: Node, Python, git, Xcode, the sd worker. Machines' existing steps cover those.
- Windows workers. Machines supports macOS and Linux today (`MachineOs`).
- Updating a tool on a machine Machines doesn't list.

**Anti-goals** (failures even if everything works)

- A rollout interrupts a turn, a question or an attached terminal on a worker.
- Two workers install at the same time, or a worker gets a version other than the Mac's.
- A version marked failed on the Mac reaches a worker.
- A worker's settings file loses a key, a hook or a comment the user had, or is written without a backup.
- An agent turns on automatic rollout, skips the check, or rolls a worker back, by any path.
- A secret from a worker's settings file reaches a log, a plan shown on screen or an error message.
- Notchling downloads a binary it can't check against the vendor's checksum.
- A rollout starts from a stale reference, or resumes without the user saying so.
- A worker is moved to an older version without its own click.

## 3. Constraints

- **Switches** (0007, `crates/notchlingd/src/switches.rs`). `acting`, the bridge, `accounts`, `vault` and `install` are off at every daemon start, and only an app-tier call in that run turns one on. Updating a tool on the Mac sits behind `install`. The smoke check as built only reads and needs no switch; the scratch-session addition would need `acting`.
- **App tier for anything that acts** (0007). Reading versions is agent tier. Updating, checking, rolling out and rolling back are app tier, and approvals fail closed without an app attached.
- **Machines' `--live` gate** (`crates/app/src/machines/cli.rs`, `data.rs`). Without `--live`, every process goes to the `Refusing` runner. Machines is a separate binary, `notchling-machines`, apart from the daemon (0004's designer's call), and not handed over yet.
- **Plan, confirm, run** (`machines/engine_plan.rs`). Every machine change shows its plan, takes one click, then rebuilds the plan from the machine as it is and refuses unless every displayed line still matches.
- **The vault for any credential.** None is needed here: every download is public. A worker's GitHub token is never used to fetch a release.
- **Capability checks** (`docs/rules.md`). New process launches (`CAP-PROCESS`), a pty (`CAP-PTY`), network calls (`CAP-NETWORK`) and file writes (`CAP-FILEWRITE`) each send the pull request to the maintainer. Most phases trip at least one.
- **No secrets in logs.** Machine output is masked with every vault value before it is cut (`machines/masks.rs`). A worker's `~/.claude/settings.json` can hold tokens in its `env` block, so its content is never logged, shown or put in an error. A plan shows only the one line that changes.
- **Anything a machine reports is untrusted** (`shell::is_safe_machine_path`). A version string enters a script only after it matches `^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$` and is at most 32 characters (the logging design's `Version` type). `stable` and `latest` never appear in a rollout: it always names a version.
- **The local API and the phone protocol change additively only.**
- **Goals, "What Notchling will never do":** "Change more of your config than its own part: only Notchling's hook entries in `~/.claude/settings.json`". Turning self-update off on a worker writes one more key. That line needs an amendment (Phase 0, protected).
- **Code rules.** No comments in code (the why goes in the crate's `AGENTS.md`), files under 1,000 lines, `app` and `kit` deny `unwrap` and `expect` outside tests.
- **Tests** never touch ssh, `gh`, Keychain, `~/.claude`, `~/.codex`, `~/.gemini` or the network. Roots are parameters.

## 4. Approaches

| | **A. The Mac is the reference** (chosen) | **B. Known-good versions per Notchling release** | **C. Each machine follows the vendor's channel** |
| --- | --- | --- | --- |
| Who picks a version | The user, on the Mac, or the CLI's auto-update there | The maintainer, by listing tested versions in each Notchling release | The vendor: Claude Code's `stable` channel, Homebrew's cask, each tool's updater |
| Workers | Follow the Mac, after a check | Follow the list; the Mac too | Update on their own |
| Same version everywhere | Yes, except during a rollout | Yes | No: machines update at different times, and agy has no channel |
| A breaking CLI release | Version, flag and transcript breaks are caught by the Mac's check before any worker gets it; screen and dialog breaks show on the Mac first, where the user works; the Mac itself is exposed | Never installed until a release lists it | Reaches every machine within a day |
| New features and models | As soon as the user updates the Mac | When Notchling ships a release that lists the version | When the channel has them (stable is about a week behind) |
| Cost | Detection, a check, a rollout engine | All of A, plus a list kept current and the Mac held to it (its config written, against goals.md) | Smallest: drift detection and a dispatch handshake only |

**A was picked by the maintainer** on 2026-11-13: the user stays in charge of when to update, workers can't drift, and a bad version stops at the Mac. B's list is kept as a slot ([24](#24-the-slot-for-approach-b)), checked before the Mac step, so B can be added later without changing A's rollout. C's handshake becomes part of A (dispatch warns on a mismatch from Phase 1 and refuses from Phase 6, [17.5](#175-dispatch)).

## 5. What exists today

Checked in the code on `agent/move-auto-trust`:

- **Machines' Claude Code step** (`crates/engine/src/machines/steps_machine.rs`). The probe is `command -v claude >/dev/null 2>&1 && claude --version 2>/dev/null`, run on every refresh, and passes on any first line with a digit. The fix is `CLAUDE_INSTALL`, `curl -fsSL https://claude.ai/install.sh` piped to `bash`, which installs the latest version. Nothing compares the version with the Mac's.
- **The runner** (`machines/runner.rs`). `Runner::run(&Call)` with `argv`, `stdin`, `env`, timeout and output streaming. `ScriptedRunner` is its fake. A secret travels only in `stdin`, and a Fix refuses (126) a call whose argv or env holds one (`step::run::guarded`).
- **Probes and cadence** (`engine_check.rs`, `engine.rs`). One ssh runs a machine's probes, cut at nonce markers. `EveryRefresh` probes run about once a minute; `Slow` ones every 10 minutes (`SLOW_EVERY`).
- **Offers** (`machines/offers.rs`). When a vault value changes, Machines offers to send it again to the machines that use it, kept in `notchling.db` (`machines/pending-updates`). A rollout offer works the same way.
- **The poller** (`machines/poller.rs`) reads the sd worker's status every 60 s. `WorkerStatus.current` says whether the worker is on a job.
- **Idle checks.** `accounts::antigravity::store::idle_for_auto_switch` reads `ps` to see whether `agy` runs before switching, with `FakeProcs` in its tests.
- **Settings file edits** (`crates/engine/src/install/`). `edit.rs` splices bytes, so everything outside the touched range stays. `file.rs` writes a backup first (0600), then a temp file and a rename, and re-reads just before the swap. A duplicate key is refused (`dup_tests.rs`). All of it works on local files.
- **The update check** (`crates/app/src/welcome/updates.rs`). It reads the appcast only when `NOTCHLING_RELEASES` is exactly `on`, never downloads or installs, and compares `Version`s (beta below stable).
- **Releases** (`.github/workflows/beta.yml`, off). They build the Swift app with `./build.sh` into `Notchling-<v>.dmg` and `.zip`. No Rust daemon is built for any target, and no Linux build exists. Decision 0010 plans a static manifest signed with ed25519 for the Rust app; it isn't built.
- **Notchling on workers: none.** Machines installs only sd's service (`service_files.rs`: `com.sd.worker.plist`, systemd units). No `notchling` runs on a worker. The cluster design installs `notchlingd` on each node when it joins (v1 step 1).
- **Version handshake: none.** The local API's `hello` carries the client's `version` and `api` (`crates/local-api/src/wire.rs`); nothing compares tool versions across machines.
- **Versions on the Mac** (0019's spec, on `agent/agents-setup-decision`). Settings > Agents' Client part reads versions from files only: Claude Code's `versions/<v>` link and Codex's `codex-package.json`. Antigravity shows "Version unknown". No process starts to read one. On 2026-11-13 the Mac had Claude Code 2.1.323 and Codex 0.166.0.

## 6. Terms

| Term | Meaning |
| --- | --- |
| Tool | `claude`, `codex`, `agy` or `notchling` |
| Method | How a tool was installed on a machine: `native` (Claude Code's installer), `standalone` (Codex's or agy's installer), `npm`, `brew`, `apt`/`dnf`/`apk`, `release` (Notchling's own layout), `unknown` |
| Installed version | The version the machine's launcher runs for a new process |
| Running versions | The versions live processes on that machine still run (old sessions keep their binary) |
| Reference | The Mac's installed version of a tool after it passed its check; what workers follow |
| Usable reference | A reference whose check ran against the binary the Mac has installed now: the same version string and the same SHA-256. Anything else is stale *(coordinator's call 1)* |
| Tool list | The tools a worker has installed. A tool it lacks is Not Installed, skipped by rollouts and ignored by Done and drift *(coordinator's call 4)* |
| Busy | A process of that tool runs on the worker, other than a Claude Code background session under its supervisor *(coordinator's call 7)* |
| Rollout | Moving every worker to one reference version of one tool |
| Drift | A worker whose installed version differs from the reference outside a rollout |
| Hold | A worker the user excluded from rollouts |
| Needs You | A worker the rollout stopped at and can't fix on its own: its plan changed, or its rollback failed |

## 7. Reading a version

**On the Mac**, the daemon reads files. It starts no tool, so reading costs nothing and changes nothing. It watches the folders below with `platform::watch` and reads them again every 10 minutes.

| Tool | Method | Where the version comes from | What actually ran |
| --- | --- | --- | --- |
| Claude Code | native | `~/.local/bin/claude` is a symlink into `~/.local/share/claude/versions/<v>` (docs) | the `version` field Claude Code writes on transcript lines (to confirm on a recording: the repo's fixtures strip it) |
| Claude Code | npm | `<npm prefix>/lib/node_modules/@anthropic-ai/claude-code/package.json` | same |
| Claude Code | brew | the cask's folder under `$(brew --prefix)/Caskroom/claude-code*/<v>` | same |
| Codex | standalone | `codex-package.json` (0019's finding), under `~/.codex/packages/standalone/` (installer) | `cli_version` in each rollout's `session_meta` (seen in `codex-rollout.jsonl`: `0.166.0`) |
| Codex | npm | `@openai/codex/package.json` | same |
| Codex | brew | the `codex` cask's folder | same |
| agy | standalone | no file known; `agentInfo` in ACP's `initialize` reply when Notchling opens an `agy acp` session (to verify), else `agy --version` run by the daemon at most every 10 minutes | the same `initialize` reply |
| Notchling | app | the app's own `CFBundleShortVersionString` and the daemon's `notchling --version` | the daemon's `hello` |

Two Claude Code installs on one machine (an old npm one beside the native one) are reported as a problem: the launcher on `PATH` decides what runs, and `claude doctor` flags it too.

**Binary identity.** Beside each version the daemon keeps the SHA-256 of the binary the launcher resolves to (for npm, the package's binary). It is computed when the watcher sees a change and cached by path, size, mtime and inode, so an unchanged file is never hashed again. A check records the identity it ran against.

**Machines asks the daemon** *(coordinator's call 1)*. In Phases 1 to 4, Machines reads the Mac's versions with `versions.get`, a read-only, agent-tier op ([18](#18-where-the-code-goes)), and never reads the Mac's files itself. Each tool comes back with `installed` (version and identity), `reference` (version, identity, check result and time) and `usable`, true only when the two match. If the daemon is down or doesn't answer within the call timeout, Machines shows "Notchling isn't running" on its rollout banner, shows the workers' own versions without a mark against the Mac, and starts no rollout. A stale reference shows "The Mac's check is out of date" and starts nothing; the daemon checks the installed version again on its own, and Check Again in Settings > Agents does it at once.

**On a worker**, Machines adds one step, **Tool Versions** (`StepId::TOOL_VERSIONS`), to its batched probe. It runs read-only commands and prints keyed lines, like the other probes. Nothing in it needs a login *(coordinator's call 4)*: `--version`, `readlink` and file reads work on a worker where no account is signed in.

```sh
for t in claude codex agy notchling; do
  p=$(command -v "$t" 2>/dev/null); echo "$t.path=$p"
  [ -n "$p" ] && echo "$t.link=$(readlink "$p" 2>/dev/null)"
done
echo "claude.version=$(claude --version 2>/dev/null | head -1)"
echo "codex.version=$(codex --version 2>/dev/null | head -1)"
echo "notchling.version=$(notchling --version 2>/dev/null | head -1)"
echo "claude.selfupdate=..."   # the knob's state in each place, section 9
```

- **The tool list** *(coordinator's call 4)*: a tool whose `command -v` finds nothing is Not Installed on that worker. The rollout skips it, and Done and drift ignore it. Installing Codex or agy on a worker is a separate Machines step, outside this design.
- The method comes from the path and the link: `~/.local/share/claude/versions/` is native, `node_modules` is npm, `Caskroom` or `Cellar` is brew, `~/.codex/packages/standalone` is Codex's installer.
- `agy --version` runs only at the `Slow` cadence and only once agy's self-update is off on that worker: the installer says agy "self-updates in the background during regular runs", and it isn't known whether `--version` counts as one. Until that is checked, a 60-second probe could be what updates it.
- Claude Code's background sessions: `claude agents --json` prints the active ones (CLI reference) and `claude daemon status` prints the supervisor's version. Both run at the `Slow` cadence, and only on a worker where `claude` is found.
- The parse keeps only versions that match the version pattern ([3](#3-constraints)); anything else reads as "can't tell", never as a version. Paths must pass `is_safe_machine_path` before they are kept.

**Latest**, for the "Latest" line in Settings > Agents, is read only when that page opens (at most once an hour) or on Check for Updates, never in the background:

| Tool | Source | Notes |
| --- | --- | --- |
| Claude Code | `https://downloads.claude.ai/claude-code-releases/latest`, the file the official installer reads | A `stable` file beside it is likely but unverified |
| Codex | npm's registry, `https://registry.npmjs.org/@openai/codex/latest` | Or GitHub's releases; the builder picks one host |
| agy | `https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/<platform>.json`, the URL the official installer reads | Google may move it; read from the installer, not trusted beyond its checksum |

Each is a new network call ([21](#21-security-and-privacy)). None sends an identifier.

## 8. Installing an exact version

The rollout always names a version, and only updates a tool the worker already has *(coordinator's call 4)*. A method that can't install an exact version can't be managed; the worker shows a Fix that moves it to one that can.

One suggestion beside call 4: Machines' existing Claude Code step installs the vendor's latest (`CLAUDE_INSTALL`). Once a usable reference exists, that Fix should pass it (`bash -s <v>`), so a worker set up later starts on the Mac's version instead of drifting on its first refresh.

**Which installers Notchling manages** *(user's decision 1)*. On workers, only the vendors' own: Claude Code's native installer, and Codex's and agy's standalone installers. A tool installed with Homebrew shows Can't Pin, with a Fix, **Switch to the Native Installer** (for Codex and agy, the Standalone Installer). It is its own click with its own plan, runs only while no process of that tool runs, installs the reference with the vendor's installer, removes the Homebrew cask (`brew uninstall --cask <cask>`) so the vendor's launcher is the one on `PATH`, and checks `command -v` and `--version`. npm stays allowed for Claude Code and Codex: an npm install runs only while no process of that tool runs (call 7) and never under Automatic (call 8). apt, dnf and apk stay out.

| Tool | Method | Exact version | Keeps the previous one | Source |
| --- | --- | --- | --- | --- |
| Claude Code | native | `claude install <v>` on a machine that has it; else `curl -fsSL https://claude.ai/install.sh \| bash -s <v>` | Yes: `~/.local/share/claude/versions/` keeps the launcher's version, any version a session on that machine runs, and the two newest | Verified: setup page, CLI reference, `bootstrap.sh` |
| Claude Code | npm | `npm install -g @anthropic-ai/claude-code@<v>` | No: an in-place upgrade deletes the previous binary, and a session running it crashes (setup page, "network storage") | Verified (npm's own syntax; the docs show `@latest`) |
| Claude Code | brew | No: the casks track the current `stable` (`claude-code`) or `latest` (`claude-code@latest`) only | Old versions stay on disk until `brew cleanup`, not selectable | Verified: setup page |
| Claude Code | apt, dnf, apk | `apt install claude-code=<v>` if the repository keeps old versions | Unknown | Unverified |
| Codex | standalone | `curl -fsSL https://chatgpt.com/codex/install.sh \| sh -s -- --release <v>` (or `CODEX_RELEASE=<v>`) | Release files live under `~/.codex/packages/standalone/`; whether old ones stay is unverified | Verified: the installer script |
| Codex | npm | `npm install -g @openai/codex@<v>` | No | Verified (README names the package) |
| Codex | brew | No: `brew install --cask codex` tracks the current version only | No | README verified; "can't pin" from third-party guides |
| agy | standalone | No: the installer takes only `--dir`, always installs the manifest's latest, and exits if `agy` is already there | No | Verified: the installer script |
| Notchling | release | Notchling's own layout ([16](#16-notchling-itself)) | Yes | This design |

**How each install is checked.** After every install, the worker runs `<tool> --version` and the step passes only on the exact version. For Claude Code the launcher's target must also be `versions/<v>`. Claude Code's installer already checks the binary against the release's `manifest.json`; a later phase may also check that manifest's GPG signature (fingerprint `31DD DE24 DDFA B679 F42D 7BD2 BAA9 29FF 1A7E CACE`, from 2.1.89 on). Codex's and agy's installers check SHA-512 and SHA-256 sums against their own manifests.

**agy, which can't pin.** Two ways, tried in order:

1. **Vendor latest, then verify.** While the Mac's version is still agy's latest for the worker's platform, the worker installs latest (`agy update`, from third-party guides; or the installer on a fresh machine) and the step passes only if `agy --version` equals the Mac's. If agy shipped again since, the step fails with "agy 1.4.11 is out; update the Mac first" and installs nothing.
2. **Notchling's cache.** When the Mac's agy version changes, the Mac reads agy's manifest for each platform its workers use. Where the manifest still names the Mac's version, it downloads that binary, checks its SHA-512 against the manifest and keeps it in `<data>/versions/cache/agy/<v>/<platform>/`, at most two versions. A Mac worker of the same architecture can use a copy of the Mac's own binary instead. The rollout sends a cached binary over ssh's standard input, with its hash, and the worker checks the hash before it moves the file into place.

**Keeping the previous binary on the worker.** Before any install whose method doesn't keep old versions (npm, standalone agy, Codex if its packages folder doesn't), the worker copies the current binary or package to `~/.local/share/notchling/previous/<tool>/<v>/`. Rollback puts it back. One previous version per tool is kept.

**Install scripts.** The download-and-pipe pattern stays as `CLAUDE_INSTALL` writes it today, `s=$(curl -fsSL <url>) && printf '%s\n' "$s" | sh -s -- <args>`, so a cut download never runs half a script. Every argument is a checked version, never text from a machine. Each step has a timeout (install 600 s, verify 30 s) and runs as the worker account, never as admin: none of these methods needs root, and apt, dnf and apk are left out until a worker needs them.

## 9. Self-update off on workers

A worker never updates a tool on its own. Each tool has one knob, and Machines turns it on with a Fix like any other step: **Self-Update Off**, shown per tool, with the exact line it adds.

| Tool | Knob | Where | Status |
| --- | --- | --- | --- |
| Claude Code | `"env": {"DISABLE_AUTOUPDATER": "1"}` | `settings.json` in every Claude Code config folder Machines knows for the worker, and `DISABLE_AUTOUPDATER=1` in the sd service files' environment *(coordinator's call 6)* | Verified: setup page. `claude install` and `claude update` still work, so Notchling's installs do. `claude doctor` then reports auto-updates disabled, set by env `DISABLE_AUTOUPDATER`; the probe reads the key from the file and doesn't depend on that text. |
| Claude Code | not `DISABLE_UPDATES` | | Verified: it blocks `claude install` too, so Notchling couldn't install. |
| Claude Code | not `autoUpdates: false` | | Not in the current settings reference; it was a `~/.claude.json` key in older versions. Unverified, not used. |
| Claude Code | `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE` must be unset | the worker's shell and settings | Verified: set to 1 it makes Homebrew and WinGet installs upgrade themselves. The probe reports it. |
| Codex | `check_for_update_on_startup = false` | `~/.codex/config.toml`, top level | Verified: config reference ("set to false only when updates are centrally managed"). |
| Codex | `daemon_auto_start = false` under `[features]`, or `codex features disable daemon_auto_start` | `~/.codex/config.toml` | Unverified: from openai/codex #48195 (open, no maintainer reply). Since 0.157.0 an interactive Codex starts a background server whose `pid-update-loop` updates hourly. The probe also looks for a running `app-server daemon pid-update-loop` and reports it as a drift risk. |
| agy | `AGY_CLI_DISABLE_AUTO_UPDATE=true` | the environment of every process that runs `agy` on the worker, including the sd service files | Unverified: one third-party guide. Until checked, agy on workers is "detect and repair": drift is found by the probe and fixed by the rollout steps. |
| Notchling | the worker has no updater of its own | | By design: only a rollout installs Notchling on a worker. |

**Every place the knob goes** *(coordinator's call 6)*.

- Each Claude Code config folder Machines knows for that worker: the ssh user's `~/.claude`, and each lane's `CLAUDE_CONFIG_DIR` there. Today Machines sets up no lanes on workers, so that is `~/.claude` alone; the list grows when it does.
- The sd service files Machines writes (`service_files.rs`): `DISABLE_AUTOUPDATER=1` (and agy's variable, once verified) beside the `PATH` line in the systemd unit's `Environment=` lines and the plist's `EnvironmentVariables`. A changed service file takes effect when the service restarts, which Machines does only while the sd worker has no job (`WorkerStatus.current` empty).
- The probe reports each place on its own: set, not set, or can't read. A place it can't read is flagged on the card, and the step doesn't pass while any place is unread or unset.

**How a worker's settings file is changed.** The hook rule applies (goals.md, `install/file.rs`): change only that key, keep a backup, keep everything else.

1. The Mac reads the worker's file over ssh (`cat`, at most 1 MiB; a bigger, non-UTF-8 or unreadable file stops the Fix).
2. The engine plans the edit on the Mac with the same pure code Install Hook uses (`install::jsonc`, `install::edit`): add `env` if missing, add or set the one key, refuse a duplicate key, and check that the spliced text equals the same change made to the parsed data. The plan shows the one line, never the file.
3. The remote script gets the new text on standard input with the SHA-256 of the text it was planned from. It refuses if the file's hash changed since, writes a backup `settings.json.notchling-backup-<ms>` with mode 0600 (the newest 5 kept), writes a temp file in the same folder with the old mode, and renames it over the file.
4. The probe reads the knob again and the step passes only when it is set.

A TOML file gets the same treatment with a small TOML splice in the engine: one top-level key, placed before the first table, or one key under `[features]`. Comments and order stay. This is new code (`install::toml_edit`), tested like `jsonc`.

**Undo.** Removing a worker from Machines, or **Self-Update On** on its card, removes exactly the key Notchling added, if it is still the value Notchling wrote, with a backup again.

**Not on the Mac.** The Mac's tool configs are never written, the auto-update there stays on, and rollback on the Mac is temporary ([15](#15-failure-and-rollback)).

## 10. The Mac step

The Mac's version changes in one of two ways.

1. **The Update button** in Settings > Agents > *tool* > Client. The app calls `versions.update_local {tool}` (app tier, `install` switch on). The daemon runs the method's update: `claude update` (native, follows the channel the user chose), `npm install -g <package>@latest`, `brew upgrade --cask <cask>`, `codex update` (newer Codex, third-party; else the installer), `agy update` (third-party). With the B gate later, the target version is checked before this runs ([24](#24-the-slot-for-approach-b)).
2. **The CLI's own auto-update**, which stays on on the Mac. It installs in the background and takes effect at the next start of that tool.

Either way the daemon's watcher sees the installed version change and records it. A new running version seen in a transcript (`version` on Claude Code's lines, `cli_version` in a rollout) after that confirms it.

A version change on the Mac then goes through the smoke check, and only a passed check makes it the **reference** for that tool. The reference is usable only while the Mac's installed binary is the one the check ran against ([7](#7-reading-a-version)).

**Going down** *(coordinator's call 3)*. A Mac that goes back (a rollback, or the user installing an older version) makes the older version the reference once its check passes. A rollout only ever moves workers up to it. Workers left ahead of the Mac are drift: shown, never moved down automatically. Each takes its own **Match the Mac** click, whose plan warns that a `minimumVersion` in that worker's settings or a managed `requiredMinimumVersion` can make Claude Code refuse the older version or update itself back up; the probe reads `minimumVersion` from the places it already reads, and says so when it finds one.

**First run** *(coordinator's call 3)*. Turning the feature on (`versionRollout` from Off to Ask, [20](#20-settings-keys)) checks the Mac's current version of each tool and then offers a rollout to the workers behind it. The first rollout is never automatic, even once Automatic exists.

Two changes close together: the newest wins. A rollout under way for the older one is **superseded** ([12](#12-the-rollout)).

## 11. The smoke check

What it must answer: does Notchling still read this version? It runs on the Mac, in the daemon, against the newly installed binary. As built it is **files and flags only** *(user's decision 2)*: it starts no session, opens no pty, sends no turn and writes nothing to the user's config. Its only processes are each tool's own one-shot commands, which print and exit. It needs no switch, since nothing in it acts, and it takes seconds.

### 11.1 What the check runs

| Tool | Check | Proves |
| --- | --- | --- |
| all | `<tool> --version` prints exactly the installed version | the binary runs, and it is the one the watcher saw |
| Claude Code | `claude doctor` exits 0 and has an auto-updates line (read only, per the CLI reference) | the binary starts and reads its settings |
| Codex | `codex exec --help` lists every flag Notchling's turn builder passes (`tools/codex/plan.rs`: `--json`, `resume`, the sandbox and model flags) | Notchling's argv still runs |
| all | Notchling's readers in strict mode ([11.3](#113-strict-mode-and-the-soak)) over the transcripts already on the Mac, read only: the newest Claude Code transcripts, Codex rollouts and the agy logs Notchling reads | the readers still read what is there, including any lines the new version has written |
| Notchling | `hello` with the new daemon, `sessions.list` answers, the readers run over the fixtures compiled into the build | the build works |

**What it can't see.** Screen and dialog changes (the trust prompt, the idle screen Notchling types into, the ask and permission dialogs), and lines a new version writes only during a turn, such as queue lines, until a transcript holds them. Right after an update the new version has usually written little, so the transcript part mostly confirms the readers on what is there. That is why every result is **Partly Checked** ([11.4](#114-results)), and why the soak keeps reading as new turns arrive ([11.3](#113-strict-mode-and-the-soak)).

### 11.2 Possible later additions

Not built now and in no phase *(user's decision 2)*. Each would ship behind its own setting, off by default, with its own decision record. With both built and passing, a check could be fully **Passed**, which is what Automatic needs.

**Scratch screens** (`versionCheckScreens`). Claude Code on a pty (50 by 200, Notchling's reply size) in scratch folders under `<data>/versions/scratch/<tool>-<v>-<random>/`, each made fresh with a `.notchling-scratch` file and a `git init`, the way the recorder's scratch rule works (`docs/recording.md`), and removed afterwards. It would need the `acting` switch and the ptyhost, and Claude Code would write one trust entry for the scratch root in the Mac's `~/.claude.json`, the way Trust Folder does.

- In a never-trusted scratch folder, the screen must show the **trust prompt** with the option text `control::trust` looks for (`TRUST_PROMPT`); the check answers "No, exit".
- In a subfolder of the trusted scratch root, the **idle input box** must draw and go quiet within 15 s, and `ask_dialog` and `permission_dialog` must find no dialog there.
- Typing `/` draws the command menu, which must not be mistaken for a dialog; then Esc, and Ctrl+C twice.
- For agy: start `agy acp`, send `initialize`, read `protocolVersion` and the agent's info, read the Model and Thinking Level options with `ConfigOption::parse`, close.
- Screens are compared as classifications, not pixels: what Notchling's readers concluded (trust prompt, idle, menu, no dialog) against what they conclude on the repo's recordings of the previous version. New screens would be saved under `<data>/versions/recordings/claude/<v>/` and never leave the Mac; a banner can show the account's email, so outputs are never logged.

**One paid turn** (`versionCheckTurn`), once per new version, on top of the scratch screens:

- A Claude Code session on a pty in a trusted scratch folder, on the catalog's cheapest current Claude model, with a fixed prompt that asks one question with AskUserQuestion, then asks to run `echo notchling-check` with Bash. While it works, the check queues a second message.
- It then checks, against the new version's own output: the transcript parses with no unknown line or block type; `pending_question` finds the question and the ask dialog reader reads its first page; the queue's `enqueue` and `dequeue` (or `remove`) lines resolve the queued message (the 2.1.322 case); the permission dialog reader reads the Bash dialog's tool, command and options; the `PermissionRequest` hook event reaches the daemon and parses.
- The check answers its own question with the first option and its own permission dialog with **No**. It is Notchling's own session in a scratch folder, so this is not answering the user's prompt; the decision record for the addition would state it. It ends the session (Ctrl+C twice, then the pty is closed) and removes the folder.
- Codex's and agy's would be one turn each with the cheapest model ("Reply with OK"), parsing every event through the strict readers. Cost: a few cents a version, on the account in use, shown in Activity.

### 11.3 Strict mode and the soak

The readers are lenient by design (unknown lines are skipped). The check runs them in a **strict mode** that counts what they skipped: unknown line types, unknown content blocks, unknown queue operations, lines that failed to parse, ACP updates with an unknown `sessionUpdate`. A count above zero on a line the new version wrote (Claude Code's per-line `version`, a Codex rollout's `cli_version`) fails the check, with the kind of thing skipped, never its text. Counts on older lines are shown, not failing.

The same counters keep running on the Mac's **real sessions** after an update: the **soak**. Settings > Agents shows "Since 2.1.324: 14 turns read, nothing unknown", and the Roll Out sheet and the rollout's Inbox item carry the same line. The soak never gates a rollout *(user's decision 3)*: a count above zero shows in red there, and the user decides.

### 11.4 Results

| Result | Means | Then |
| --- | --- | --- |
| Passed | Every check ran and passed, which needs the later additions ([11.2](#112-possible-later-additions)) | The version becomes the reference; Automatic may use it (Phase 6) |
| Partly Checked | What the built check always gives: everything it ran passed, and screens, dialogs and turns weren't checked | The version becomes the reference, labelled **Partly Checked** wherever it shows, with what wasn't checked. A Roll Out click may use it, so Ask works; Automatic never does *(coordinator's call 3)*, so Automatic stays unavailable while the check is partial |
| Failed | A check failed | The version is marked failed on this Mac; it is never rolled out; Inbox item |
| Can't tell | The binary didn't start, or a timeout | Tried again once after 2 minutes, then Failed |

A failed version stays failed until the user presses **Check Again** or the Mac moves to another version.

A result belongs to the binary it ran against. When the Mac's installed binary changes, even under the same version string, the old result is stale and the daemon checks again before Machines can use it *(coordinator's call 1)*.

## 12. The rollout

One rollout per tool and target version. It runs in whatever process holds the Machines runner ([18](#18-where-the-code-goes)): one worker at a time, in the registry's order, held workers and workers without the tool skipped. It starts only from a usable reference *(coordinator's call 1)*.

```
Mac version changes
        |
     Detected ──► Gated ──► Checking ──► CheckFailed ──(Check Again)──► Checking
                  (B slot)      |
                                ▼
                              Ready ──(setting Off)──► Parked
                                |
                    (Ask) ──► AwaitingApproval ──(Not Now)──► Parked
                                |  (Roll Out click; Automatic from Phase 6)
                                ▼
                             Rolling ──► Complete
                                |  ├──► Halted ──(Retry / Skip / Roll Back)──► Rolling
                                |  ├──► Superseded (the Mac moved on)
                                |  └──► Cancelled (the user)

Each worker, one at a time:
 Pending ─► Offline ─────────────────(comes back)──► Pending
    │
    ├─► Held (excluded by the user)
    ▼
 Waiting (N processes) ─► PlanCheck ─► Saving ─► Installing ─► Verifying ─► Moving ─► Done
    │ (Cancel)                │ (plan changed)           │             │   (supervised idle sessions)
    ▼                         ▼                          └──────┬──────┘
 Skipped                   NeedsYou                             ▼
                                                             Failed ─► RollingBack ─► RolledBack (halts)
                                                                            └─(fails)─► NeedsYou
```

**The steps on one worker**

| Step | What runs on the worker | Passes when | Timeout |
| --- | --- | --- | --- |
| Precheck | the Tool Versions probe, free disk (from Reachable, at least 1 GB), the method is one that can pin | online, method supported, self-update knob set | 30 s |
| Waiting | the busy probe ([13](#13-running-sessions-during-an-update)), every minute | no process of this tool runs, apart from supervised background sessions *(coordinator's call 7)* | none; a notice after `versionWaitIdleMinutes`; Cancel skips the worker |
| PlanCheck | rebuild this worker's plan from the machine as it is | every line equals the plan shown at the Roll Out click, else Needs You and the rollout halts *(coordinator's call 2)* | 30 s |
| Saving | copy the current binary to `previous/` (methods that don't keep it) | the copy's hash equals the source's | 120 s |
| Installing | the method's exact install ([8](#8-installing-an-exact-version)) | exit 0 | 600 s |
| Verifying | `<tool> --version`; the launcher's target; the knob still set | the exact version | 30 s |
| Moving | for Claude Code, `claude respawn <id>` for each idle background session still on the old binary | no idle session left on the old version | 120 s |

**Approval** *(coordinator's call 2)*. `versionRollout` decides what happens after a passed check:

- **Off** (the default until the user turns the feature on): nothing rolls out; Machines shows drift and offers Match the Mac per worker.
- **Ask**: Machines shows a **Roll Out** sheet: the version, **Partly Checked** with what wasn't checked (the built check always is, *user's decision 2*), the soak's line, and one plan per worker with the tool, its method, the exact lines it will run and the order. One **Roll Out** click approves that version for every worker on the sheet. The daemon raises a notice in the Inbox ("Claude Code 2.1.324 passed on the Mac. 3 workers are behind.") whose only action is **Open Machines**; no Inbox answer starts anything in Phases 1 to 4.
- **Automatic** (Phase 6 on, once the daemon runs rollouts): the rollout starts after a check with no click, and the Inbox gets one item when it finishes or halts. Never for the first rollout after the feature is turned on, a Partly Checked version, an npm install or agy *(coordinator's calls 3 and 8)*; those wait for a click. As built every check is Partly Checked *(user's decision 2)*, so Automatic stays unavailable, shown off with "Needs a full check", until a later addition ([11.2](#112-possible-later-additions)) makes a full check possible.

**Each worker's plan is checked again** *(coordinator's call 2)*. Right before Saving, Machines rebuilds that worker's plan from the machine as it is now and compares it, line by line, with the plan the sheet showed. Any difference (the worker's version moved, its method changed, its disk filled) halts the rollout at that worker as **Needs You**; the workers already done stay done, and the rest keep their version. A worker offline at the click is on the sheet with the plan its last probe gives ("when back: native installer, 2.1.323 to 2.1.324"); when it comes back, its rebuilt plan is compared with that one, so a catch-up never runs a plan nobody saw.

**Superseded.** If the Mac's reference moves on during a rollout, the worker in a step finishes it (never cut mid-install), the rollout ends as Superseded, and a new one starts for the newer version, which needs its own check and its own Roll Out click. Workers already done move again.

**One at a time, everywhere.** At most one install runs across all tools: a Claude Code rollout and a Codex rollout queue behind each other, not side by side, so a failure is never ambiguous.

**The lock** *(coordinator's call 9)*. One row in the rollout journal is the lock: the owner's pid, its start time and a heartbeat the owner writes every 10 seconds while a rollout runs. A process takes it in one SQLite transaction (`BEGIN IMMEDIATE`), only when it is free or stale. Stale means the heartbeat is older than a minute, or the pid is no longer running or belongs to another process start. A second Machines window finds the lock held and shows "A rollout is running in another Machines window".

**Resume, never silently** *(coordinator's call 9)*. Closing the Machines window during a rollout stops it after the worker step in progress, or where the process died. The next time Machines opens it reads the journal and shows **Resume Rollout** ("Claude Code 2.1.324 stopped at vps-sing-1, installing") beside **Cancel Rollout**. Nothing continues until that click, and a resume still checks that the reference is usable and rebuilds the next worker's plan against the one shown.

**The journal.** Every state change is saved before its effect runs, in `notchling.db` as `versions/rollouts` (the same store as `machines/pending-updates`), with the lock row beside it. On Resume Rollout, a worker left in Installing or Verifying is probed: if it runs the target it continues at Verifying, if it runs the old version Installing is tried once more, and anything else is Needs You with `unknown_state`.

**The engine is a pure state machine.** `next(state, event) -> (state, effects)`. Events: `MacVersion`, `GateResult`, `CheckFinished`, `Approved`, `Declined`, `WorkerProbed`, `PlanRebuilt`, `StepFinished`, `Tick`, `Retry`, `Skip`, `RollBack`, `Cancel`, `Resume`. Effects: `RunCheck`, `RunStep`, `RaiseInbox`, `Save`, `Emit`. The process that holds the runner performs the effects.

## 13. Running sessions during an update

The rule: **never stop a turn, a question or an attached terminal.** A tool installs on a worker only when none of its processes run there, and the only sessions moved to the new binary are Claude Code's background sessions under its supervisor *(coordinator's call 7)*.

| Tool | Busy means | Install waits for | After install |
| --- | --- | --- | --- |
| Claude Code | any `claude` process that is not the supervisor or one of its background sessions: a plain `claude` in tmux or a terminal, a `claude -p` job, a cluster or sd job (`WorkerStatus.current`) | none of those running | Supervised background sessions: Notchling respawns each idle one (`claude respawn <id>`), one by one. Working, waiting or attached ones aren't touched; they move when the supervisor moves them or when the user reopens them, and the card shows them |
| Codex | any `codex` process (one per turn) | none running | The next turn uses the new one |
| agy | any `agy` process, `agy acp` included | none running | The next session uses the new one |
| Notchling | an op in flight on the worker's daemon | the daemon between ops | The daemon restarts only between ops. `notchling-ptyhost` restarts only when it holds no terminal (0004: updating the daemon never ends a terminal session) |

The native Claude Code installer would be safe beside running processes (the launcher moves; running ones keep their file, setup page), and agy's would be too when the file moves in by rename. Waiting anyway means "on version X" is true of everything on the worker, and it costs only time, which the Waiting state shows.

**Busy probe.** One read-only script per worker: `ps -axo pid=,ppid=,command=` matched by program name the way `agy_running` matches (the last path part, exactly), with the supervisor's process and its descendants set apart for Claude Code; `claude agents --json` for the supervised sessions; and the sd worker's `current`. The probe returns names and counts only.

**Waiting, visibly** *(coordinator's call 7)*. A worker in Waiting shows "Waiting for 3 processes (claude 2, sd job 1)" on its card and on the rollout banner, with **Cancel**. Cancel skips that worker: it keeps its version, is marked Behind, and the rollout goes on to the next worker. A skipped worker moves only by its own Match the Mac click. After `versionWaitIdleMinutes` (default 60) a notice says "m1 has waited an hour for 3 Claude Code processes". There is no Install Now that kills anything.

**Running versions.** Machines shows both numbers until they agree: "2.1.324 installed, 2 background sessions still on 2.1.323 (working)". The worker counts as Done when the install is verified; supervised sessions left on the old version are shown, not waited for.

## 14. Offline workers and drift

- **Offline.** A worker that fails Reachable at its turn becomes Offline, and the rollout goes on to the next. The rollout can be Complete with workers still Pending. When Machines sees the worker again, its catch-up runs the same steps under the original click, and its rebuilt plan must equal the one the sheet showed for it ([12](#12-the-rollout)); otherwise it is Needs You. If the reference moved on meanwhile, that is a new rollout and a new click.
- **A worker that was off for a long time** may be several versions behind. It installs the reference directly; it never walks through versions.
- **Drift** is any difference found by the probe outside a rollout, for a tool the worker has: self-update wasn't off, someone updated by hand, the knob was removed. Absent tools are never drift *(coordinator's call 4)*. Machines marks the tool on that card, and the Inbox gets one notice per worker and tool, "vps-sing-1 runs Codex 0.167.0, the Mac 0.166.0.", with **Open Machines**. Match the Mac there is a one-worker rollout with its own plan.
- **A worker ahead of the Mac** is drift, shown and never moved down automatically *(coordinator's call 3)*. Match the Mac downward is its own click, and its plan carries the `minimumVersion` warning ([10](#10-the-mac-step)).
- **Drift on the Mac's side.** If the Mac updates while a worker is held, that worker drifts on purpose; its card says Held.

## 15. Failure and rollback

**A worker fails** when any step fails, with a reason from a fixed list: `offline`, `unsupported_method`, `disk_full`, `download_failed`, `checksum_mismatch`, `version_mismatch`, `knob_lost`, `timeout`, `ssh_failed`, `plan_changed`, `unknown_state`. A changed plan stops before anything is installed, so it goes straight to Needs You with no rollback.

1. If the install changed anything (Installing or later), the worker rolls back at once, **best effort** *(coordinator's call 8)*: the method's exact install of the previous version, or the copy in `previous/`, then Verifying against the previous version.
2. The rollout **halts**. Every worker not yet reached keeps its version.
3. Machines shows **Retry**, **Skip m1** and **Roll Back** on the banner, and the Inbox gets a notice, "Claude Code 2.1.324 failed on m1: the installed version is 2.1.323. m1 is back on 2.1.323. 2 workers not updated.", with **Open Machines**.
4. If the rollback fails too, or its verify doesn't show the previous version, the worker is marked **Needs You**, and its card shows the last output (masked, cut to 300 characters). Nothing more runs on it until the user acts.

**Rolling back a worker by hand.** Each tool row on a Machines card has **Roll Back to <previous>**, shown only when a previous version is known and kept. It is its own click with its own plan, never part of Set Up All (the precedent is Replace Key, `steps_replace.rs`). A rolled-back worker is Held for that tool until the user releases it, or it would be rolled forward again on the next pass.

**Rolling back the Mac.** Settings > Agents > *tool* has **Go Back to <previous>** when the method can install an exact version. It runs that install and says plainly: "Claude Code will update itself again at its next check. Workers stay on 2.1.323." The failed version stays marked failed, so even if the Mac's auto-update brings it back, it is never rolled out. Notchling doesn't turn the Mac's auto-update off.

**A failed version** is recorded per tool in `versions/failed` with the check part that failed. Pressing Check Again clears it if the check passes.

## 16. Notchling itself

Notchling is the fourth tool in the same machinery, with its own adapter. Two parts are not built and belong to the maintainer: the release path, and Notchling running on workers at all (cluster v1, step 1).

**On the Mac.** The app's update check exists and is off (`welcome/updates.rs`, `NOTCHLING_RELEASES`). The updater is **Sparkle** *(user's decision 4)*, with the existing appcast (`tools/release/appcast.py`) and its EdDSA key, and it installs an update only when the app is idle (0010). A new decision record amends 0010, whose plan for the Rust app was a separate ed25519 manifest, and Sparkle waits for the Developer ID, as releases already do. After the restart, the new build is the Mac's Notchling version, checked by its own smoke check ([11.1](#111-what-the-check-runs)), and becomes the reference.

**What a worker needs.** A worker runs no app, only `notchling` (the daemon and its command line) and `notchling-ptyhost`. So each release also carries, per target:

| Asset | Contents |
| --- | --- |
| `notchling-<v>-aarch64-apple-darwin.tar.gz`, `-x86_64-apple-darwin`, `-x86_64-unknown-linux-gnu`, `-aarch64-unknown-linux-gnu` | `notchling`, `notchling-ptyhost` |
| `notchling-manifest.json` | each asset's name, size and SHA-256, and the build's own version string |
| `notchling-manifest.json.sig` | an ed25519 signature over the manifest |

- **The version is the build's own string.** A promoted Stable keeps its Beta's version (`1.3.0-beta.2`, automation.md), so the rollout matches the Mac's build string, never the release tag. The manifest maps one to the other.
- **Signing.** The manifest is signed in the `sign` job, which runs no repository code (0010). It uses Sparkle's EdDSA (ed25519) key *(user's decision 4)*: one key and one offline backup for the appcast and the worker manifest; the public key is compiled into `notchling`.
- **Checking.** The Mac downloads the manifest, checks the signature with the compiled-in key, and sends the worker the asset's URL and SHA-256. The worker downloads it itself (`curl --proto =https`, GitHub only) and checks the hash before unpacking. The worker trusts only what the Mac sent.
- **Layout on a worker,** like Claude Code's: `~/.local/share/notchling/versions/<v>/` with both binaries, `~/.local/bin/notchling` a symlink to the current one, the service file pointing at the symlink. Install is unpack, verify, move the link; rollback moves it back. Two versions kept.
- **Restart.** `launchctl kickstart -k` or `systemctl --user restart` for the daemon, between ops. The worker's daemon starts with every switch off, so it needs the cluster's node tier and headless switch policy (cluster review issue 3) before Phase 5 can restart one unattended.
- **Mixed versions during a rollout** are fine for the local API and the phone protocol (additive only). The cluster's handshake allows a Notchling mismatch within one `api` level and warns; on a tool mismatch it warns until Phase 6 and refuses from then ([17.5](#175-dispatch)).

## 17. What the user sees

### 17.1 Settings > Agents

0019 gives each tool a page with a **Client** part. This design fills it:

```
Claude Code
--------------------------------------------------------------------------------
Client      Found at ~/.local/bin/claude · 2.1.324 · native installer
            Latest 2.1.325                                         [ Update ]
            Check       Partly checked 14:03 · version, doctor, transcripts  [ Check Again ]
            Since 2.1.324: 14 turns read, nothing unknown
Workers     m1          2.1.324                                    Done
            vps-sing-1  2.1.323 → 2.1.324                          Waiting for 2 processes
            ub-2        2.1.323                                    Offline, will catch up
            ub-3        Not installed
            Keep workers on the Mac's versions  [ Ask Me        v ]
                                                       [ Go Back to 2.1.323 ]
```

- **Update** shows only when Latest is newer and the method can update. With the `install` switch off it says so and does nothing.
- The Workers lines are read only here; rollouts start in Machines in Phases 1 to 4.
- The Check line says Partly Checked, as the built check always does, and what it didn't check: screens, dialogs and turns *(user's decision 2)*. The soak line under it never holds a rollout *(user's decision 3)*.
- A failed check turns the Check line red: "Failed 14:03: `codex exec` no longer takes `--json`. Workers stay on 0.166.0." with **Details** (which check, never the screen).
- Notchling's own row is on the General page beside About and Check for Updates, with the same Workers lines.

### 17.2 Machines

- Each machine card gets a **Tool Versions** step: one row per tool with its version and method, and a mark against the Mac's version (same, behind, ahead, Held, Can't Pin, Not Installed). Its Fix is **Match the Mac**: the plan lists each tool's install line and the order; Run takes one click. A row that would move a tool down carries the `minimumVersion` warning.
- With no answer from the daemon, the banner says **Notchling isn't running** and nothing starts; with a stale reference it says **The Mac's check is out of date** *(coordinator's call 1)*.
- The **Roll Out** sheet lists every worker's plan, says Partly Checked (as the built check always does) with the soak's line, and takes one click *(coordinator's call 2)*.
- **Resume Rollout** and **Cancel Rollout** when the journal holds a rollout that stopped *(coordinator's call 9)*.
- **Self-Update Off** is a step per tool with a Fix ([9](#9-self-update-off-on-workers)).
- **Roll Back to <previous>** is on each tool row, its own click.
- A **rollout banner** at the top while one runs: "Rolling out Claude Code 2.1.324: m1 done · vps-sing-1 waiting for 2 processes · ub-2 offline". Cancel finishes the step in progress first; a worker in Waiting has its own Cancel that skips it *(coordinator's call 7)*.
- A Homebrew install shows "Homebrew can't install an exact version" with **Switch to the Native Installer** (or the Standalone Installer), its own click with its own plan *(user's decision 1)*.

### 17.3 Inbox

New kinds, additive to `inbox::model::Kind`. In Phases 1 to 4 every one is a notice whose action is **Open Machines** (`open`) *(coordinator's call 2)*; the Phase 6 column is what the daemon can offer once it runs rollouts.

| Kind | Title | Phases 1 to 4 | Phase 6 on |
| --- | --- | --- | --- |
| `version_rollout` | Claude Code 2.1.324 Passed on the Mac | Open Machines | Roll Out (`continue`), Not Now (`cancel`) |
| `version_check_failed` | Claude Code 2.1.324 Didn't Pass | Open Settings (`open`) | Check Again (`continue`) |
| `version_rollout_halted` | Claude Code 2.1.324 Stopped at m1 | Open Machines | Retry (`continue`), Skip m1 (`cancel`) |
| `version_busy` | m1 Is Still Waiting | Open Machines | Skip m1 (`cancel`) |
| `version_drift` | vps-sing-1 Runs Another Codex | Open Machines | Match the Mac (`continue`, never downward) |

Item ids are fresh per event (`rollout:<random>`), as the inbox requires. A done row says what was decided and where ("Rolled Out · Mac", "Skipped · Phone").

While the rollout runs in the Machines window (Phases 1 to 4), the daemon can't act on an answer, so the phone shows "Open on Mac". Once the daemon coordinates (Phase 6), Roll Out and Retry work from the phone too, with `acting` and the bridge on, like other inbox answers.

### 17.4 Notch and phone

- **Notch:** nothing new. The Inbox badge already counts these items.
- **Phone:** the Inbox items, and per computer the tool versions in a later protocol field. No new screen.

### 17.5 Dispatch

**From Phase 1, a warning** *(coordinator's call 10)*. Phase 1 ships a pure, read-only check, `versions::mismatch(reference, worker) -> Vec<Mismatch>`, over the facts `versions.get` and the Tool Versions probe already hold. The cluster's step-1 version handshake and `notchling delegate --machine` call it and warn: "m1 runs Claude Code 2.1.323, the Mac 2.1.324". The work still goes. A tool the worker lacks isn't a mismatch; the dispatch fails on its own if it needs that tool.

**From Phase 6, a refusal.** When the cluster dispatches work (New Session's Run on column, `notchling delegate --machine`), a mismatch refuses with the same words and offers **Match the Mac** and **Run Anyway** in the app; an agent's `delegate --machine` gets the refusal, never Run Anyway. This is the cluster review's issue 10, answered here.

## 18. Where the code goes

| Part | Where | Zone |
| --- | --- | --- |
| Versions, methods, parsers for each tool's `--version` and files, the version pattern, comparisons | `crates/engine/src/versions/` (`version.rs`, `method.rs`, `parse.rs`) | maintainer |
| The mismatch check (Phase 1) | `versions/mismatch.rs` | maintainer |
| Tool adapters: probe script, busy probe, exact install, save previous, restore, knob plan | `versions/adapters/{claude,codex,agy,notchling}.rs` behind one trait | maintainer |
| The rollout state machine, journal and lock | `versions/rollout.rs`, `versions/journal.rs`, `versions/lock.rs` | maintainer |
| The B gate | `versions/gate.rs`: `trait Gate { fn check(&self, tool, v) -> GateResult }`, `OpenGate` in A | maintainer |
| Strict counters for the readers | `crates/engine/src/transcript/` (a `Strict` mode beside the lenient one) | auto (`transcript/**`) |
| Mac detection and watch | `crates/notchlingd/src/versions_ops.rs`, an `OpGroup` | maintainer |
| Smoke check | `notchlingd/src/versions_check.rs` over the tools' one-shot commands and the strict readers; the later additions would add `engine::terminal`, `reply` and `tools::acp` | maintainer, `CAP-PROCESS` |
| Machines steps: Tool Versions, Self-Update Off, Match the Mac, Roll Back | `crates/engine/src/machines/steps_versions.rs` | maintainer |
| Remote settings edit | `machines/remote_edit.rs` over `install::{jsonc,edit}` and a new `install::toml_edit` | maintainer, `CAP-FILEWRITE` |
| Settings > Agents Client part, Machines rows and banner, Inbox rows | `crates/app/src/sheets/`, `crates/app/src/machines/`, `crates/app/src/inbox/` | auto |

**Who runs the rollout.**

| Phases | Coordinator | Effects through | Approval |
| --- | --- | --- | --- |
| 1 to 4 | `notchling-machines` (Machines window, `--live`), reading the reference from the daemon's `versions.get` | Machines' `Runner` over ssh | One Roll Out click in Machines; each worker's plan checked again before its install; the daemon raises notices from the facts Machines saves *(coordinator's calls 1 and 2)* |
| 6 on | the leader's daemon (cluster) | the node link to each worker's own daemon, which installs locally | the Inbox, from the Mac or a phone |

This keeps 0004's call (Machines is a separate tool) until the cluster moves it. Nothing rolls out while the Machines window is closed; the daemon's notice says "Workers are behind the Mac: open Machines". Automatic needs a host that is always there, so it waits for Phase 6. The rollout engine is the same pure code in both, behind a `RolloutHost` trait with `SshHost` and, later, `NodeHost`.

**Local API changes,** all additive, in `docs/local-api-ops.md`:

| Op | Tier, spec | Args | Reply |
| --- | --- | --- | --- |
| `versions.get` | agent, `AGENT` | `{tool?}` | `{tools: [{tool, installed: {version, sha256}, method, path, running: [v], latest?, reference?: {version, sha256, result, partly, at}, usable, failed: [v], check: {result, at, parts}}]}`; Machines reads it in Phases 1 to 4 |
| `versions.latest` | app, `APP_QUIET` | `{tool}` | `{latest, at}` (network, at most once an hour) |
| `versions.update_local` | app, `APP`, `install` switch | `{tool, to?}` (`to` an exact version, for Go Back) | at once `{started}`; result as `versions.updated` |
| `versions.check` | app, `APP` | `{tool, turn?}` | at once `{started}`; result as `versions.checked` |
| `versions.workers` | app, `APP_QUIET` | `{}` | per worker and tool, from the facts Machines saved |
| Topic `versions` | app | | `versions.changed`, `versions.checked`, `versions.updated` |

Work that can outlast 5 s answers at once and sends its result as an event, the rule in `crates/AGENTS.md`.

## 19. Events

Kinds for the logging design's registry, group **Versions**. Every field is a typed field from its list: `Version`, `Choice`, `Ref<Machine>`, `Count`, `Millis`, `Money`. No text.

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `tool.version_seen` | Tool Versions | Toggle, on | Ac D | Already in the logging design: the first sighting of a version |
| `version.changed` | Tool Versions Changed | Toggle, on | Ac D | `tool`, `machine`, `from`, `to`, `method` (`native`, `standalone`, `npm`, `brew`, `release`, `unknown`), `cause` (`user_update`, `self_update`, `rollout`, `rollback`, `outside`) |
| `version.checked` | New Version Checks | Toggle, on | Ac D | `tool`, `version`, `tier` (`files_flags`; later `screens`, `turn`), `result` (`passed`, `partly`, `failed`, `cant_tell`), `failed_part` (`version`, `doctor`, `exec_flags`, `transcript`, `self_test`; later additions add their own), `unknown_count`, `duration_ms`, `cost` |
| `rollout.started` | Rollouts Started | Always | A Ac D | `tool`, `to`, `workers`, `approved_by` (`machines`, `inbox_mac`, `inbox_phone`, `automatic`), `partly` (Flag), `first_run` (Flag), `gate` (`open`, `known_good`, `unknown`, `known_bad`) |
| `rollout.step` | Rollout Steps | Always | A Ac D | `tool`, `machine`, `from`, `to`, `step` (`precheck`, `wait`, `plan_check`, `save`, `install`, `verify`, `move`), `result` (`ok`, `failed`, `offline`, `skipped`, `needs_you`), `why` (the list in [15](#15-failure-and-rollback)), `waiting_for` (Count), `duration_ms` |
| `rollout.lock` | Rollout Lock | Always | A Ac | `action` (`taken`, `taken_over`, `released`, `refused`), `resumed` (Flag) |
| `rollout.finished` | Rollouts Finished | Always | A Ac | `tool`, `to`, `result` (`complete`, `halted`, `superseded`, `cancelled`), `done`, `failed`, `pending` |
| `rollout.rolled_back` | Rollbacks | Always | A Ac D | `tool`, `machine` (the Mac or a worker), `from`, `to`, `cause` (`user`, `failed_step`), `result` |
| `selfupdate.changed` | Self-Update on Workers | Always | A Ac | `tool`, `machine`, `knob` (`claude_disable_autoupdater`, `codex_update_check`, `codex_daemon_auto_start`, `agy_env`), `place` (`config_dir`, `lane_config_dir`, `codex_home`, `service_file`), `action` (`off`, `on`, `already`), `result` (`ok`, `failed`, `cant_read`) |
| `version.drift` | Versions Out of Step | Toggle, on | Ac | `tool`, `machine`, `expected`, `found` |

The rollout kinds are `Always` because each is a process run with the user's rights on another machine. Until the logging design lands, the same facts go to `audit.jsonl` as today's lines do, with no free text.

## 20. Settings keys

Classes from decision 0020 (Free: set at once; Ask: an Inbox item; Never: refused). A key with no class is Never.

| Key | Type, default | Class | Why the class |
| --- | --- | --- | --- |
| `versionRollout` | `off`, `ask`; `automatic` added in Phase 6 and offered only for a full check; `off` | **Never** | It decides whether software installs on other machines without the user. An agent misled by a prompt must not turn on Automatic, and turning it Off would let drift in quietly. Off to Ask is the first run *(coordinator's call 3)* |
| `versionCheckScreens` | bool; false | **Ask** | A later addition, not built now *(user's decision 2)*. It would start scratch Claude Code sessions on a pty and add one trust entry to the Mac's `~/.claude.json`. |
| `versionCheckTurn` | bool; false | **Ask** | A later addition, not built now *(user's decision 2)*. It would spend money on the account in use, a few cents a version. |
| `versionWaitIdleMinutes` | 5 to 1440; 60 | **Free** | It only changes when a notice says a worker is still waiting for processes. Nothing is ever killed, and the wait itself has no end but Cancel. |
| `versionHeld` | list of machine names; empty | **Ask** | A held worker drifts on purpose; the item says which. |
| `versionGate` | `off`, `warn`, `block`; `off` | **Never** | The B slot. Loosening it is loosening a guard rail. |

Keys are named in `crates/engine/src/settings/keys.rs` with their title, description and reason. A test holds that `versionRollout` and `versionGate` are Never. The settings store is a file an agent could write (0007's note of 2026-11-08). In Phases 1 to 4 that matters little: the setting only decides whether Machines offers, and nothing moves without a click. From Phase 6 the daemon takes Automatic from the app in each run, never from the store alone, the way 0019 handles its toggles.

## 21. Security and privacy

- **What runs on a worker** is a fixed set of scripts, each shown in its plan before the click (plan, confirm, run). Variable parts are checked versions, checked paths and hashes. The tests in [22](#22-tests) hold it.
- **No admin.** Every step runs as the worker account. `run::admin` is never used by this design.
- **Downloads** come only from the vendor's own hosts named in [7](#7-reading-a-version) and [8](#8-installing-an-exact-version), and GitHub for Notchling, always over https (`--proto =https`), each checked against a checksum before use.
- **New network calls** from the Mac: the Latest reads, agy's manifest and binaries for the cache, Notchling's release manifest. Each is a `CAP-NETWORK` change that updates `docs/policies/privacy.md` in the same pull request: what is fetched, when (on demand, at most hourly), and that nothing identifying is sent. Workers' own downloads go in the Machines section, beside the installers it already lists.
- **Worker settings files** are read into memory for the plan, never logged, never shown beyond the changed line, and sent back only over ssh's standard input. Masks apply to every output (`masks.rs`).
- **Goals amendment** (Phase 0, protected): "only Notchling's hook entries in `~/.claude/settings.json`" becomes "only Notchling's hook entries in `~/.claude/settings.json`, and on a worker set up in Machines, the one key per tool that turns its self-update off; each with a backup and everything else kept".
- **0011, one owner per resource,** extended: on a worker, each tool's version has one owner, Notchling. Its self-updater is off so the two never fight.
- **Agents.** `versions.get` is agent tier (reading versions gives nothing). Everything that acts is app tier. An agent can't approve a rollout: in Phases 1 to 4 only a click in the Machines window does, and from Phase 6 an Inbox answer from the Mac or a paired phone.

## 22. Tests

With fakes only: `ScriptedRunner` for every ssh call, `FakeLauncher` for the Mac's commands, a `FeedFetcher`-style fake for Latest and manifests, a fake clock. No test runs ssh, an installer, a real CLI or the network, or reads `~/.claude`.

| Area | Tests |
| --- | --- |
| Parsing | Each tool's `--version` output, today's and older forms (`2.1.89 (Claude Code)` from the setup page; Codex's and agy's forms taken from real runs when the parsers are written); hostile outputs (a version with `;`, a 10 KB line, a vault value) never become a version; paths from links map to methods |
| Mac detection | A temp home with a native layout, an npm layout, a brew layout, two installs at once; a symlink moved makes `versions.changed`; nothing is started (the launcher fake records no call) |
| Smoke check | Over `FakeLauncher`: only one-shot commands run (`--version`, `claude doctor`, `codex exec --help`), never a pty, a session or a turn, and no path under `~/.claude`, `~/.codex` or `~/.gemini`, nor `~/.claude.json`, is written (a recording file system); a `codex exec --help` text missing `--json` fails with `exec_flags`; strict mode over a temp copy of transcripts fails on an unknown block type in a new-version line and only counts one in an older line; every result is Partly Checked; a Partly Checked reference allows a Roll Out click and never Automatic; the soak never changes a rollout's state |
| Rollout engine | Every transition of [12](#12-the-rollout); one click approves every worker on the sheet; a catch-up whose rebuilt plan equals the shown one runs, any other is Needs You; superseded mid-step finishes the step; the journal resumes from each state only after a Resume event, never on open |
| Reference | `versions.get` failing or timing out starts nothing and shows Notchling isn't running; a reference whose SHA-256 or version differs from the installed one is stale and starts nothing; Partly Checked never starts Automatic; the first rollout after Off to Ask is never Automatic; npm and agy never Automatic |
| Lock | Two owners can't both hold it; a lock with an old heartbeat or a dead pid is taken over; a live one is refused; a closed window leaves the journal resumable |
| Tool list and drift | An absent tool is Not Installed, never installed, never drift; a worker ahead is drift and no rollout moves it down; Match the Mac downward carries the warning |
| Busy | The probe counts plain `claude`, `claude -p` and sd jobs as busy and leaves supervised background sessions out (over recorded `ps` and `claude agents --json` outputs); Waiting never ends but by idle or Cancel; Cancel skips only that worker |
| Mismatch | `versions::mismatch` over references and worker facts, absent tools left out |
| Fleet simulation | Seeded scenarios over 1 to 6 fake workers (online, offline, busy, npm, brew, failing install, failing verify, flapping), checking after every step: at most one install at a time anywhere; only the reference version is ever installed; a failure halts and leaves unreached workers untouched; a failed worker ends on its previous version or Needs You; no argv ever holds `kill`, `pkill`, `claude stop` or `respawn` aimed at a working session; a version marked failed is never installed; every remote write is preceded by a backup step |
| Remote edit | Over the `install` fixtures: one key added, everything else byte for byte; a duplicate key refused; a hash that changed between plan and run refuses; the plan's display holds the one line and never a token from the file; undo removes only Notchling's value; every known place gets the knob, the service files get the variable, and a place that can't be read is reported, not skipped |
| TOML edit | Top-level key before the first table, `[features]` key, comments and order kept, a file with the key already set unchanged |
| Safety (like `tests_safety*`) | A refresh runs only read-only probes; no rollout step runs without its confirm; Match the Mac and Roll Back are not in Set Up All; no step uses `run::admin`; every download URL is on the allowed hosts and https |
| Settings | `versionRollout` and `versionGate` are Never; an agent-tier `settings.set` on them is refused and audited |
| Notchling | The manifest's signature checked with a test key; a wrong signature, a wrong hash, a version not in the manifest each refuse; a Beta build string maps to its Stable tag |

New tests go in new files (`tests_versions*.rs`, `versions/*_tests.rs`). Existing tests aren't edited, except where a phase changes a step's catalog, which is a `WEAK-TEST-EDIT` the maintainer approves.

## 23. Phases

Each ships alone and leaves the app working. Docs (`crates/AGENTS.md`, the engine's and app's `AGENTS.md`, `docs/feature-map.md`, `docs/local-api-ops.md`, privacy.md) change with the phase that changes what they describe.

| Phase | Ships | Tests | Zone |
| --- | --- | --- | --- |
| **0. Decisions** | A decision record "Tool versions follow the Mac" (next free number); a decision record amending 0010: Sparkle for the Rust app, one EdDSA key for the appcast and the worker manifest *(user's decision 4)*; the goals.md amendment ([21](#21-security-and-privacy)); privacy.md's Planned list gains the version reads | none | Maintainer: `docs/decisions/`, `docs/goals.md`, `docs/policies/` |
| **1. See versions** | `engine::versions` (types, parsers, methods, binary identity); Mac detection in the daemon, `versions.get`, the `versions` topic; Machines' Tool Versions step (read only, per-worker tool list) reading the Mac's side from `versions.get`; Settings > Agents' Client lines and Machines' rows with drift marks; the read-only mismatch check, used by the cluster's step-1 handshake and `delegate --machine` to warn *(coordinator's call 10)*; no Latest yet | Parsing, Mac detection, the probe's safety, mismatch | Maintainer (engine, daemon, Machines steps); the views are auto |
| **2. Check on the Mac** | The files-and-flags smoke check *(user's decision 2)*; strict counters and the soak, shown and never gating *(user's decision 3)*; `versions.check`; failed versions; the notice on a failure; Latest and the Update button (`versions.update_local`, `install` switch) | Smoke check, strict mode, update over `FakeLauncher` | Maintainer: `CAP-PROCESS`, `CAP-NETWORK`; the transcript part is auto |
| **3. Workers stay put** | Self-Update Off per tool in every known place and the sd service files; the remote edit and `install::toml_edit`; drift detection and its notice | Remote edit, TOML edit, safety | Maintainer: `CAP-FILEWRITE`, Machines |
| **4. Roll out** | The rollout engine, journal, lock and Resume Rollout; the Roll Out sheet and the per-worker plan check; first run; adapters for Claude Code (native, npm) and Codex (standalone, npm); busy probe and Waiting with Cancel; Match the Mac (downward with its warning); Roll Back; the banner; `versionRollout` (Off, Ask), `versionWaitIdleMinutes`, `versionHeld`; then agy (vendor latest, then the cache) | Engine, fleet simulation, lock, safety | Maintainer: Machines, `CAP-PROCESS`, `CAP-NETWORK` (the agy cache) |
| **5. Notchling on workers** | Release assets per target and the manifest signed with the Sparkle EdDSA key (beta.yml, promote.yml, appcast.py's neighbor); Sparkle for the Mac app, once the Developer ID exists *(user's decision 4)*; the worker layout, install, rollback and restart | Manifest and signature, layout, the Beta-to-Stable mapping | Maintainer only: the release path. Waits for cluster v1 step 1 (a daemon on each worker, the node tier) |
| **6. The daemon coordinates** | The rollout moves to the leader's daemon on the node link; Inbox answers from the Mac and the phone; Automatic (never for the first rollout, Partly Checked, npm or agy, so not offered while the check is files and flags only); dispatch refuses a mismatch | Same engine tests over `NodeHost`; Automatic's exclusions; dispatch handshake | Maintainer: daemon, cluster |
| **7. Approach B** | Known-good lists in Notchling's release manifest; the `Gate`; `versionGate` | Gate results; a known-bad version blocks before the Mac step | Maintainer: release path, engine |

Phases 2 and 3 can run side by side once 1 is in. Phase 4 needs both. The later additions of [11.2](#112-possible-later-additions) are in no phase; each would get its own decision record and setting.

## 24. The slot for approach B

A takes one decision point that B fills later without changing the rollout:

```rust
pub enum GateResult { Open, KnownGood, Unknown, KnownBad { why: Reason } }
pub trait Gate: Send + Sync { fn check(&self, tool: Tool, version: &Version) -> GateResult; }
```

- **Where it runs:** before the Mac step when the user presses Update (the target is known then), and in the rollout's Gated state for anything the Mac's auto-update installed on its own.
- **In A:** `OpenGate` answers `Open` for everything. Gated passes straight through, and the event records `gate: open`.
- **In B:** each Notchling release's signed manifest carries `tools: {claude: {good: ["2.1.320", "2.1.324"], bad: ["2.1.322"]}, ...}`, made from the versions its own checks and recordings passed. `versionGate` decides what Unknown and KnownBad do: `warn` shows a line under Update and on the rollout item, `block` refuses the Update button and the rollout. The Mac's own auto-update is never stopped; a known-bad version it brings is simply never rolled out.

## 25. Left to the builder

- Verify before building on them: agy's self-update knob, and whether `agy --version` triggers an update; Codex's `daemon_auto_start` key; whether a manual `claude install` makes the background supervisor restart; whether Codex's standalone packages keep old versions; Claude Code's per-line transcript `version` field on a fresh recording; ACP's `agentInfo` in agy's `initialize`; whether an agy `session/new` costs a turn; the `stable` file beside Claude Code's `latest`.
- The exact strict-mode counters per reader, and the threshold (zero, or zero of each kind that matters).
- Which existing transcripts the check's strict pass reads (how many, how far back), and how it tells a line's version (Claude Code's per-line `version`, Codex's `cli_version`; agy's logs may carry none).
- Timeouts, the disk floor, how many versions the agy cache keeps, the lock's heartbeat and stale age.
- How the busy probe tells the supervisor's background sessions from other `claude` processes (the supervisor's command line and process tree, checked on a real worker), and what `claude agents --json` prints for each.
- Where Claude Code's managed settings file sits on macOS and Linux, for the `requiredMinimumVersion` warning, from Claude Code's settings docs.
- What the sd service files look like once they carry the extra environment lines, and the restart that applies them.
- Which host Codex's Latest reads (npm registry or GitHub).
- The remote scripts' exact text, inside the rules in [8](#8-installing-an-exact-version) and [9](#9-self-update-off-on-workers).
- The layout of the Client part, the Tool Versions rows and the banner, inside the kit's tokens and menus.
- Whether the fleet simulation shares the cluster's deterministic simulator once that exists.

## 26. What was checked on the web

Checked on 2026-11-13.

**Verified** (official pages or the vendor's own scripts):

- Claude Code [setup page](https://code.claude.com/docs/en/setup): native install of a version (`bash -s 2.1.89`), the launcher symlink into `~/.local/share/claude/versions/` and which versions it keeps, `DISABLE_AUTOUPDATER` in settings' `env` (and that `claude install` and `claude update` still work), `DISABLE_UPDATES` blocks them, `autoUpdatesChannel`, `minimumVersion`, managed `requiredMinimumVersion`/`requiredMaximumVersion`, npm's package, Homebrew's two casks and that they don't auto-update, `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE`, npm upgrades deleting the running binary, the signed `manifest.json` and its key fingerprint, updates taking effect at the next start.
- Claude Code [CLI reference](https://code.claude.com/docs/en/cli-reference): `claude install [version]` (a version, `stable` or `latest`), `claude respawn` (to pick up an updated binary), `claude daemon status` (the supervisor's version), `claude agents --json`, `claude stop`, `claude doctor`.
- Claude Code [agent view](https://code.claude.com/docs/en/agent-view): after an auto-update the supervisor restarts onto the new version and moves idle sessions; working, waiting or attached ones aren't interrupted.
- Claude Code's [bootstrap installer](https://downloads.claude.ai/claude-code-releases/bootstrap.sh): reads `<base>/latest`, downloads `<base>/<v>/manifest.json` and the platform binary, then runs `<binary> install <target>`; the target must be `stable`, `latest` or `x.y.z`.
- Codex [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference): `check_for_update_on_startup`.
- Codex [README](https://github.com/openai/codex): `npm install -g @openai/codex`, `brew install --cask codex`, the standalone installer, release asset names.
- Codex [installer](https://releases.openai.com/codex/install.sh): `--release <v>` and `CODEX_RELEASE`, `~/.local/bin`, release files under `~/.codex/packages/standalone`, SHA checks.
- Antigravity CLI [installer](https://antigravity.google/cli/install.sh): only `--dir`; always the manifest's latest; `~/.local/bin/agy`; SHA-512 check; exits if agy exists, saying it self-updates in the background.

**Not verified** (third-party or user reports only, marked where used):

- Codex's background server and hourly updater since 0.157.0, and the `daemon_auto_start` opt-out: [openai/codex #48195](https://github.com/openai/codex/issues/48195) (open, no maintainer reply) and a [summary of the change](https://braindetox.kr/en/posts/codex_cli_0157_daemon_auto_update_backlash_2026.html); the claim that its auto-update stops running turns (#40969) comes from that summary.
- `codex update` (third-party guides, from about 0.128.0), and that the Homebrew cask can't be pinned.
- `AGY_CLI_DISABLE_AUTO_UPDATE=true` ([a third-party guide](https://wikidocs.net/365257)) and `agy update`.
- `autoUpdates` as a Claude Code setting (absent from the current settings reference).
- Whether a manual `claude install` restarts the background supervisor; apt's old versions; Codex standalone keeping old releases; agy's ACP `agentInfo`.

## 27. Open questions

None. The user decided the four that were open on 2026-11-14, each as recommended; they are recorded in [Decided by the user](#decided-by-the-user) and marked in the sections they change.
