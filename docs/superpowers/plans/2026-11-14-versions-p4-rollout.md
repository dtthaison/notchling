# Version sync, Phase 4: Roll Out Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One **Roll Out** click in Machines moves every worker to the Mac's checked version of a tool, one worker at a time: wait until none of that tool's processes run, check the worker's plan is still the one shown, keep the previous binary, install the exact version, verify it, move idle supervised Claude Code sessions. A failure rolls that worker back (best effort, else Needs You) and halts the rollout. Offline workers catch up under the same click. One rollout at a time, by a lock; a stopped rollout resumes only on Resume Rollout. Match the Mac, Roll Back and Switch to the Native Installer are their own clicks.

**Architecture:** A pure state machine `versions::rollout::next(state, event) -> (state, effects)` with per-worker states, plans (`versions::plan`) compared line by line, approval rules (`versions::approval`), an open `Gate` (approach B's slot), a journal and a lock in `notchling.db` (`versions/rollouts`, `versions/lock`, taken in `update_doc`'s IMMEDIATE transaction). Tool adapters (`versions::adapters`) write the exact scripts. `machines::rollout_host::SshHost` performs effects through Machines' `Runner`; the Machines window drives it. A seeded fleet simulation checks the invariants after every step. The daemon raises notices from the journal Machines writes.

**Tech Stack:** Rust (engine, notchlingd, GPUI app), `ScriptedRunner`, `FakeProcs`-style process tables, a fake clock.

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: sections 8, 10 (Going down, First run), 12, 13, 14, 15, 17.2, 17.3, 18 (Who runs the rollout), 19 (rollout kinds), 20, 21, 22 (Rollout engine, Reference, Lock, Tool list and drift, Busy, Fleet simulation, Safety, Settings), 24 (the gate), 23 (Phase 4), user's decision 1, coordinator's calls 1, 2, 3, 4, 7, 8, 9.

**Branch:** `agent/versions-p4-rollout`, off the merge of Phases 2 and 3.

