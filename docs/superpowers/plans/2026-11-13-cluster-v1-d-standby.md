# Cluster v1, plan D: Standby Snapshots and the Planned Move Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Standby always holds a consistent, recent copy of everything needed to carry on the home's main sessions (their repositories with uncommitted work, their transcripts, the cluster's database, phone pairing), taken at turn ends and every 5 minutes; and the maintainer can move the home to a Standby on purpose (closing the laptop for the night) with nothing lost: the old home stops, the new one resumes every main session at the same paths, the phone and the app follow.

**Architecture:** A pure snapshot planner in `engine::cluster::snapshot` decides when and what (quiet points, a `ResumeSet` per main session, exclusions); runners take consistent copies without touching the user's live repositories or databases (git through a shadow repository with a private index, SQLite through its backup API, transcripts copied whole only when quiet). Sets go to each Standby over plan B's blob transfer and become current only when complete. A restore planner lays a set out at identical paths. The planned move is a saga in `engine::cluster::move_home` driven by `NodeCore`, with compensation up to the switch of terms. Inbox state moves from `notchling.db` into `cluster.db`, so items and their ids survive the move.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app), git (as a child process, hardened as in plan B Task 14), rusqlite's backup API, the plan B node link and blobs, `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (Replication, Must exist first: the resume proof and the db split), "Main sessions and the Standby", "Planned handoff", review issues 6 (live `.git` and SQLite), 7 (paths), 8 (encryption), invariant I4, data ownership. Plans B and C merged first (C's `EXCLUDED_DIRS` are refused here).

## Global Constraints

