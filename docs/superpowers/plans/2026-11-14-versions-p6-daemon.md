# Version sync, Phase 6: The Daemon Coordinates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rollouts move from the Machines window to the leader's daemon, which reaches each worker's own daemon over the cluster's node link. Version items in the Inbox get real answers (Roll Out, Not Now, Retry, Skip, Match the Mac) from the Mac or a paired phone. `versionRollout` gains Automatic, offered only for a fully checked version, so it stays unavailable while the check is files and flags only. Dispatch refuses a tool-version mismatch, with Run Anyway in the app only.

**Architecture:** The same pure engine (`versions::rollout`) behind a second `RolloutHost`, `NodeHost`, whose effects are node-tier calls (`versions.local_step`) that each worker's daemon runs locally with the Phase 4 adapters. `notchlingd/src/versions_rollout.rs` owns the `Driver`, the journal and the lock on the leader; Machines becomes a view over `versions.rollout.*` ops. Inbox items get `Responder`s that turn answers into engine events. The cluster's `compat` tool rule goes back from Warn to Refuse.

**Tech Stack:** Rust (engine, notchlingd, local-api, cluster core, GPUI app), `agentconsole.v1` additive fields for the phone's version items.

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: sections 12 (Automatic), 16 (Restart, Mixed versions), 17.3 (Phase 6 column), 17.5 (From Phase 6, a refusal), 18 (Who runs the rollout, Phase 6 row), 20 (`versionRollout`), 21 (Agents), 23 (Phase 6), coordinator's calls 2, 3, 8, 10.

**Branch:** `agent/versions-p6-daemon`.

**Depends on:** Phase 4; cluster plan B (node link, `ClusterNode::local(Forward)`, the node tier `policy::RULES`, headless switches) merged; Phase 5 for Notchling rollouts over the node link (the other tools don't need it). The move of the coordinator from the Machines window to the leader's daemon is already in Phase 0's record "Tool versions follow the Mac" (it names when 0004's separate-tool call stops covering rollouts), so no new record is needed here.

## Global Constraints