**Depends on:** Phase 2 (usable references, `versions.get`'s reference fields) and Phase 3 (Self-Update Off: Precheck needs the knob set; `remote_edit`). Cluster plan B Task 7's simulator is optional: Task 8 builds a versions-only one and notes the merge (overview, call V8).

## Global Constraints

- Phases 1 to 3's constraints hold. In addition:
- **Never stop a turn, a question or an attached terminal.** No script in this phase holds `kill`, `pkill`, `killall`, `claude stop`, or a `claude respawn` aimed at a session the probe didn't report idle. There is no Install Now.
- **At most one install anywhere**, across tools and windows. **Only the reference version** is ever installed, or the worker's previous one on a rollback. A version marked failed is never installed.
- **A rollout starts only from a usable reference** (same version and SHA-256 as the Mac's installed binary), and **every worker's plan is rebuilt and compared** with the one shown at the click, right before Saving; any difference is Needs You and halts.
- **Automatic doesn't exist in this phase.** `versionRollout` takes `off` and `ask` only. No Inbox answer starts anything; every version item's only action is Open Machines.
- **Every step runs as the worker account,** never `run::admin`; install 600 s, verify 30 s, the others per spec §12.
- **Downloads** only from the hosts in spec §7/§8, `curl --proto =https -fsSL`, pipe pattern `s=$(curl …) && printf '%s\n' "$s" | sh -s -- <args>`; every argument a checked `Version`.
- Match the Mac, Roll Back, Switch to the Native Installer and Self-Update On are never in Set Up All.
- Keys: `versionRollout` (Never), `versionWaitIdleMinutes` (Free, 5..1440, 60), `versionHeld` (Ask). `versionCheckScreens`, `versionCheckTurn` and `versionGate` are not added (overview, call V5).
- `CAP-PROCESS` (worker scripts through the runner), `CAP-NETWORK` (agy's cache, Task 15), `CAP-FILEWRITE` (journal, worker binaries). Engine `machines/` and `versions/` are maintainer zone; `crates/app/**` auto.

## Review Focus

1. Two Machines windows: only one holds the lock; a dead owner's lock is taken over after a minute; nothing resumes without Resume Rollout. (Task 4 `two_owners_cannot_both_hold`, `stale_lock_is_taken_over`, Task 9 `reopened_window_offers_resume_and_runs_nothing`.)
2. A worker whose version moved between the click and its turn: Needs You before anything is installed, earlier workers stay done, later ones untouched. (Task 3 `changed_plan_is_needs_you`, Task 8 seeds with `plan_drift`.)
3. A worker with a plain `claude` in tmux: Waiting for 1 process, forever, until it exits or Cancel; Cancel skips only that worker. (Task 6 `plain_claude_is_busy`, Task 1 `cancel_in_waiting_skips_only_that_worker`.)
4. A failed verify after install: the worker goes back to its previous version through `previous/` or the exact installer; a rollback that fails too is Needs You; the rollout halts either way. (Task 1 `failed_verify_rolls_back_and_halts`, `failed_rollback_is_needs_you`.)
5. The fleet simulation's invariants, after every step of every seed. (Task 8.)

---

### Task 1: The rollout state machine

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/rollout.rs`, `crates/engine/src/versions/rollout_worker.rs`, `crates/engine/src/versions/gate.rs`
- Test: `crates/engine/src/versions/rollout_tests.rs`, `crates/engine/src/versions/rollout_worker_tests.rs`, `crates/engine/src/versions/gate_tests.rs`
- Modify: `crates/engine/src/versions/mod.rs`

**Interfaces:**
- Produces:
  - `gate.rs`: `pub enum GateResult { Open, KnownGood, Unknown, KnownBad { why: String } }`, `pub trait Gate: Send + Sync { fn check(&self, tool: Tool, version: &Version) -> GateResult; }`, `pub struct OpenGate` (always `Open`).
  - `pub enum Phase { Detected, Gated, Checking, CheckFailed, Ready, Parked, AwaitingApproval, Rolling, Halted, Complete, Superseded, Cancelled }`.
  - `pub enum WorkerStep { Pending, Offline, Held, Skipped, Waiting { count: u32, names: Vec<(String, u32)> }, PlanCheck, Saving, Installing, Verifying, Moving, Done, Failed { why: Why }, RollingBack, RolledBack, NeedsYou { why: Why } }`.
  - `pub enum Why { Offline, UnsupportedMethod, DiskFull, DownloadFailed, ChecksumMismatch, VersionMismatch, KnobLost, Timeout, SshFailed, PlanChanged, UnknownState }` (serde `snake_case`, spec §15's list).
  - `pub struct Rollout { pub id: String, pub tool: Tool, pub to: Version, pub sha256: String, pub phase: Phase, pub workers: Vec<WorkerRun>, pub first_run: bool, pub partly: bool, pub approved_at_ms: Option<i64> }`, `pub struct WorkerRun { pub machine: String, pub from: Option<Version>, pub step: WorkerStep, pub shown: Option<WorkerPlan>, pub previous_kept: bool }`.
  - `pub enum Event { MacVersion { version, sha256 }, GateResult(GateResult), CheckFinished(CheckResult), Approved { at_ms }, Declined, WorkerProbed { machine, online: bool, busy: Busy, tools: WorkerTools }, PlanRebuilt { machine, plan: WorkerPlan }, StepFinished { machine, ok: bool, why: Option<Why>, found: Option<Version> }, Tick { now_ms }, Retry, Skip { machine }, RollBack { machine }, Cancel, CancelWorker { machine }, Resume }`.
  - `pub enum Effect { RunCheck, Probe { machine }, RebuildPlan { machine }, RunStep { machine, step: StepKind }, RaiseNotice(Notice), Save, Emit(Fact) }`, `pub enum StepKind { Precheck, Busy, Save, Install, Verify, Move, Restore, VerifyPrevious }`.
  - `pub fn next(state: Rollout, event: Event) -> (Rollout, Vec<Effect>)`: every edge of spec §12's two diagrams; `Save` precedes every `RunStep` in the returned effects (the journal is written before the effect); one worker active at a time; `Superseded` lets the active worker's step finish; a worker in Installing or Verifying on `Resume` is probed and continues at Verifying if it runs the target, retries Installing once if it runs the old version, else `NeedsYou(UnknownState)`.
- [ ] **Step 1: Write the failing tests** (one per edge; the overview's state list is the checklist):
  - `rollout_tests.rs`: `detected_goes_through_open_gate_to_checking`, `failed_check_never_rolls`, `off_parks`, `ask_awaits_approval`, `not_now_parks`, `approved_rolls_workers_in_registry_order`, `held_and_absent_are_skipped`, `complete_with_offline_still_pending`, `superseded_finishes_the_active_step_first`, `cancel_finishes_the_step_in_progress`, `save_precedes_every_step_effect`, `resume_needs_the_event_never_on_open`
  - `rollout_worker_tests.rs`: `waiting_until_idle`, `cancel_in_waiting_skips_only_that_worker`, `changed_plan_is_needs_you_without_rollback`, `failed_install_rolls_back_and_halts`, `failed_verify_rolls_back_and_halts`, `failed_rollback_is_needs_you`, `resume_in_installing_probes_then_continues_or_needs_you`, `retry_after_halt_resumes_at_that_worker`, `skip_after_halt_moves_on`
  - `gate_tests.rs`: `open_gate_is_open`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::rollout` (then `versions::gate`). Expected: FAIL.
- [ ] **Step 3: Implement.** Pure: no clock, no I/O, `BTreeMap`/`Vec` only.
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `versions: the rollout as a pure state machine`.

### Task 2: Approval rules

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/approval.rs`
- Test: `crates/engine/src/versions/approval_tests.rs`

**Interfaces:**
- Produces: `pub enum Setting { Off, Ask }` (Automatic is Phase 6), `pub enum Start { Nothing, Offer, Refuse(&'static str) }`, `pub fn may_start(setting: Setting, reference_usable: bool, version_failed: bool, gate: &GateResult) -> Start` ("The Mac's check is out of date", "This version failed its check", "Notchling isn't running" when the caller couldn't reach the daemon); `pub fn automatic_allowed(first_run: bool, partly: bool, methods: &[Method], tool: Tool) -> bool` (always false while any of: first run, partly, an npm method, agy; exists now so Phase 6 only adds the setting).
- [ ] **Step 1: Write the failing tests:** `stale_reference_starts_nothing`, `failed_version_starts_nothing`, `off_offers_nothing`, `ask_offers`, `partly_never_automatic`, `first_run_never_automatic`, `npm_never_automatic`, `agy_never_automatic`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::approval`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: who may start a rollout`.

### Task 3: Worker plans and the plan check

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/plan.rs`
- Test: `crates/engine/src/versions/plan_tests.rs`

**Interfaces:**
- Consumes: Phase 1 (`WorkerTools`, `Method`), Task 5's adapters (through a trait object passed in, so this task can land first with a fake adapter).
- Produces: `pub struct WorkerPlan { pub machine: String, pub tool: Tool, pub method: Method, pub from: Option<Version>, pub to: Version, pub lines: Vec<String>, pub offline: bool, pub warning: Option<String> }`; `pub fn build(machine: &str, tool: Tool, tools: &WorkerTools, to: &Version, adapter: &dyn ToolAdapter, disk_ok: bool) -> Result<WorkerPlan, Why>`; `pub fn compare(shown: &WorkerPlan, rebuilt: &WorkerPlan) -> Result<(), Vec<String>>` (line by line, plus method, from, to); `fn summary(&self) -> String` ("when back: native installer, 2.1.323 to 2.1.324"); a downward plan carries the `minimumVersion` warning: "A minimumVersion in this worker's settings, or a managed requiredMinimumVersion, can stop Claude Code from running 2.1.323 or make it update itself back up."
- [ ] **Step 1: Write the failing tests:** `plan_lists_the_exact_lines`, `changed_plan_is_needs_you` (version moved, method changed, disk filled: each a difference), `offline_plan_from_last_probe`, `downward_plan_warns`, `cant_pin_method_is_unsupported`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::plan`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: per-worker plans, checked again before each install`.

### Task 4: The journal and the lock

**Zone:** maintainer (`crates/engine`). `CAP-FILEWRITE` (rows in `notchling.db`).

**Files:**
- Create: `crates/engine/src/versions/journal.rs`, `crates/engine/src/versions/lock.rs`
- Test: `crates/engine/src/versions/journal_tests.rs`, `crates/engine/src/versions/lock_tests.rs`

**Interfaces:**
- Produces:
  - `journal.rs`: `pub const DOC: &str = "versions/rollouts"`; `pub fn save(data: &Path, r: &Rollout) -> Result<(), String>` (one map by id, the last 20 finished kept); `pub fn load(data: &Path) -> Vec<Rollout>`; `pub fn stopped(data: &Path) -> Option<Rollout>` (one not Complete, Superseded or Cancelled).
  - `lock.rs`: `pub const DOC: &str = "versions/lock"`; `pub struct Owner { pub pid: u32, pub started_ms: i64, pub heartbeat_ms: i64 }`; `pub trait Alive { fn same_process(&self, pid: u32, started_ms: i64) -> bool; }` (a fake in tests; the real one in Task 7 reads the process table the way `accounts::procs` does); `pub fn take(data: &Path, me: &Owner, now_ms: i64, alive: &dyn Alive) -> Result<Taken, Held>` in one `store::update_doc` (IMMEDIATE): free → `Taken::Fresh`, stale (heartbeat older than 60 s, or `!same_process`) → `Taken::Over`, else `Held { owner }`; `pub fn beat(data, me, now_ms)` (every 10 s), `pub fn release(data, me)`.
- [ ] **Step 1: Write the failing tests** (temp data folders, two `Owner`s):
  - `two_owners_cannot_both_hold`, `stale_heartbeat_is_taken_over`, `dead_pid_is_taken_over`, `live_owner_is_refused`, `release_frees_it`, `beat_keeps_it`
  - `journal_round_trips`, `stopped_finds_the_unfinished_one`, `finished_ones_are_capped`
  - `concurrent_take_from_threads_gives_one_owner` (`#[cfg(unix)]`, 8 threads, one winner)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::lock` (then `versions::journal`). Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: the rollout journal and its lock`.

### Task 5: Tool adapters for Claude Code and Codex

**Zone:** maintainer (`crates/engine`). No CAP (script text only; Task 7 runs them).

**Files:**
- Create: `crates/engine/src/versions/adapters/mod.rs`, `crates/engine/src/versions/adapters/claude.rs`, `crates/engine/src/versions/adapters/codex.rs`
- Test: `crates/engine/src/versions/adapters/claude_tests.rs`, `crates/engine/src/versions/adapters/codex_tests.rs`, `crates/engine/src/versions/adapters/safety_tests.rs`
- Modify: `docs/feature-map.md` (the `versions/adapters/` folder)

**Interfaces:**
- Produces: `pub trait ToolAdapter: Send + Sync { fn tool(&self) -> Tool; fn can_pin(&self, m: Method) -> bool; fn save_previous(&self, m: Method, from: &Version) -> Option<String>; fn install(&self, m: Method, to: &Version) -> Result<String, Why>; fn verify(&self, to: &Version) -> String; fn parse_verify(&self, out: &RemoteOutput, to: &Version) -> Result<(), Why>; fn restore(&self, m: Method, previous: &Version) -> Result<String, Why>; fn moving(&self) -> Option<String>; }`.
  - Claude: native `claude install <v>` (falls back to the installer pipe `bash -s <v>` when `claude install` is missing); npm `npm install -g @anthropic-ai/claude-code@<v>` after `save_previous` copies the package folder to `~/.local/share/notchling/previous/claude/<v>/`; verify `claude --version` plus `readlink ~/.local/bin/claude` ends in `versions/<v>` for native, and the knob still `Set`; restore native `claude install <prev>`, npm moves the saved folder back; `moving` lists `claude agents --json` and runs `claude respawn <id>` only for ids the parse marks idle and on an older version (checked against Task 6's parse at run time, never a fixed list).
  - Codex: standalone the installer pipe with `--release <v>`; npm like Claude's; verify `codex --version`; restore by the same exact install of `prev` or the saved npm folder.
  - Every script's variable parts are a `Version`, a known path or a hash.
- [ ] **Step 1: Write the failing tests:** `native_install_names_the_version`, `npm_saves_previous_first`, `verify_checks_the_link_for_native`, `restore_is_the_exact_previous`, `codex_release_flag`, and in `safety_tests.rs`: `no_script_holds_kill_or_stop` (`kill`, `pkill`, `killall`, `claude stop`, `sudo`), `downloads_are_https_on_allowed_hosts`, `pipe_pattern_never_runs_half_a_script`, `respawn_only_inside_moving`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::adapters`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: exact installs for Claude Code and Codex`.

### Task 6: The busy probe

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/busy.rs`, recorded outputs `crates/engine/tests/fixtures/versions/ps-*.txt`, `claude-agents-*.json` (new files; shapes confirmed on a real worker by the maintainer, Checkpoint 3)
- Test: `crates/engine/src/versions/busy_tests.rs`

**Interfaces:**
- Produces: `pub const BUSY_SCRIPT: &str` (`ps -axo pid=,ppid=,command=`, `claude agents --json 2>/dev/null` and `claude daemon status 2>/dev/null` between nonce markers; the sd worker's `current` is read from the poller, not here); `pub struct Busy { pub counts: BTreeMap<String, u32>, pub supervised: Vec<Supervised> }`, `Supervised { id: String, state: String, version: Option<Version> }`; `pub fn parse(tool: Tool, ps: &str, agents: &str, sd_job: bool) -> Busy`: programs matched by the command's last path part, exactly; for Claude Code the supervisor's process and its descendants are set apart and the rest counted as `claude`; an sd job counts as `sd job`; `fn total(&self) -> u32`, `fn text(&self) -> String` ("Waiting for 3 processes (claude 2, sd job 1)"). Names and counts only; command lines never leave the parse.
- [ ] **Step 1: Write the failing tests:** `plain_claude_is_busy`, `claude_p_is_busy`, `sd_job_is_busy`, `supervised_sessions_are_not_busy`, `codex_any_process_is_busy`, `agy_acp_is_busy`, `similar_names_dont_match` (`claude-helper`, `/usr/bin/codexx`), `parse_keeps_no_command_line`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::busy`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: tell busy workers from idle ones`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib versions::`, clippy, fmt, `guard lint`. Push. The pure core is reviewable before anything runs on a machine.

### Task 7: `SshHost`: effects through Machines' runner

**Zone:** maintainer (`crates/engine/src/machines`). `CAP-PROCESS` (through the existing runner).

**Files:**
- Create: `crates/engine/src/machines/rollout_host.rs`, `crates/engine/src/machines/rollout_driver.rs`
- Test: `crates/engine/src/machines/tests_rollout_host.rs`

**Interfaces:**
- Consumes: Tasks 1 to 6; `Runner`, `run::script`, `Masks`, Phase 1's probe, the poller's `WorkerStatus`.
- Produces:
  - `pub trait RolloutHost: Send + Sync { fn probe(&self, machine: &str) -> Event; fn rebuild(&self, machine: &str, r: &Rollout) -> Event; fn step(&self, machine: &str, r: &Rollout, kind: StepKind) -> Event; }`; `SshHost { engine: Arc<SetupEngine>, adapters: BTreeMap<Tool, Arc<dyn ToolAdapter>>, data: PathBuf, clock }` implements it (timeouts from spec §12; outputs masked and cut to 300 characters into a `last_output` kept on the worker's run).
  - `pub struct Driver { host: Arc<dyn RolloutHost>, data: PathBuf, me: Owner, alive: Arc<dyn Alive>, recorder: Arc<dyn Recorder> }`: `fn start(&self, r: Rollout) -> Result<(), Held>` takes the lock, then loops `next` and performs effects (`Save` → `journal::save`; `Emit` → recorder; notices are left to the daemon, Task 11), beating every 10 s; `fn stop_after_step(&self)` (window closing); releases the lock when the rollout ends.
- [ ] **Step 1: Write the failing tests** (World + ScriptedRunner answering each script by needle):
  - `one_worker_goes_from_pending_to_done`, `offline_worker_is_skipped_and_pending`, `busy_worker_waits_and_ticks_every_minute`
  - `outputs_are_masked_and_cut`, `install_timeout_is_600s_and_verify_30s`
  - `closing_stops_after_the_step_in_progress`, `lock_held_elsewhere_refuses_start`
  - `no_step_uses_admin` (no recorded call is a sudo/root call; `is_sudo` from the World)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_rollout_host`. Expected: FAIL.
- [ ] **Step 3: Implement.** No lock held across a runner call.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `machines: run a rollout's steps over ssh`.

### Task 8: The fleet simulation

**Zone:** maintainer (`crates/engine`). No CAP (test code).

**Files:**
- Create: `crates/engine/src/versions/sim.rs` (`#[cfg(test)]`), `crates/engine/src/versions/sim_tests.rs`

**Interfaces:**
- Produces: `pub struct Fleet` of 1 to 6 fake workers, each with an installed version, method, online flag, busy count, a scripted failure (`install`, `verify`, `rollback`) and flapping; a `SimHost: RolloutHost` that answers from the fleet and changes it on install and restore; a seeded RNG (an LCG in the file; no new crate); `fn run(seed: u64, steps: usize) -> Report`.
- Invariants checked after every step (spec §22): at most one worker in Saving, Installing, Verifying or RollingBack at once; every installed version is the target or that worker's previous; a failure halts and leaves unreached workers' versions unchanged; a failed worker ends on its previous version or NeedsYou; no step effect is issued without a `Save` before it; a version marked failed is never installed; no recorded script holds `kill`, `pkill`, `claude stop`, or `respawn` of a session the probe called working; every remote write follows a backup step.
- [ ] **Step 1: Write the failing tests:** `seeds_0_to_499_hold_every_invariant`, `plan_drift_seed_is_needs_you`, `flapping_worker_catches_up_under_the_same_click`, `superseded_mid_install_finishes_the_install`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.** If cluster B Task 7's `World` is merged, keep this file standalone anyway and note in `crates/engine/AGENTS.md` that the two may merge (overview, call V8).
- [ ] **Step 4: Run** (under 30 s in debug). Expected: PASS.
- [ ] **Step 5: Commit** `versions: a seeded fleet simulation over every rollout invariant`.

### Task 9: Machines' plan kinds: Roll Out, Match the Mac, Roll Back, Resume

**Zone:** maintainer (`crates/engine/src/machines`). No new CAP.

**Files:**
- Modify: `crates/engine/src/machines/engine_plan.rs` (`PlanKind::RollOut(Tool)`, `MatchTheMac(Tool)`, `RollBack(Tool)`, `ResumeRollout`, `CancelRollout`), `crates/engine/src/machines/engine.rs` (hold a `Driver`; on open, `journal::stopped` gives the Resume prompt and nothing runs)
- Create: `crates/engine/src/machines/versions_actions.rs`
- Test: `crates/engine/src/machines/tests_versions_actions.rs`, `crates/engine/src/machines/tests_versions_safety.rs`

**Interfaces:**
- Produces: `fn roll_out_sheet(&self, tool: Tool, mac: &MacSide) -> Result<Sheet, String>` (one `WorkerPlan` per worker that has the tool and isn't held; Partly Checked and the unchecked list; the soak's line), confirming it gives `Approved` for every worker on it (coordinator's call 2); Match the Mac is a one-worker rollout (its own plan, downward with the warning); Roll Back to `<previous>` is shown only when the previous is known and kept, runs the adapter's restore and marks that worker Held for the tool (`versionHeld`); Resume checks the reference is still usable first.
- [ ] **Step 1: Write the failing tests:** `one_click_approves_every_worker_on_the_sheet`, `stale_reference_has_no_sheet`, `reopened_window_offers_resume_and_runs_nothing`, `resume_rechecks_the_reference`, `roll_back_holds_the_worker`, `match_the_mac_down_warns`, and in `tests_versions_safety.rs`: `rollout_kinds_are_not_in_set_up_all`, `refresh_runs_no_rollout_step`, `no_rollout_step_without_its_confirm`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_versions_actions` (then `tests_versions_safety`). Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run** `cargo test -p notchling-engine --lib machines::`. Expected: PASS.
- [ ] **Step 5: Commit** `machines: Roll Out, Match the Mac, Roll Back and Resume Rollout`.

### Task 10: Settings keys

**Zone:** maintainer (`crates/engine/src/settings`). No CAP.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`versionRollout` choice `off`/`ask`, default `off`, `.never(..)`; `versionWaitIdleMinutes` ranged 5..1440, default 60, `.free(..)`; `versionHeld` string list (`machine:tool`), default empty, `.ask(..)`; titles, abouts and whys from spec §20)
- Test: `crates/engine/src/settings/versions_keys_tests.rs`, `crates/notchlingd/src/settings_versions_tests.rs`

- [ ] **Step 1: Write the failing tests:** `version_rollout_is_never`, `wait_minutes_is_free_and_ranged`, `held_is_ask`, `agent_settings_set_on_version_rollout_is_refused_and_audited` (through `settings_ops`), `off_to_ask_is_the_first_run` (the app side marks the first rollout `first_run`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib settings::versions_keys` and `cargo test -p notchling-notchlingd --lib settings_versions`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `settings: the rollout's keys, with Never for the one that installs software`.

### Task 11: The daemon's notices from the journal

**Zone:** maintainer (`notchlingd`, `crates/engine/src/inbox`). No CAP.

**Files:**
- Create: `crates/notchlingd/src/versions_notices.rs`
- Test: `crates/notchlingd/src/versions_notices_tests.rs`
- Modify: `crates/engine/src/inbox/model.rs` (`Kind::VersionRollout`, `VersionRolloutHalted`, `VersionBusy`), `crates/engine/src/versions/record.rs` (`Fact::RolloutStarted`, `RolloutStep`, `RolloutLock`, `RolloutFinished`, `RolledBack`, fields from spec §19), `crates/notchlingd/src/versions_ops.rs` (every 10 s and on a rescan, read `versions/rollouts` and `versions/workers`), `crates/app/src/inbox/model.rs`

**Interfaces:**
- Produces: notices, each with a fresh id `rollout:<random>` and only `Open "Open Machines"`:
  - `version_rollout` "Claude Code 2.1.324 Passed on the Mac" / "3 workers are behind." when a usable reference exists, `versionRollout` is `ask` and workers are behind; "Workers are behind the Mac: open Machines" when the window is closed.
  - `version_rollout_halted` "Claude Code 2.1.324 Stopped at m1" with spec §15's detail.
  - `version_busy` "m1 Is Still Waiting" after `versionWaitIdleMinutes`.
  - Facts the journal holds (`rollout.*`) go to the recorder once each (kept by journal sequence in `versions/recorded`).
- [ ] **Step 1: Write the failing tests:** `behind_workers_raise_one_offer`, `halt_raises_one_item_with_the_story`, `long_wait_raises_busy_once`, `every_item_only_opens_machines`, `facts_recorded_once`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_notices`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: rollout notices from what Machines records`.

### Checkpoint 2

- [ ] `cargo test -p notchling-engine --lib versions::`, `cargo test -p notchling-engine --lib machines::`, `cargo test -p notchling-notchlingd --lib versions_`, clippy, fmt, `guard lint`. Push. Everything but the views and agy is in.

### Task 12: Machines' views: the sheet, the banner, Waiting, Resume

**Zone:** auto (`crates/app/src/machines`). No CAP.

**Files:**
- Create: `crates/app/src/machines/rollout_model.rs`, `crates/app/src/machines/rollout_view.rs`
- Test: `crates/app/src/machines/rollout_model_tests.rs`
- Modify: `crates/app/src/machines/model.rs`, `cards.rs` (per tool: Match the Mac, Roll Back to `<previous>`, Held), `chrome_view.rs` (the banner), `sheets_view.rs` (the Roll Out sheet), `window.rs` (closing asks the driver to stop after the step), `renders.rs` (`machines-rollout-sheet`, `machines-rollout-banner`, `machines-rollout-waiting`, `machines-rollout-resume`, `machines-rollout-halted`)

**Interfaces:**
- Produces: `RolloutBanner { text: String, actions: Vec<(&'static str, Action)> }` ("Rolling out Claude Code 2.1.324: m1 done · vps-sing-1 waiting for 2 processes · ub-2 offline"; halted: Retry, Skip m1, Roll Back; another window: "A rollout is running in another Machines window"); `SheetModel { title, partly_line, unchecked, soak_line, plans: Vec<(String, Vec<String>)> }`; parts `"Rollout Banner"`, `"Roll Out Sheet"`, `"Resume Rollout"`.
- [ ] **Step 1: Write the failing tests:** `banner_lists_each_worker`, `halted_banner_offers_retry_skip_roll_back`, `waiting_shows_counts_and_cancel`, `sheet_says_partly_checked_and_whats_not`, `resume_prompt_names_where_it_stopped`, `other_window_holds_the_lock`, `running_versions_shown_until_they_agree`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib machines::rollout_model`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run;** render the five states (light and dark) and look.
- [ ] **Step 5: Commit** `machines: the Roll Out sheet, the banner and Resume Rollout`.

### Task 13: Settings > Agents: the rollout setting and first run

**Zone:** auto (`crates/app/src/sheets`). No CAP.

**Files:**
- Modify: `crates/app/src/sheets/client_model.rs`, `client_view.rs` ("Keep workers on the Mac's versions" with Off and Ask Me through the kit's menu; Automatic shown off with "Needs a full check"), `settings.rs` (Off → Ask asks the daemon for a check of each tool, then the offer appears in Machines: first run)
- Test: `crates/app/src/sheets/client_rollout_tests.rs`

- [ ] **Step 1: Write the failing tests:** `picker_offers_off_and_ask`, `automatic_is_shown_off_with_needs_a_full_check`, `off_to_ask_checks_each_tool_first`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib sheets::client_rollout`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run;** render. **Step 5: Commit** `settings: keep workers on the Mac's versions`.

### Task 14: agy, latest then verify

**Zone:** maintainer (`crates/engine`). No new CAP beyond Task 7's.

**Files:**
- Create: `crates/engine/src/versions/adapters/agy.rs`
- Test: `crates/engine/src/versions/adapters/agy_tests.rs`

**Interfaces:**
- Produces: `Agy: ToolAdapter`: `can_pin` false; `install` runs `agy update` (a rollout never installs agy where it is missing, coordinator's call 4; the installer is used only by Task 16's switch) then the step passes only if `agy --version` equals the Mac's; if not, `Err(VersionMismatch)` with the plan's line "agy <latest> is out; update the Mac first"; `save_previous` copies `~/.local/bin/agy` to `previous/agy/<v>/`; `restore` moves it back.
- [ ] **Step 1: Write the failing tests:** `latest_equal_to_mac_passes`, `newer_latest_fails_and_says_update_the_mac`, `previous_binary_saved_and_restored`, `agy_is_never_automatic` (through `approval`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::adapters::agy`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run.** **Step 5: Commit** `versions: agy follows the Mac while the Mac has agy's latest`.

### Task 15: agy's cache on the Mac

**Zone:** maintainer. `CAP-NETWORK` (agy's manifest and binaries), `CAP-FILEWRITE` (`<data>/versions/cache/`).

**Files:**
- Create: `crates/engine/src/versions/agy_cache.rs`, `crates/notchlingd/src/versions_agy_cache.rs`
- Test: `crates/engine/src/versions/agy_cache_tests.rs`, `crates/notchlingd/src/versions_agy_cache_tests.rs`
- Modify: `crates/engine/src/versions/adapters/agy.rs` (when the cache holds the Mac's version for the worker's platform, the install sends it over stdin with its SHA-512; the worker checks the hash, then renames it into place), `docs/policies/privacy.md`

**Interfaces:**
- Produces: `parse_manifest(body: &str) -> Option<Manifest { version, platforms: BTreeMap<String, (url, sha512)> }>`; the daemon, when the Mac's agy changes, fetches the manifest for each platform the workers use (from `versions/workers`), downloads where it still names the Mac's version, checks SHA-512, keeps two versions under `<data>/versions/cache/agy/<v>/<platform>/`; a same-architecture Mac worker can get a copy of the Mac's own binary.
- [ ] **Step 1: Write the failing tests:** `manifest_parses`, `wrong_sha_is_refused_and_deleted`, `keeps_two_versions`, `install_sends_bytes_on_stdin_with_hash`, `worker_script_checks_hash_before_rename`, `downloads_only_from_the_manifest_host_over_https`.
- [ ] **Step 2: Run** the two test filters. Expected: FAIL.
- [ ] **Step 3: Implement** (fetch through the daemon's `Launcher` and `curl`, like Latest). **Step 4: Run.** **Step 5: Commit** `versions: keep agy's binaries the Mac checked, for workers`.

### Task 16: Homebrew to the vendor's installer, and the Claude Code step's version

**Zone:** maintainer (`crates/engine/src/machines`). `CAP-PROCESS`; `WEAK-TEST-EDIT` if `CLAUDE_INSTALL`'s tests change.

**Files:**
- Create: `crates/engine/src/machines/steps_switch_installer.rs`
- Test: `crates/engine/src/machines/tests_switch_installer.rs`
- Modify: `crates/engine/src/machines/steps_machine.rs` (`claude_code_fix` passes the usable reference: `bash -s <v>`; with none it installs the latest as today)

**Interfaces:**
- Produces: a Fix on a Can't Pin tool row, "Switch to the Native Installer" (Codex and agy: "Switch to the Standalone Installer"): its own click and plan; runs only while the busy probe is empty; installs the reference with the vendor installer; `brew uninstall --cask <cask>`; checks `command -v` resolves to the vendor's path and `--version` equals the reference (user's decision 1).
- [ ] **Step 1: Write the failing tests:** `switch_is_its_own_click`, `switch_waits_for_no_process`, `switch_installs_reference_then_removes_cask`, `switch_verifies_path_and_version`, `new_worker_gets_the_macs_version` (`claude_code_fix`'s script ends `bash -s 2.1.324` with a usable reference).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_switch_installer`. Expected: FAIL.
- [ ] **Step 3: Implement.** **Step 4: Run** `cargo test -p notchling-engine --lib machines::`. **Step 5: Commit** `machines: switch a Homebrew tool to its vendor's installer`.

### Task 17: Docs and checks

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/AGENTS.md` (Versions: rollouts run in the Machines window until Phase 6; the lock; resume; never kill; the fleet simulation), `crates/engine/AGENTS.md`, `crates/app/AGENTS.md`, `docs/feature-map.md`, `docs/local-api-ops.md`, `CHANGELOG.md`
- [ ] **Step 1: Run** fmt, clippy (on the Mac), `cargo test --workspace --locked`, `guard lint`, `featuremap`, `rules`, `strip_comments --check`. Expected: clean.
- [ ] **Step 2: Write** the docs. **Step 3: Commit** `docs: tool versions, phase 4`. The PR lists every `CAP-*` line and `WEAK-TEST-EDIT`.

### Checkpoint 3 (maintainer, real workers)

On two test workers with `--live`: the Roll Out sheet shows each plan; one click moves both, one at a time; a plain `claude` left running on one keeps it in Waiting until it exits; closing the window mid-rollout and reopening shows Resume Rollout. The maintainer records in the ledger, from those workers: `ps` and `claude agents --json` outputs (redacted to names and states) for Task 6's fixtures, whether a manual `claude install` restarts the background supervisor, whether Codex's standalone packages keep old versions, and whether `agy update` exists and installs the manifest's latest (spec §25, §26). Until `agy update` is confirmed, Task 14's adapter is merged but agy rows show "Not managed yet" and offer no Roll Out (a const `AGY_UPDATE_VERIFIED`, false).
