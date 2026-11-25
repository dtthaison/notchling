# Version sync, Phase 7: Known-Good Versions (Approach B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each Notchling release's signed manifest lists the tool versions its checks and recordings passed (`good`) and the ones that broke it (`bad`). A gate checks a version against that list before the Mac's Update runs and in the rollout's Gated state; `versionGate` (`off`, `warn`, `block`) decides what Unknown and KnownBad do. The Mac's own auto-update is never stopped: a known-bad version it brings is simply never rolled out.

**Architecture:** `versions::gate::ManifestGate` fills the `Gate` slot Phase 4 left open (`OpenGate`), reading the `tools` field of the newest verified manifest (Phase 5's `versions::manifest`). The rollout engine already has the Gated state and the `gate` field on `rollout.started`; nothing in the rollout changes.

**Tech Stack:** Rust (engine, notchlingd, GPUI app), `tools/release` (Python).

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: sections 4 (B), 20 (`versionGate`), 24, 23 (Phase 7).

**Branch:** `agent/versions-p7-known-good`.

**Depends on:** Phase 4 (the Gated state, `Gate` trait), Phase 5 (the signed manifest, Tasks 1 to 3).

## Global Constraints

- Phase 4's and 5's constraints hold. `versionGate` is class **Never**; loosening it is loosening a guard rail.
- The gate never writes the Mac's tool configs and never stops the Mac's auto-update.
- A manifest whose signature fails is no list at all: the gate answers `Unknown`.

## Review Focus

1. `block` with a known-bad version: Update is refused before anything runs, and a rollout of it never starts. (Task 2 `block_refuses_update_and_rollout`.)
2. A forged or unsigned list: ignored, `Unknown`. (Task 1 `unsigned_list_is_unknown`.)

---

### Task 1: `ManifestGate`

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Modify: `crates/engine/src/versions/manifest.rs` (optional `tools: {<tool>: {good: [..], bad: [..]}}`), `crates/engine/src/versions/gate.rs` (`ManifestGate { list: Option<Lists> }`)
- Test: `crates/engine/src/versions/gate_manifest_tests.rs`

- [ ] **Step 1: Write the failing tests:** `good_is_known_good`, `bad_is_known_bad_with_why`, `absent_is_unknown`, `unsigned_list_is_unknown`, `older_manifest_without_tools_is_unknown`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::gate_manifest`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `versions: a gate from the release's known-good lists`.

### Task 2: Wire the gate and `versionGate`

**Zone:** maintainer (`settings/keys.rs`, `notchlingd`); the view is auto.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`versionGate` choice `off`/`warn`/`block`, default `off`, `.never(..)`), `crates/notchlingd/src/versions_update.rs` (check before Update with the target known), `crates/notchlingd/src/versions_rollout.rs` and `crates/engine/src/machines/engine.rs` (the Gated state uses `ManifestGate`), `crates/app/src/sheets/client_model.rs` (a line under Update and on the rollout item for `warn`)
- Test: `crates/notchlingd/src/versions_gate_tests.rs`, `crates/engine/src/settings/version_gate_key_tests.rs`, `crates/app/src/sheets/client_gate_tests.rs`

- [ ] **Step 1: Write the failing tests:** `version_gate_is_never`, `off_lets_everything_through`, `warn_shows_a_line`, `block_refuses_update_and_rollout`, `mac_auto_update_is_never_stopped` (no write to any tool config), `rollout_started_records_the_gate`.
- [ ] **Step 2: Run** the three filters. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `versions: versionGate warns or blocks versions a release marked bad`.

### Task 3: The release lists (maintainer)

**Zone:** maintainer only (`tools/release`, `.github/`).

**Files:** `tools/release/worker_manifest.py` (`--tools <json>` adds the lists), `tools/release/tests/test_worker_manifest.py`, the maintainer's own list file per release.
- [ ] **Step 1:** Write the failing test `tools_lists_are_written_sorted`. **Step 2: Run** `tools/test-python.sh`. **Step 3: Implement.** **Step 4: Commit** `release: known-good tool lists in the manifest`.

### Task 4: Docs

**Files:** `crates/AGENTS.md`, `crates/engine/AGENTS.md`, `docs/feature-map.md`, `CHANGELOG.md`; a decision record if the maintainer adopts B as more than a slot.
- [ ] Full checks, then the docs, then commit `docs: known-good versions`.
