# Version sync, Phase 0: Decisions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The two decision records, the goals.md amendment and the privacy.md entries the version-sync design needs, drafted as Proposed for the maintainer to accept. No code.

**Architecture:** Docs only, all in protected paths (`docs/decisions/`, `docs/goals.md`, `docs/policies/`). An agent drafts; the maintainer edits, accepts and merges.

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: In short, Decided by the user (all four), Calls settled after review, sections 2, 3, 16, 18 (Who runs the rollout), 20, 21 (Goals amendment, 0011), 23 (Phase 0).

**Branch:** `agent/versions-p0-decisions`.

**Depends on:** nothing. Phase 3 can't merge before Task 3 is accepted; Phase 5's release work can't merge before Task 2 is.

## Global Constraints

- Decision numbers: take the next free number when merging. Taken or reserved now: 0019 (`agent/agents-setup-decision`), 0020 (merged on `merge/settings-batch`), 0021 to 0025 (cluster v1 plans B to F), and the logging design's own record. These drafts use `0026` and `0027` as placeholders; renumber at merge and fix every link.
- Use the template in `docs/decisions/README.md`; one-line decision bullets; plain words.
- Status: Proposed. The maintainer settles each by merging.

## Review Focus

1. The goals.md line keeps "only Notchling's own part" true: one key per tool, workers only, backup, everything else kept. (Task 3.)
2. The Sparkle record says plainly that it waits for the Developer ID and that one key with one offline backup signs both the appcast and the worker manifest. (Task 2.)

---

### Task 1: Decision record "Tool versions follow the Mac"

**Zone:** maintainer (`docs/decisions/`). No CAP.

**Files:**
- Create: `docs/decisions/0026-tool-versions-follow-the-mac.md`
- Modify: `docs/decisions/README.md` (index row)

- [ ] **Step 1: Write the record.** Decision bullets:
  - Approach A: the Mac is the reference; workers follow the Mac's checked version of Claude Code, Codex, Antigravity and Notchling; approach B is a gate slot only.
  - The Mac's versions are read from files, never by starting a tool (agy: `--version` at most every 10 minutes until a file is known).
  - The check is files and flags only (user's decision 2): every result is Partly Checked; scratch screens and a paid turn are possible later additions, each with its own record and setting.
  - A reference is usable only for the binary it checked (same version, same SHA-256).
  - One Roll Out click in Machines approves a version for every worker; each worker's plan is rebuilt and compared before its install; a difference is Needs You.
  - Workers: the vendors' own installers only (user's decision 1); Homebrew gets a Switch fix; npm allowed for Claude Code and Codex, only while no process of that tool runs, never Automatic.
  - Never stop a turn: a tool installs on a worker only when none of its processes run; only Claude Code's supervised background sessions are moved.
  - One rollout at a time, by a lock in `notchling.db`; a stopped rollout resumes only on Resume Rollout.
  - On workers each CLI's self-update is off, one key per tool, in every place Machines knows, with a backup; the Mac's configs are never written.
  - The soak is shown, never gating (user's decision 3).
  - Who coordinates: the Machines window until Phase 6 (0004's separate-tool call holds), then the leader's daemon over the cluster's node link; Automatic exists only from then and never for a partial check, a first run, npm or agy.
  - Dispatch warns on a tool mismatch from Phase 1 and refuses from Phase 6 (the cluster review's issue 10).
  - `versionRollout` and `versionGate` are class Never (decision 0020).
- [ ] **Step 2: Consequences** name what it amends: 0011 (on a worker, each tool's version has one owner, Notchling), the cluster design's review issue 10 (answered: warn, then refuse), 0004 (rollouts move to the daemon in Phase 6), decision 0012's "computers never relay" is untouched. Status Proposed.
- [ ] **Step 3: Commit** `docs: decision 0026, tool versions follow the Mac (proposed)`.

### Task 2: Decision record amending 0010: Sparkle, one EdDSA key

**Zone:** maintainer (`docs/decisions/`). No CAP.

**Files:**
- Create: `docs/decisions/0027-sparkle-and-one-release-key.md`
- Modify: `docs/decisions/README.md`, `docs/decisions/0010-build-then-sign-beta-then-stable.md` (a dated note at the end pointing to 0027; the body stays)

- [ ] **Step 1: Write the record.** Decision bullets: the Rust app updates with Sparkle 2 from the existing appcast (`tools/release/appcast.py`), installing only when the app is idle (0010's rule); 0010's separate ed25519 manifest for the app is dropped; the same EdDSA key signs the appcast and `notchling-manifest.json` for workers; one key, one offline backup, held by the maintainer; the public key is compiled into `notchling` and set as `SUPublicEDKey`; the `sign` job runs no repository code; Sparkle waits for the Developer ID, as releases already do; workers get per-target `notchling` and `notchling-ptyhost` tarballs and download them from GitHub with the hash the Mac sent.
- [ ] **Step 2: Consequences:** a lost key means a new key and a release that carries both public keys for one cycle; a leaked key means the same plus revoking the old appcast entries.
- [ ] **Step 3: Commit** `docs: decision 0027, Sparkle and one release key (proposed)`.

### Task 3: goals.md and privacy.md

**Zone:** maintainer (`docs/goals.md`, `docs/policies/privacy.md`). No CAP.

**Files:**
- Modify: `docs/goals.md` ("What Notchling will never do", the "Change more of your config than its own part" bullet: after "only Notchling's hook entries in `~/.claude/settings.json` (with a backup, your other hooks kept)," insert "on a worker set up in Machines, only the one key per tool that turns its self-update off (with a backup, everything else kept)," — spec §21's wording, fitted to the bullet as it reads on `merge/settings-batch`), `docs/policies/privacy.md` (Planned: the Latest reads from `downloads.claude.ai`, `registry.npmjs.org` and agy's manifest host, on demand and at most hourly, sending nothing identifying; agy's manifest and binaries for the cache; Notchling's release manifest from GitHub; workers' downloads beside the installers Machines already lists)

- [ ] **Step 1: Edit** both files as above.
- [ ] **Step 2: Run** `PYTHONPATH=tools/guard python3 -m guard lint` and `python3 -m guard rules`. Expected: clean.
- [ ] **Step 3: Commit** `docs: goals and privacy for tool versions (proposed)`. Open one pull request for Tasks 1 to 3; it waits for the maintainer.