- Phase 4's constraints hold. In addition:
- **The leader runs rollouts;** a worker's daemon only runs the step it is asked, with the adapters, as its own user, and answers with the step's result. A Worker never starts a rollout or a step on another node (the cluster's rule).
- **Automatic** starts only when `approval::automatic_allowed` is true (never first run, Partly Checked, npm or agy) and the app sent `settings.set_rollout_automatic` in this daemon run, at the app tier: the store alone never turns it on (decision 0019's toggles; spec §20). As built, every check is Partly Checked, so Automatic is shown off with "Needs a full check".
- **Inbox answers** follow the inbox's rules: first answer wins, a fresh id per event, a responder that only sets a slot. From a phone they need the bridge and `acting` on, like other answers. An agent can't answer (app tier only).
- **Dispatch refuses** a tool mismatch from this phase on, naming both versions; Run Anyway only from the app tier, never for an agent's `delegate --machine`.
- The local API and `agentconsole.v1` change additively only.

## Review Focus

1. A phone answering Roll Out while the Mac answers Not Now: one wins, the engine sees one event. (Task 3 `phone_and_mac_answer_once`.)
2. Automatic with a Partly Checked reference, a first run, an npm worker or agy: never starts. A store file that says `automatic` without the app's call this run: never starts. (Task 4 `automatic_needs_this_runs_app_call`, `automatic_never_for_partly`.)
3. The leader's daemon restarts mid-rollout: the journal resumes only on Resume Rollout. (Task 2 `daemon_restart_needs_resume`.)
4. An agent's `delegate --machine` to a mismatched worker is refused with no Run Anyway. (Task 5 `agent_dispatch_refused_without_run_anyway`.)

---

### Task 1: `NodeHost` and the worker's local step

**Zone:** maintainer (`notchlingd`, `crates/engine/src/cluster`, `local-api`). `CAP-PROCESS` (the worker runs its install scripts locally).

**Files:**
- Create: `crates/notchlingd/src/versions_node.rs`, `crates/engine/src/versions/node_host.rs`
- Test: `crates/notchlingd/src/versions_node_tests.rs`, `crates/engine/src/versions/node_host_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (node-only ops `VERSIONS_LOCAL_PROBE = "versions.local_probe"`, `VERSIONS_LOCAL_STEP = "versions.local_step"`), `crates/engine/src/cluster/policy.rs` (rows: the home may call both on a member; nobody else)

**Interfaces:**
- Produces: `NodeHost: RolloutHost` over `ClusterNode::local(LocalCmd::Forward { node, op, args, origin })`; `versions.local_probe {}` → the worker's own `WorkerTools` and `Busy` (its own process table and `claude agents --json`); `versions.local_step {rolloutId, tool, kind, to, previous}` runs the adapter's script for that kind through the worker's `Launcher` with spec §12's timeouts, answers at once and sends the result as a node event (work over 5 s answers by event, crates/AGENTS.md).
- [ ] **Step 1: Write the failing tests** (two in-process nodes on cluster B's `MemTransport`): `home_runs_a_step_on_a_worker`, `worker_cannot_ask_the_home`, `step_answer_by_event_after_the_install`, `offline_worker_is_offline_not_failed`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_node` and `cargo test -p notchling-engine --lib versions::node_host`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `versions: run rollout steps through each worker's own daemon`.

### Task 2: The leader's daemon runs rollouts

**Zone:** maintainer (`notchlingd`). `CAP-FILEWRITE` (journal).

**Files:**
- Create: `crates/notchlingd/src/versions_rollout.rs`
- Test: `crates/notchlingd/src/versions_rollout_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (ops `VERSIONS_ROLLOUT_SHEET`, `VERSIONS_ROLLOUT_START`, `VERSIONS_ROLLOUT_ACT` (retry, skip, roll_back, cancel, cancel_worker, resume), `VERSIONS_ROLLOUT_STATE`; events `versions.rollout`), `crates/notchlingd/src/daemon_wiring.rs`, `crates/engine/src/machines/engine.rs` (with a leader daemon reachable, Machines' rollout actions call these ops instead of its own `Driver`; Phase 4's window-driven path stays for a Mac with no cluster)

**Interfaces:**
- Produces: the same `Driver`, journal and lock, owned by the leader's daemon; `versions.rollout.start {tool, version, sheetHash}` (`OpSpec::APP`) starts only if the sheet the app showed hashes to the one the daemon builds now (the click's approval, call 2); state on topic `versions`.
- [ ] **Step 1: Write the failing tests:** `start_needs_the_shown_sheet`, `daemon_restart_needs_resume`, `machines_window_uses_the_daemon_when_present`, `no_cluster_keeps_the_window_driver`, `ops_are_app_tier`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_rollout`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `notchlingd: the leader coordinates rollouts`.

### Task 3: Inbox answers for version items

**Zone:** maintainer (`notchlingd`, `crates/engine/src/inbox`, the bridge); the app's and phone's rows are auto (phone features) except the protocol (protected generation).

**Files:**
- Modify: `crates/notchlingd/src/versions_notices.rs` (items carry actions and a `Responder`: `version_rollout` Roll Out `continue` / Not Now `cancel`; `version_rollout_halted` Retry `continue` / Skip `cancel`; `version_busy` Skip `cancel`; `version_drift` Match the Mac `continue`, never downward; `version_check_failed` Check Again `continue`), `protocol/agentconsole.v1.schema.json` and regenerated files (the kinds, additive; `.claude/skills/add-protocol-field`), `apps/flutter/lib/features/inbox/` (the new kinds' rows)
- Test: `crates/notchlingd/src/versions_answers_tests.rs`, `apps/flutter/test/inbox_versions_test.dart`, fixtures under `protocol/fixtures/`

- [ ] **Step 1: Write the failing tests:** `roll_out_answer_starts_the_rollout`, `phone_and_mac_answer_once`, `phone_needs_acting_and_bridge`, `agent_tier_cannot_answer`, `drift_answer_never_moves_down`, `old_phone_ignores_new_kinds`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_answers`, the protocol fixture tests, `~/flutter/bin/flutter test test/inbox_versions_test.dart`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `inbox: version items answered from the Mac or a phone`.

### Task 4: Automatic

**Zone:** maintainer (`crates/engine/src/settings`, `notchlingd`); the Settings view is auto.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`versionRollout` allows `automatic`; still Never), `crates/notchlingd/src/versions_rollout.rs` (start after a check with no click only under `automatic_allowed` and the app's per-run call), `crates/local-api/src/wire.rs` (`settings.set_rollout_automatic`, app tier, per run), `crates/app/src/sheets/client_model.rs` (Automatic offered only when a reference is fully Passed; else off with "Needs a full check")
- Test: `crates/notchlingd/src/versions_automatic_tests.rs`, `crates/app/src/sheets/client_automatic_tests.rs`

- [ ] **Step 1: Write the failing tests:** `automatic_needs_this_runs_app_call`, `automatic_never_for_partly`, `never_first_run_npm_or_agy`, `automatic_item_only_when_finished_or_halted`, `picker_keeps_automatic_off_without_a_full_check`.
- [ ] **Step 2: Run** both filters. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `versions: Automatic, never for a partial check`.

### Task 5: Dispatch refuses a mismatch

**Zone:** maintainer (`crates/engine/src/cluster`, `notchlingd`). `WEAK-TEST-EDIT` (Phase 1 Task 10's warn test changes).

**Files:**
- Modify: `crates/engine/src/cluster/versions.rs` (`Purpose::Dispatch` tool mismatch → `Refuse(text)` again), `crates/notchlingd/src/cluster_dispatch.rs`, the app's New Session Run on column (offers Match the Mac and Run Anyway)
- Test: `crates/engine/src/cluster/versions_refuse_tests.rs`, `crates/notchlingd/src/cluster_dispatch_versions_tests.rs`

- [ ] **Step 1: Write the failing tests:** `dispatch_refuses_naming_both`, `run_anyway_only_from_the_app`, `agent_dispatch_refused_without_run_anyway`, `absent_tool_still_refuses`.
- [ ] **Step 2: Run** both filters. Expected: FAIL. Phase 1's `dispatch_warns_on_a_tool_mismatch_until_phase_6` now fails by design: the maintainer approves its removal (`WEAK-TEST-EDIT`).
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `cluster: dispatch refuses a tool-version mismatch`.

### Task 6: Docs and checks

**Zone:** maintainer. **Files:** `crates/AGENTS.md` (who runs rollouts now), `crates/notchlingd/AGENTS.md`, `docs/local-api-ops.md`, `docs/feature-map.md`, `ARCHITECTURE.md`, `docs/policies/privacy.md` (phones answer version items), `CHANGELOG.md`.
- [ ] **Step 1: Run** the full checks plus `flutter analyze` and `flutter test`. **Step 2: Write** the docs. **Step 3: Commit** `docs: the daemon coordinates rollouts`.

### Checkpoint (maintainer)

With two cluster workers: Roll Out from the phone's Inbox moves both; Not Now on the Mac at the same moment loses or wins cleanly; a Codex dispatch to a worker one version behind is refused with both versions named and Run Anyway works from the app.
