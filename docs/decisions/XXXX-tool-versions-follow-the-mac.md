# XXXX: Tool versions follow the Mac

## Context

Claude Code, Codex and Antigravity's `agy` update themselves on every machine on their own schedule, and Notchling reads what each one writes and draws. A new version can change a transcript line, a dialog, a flag or a protocol, and Chat, the notch and the phone break until the readers learn it. Machines shows a worker's `claude --version` as a pass or fail, never against the Mac. The cluster design's review (issue 10) asks for a handshake that stops a dispatch to a worker on another tool version.

The design is [version-sync-design](../superpowers/specs/2026-11-13-version-sync-design.md), with the user's four decisions of 2026-11-14 and the coordinator's ten calls. Its approach A, the Mac as the reference, was chosen by the maintainer on 2026-11-13.

## Decision

- **The Mac is the reference.** Workers follow the Mac's checked version of Claude Code, Codex, Antigravity and Notchling. Approach B (known-good versions per Notchling release) is a gate slot only: `Gate` answers `Open` for everything until it is built.
- **The Mac's versions are read from files, never by starting a tool.** A symlink, a `package.json`, a binary's hash. For agy, `--version` runs at most every 10 minutes and only until a file names the version.
- **The check is files and flags only** *(user's decision 2)*: `--version`, `claude doctor`, `codex exec --help` and Notchling's readers in strict mode over transcripts already on the Mac. Every result is Partly Checked. A scratch session and a paid turn are possible later additions, each with its own record and setting.
- **A reference is usable only for the binary it checked**: the same version string and the same SHA-256. Otherwise it is stale and is checked again before any rollout.
- **One Roll Out click in Machines approves a version for every worker.** Before each worker's install its plan is rebuilt and compared with the one shown; any difference halts the rollout at that worker as Needs You, and workers already done stay done.
- **Workers use the vendors' own installers only** *(user's decision 1)*. A worker whose tool came from Homebrew gets a Switch fix. npm is allowed for Claude Code and Codex, only while no process of that tool runs, and never Automatic.
- **A turn is never stopped.** A tool installs on a worker only when none of its processes run. Only Claude Code's supervised background sessions are moved to the new binary.
- **One rollout at a time**, by a lock row in `notchling.db` holding the owner's pid and a heartbeat. A stale lock is taken over. A stopped rollout resumes only on Resume Rollout, never silently.
- **On workers each CLI's self-update is off**: one key per tool, written in every place Machines knows for that worker, with a backup. The Mac's configs are never written.
- **The soak is shown, never gating** *(user's decision 3)*.
- **Who coordinates.** The Machines window until Phase 6 (decision 0004's call that Machines is a separate tool holds until then), then the leader's daemon over the cluster's node link. Automatic exists only from then, and never for a partial check, a first run, npm or agy.
- **Dispatch warns on a tool mismatch from Phase 1 and refuses from Phase 6.** This answers the cluster review's issue 10.
- **`versionRollout` and `versionGate` are class Never** (decision 0020).

## Consequences

- It amends decision 0011: on a worker, each tool's version has one owner, Notchling, and its self-updater is off so the two never fight.
- It answers the cluster design's review issue 10: warn first, refuse once rollouts keep workers in step.
- It amends decision 0004 for rollouts only: from Phase 6 the daemon coordinates them. Decision 0012's "computers never relay" is untouched.
- `docs/goals.md` gains one allowed config write on a worker (the self-update key), and `docs/policies/privacy.md` lists the new reads.
- Phase 1 marks drift against the Mac's installed version, labelled "(not checked yet)", since no check exists yet. Nothing can roll out until Phase 4.
- Every reference is Partly Checked while the check is files and flags only, so Automatic stays unavailable ("Needs a full check").
- Nothing here is built. It lands in eight phases, each of which leaves the app working.

## Status

Proposed. The maintainer settles it by accepting and merging it, together with the goals.md line and the privacy.md entries. The number is a placeholder, assigned at merge.

## Date

2026-11-14