- Plan B's constraints hold.
- Never write the user's repository: no `git add`, `stash`, `commit`, index or ref change in it. Snapshots use a shadow repository under `<data>/cluster/shadow/` with `GIT_INDEX_FILE` private and `objects/info/alternates` pointing at the user's objects (read only).
- Never copy a live SQLite file: databases go through the backup API. Never copy a transcript while its session is working; a working session's transcript waits for the next quiet point (or the timer's next quiet point).
- Paths: a set restores only at the same absolute paths and under the same home folder. A Standby whose home path differs refuses to be a Standby (Task 11). No path rewriting in v1.
- Exclusions: `EXCLUDED_DIRS` (plan C), `.env*` files unless the maintainer picks them for that repository, anything over 100 MB, `node_modules`, `target`, `.venv`, `build`, `dist` unless tracked.
- Notchling doesn't encrypt sets in v1; a Standby must have disk encryption on (FileVault, LUKS), checked in Task 11. Sets are 0600 in 0700 folders.
- A set is current on a Standby only when every part arrived and its hashes match (`.partial` → rename). A Standby never holds a set ahead of what the home acknowledged (I4).
- The move is live-gated by `RESUME_PROOF_RECORDED` (a code const, `false` until the maintainer's proof M5 in the overview). Tests drive it with the flag passed in.
- Main sessions in v1: Claude Code and Codex sessions the maintainer marks as main. Antigravity is not.

## Review Focus

1. A snapshot taken while git runs or the session works: never a torn copy. (Task 3 `git_lock_blocks_quiet`, `working_session_waits`; Task 4 `snapshot_never_changes_user_repo`.)
2. A Standby that crashes mid-transfer: its current set stays the last complete one. (Task 6 `partial_set_never_becomes_current`, Task 7 simulator I4.)
3. A move that fails before the switch: the old home is back as it was, sessions running. (Task 9 `failure_before_switch_compensates`.)
4. A restore over a folder with the Standby's own changes: refused, nothing overwritten. (Task 8 `restore_refuses_a_folder_with_its_own_changes`.)
5. Inbox items and their ids after a move: the same ids, answerable on the new home. (Task 1 `inbox_moves_to_cluster_db_once`, Task 9 `inbox_survives_move`.)

---

### Task 1: The inbox moves into `cluster.db`

**Zone:** maintainer. CAP-FILEWRITE.

**Files:**
- Modify: `crates/engine/src/inbox/persist.rs` (open the inbox table in `cluster.db` when the node is in a cluster, else `notchling.db` as today), `crates/engine/src/cluster/persist.rs` (an `inbox` table with the same columns), `crates/engine/src/store_class.rs`, `crates/notchlingd/src/inbox_ops.rs` (`InboxOps::load` takes the store to use)
- Test: `crates/engine/src/inbox/tests_persist.rs` (new cases), `crates/engine/src/cluster/persist_tests.rs`

**Interfaces:**
- Produces: `pub fn migrate_inbox(notchling_db: &Path, cluster_db: &Path) -> Result<usize, String>` (copies every row in one transaction into `cluster.db`, then deletes them from `notchling.db`; idempotent: a second run copies nothing; ids and rowid order kept); called once when a node joins or creates a cluster, and at start when both hold rows.
- [ ] **Step 1: Write the failing tests:** `inbox_moves_to_cluster_db_once`, `migration_keeps_ids_and_order`, `crash_between_copy_and_delete_is_safe` (rows in both: the second run dedupes by id), `node_without_cluster_keeps_notchling_db`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib inbox::tests_persist` and `cluster::persist`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both and `cargo test -p notchling-notchlingd --lib inbox_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: the inbox lives in cluster.db`.

### Task 2: Main sessions

**Zone:** maintainer for engine and daemon; auto for the app toggle (Task 12). No CAP.

**Files:**
- Create: `crates/engine/src/cluster/main_sessions.rs`
- Test: `crates/engine/src/cluster/main_sessions_tests.rs`
- Modify: `crates/engine/src/cluster/state.rs` (`ClusterState.main: BTreeMap<String, MainSession>`, `Command::SetMain { session, main: bool }`), `crates/local-api/src/wire.rs` (`CLUSTER_MAIN_SET = "cluster.main.set"`), `crates/notchlingd/src/cluster_ops.rs`

**Interfaces:**
- Produces:
  - `pub struct MainSession { pub id: String, pub tool: Tool, pub cwd: String, pub config_dir: Option<String>, pub transcript: Option<String>, pub respawn: Vec<String>, pub since_ms: i64 }`
  - `pub fn may_be_main(agent: &Agent) -> Result<(), String>` (Claude Code or Codex, on the home, with a folder; else `"Only Claude Code and Codex sessions on the home can be main"`)
  - `cluster.main.set {id, main}` (`OpSpec::APP`, forwarded to the home)
  - a session the phone starts on the home is marked main at start (the bridge's `session.new` path)
- [ ] **Step 1: Write the failing tests:** `claude_and_codex_on_home_may_be_main`, `agy_may_not`, `worker_session_may_not`, `set_main_is_journaled`, `ended_session_leaves_main_after_a_day`, `phone_started_session_is_main`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::main_sessions`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: main sessions`.

### Task 3: The snapshot planner

**Zone:** maintainer. No CAP.

**Files:**
- Create: `crates/engine/src/cluster/snapshot/mod.rs`, `snapshot/plan.rs`, `snapshot/resume_set.rs`, `snapshot/exclude.rs`
- Test: `crates/engine/src/cluster/snapshot/plan_tests.rs`, `snapshot/resume_set_tests.rs`, `snapshot/exclude_tests.rs`
- Modify: `docs/feature-map.md`

**Interfaces:**
- Produces:
  - consts `TIMER_MS = 300_000`, `QUIET_MS = 10_000`, `MAX_FILE = 100 * 1024 * 1024`
  - `pub struct Observed { pub now_ms: i64, pub session_state: BTreeMap<String, SessionState>, pub git_locks: BTreeMap<String, bool>, pub last_write_ms: BTreeMap<String, i64>, pub last_set_ms: Option<i64> }`, `pub enum SessionState { Working, Idle, Ended }`
  - `pub enum Trigger { TurnEnd(String), Timer, Final }`
  - `pub fn due(prev: &Observed, now: &Observed) -> Option<Trigger>` (a main session moving Working → Idle; else the timer; never while a git lock exists in a main session's repository or any main transcript was written in the last `QUIET_MS`; `Final` is asked by the move)
  - `pub struct ResumeSet { pub session: String, pub tool: Tool, pub repo: Option<PathBuf>, pub files: Vec<PathBuf>, pub dirs: Vec<PathBuf> }`; `pub fn resume_set(m: &MainSession, home: &Path) -> ResumeSet`: Claude Code: the transcript, `<projects>/<slug>/<session>/` (subagents, tool results), `file-history/<session>/`, `todos/<session>-*.json`, the transcripts a `continued-in` chain reaches (`transcript::continued::earlier`); Codex: the rollout file and `session_index.jsonl` (its line only). The exact list is confirmed by M5; the planner holds it in one table.
  - `pub fn excluded(path: &Path, picked: &BTreeSet<PathBuf>) -> bool`
  - `pub struct SetPlan { pub id: String, pub trigger: Trigger, pub repos: Vec<PathBuf>, pub files: Vec<PathBuf>, pub dbs: Vec<PathBuf> }` (`dbs`: `cluster.db`; files: the resume sets, `<data>/pages/`, the bridge's `devices.json`); `pub fn plan_set(main: &[MainSession], home: &Path, data: &Path, seq: u64) -> SetPlan` (id `s-<seq>`)
- [ ] **Step 1: Write the failing tests:** `turn_end_triggers`, `timer_triggers_after_five_minutes`, `git_lock_blocks_quiet`, `working_session_waits`, `recent_write_waits_ten_seconds`, `resume_set_for_claude_lists_its_files`, `resume_set_follows_continued_in`, `resume_set_for_codex`, `grants_and_logins_are_excluded`, `env_files_only_when_picked`, `big_files_are_excluded`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::snapshot`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: when and what a snapshot takes`.

### Task 4: Git snapshots through a shadow repository

**Zone:** maintainer. CAP-PROCESS (git), CAP-FILEWRITE.

**Files:**
- Create: `crates/engine/src/cluster/snapshot/git_snap.rs`, `crates/notchlingd/src/snap_git.rs`
- Test: `crates/engine/src/cluster/snapshot/git_snap_tests.rs`, `crates/notchlingd/src/snap_git_tests.rs` (`#[cfg(unix)]`, real git in temp folders)

**Interfaces:**
- Produces:
  - `pub fn shadow_dir(data: &Path, repo: &Path) -> PathBuf` (`<data>/cluster/shadow/<sha256(repo path)[..16]>.git`)
  - `pub fn plan_snapshot(repo: &Path, shadow: &Path, set: &str, prev: Option<&str>) -> Vec<GitCall>`: `git init --bare` the shadow once and write its `objects/info/alternates`; with `GIT_DIR=<repo>/.git`, `GIT_WORK_TREE=<repo>`, `GIT_INDEX_FILE=<shadow>/index`, `GIT_OBJECT_DIRECTORY=<shadow>/objects`, `GIT_ALTERNATE_OBJECT_DIRECTORIES=<repo>/.git/objects`: `read-tree HEAD`, `add -A` (respecting `.gitignore` and the exclusions as pathspecs), `write-tree`, `commit-tree <tree> -p HEAD -m "notchling snapshot <set>"`; then `update-ref` in the shadow only (`refs/notchling/snap/<set>`, `refs/notchling/head` = the user's HEAD and its branch name); every call hardened (plan B Task 14)
  - `pub fn plan_bundle(shadow: &Path, set: &str, have: Option<&str>) -> GitCall` (a bundle of the snapshot commit and the user's HEAD, thin against the commit the Standby acknowledged last)
  - `pub struct GitSnap { pub repo: PathBuf, pub head: String, pub branch: Option<String>, pub snap: String, pub bundle: PathBuf }`
- [ ] **Step 1: Write the failing tests** (real git):
  - `snapshot_never_changes_user_repo` (index file, refs, `git status --porcelain`, stash list and every file's mtime under `.git` the same before and after)
  - `snapshot_holds_unstaged_and_untracked`, `ignored_files_stay_out`, `deleted_tracked_file_is_absent_in_snapshot`
  - `bundle_is_thin_against_acked`, `hooks_and_fsmonitor_never_run`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::snapshot::git_snap` and `cargo test -p notchling-notchlingd --lib snap_git`. Expected: FAIL.
- [ ] **Step 3: Implement.** Git objects written by `add` go to the shadow's object folder only (`GIT_OBJECT_DIRECTORY`).
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: git snapshots that never write the user's repository`.

### Task 5: Databases and files

**Zone:** maintainer. CAP-FILEWRITE.

**Files:**
- Create: `crates/notchlingd/src/snap_files.rs`
- Test: `crates/notchlingd/src/snap_files_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn backup_db(src: &Path, dst: &Path) -> Result<(), String>` (rusqlite `backup::Backup`, 256 pages a step, 5 ms between steps, the destination 0600)
  - `pub fn copy_quiet(src: &Path, dst: &Path, quiet_ms: i64, now_ms: i64) -> Result<Copied, String>` (`Copied::Done { len, sha256 }` or `Copied::Busy` when the file changed during the copy or within `quiet_ms`: compare size and mtime before and after)
  - `pub fn stage_set(plan: &SetPlan, git: &[GitSnap], stage: &Path) -> Result<Manifest, String>` (everything under `<data>/cluster/outgoing/<set>/`, then `Manifest { set, seq, term, home, created_ms, parts: Vec<Part { kind: Repo|Db|File|Dir, path, sha256, len }> }`)
- [ ] **Step 1: Write the failing tests:** `backup_of_a_db_being_written_is_consistent` (a writer thread inserts while backing up; the copy opens and passes `PRAGMA integrity_check`), `busy_file_is_not_copied`, `manifest_lists_every_part_with_hash`, `stage_is_0600_in_0700`, `excluded_paths_never_staged`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib snap_files`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: consistent copies of databases and quiet files`.

### Task 6: Sending sets; the Standby's store

**Zone:** maintainer. CAP-FILEWRITE, CAP-NETWORK.

**Files:**
- Create: `crates/engine/src/cluster/standby_store.rs`, `crates/notchlingd/src/snap_send.rs`
- Test: `crates/engine/src/cluster/standby_store_tests.rs`, `crates/notchlingd/src/snap_send_tests.rs`
- Modify: `crates/engine/src/cluster/policy.rs` (`snap.offer`, `snap.part`, `snap.commit` from `Home` origin `Node` to a Standby; `snap.ack` back), `crates/local-api/src/wire.rs`

**Interfaces:**
- Produces:
  - `StandbyStore` at `<data>/cluster/standby/<home id>/`: `fn begin(&self, m: &Manifest) -> Result<(), String>` (`sets/<set>.partial/`), `fn part(&self, set: &str, part: &Part, from: &Path) -> Result<(), String>` (hash checked), `fn commit(&self, set: &str) -> Result<(), String>` (every part present and hashed → rename to `sets/<set>`, write `current.json` atomically), `fn current(&self) -> Option<Manifest>`, `fn prune(&self, keep: usize)` (keeps the current set and one before), `fn missing(&self, m: &Manifest) -> Vec<Part>` (resume after a drop)
  - repositories on the Standby: each bundle fetched into a bare mirror `standby/<home>/repos/<sha16>.git` (`refs/notchling/snap/<set>`, `refs/notchling/head`)
  - `snap.ack {set}` after `commit`; the home records the Standby's acknowledged set (for thin bundles and the move)
  - the sender runs on its own thread, one set at a time, newest wins (a set staged while one is sending replaces the queued one, never the one in flight)
- [ ] **Step 1: Write the failing tests:** `complete_set_becomes_current`, `partial_set_never_becomes_current`, `bad_hash_part_is_refused`, `resume_sends_only_missing_parts`, `prune_keeps_two`, `newest_queued_set_wins`, `ack_records_standby_set`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::standby_store` and `cargo test -p notchling-notchlingd --lib snap_send`. Expected: FAIL.
- [ ] **Step 3: Implement** over plan B's `node.put` chunks.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: send snapshot sets to Standbys`.

### Task 7: Snapshots in the core and the simulator

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/engine/src/cluster/core/mod.rs` (`Input::Observed`, `Effect::Snapshot { plan }`, acks), `crates/engine/src/cluster/sim/world.rs`, `sim/host.rs` (fake snapshots: a set is a version number of each main session's fake state; transfers take simulated time and can be cut), `sim/invariants.rs`, `crates/notchlingd/src/cluster_node.rs` (feeds `Observed` from the session monitor and lock files every 2 s, runs `Snapshot` effects on a worker)
- Test: `crates/engine/src/cluster/sim/sim_tests.rs`

**Interfaces:**
- Produces `Violation::{StandbyAhead { node, set }, TornSet { node, set }}`: I4 (a Standby's current set is never one the home didn't stage, never newer than the home's newest staged set, and every current set is complete).
- [ ] **Step 1: Write the failing tests:** `standby_never_ahead_over_seeds`, `crash_during_transfer_keeps_last_complete_set`, `a_planted_torn_commit_is_caught`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** with `NOTCHLING_SIM_SEEDS=2000`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: snapshots in the node core, checked by the simulator`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib cluster::`, `cargo test -p notchling-notchlingd`: PASS. fmt, clippy, guard: clean.
- [ ] Controller runs `snapshot_never_changes_user_repo` and reads its before/after listing.

### Task 8: Restore at identical paths

**Zone:** maintainer. CAP-PROCESS (git), CAP-FILEWRITE.

**Files:**
- Create: `crates/engine/src/cluster/restore.rs`, `crates/notchlingd/src/snap_restore.rs`
- Test: `crates/engine/src/cluster/restore_tests.rs`, `crates/notchlingd/src/snap_restore_tests.rs` (`#[cfg(unix)]`, real git)

**Interfaces:**
- Produces:
  - `pub enum RestoreStep { CloneRepo { mirror, path, branch, head }, ApplyTree { path, head, snap }, PutFile { from, to }, PutDir { from, to }, RestoreDb { from, to } }`; `pub fn plan_restore(m: &Manifest, store: &Path, home: &Path, probe: &dyn Fn(&Path) -> PathState) -> Result<Vec<RestoreStep>, String>`; `pub enum PathState { Missing, OurCopy { set: String }, Clean { head: String }, Changed }`
  - a repository folder that is missing is cloned from the mirror at the user's branch and HEAD; one that is `OurCopy` (it has `.git/notchling-standby` naming an earlier set) or `Clean` at an ancestor of the snapshot's HEAD is updated; one that is `Changed` refuses `"<path> on <node> has its own changes"`
  - `ApplyTree`: the branch at the user's HEAD, then the snapshot's tree laid into the working tree only (`git restore --source=<snap> --worktree -- :/`, then files tracked at HEAD and absent in the snapshot deleted); the index stays at HEAD, so every change is unstaged and new files untracked, as they were
  - transcripts and session folders are written only where no file exists or the existing one is a prefix of the set's (a longer local transcript refuses: the session ran here after the set)
  - `RestoreDb` for `cluster.db` only through the backup API into a temp file, then a rename, with the daemon's own connection closed first
- [ ] **Step 1: Write the failing tests:** `restore_into_missing_folder_matches_the_snapshot` (the working tree, `git status --porcelain` and the branch equal the home's), `restore_refuses_a_folder_with_its_own_changes`, `restore_updates_our_copy`, `longer_local_transcript_refuses`, `db_restore_is_atomic`, `restore_steps_never_touch_excluded_dirs`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::restore` and `cargo test -p notchling-notchlingd --lib snap_restore`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: restore a set at the same paths`.

### Task 9: The planned move of the home

**Zone:** maintainer. CAP-PROCESS (`claude stop`, `claude --bg --resume` through `control::run`).

**Files:**
- Create: `crates/engine/src/cluster/move_home.rs`, `crates/notchlingd/src/cluster_move.rs`
- Test: `crates/engine/src/cluster/move_home_tests.rs`, `crates/notchlingd/src/cluster_move_tests.rs`
- Modify: `crates/engine/src/cluster/core/control.rs`, `crates/engine/src/cluster/versions.rs` (`pub const RESUME_PROOF_RECORDED: bool = false`), `crates/local-api/src/wire.rs` (`CLUSTER_MOVE_HOME = "cluster.move_home"`, node-only `MOVE_FREEZE`, `MOVE_STOP`, `MOVE_RESUME`, `MOVE_UNDO`), `crates/engine/src/cluster/policy.rs`, `crates/engine/src/cluster/sim/` (scenario op `MoveHome`)

**Interfaces:**
- Produces:
  - `pub enum MoveStep { Prepare, Freeze, FinalSnapshot, StopOld, Transfer, Switch, Resume, Announce, Done, Undo(String) }`; `pub struct MoveSaga { pub from: NodeId, pub to: NodeId, pub step: MoveStep, pub started_ms: i64, pub sessions: Vec<String> }`; `pub fn next(s: &MoveSaga, ev: MoveEvent, proof: bool) -> (MoveSaga, Vec<MoveAction>)`
  - Prepare: `to` is an Active Standby, online, `compat(Promote)` Ok, Task 11's checks pass, `proof` true (else `"Moving the home needs the resume proof first"`)
  - Freeze: the old home holds Notchling's replies and answers to main sessions and waits up to 10 minutes for each to go idle (a turn started from the terminal keeps it waiting; the maintainer may cancel)
  - FinalSnapshot and Transfer: one `Final` set, acknowledged by `to`
  - StopOld: `claude stop <id>` for each Claude Code main session and `codex.stop` for Codex ones (`control::session` steps), each confirmed ended by the session monitor
  - Switch: `Term { n: n + 1, node: to }`, `home = to`, the old home becomes a Standby; journaled on both before either acts as the other
  - Resume: `to` restores (Task 8) and starts each Claude Code main session with `claude --bg --resume <id>` and its saved flags, Codex sessions resumed on the next message; none is sent a message
  - Announce: an inbox `notice` "The home moved to `<to>`" and a `cluster.changed` event; the bridge follows (Task 10)
  - Undo before Switch: unfreeze, start stopped sessions again on the old home with the same resume command, raise `notice` "Couldn't move the home: `<reason>`"; after Switch there is no undo (a failed Resume raises an `action_failed` item per session)
  - `cluster.move_home {to}` (`OpSpec::APP`, forwarded to the home)
- [ ] **Step 1: Write the failing tests:**
  - pure: `prepare_needs_proof`, `freeze_waits_for_idle`, `failure_before_switch_compensates`, `switch_bumps_term_once`, `no_undo_after_switch`, `resume_never_sends_a_message`
  - daemon (fake launcher, two in-memory nodes): `move_stops_then_resumes_with_same_flags`, `inbox_survives_move` (an open item's id answers on the new home)
  - simulator: `move_under_partitions_keeps_one_home_per_term`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::move_home`, `cargo test -p notchling-notchlingd --lib cluster_move`, `cargo test -p notchling-engine --lib cluster::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all three. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: move the home to a Standby on purpose`.

### Task 10: The phone and the bridge follow the home

**Zone:** maintainer (`protocol/`, `crates/engine/src/bridge/`, `apps/flutter/lib/features/pairing/`); auto for other Flutter features. CAP-NETWORK (a node serves the bridge).

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- Modify: `protocol/agentconsole.v1.schema.json` (hello `cluster: {home, term, nodes: [{id, name, addrs, fingerprint}]}`, optional; an error code `not_home` with `home`), fixtures, regenerate
- Modify: `crates/engine/src/bridge/` (hello carries `cluster`; on a Standby the bridge answers every op but `hello` and `inbox.list` with `not_home`), `crates/notchlingd/src/bridge_backend.rs`, `crates/engine/src/cluster/snapshot/plan.rs` (`devices.json` in sets), the bridge's identity load (a node with no bridge identity serves its node certificate, and `nodes[].fingerprint` names the one it serves)
- Modify: `apps/flutter/lib/features/connection/` (on `not_home` or a lost link, try the home named in the last `cluster`, pinned to its fingerprint as learned over the trusted link), `apps/flutter/lib/features/pairing/` (store learned node pins beside the paired Mac's)
- Test: `crates/engine/src/bridge/router_home_tests.rs`, `apps/flutter/test/follow_home_test.dart`
- [ ] **Step 1: Write the failing tests:** `hello_carries_cluster_nodes`, `standby_answers_not_home`, `old_phone_without_cluster_field_still_connects`; Flutter `follows_home_on_not_home`, `refuses_a_node_whose_cert_doesnt_match`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib bridge::router_home_tests`, `python3 tools/protocol/gen.py --check`, `~/flutter/bin/flutter test test/follow_home_test.dart`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: follow the home across machines (additive)`.

### Task 11: Standby readiness in Machines

**Zone:** maintainer (`engine/src/machines`). No new CAP (existing `Runner`).

**Files:**
- Create: `crates/engine/src/machines/steps_standby.rs`
- Test: `crates/engine/src/machines/tests_standby_steps.rs`
- Modify: `crates/engine/src/machines/step.rs` (`STANDBY_ENCRYPTED`, `STANDBY_SAME_HOME`, `STANDBY_SPACE`, `STANDBY_TOOLS`), `crates/engine/src/machines/catalog.rs` (when `node.role` is Standby)

**Interfaces:**
- Produces: `STANDBY_ENCRYPTED` (`fdesetup status` says On; Linux: `lsblk -o NAME,TYPE,MOUNTPOINT -J` shows the home on a `crypt` device), `STANDBY_SAME_HOME` (`echo $HOME` and `id -un` equal the home's), `STANDBY_SPACE` (`df -k $HOME`: at least twice the last set's size, 5 GB minimum), `STANDBY_TOOLS` (Claude Code and Codex versions equal the home's: version-sync's step does the update); each failing step is a HowTo, and `cluster.member.set {role: standby}` is refused until all pass (`"m1 isn't ready to be a Standby: <first failing step>"`).
- [ ] **Step 1: Write the failing tests** (`ScriptedRunner`): `filevault_on_passes`, `luks_home_passes`, `unencrypted_fails_with_howto`, `different_home_path_fails`, `low_space_fails`, `standby_role_refused_until_ready`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_standby_steps`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `machines: Standby readiness checks`.

### Task 12: The app: Standby, main sessions, replication, Move Home

**Zone:** auto. No CAP.

**Files:**
- Create: `crates/app/src/cluster/move_sheet.rs`, `crates/app/src/cluster/replication.rs`
- Test: `crates/app/src/cluster/replication_tests.rs`, `crates/app/src/cluster/move_sheet_tests.rs`
- Modify: `crates/app/src/cluster/model.rs` and `view.rs` (role Standby with its readiness; each Standby's last set age and size), `crates/app/src/chat/view/` header menu or row menu (Main Session toggle, with "Main sessions are copied to your Standby and can move with the home"), `crates/app/src/cluster/renders.rs`

**Interfaces:**
- Produces: `ReplicationRow::from(status) -> ReplicationRow { node, last_set_age_s, size_mb, behind: bool }` (`behind` after 15 minutes without a set while a main session worked); Move Home sheet states Prepare → Done with each step's line, Cancel before Switch only.
- [ ] **Step 1: Write the failing tests:** `standby_shows_readiness_failures`, `behind_after_fifteen_minutes`, `move_sheet_offers_cancel_only_before_switch`, `main_toggle_only_for_claude_and_codex_on_home`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib cluster::replication_tests` and `cluster::move_sheet_tests`. Expected: FAIL.
- [ ] **Step 3: Implement** and add `ViewSpec`s `cluster-move`, `cluster-replication`.
- [ ] **Step 4: Run**, render both (release) and look.
- [ ] **Step 5: Commit** `app: Standby, main sessions and Move Home`.

### Task 13: Docs

**Zone:** maintainer. No CAP.

**Files:**
- Create: `docs/decisions/0023-cluster-standby-and-move.md` (Proposed: sets not live files; identical paths; no encryption by Notchling, disk encryption required; main sessions; the inbox in `cluster.db`; the move saga and its undo line)
- Modify: `docs/policies/privacy.md` (what a Standby holds), `docs/local-api-ops.md`, AGENTS notes for engine, notchlingd, app, `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** the full checks. Expected: clean, PASS.
- [ ] **Step 3: Commit** `docs: Standby snapshots and moving the home`.

### Checkpoint 2: the live check (maintainer only)

Gated by M5 (resume proof: flips `RESUME_PROOF_RECORDED`), M11 (m1 set up), M14 (disk encryption). The maintainer marks one session main, works a few turns, checks the Standby's set age in the cluster view, moves the home to m1 from the app, continues the session on m1 from the phone, and moves it back.
