# Cluster v1, plan D: Standby Snapshots, the Synced Handover and Phone Continuity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every Can Be Master machine (m1, and any other the maintainer marks) holds a consistent, recent copy of what it needs to carry on the primary's sessions: their transcripts and session folders, the repository state each one is at and whether it may move, the cluster's database with the inbox, phone pairing and pages, sent at each turn end. The maintainer can hand the primary over on purpose (Move to <machine> Now while the primary is reachable, and plan E's Take Back) with nothing lost: sessions whose code is pushed move, the rest keep running where they are. The phone is never cut off: it knows every Can Be Master machine, falls back to one it can reach, and that machine relays to the primary or holds the commands until a promote or the primary's return.

**Architecture:** A pure snapshot planner in `engine::cluster::snapshot` decides when (a main session's turn end, with a timer as a backstop at quiet points) and what (a `ResumeSet` per main session, its repository's state and whether it may move, exclusions). Code never travels in a set: under the maintainer's rule (a), only sessions whose working tree is clean and pushed may move, and the machine that takes one gets its code from the repository's own remote at the recorded commit. Runners take consistent copies without touching the user's live repositories or databases (git read only, SQLite through its backup API, transcripts as appended bytes taken at quiet points). Sets go to every ready Can Be Master machine over plan B's blob transfer and become current only when complete. A restore planner lays a set out at identical paths. The synced handover is a saga in `engine::cluster::move_home` driven by `NodeCore`, with compensation up to the switch of terms. The inbox moves from `notchling.db` into `cluster.db`, so items and their ids survive a handover. On the bridge, a Can Be Master machine that isn't the primary relays the phone's ops to the primary, or holds them in a persisted queue while the primary can't be reached.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app), git (as a child process, read only for snapshots, hardened as in plan B Task 14), rusqlite's backup API, the plan B node link and blobs, `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`: "Scope: v1 first" (Replication, Must exist first: the resume proof and the db split), "Main sessions and the Standby", "Planned handoff", "Phone and desktop app", review issues 6 (live `.git` and SQLite), 7 (paths), 8 (encryption), invariant I4, data ownership. The overview's "Decisions 2026-11-14" win where they differ (proposals 1, 2a, 5, 9 and the phone flow). Plans B and C merged first (C's `EXCLUDED_DIRS` are refused here).

## Global Constraints

- Plan B's constraints hold, including its words: "home" in code is the primary; a Standby is a member with Can Be Master on that isn't the primary.
- Never write the user's repository while snapshotting: only reads (`status`, `rev-parse`, `rev-list`, `remote get-url`), with `GIT_OPTIONAL_LOCKS=0` so `status` never refreshes the index. No shadow repository and no bundles in v1: rule (a) (proposal 2a) moves only pushed sessions; continuous worktree sync (2b) is after v1.
- Never copy a live SQLite file: databases go through the backup API. Never copy a transcript while it's being written: transcripts go as the bytes after the length the target acknowledged, taken at quiet points.
- Paths: a set restores only at the same absolute paths and under the same home folder. A Can Be Master machine whose home path or user differs isn't ready (Task 12). No path rewriting in v1.
- Exclusions: `EXCLUDED_DIRS` (plan C), anything over 100 MB, and code: a set carries a repository's facts (origin, branch, commit), never its files.
- Notchling doesn't encrypt sets in v1; every Can Be Master machine must have disk encryption on (FileVault, LUKS), checked in Task 12 (proposal 1). Sets are 0600 in 0700 folders.
- A set is current on a Standby only when every part arrived and its hashes match (`.partial` → rename). A Standby never holds a set ahead of what the primary staged (I4).
- Main sessions: every Claude Code and Codex session on the primary is main unless the maintainer turns it off for that session ("Keep on <machine>"): the maintainer said running sessions continue on the standby. Antigravity isn't; Codex CLI sessions started outside Notchling can't be fenced and aren't main.
- **May move** (rule a): a main session may move at a given set only when its folder is in a git repository whose working tree has no change and no untracked file that isn't ignored, on a branch whose HEAD is already in its upstream (pushed). Every other session stays where it is: after a handover or a promote it keeps running on its machine as a worker-hosted session, or waits for that machine.
- Held commands live on the Can Be Master machine the phone reached, in its own `cluster.db`, in order; they are never run against that machine's copy in the primary's place, and each is visible on the phone until delivered, dropped or cancelled.
- The handover is live-gated by `RESUME_PROOF_RECORDED` (a code const, `false` until the maintainer's proof M5 in the overview). Tests drive it with the flag passed in.

## Review Focus

1. A snapshot taken while git runs or the session works: never a torn copy, and the user's repository is never written. (Task 3 `git_lock_blocks_quiet`, `working_session_waits`; Task 4 `reading_state_never_changes_the_repo`.)
2. A session with uncommitted or unpushed work: never marked as able to move, never restored elsewhere. (Task 4 `uncommitted_change_is_not_movable`, `ahead_of_upstream_is_not_movable`; Task 8 `unmovable_session_is_not_restored`; Task 9 `unpushed_session_stays_as_worker_hosted`.)
3. A Standby that crashes mid-transfer: its current set stays the last complete one. (Task 6 `partial_set_never_becomes_current`, Task 7 simulator I4.)
4. A handover that fails before the switch: the old primary is back as it was, sessions running. (Task 9 `failure_before_switch_compensates`.)
5. A restore over a folder with the target's own changes: refused, nothing overwritten. (Task 8 `restore_refuses_a_folder_with_its_own_changes`.)
6. A phone command sent while the primary can't be reached: held once, delivered once and in order on its return or after a promote, never lost and never run twice. (Task 10 `held_command_is_delivered_once_on_return`, `held_answer_to_a_closed_prompt_is_dropped_and_reported`.)
7. Inbox items and their ids after a handover: the same ids, answerable on the new primary. (Task 1 `inbox_moves_to_cluster_db_once`, Task 9 `inbox_survives_move`.)

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

### Task 2: Main sessions, on by default

**Zone:** maintainer for engine and daemon; auto for the app toggle (Task 13). No CAP.

**Files:**
- Create: `crates/engine/src/cluster/main_sessions.rs`
- Test: `crates/engine/src/cluster/main_sessions_tests.rs`
- Modify: `crates/engine/src/cluster/state.rs` (`ClusterState.main: BTreeMap<String, MainSession>`, `ClusterState.kept: BTreeSet<String>` (sessions the maintainer turned off), `Command::SetMain { session, main: bool }`), `crates/local-api/src/wire.rs` (`CLUSTER_MAIN_SET = "cluster.main.set"`), `crates/notchlingd/src/cluster_ops.rs`

**Interfaces:**
- Produces:
  - `pub struct MainSession { pub id: String, pub tool: Tool, pub cwd: String, pub config_dir: Option<String>, pub transcript: Option<String>, pub respawn: Vec<String>, pub since_ms: i64 }`
  - `pub fn may_be_main(agent: &Agent) -> Result<(), String>` (Claude Code or Codex, on the primary, with a folder; else `"Only Claude Code and Codex sessions on the primary can move"`)
  - `pub fn is_main(agent: &Agent, kept: &BTreeSet<String>) -> bool`: every session `may_be_main` accepts, unless its id is in `kept`
  - `cluster.main.set {id, main}` (`OpSpec::APP`, forwarded to the primary); `main: false` is the app's "Keep on <machine>"
  - the session monitor's list on the primary keeps `main` current: a new Claude Code or Codex session (started in Chat, from the phone, or in a terminal and seen by the monitor) is main at once
- [ ] **Step 1: Write the failing tests:** `claude_and_codex_on_primary_are_main_by_default`, `kept_session_is_not_main`, `agy_may_not`, `worker_session_may_not`, `set_main_is_journaled`, `ended_session_leaves_main_after_a_day`, `phone_started_session_is_main`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::main_sessions`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: main sessions, on by default`.

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
  - `pub enum Trigger { TurnEnd(String), Timer, Ready(NodeId), Final }`
  - `pub fn due(prev: &Observed, now: &Observed) -> Option<Trigger>`: a main session moving Working → Idle (a set at every turn end, proposal 1); else the timer as a backstop; never while a git lock exists in a main session's repository or any main transcript was written in the last `QUIET_MS` (the trigger waits for the next quiet moment); `Ready` is asked when a Can Be Master machine becomes ready (Task 12), so it gets a set at once; `Final` is asked by the handover
  - `pub struct ResumeSet { pub session: String, pub tool: Tool, pub transcripts: Vec<PathBuf>, pub files: Vec<PathBuf>, pub dirs: Vec<PathBuf> }`; `pub fn resume_set(m: &MainSession, home: &Path) -> ResumeSet`: Claude Code: the transcript, `<projects>/<slug>/<session>/` (subagents, tool results), `file-history/<session>/`, `todos/<session>-*.json`, the transcripts a `continued-in` chain reaches (`transcript::continued::earlier`); Codex: the rollout file and `session_index.jsonl` (its line only). The exact list is confirmed by M5; the planner holds it in one table.
  - `pub fn excluded(path: &Path) -> bool` (`EXCLUDED_DIRS`, over `MAX_FILE`)
  - `pub struct SessionEntry { pub id: String, pub tool: Tool, pub cwd: String, pub repo: Option<RepoFacts>, pub movable: Movable }` (Task 4's types)
  - `pub struct SetPlan { pub id: String, pub trigger: Trigger, pub sessions: Vec<SessionEntry>, pub tails: Vec<(PathBuf, u64)>, pub files: Vec<PathBuf>, pub dbs: Vec<PathBuf> }`: `tails` are transcripts with the smallest length any ready target acknowledged (each target is then sent from its own, Task 6); `files`: the resume sets' other files, `<data>/pages/`, the bridge's `devices.json` (paired phones, proposal 9: pairing zone); `dbs`: `cluster.db` (inbox, jobs, held queue, the `clusterFailover` value of plan E)
  - `pub fn plan_set(main: &[MainSession], repos: &BTreeMap<String, Option<RepoFacts>>, home: &Path, data: &Path, seq: u64, acked: &BTreeMap<PathBuf, u64>) -> SetPlan` (id `s-<seq>`)
- [ ] **Step 1: Write the failing tests:** `turn_end_triggers`, `timer_backstops_after_five_minutes`, `ready_machine_gets_a_set_at_once`, `git_lock_blocks_quiet`, `working_session_waits`, `recent_write_waits_ten_seconds`, `resume_set_for_claude_lists_its_files`, `resume_set_follows_continued_in`, `resume_set_for_codex`, `transcript_goes_from_smallest_acked_length`, `grants_and_logins_are_excluded`, `devices_json_is_in_every_set`, `big_files_are_excluded`, `no_code_is_ever_planned` (a repository folder never appears in `files` or `tails`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::snapshot`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: when and what a snapshot takes`.

### Task 4: Repository state and the rule for moving

**Zone:** maintainer. CAP-PROCESS (git, read only).

**Files:**
- Create: `crates/engine/src/cluster/snapshot/repo_state.rs`, `crates/notchlingd/src/snap_repo.rs`
- Test: `crates/engine/src/cluster/snapshot/repo_state_tests.rs`, `crates/notchlingd/src/snap_repo_tests.rs` (`#[cfg(unix)]`, real git in temp folders, a local bare repository as `origin`)

**Interfaces:**
- Produces:
  - `pub struct RepoFacts { pub root: PathBuf, pub origin: Option<String>, pub branch: Option<String>, pub head: String, pub upstream: Option<String>, pub unpushed: u32, pub dirty: bool, pub untracked: bool, pub main_repo: Option<PathBuf> }` (`main_repo` set when the folder is a linked worktree)
  - `pub fn read_plan(folder: &Path) -> Vec<GitCall>`: `rev-parse --show-toplevel --git-common-dir`, `status --porcelain=v2 --branch --untracked-files=normal` (ignored files don't count), `rev-list --count @{upstream}..HEAD` when there is an upstream, `remote get-url origin`; every call hardened (plan B Task 14) and with `GIT_OPTIONAL_LOCKS=0`
  - `pub fn parse(folder: &Path, outputs: &[GitOutput]) -> Result<Option<RepoFacts>, String>` (`None`: not a repository)
  - `pub enum Movable { Yes, No(String) }`; `pub fn movable(f: Option<&RepoFacts>, folder: &str) -> Movable`: none → No `"<folder> isn't a git repository"`; `dirty` → No `"<root> has uncommitted changes"`; `untracked` → No `"<root> has untracked files"`; detached HEAD → No `"<root> isn't on a branch"`; no upstream or no origin → No `"<branch> has no pushed upstream"`; `unpushed > 0` → No `"<branch> has <n> commits that aren't pushed"`; else Yes (a branch behind its upstream is pushed, so it moves)
- [ ] **Step 1: Write the failing tests** (real git):
  - `pushed_clean_tree_is_movable`, `uncommitted_change_is_not_movable`, `untracked_file_is_not_movable`, `ignored_file_doesnt_count`, `ahead_of_upstream_is_not_movable`, `behind_upstream_is_movable`, `no_upstream_is_not_movable`, `not_a_repo_is_not_movable`, `detached_head_is_not_movable`, `linked_worktree_names_its_main_repo`
  - `reading_state_never_changes_the_repo` (the index file's bytes and mtime, refs, `stash list` and every file's mtime under `.git` the same before and after), `hooks_and_fsmonitor_never_run`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::snapshot::repo_state` and `cargo test -p notchling-notchlingd --lib snap_repo`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: read a repository's state and whether its session may move`.

### Task 5: Databases and files

**Zone:** maintainer. CAP-FILEWRITE.

**Files:**
- Create: `crates/notchlingd/src/snap_files.rs`
- Test: `crates/notchlingd/src/snap_files_tests.rs`

**Interfaces:**
- Produces:
  - `pub fn backup_db(src: &Path, dst: &Path) -> Result<(), String>` (rusqlite `backup::Backup`, 256 pages a step, 5 ms between steps, the destination 0600)
  - `pub fn copy_quiet(src: &Path, dst: &Path, quiet_ms: i64, now_ms: i64) -> Result<Copied, String>` (`Copied::Done { len, sha256 }` or `Copied::Busy` when the file changed during the copy or within `quiet_ms`: compare size and mtime before and after)
  - `pub fn copy_tail(src: &Path, from: u64, dst: &Path, quiet_ms: i64, now_ms: i64) -> Result<Copied, String>` (the bytes after `from`; `Copied::Rewritten` when the file is shorter than `from` or its inode changed: the planner then sends it whole)
  - `pub fn stage_set(plan: &SetPlan, stage: &Path) -> Result<Manifest, String>` (everything under `<data>/cluster/outgoing/<set>/`, then `Manifest { set, seq, term, home, created_ms, sessions: Vec<SessionEntry>, parts: Vec<Part { kind: Db|File|Dir|Tail { from }, path, sha256, len }> }`)
- [ ] **Step 1: Write the failing tests:** `backup_of_a_db_being_written_is_consistent` (a writer thread inserts while backing up; the copy opens and passes `PRAGMA integrity_check`), `busy_file_is_not_copied`, `tail_copies_only_new_bytes`, `shorter_transcript_is_rewritten`, `manifest_lists_every_part_with_hash`, `manifest_records_each_session_and_whether_it_moves`, `stage_is_0600_in_0700`, `excluded_paths_never_staged`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib snap_files`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: consistent copies of databases, quiet files and transcript tails`.

### Task 6: Sending sets to every Can Be Master machine

**Zone:** maintainer. CAP-FILEWRITE, CAP-NETWORK.

**Files:**
- Create: `crates/engine/src/cluster/standby_store.rs`, `crates/notchlingd/src/snap_send.rs`
- Test: `crates/engine/src/cluster/standby_store_tests.rs`, `crates/notchlingd/src/snap_send_tests.rs`
- Modify: `crates/engine/src/cluster/policy.rs` (`snap.offer`, `snap.part`, `snap.commit` from `Home` origin `Node` to a Standby; `snap.ack` back), `crates/local-api/src/wire.rs`

**Interfaces:**
- Produces:
  - `StandbyStore` at `<data>/cluster/standby/<home id>/`: `fn begin(&self, m: &Manifest) -> Result<(), String>` (`sets/<set>.partial/`), `fn part(&self, set: &str, part: &Part, from: &Path) -> Result<(), String>` (hash checked; a `Tail` part is appended to the stored transcript only when its stored length equals `from`), `fn commit(&self, set: &str) -> Result<(), String>` (every part present and hashed → rename to `sets/<set>`, write `current.json` atomically), `fn current(&self) -> Option<Manifest>`, `fn lengths(&self) -> BTreeMap<PathBuf, u64>`, `fn prune(&self, keep: usize)` (keeps the current set and one before), `fn missing(&self, m: &Manifest) -> Vec<Part>` (resume after a drop)
  - the primary's sender: one queue per ready Can Be Master member that is online (Task 12's `ready`), each sending from that target's own acknowledged lengths; one set in flight per target, newest wins (a set staged while one is sending replaces the queued one, never the one in flight); a target that comes back online gets the newest set
  - `snap.ack {set, lengths}` after `commit`; the primary records each target's set and transcript lengths (for the next tails, the handover and plan E's fence)
  - a set staged on the primary is sent whatever node is primary: after a handover or a promote, the new primary sends to the others, the old one included
- [ ] **Step 1: Write the failing tests:** `complete_set_becomes_current`, `partial_set_never_becomes_current`, `bad_hash_part_is_refused`, `tail_part_appends_only_at_its_offset`, `resume_sends_only_missing_parts`, `prune_keeps_two`, `newest_queued_set_wins`, `every_ready_can_be_master_machine_gets_the_set`, `not_ready_machine_gets_nothing`, `worker_gets_nothing`, `ack_records_each_target`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::standby_store` and `cargo test -p notchling-notchlingd --lib snap_send`. Expected: FAIL.
- [ ] **Step 3: Implement** over plan B's `node.put` chunks.
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: send snapshot sets to every Can Be Master machine`.

### Task 7: Snapshots in the core and the simulator

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/engine/src/cluster/core/mod.rs` (`Input::Observed`, `Effect::Snapshot { plan }`, acks per target), `crates/engine/src/cluster/sim/world.rs`, `sim/host.rs` (fake snapshots: a set is a version number of each main session's fake state plus a fake pushed flag; transfers take simulated time and can be cut; two Can Be Master targets), `sim/invariants.rs`, `crates/notchlingd/src/cluster_node.rs` (feeds `Observed` from the session monitor and lock files every 2 s, reads repository state for main sessions at each trigger, runs `Snapshot` effects on a worker)
- Test: `crates/engine/src/cluster/sim/sim_tests.rs`

**Interfaces:**
- Produces `Violation::{StandbyAhead { node, set }, TornSet { node, set }, MovedUnpushed { session, node }}`: I4 (a Standby's current set is never one the primary didn't stage, never newer than the primary's newest staged set, and every current set is complete), and no session is restored on a node from a set entry that didn't say it may move.
- [ ] **Step 1: Write the failing tests:** `standby_never_ahead_over_seeds`, `crash_during_transfer_keeps_last_complete_set`, `each_target_keeps_its_own_set`, `a_planted_torn_commit_is_caught`, `a_planted_unpushed_restore_is_caught`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** with `NOTCHLING_SIM_SEEDS=2000`. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: snapshots in the node core, checked by the simulator`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib cluster::`, `cargo test -p notchling-notchlingd`: PASS. fmt, clippy, guard: clean.
- [ ] Controller runs `reading_state_never_changes_the_repo` and reads its before/after listing.

### Task 8: Restore at identical paths

**Zone:** maintainer. CAP-PROCESS (git), CAP-FILEWRITE.

**Files:**
- Create: `crates/engine/src/cluster/restore.rs`, `crates/notchlingd/src/snap_restore.rs`
- Test: `crates/engine/src/cluster/restore_tests.rs`, `crates/notchlingd/src/snap_restore_tests.rs` (`#[cfg(unix)]`, real git, a local bare repository as `origin`)

**Interfaces:**
- Produces:
  - `pub enum RestoreStep { CloneRepo { origin, path, branch, head }, FastForward { path, branch, head }, AddWorktree { main, path, branch, head }, AppendTail { from, to, at }, PutFile { from, to }, PutDir { from, to }, MoveAside { path, to }, RestoreDb { from, to }, MergeDevices { from } }`; `pub fn plan_restore(m: &Manifest, store: &Path, home: &Path, sessions: &BTreeSet<String>, aside: bool, probe: &dyn Fn(&Path) -> PathState) -> Result<Vec<RestoreStep>, String>`; `pub enum PathState { Missing, Clean { branch: String, head: String }, Changed }`
  - only the sessions named in `sessions` whose set entry says Yes are restored; any other is refused `"<id> can't move: <reason>"`
  - a repository folder that is missing is cloned from its recorded `origin` and checked out at the branch and commit; one that is `Clean` on the same branch at an ancestor of the commit is fetched and fast-forwarded; `Clean` on another branch refuses `"<path> on <node> is on <branch>"`; `Changed` refuses `"<path> on <node> has its own changes"`; a linked worktree is added to its main repository at the same path (which must exist there, else refused naming it). Every git call is hardened (plan B Task 14) and never prompts (`GIT_TERMINAL_PROMPT=0`): a fetch that needs a login fails with git's message (M11 sets up the machine's git access)
  - transcripts: where no file exists the stored copy is put; where a shorter local copy is a prefix of it, the rest is appended; a longer or different local copy refuses `"<id> ran on <node> after this set"`, unless `aside` (plan E's side copies), where it is first moved to `<data>/cluster/copies/<session>/<set>/` (never deleted) and then restored
  - `RestoreDb` for `cluster.db` only through the backup API into a temp file, then a rename, with the daemon's own connection closed first
  - `MergeDevices`: the set's paired phones are added to this machine's bridge devices (pairing zone); none of this machine's own are removed
- [ ] **Step 1: Write the failing tests:** `restore_clones_a_missing_repo_at_its_commit` (the working tree, `git status --porcelain` and the branch equal the primary's), `restore_fast_forwards_a_clean_clone`, `restore_refuses_a_folder_with_its_own_changes`, `restore_refuses_a_clone_on_another_branch`, `unmovable_session_is_not_restored`, `linked_worktree_is_added_to_its_main_repo`, `longer_local_transcript_refuses`, `aside_keeps_the_local_copy` (the moved file exists byte for byte), `db_restore_is_atomic`, `devices_are_merged_never_removed`, `restore_steps_never_touch_excluded_dirs`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::restore` and `cargo test -p notchling-notchlingd --lib snap_restore`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: restore a set at the same paths`.

### Task 9: The synced handover

**Zone:** maintainer. CAP-PROCESS (`claude stop`, `claude --bg --resume` through `control::run`).

**Files:**
- Create: `crates/engine/src/cluster/move_home.rs`, `crates/notchlingd/src/cluster_move.rs`
- Test: `crates/engine/src/cluster/move_home_tests.rs`, `crates/notchlingd/src/cluster_move_tests.rs`
- Modify: `crates/engine/src/cluster/core/control.rs`, `crates/engine/src/cluster/versions.rs` (`pub const RESUME_PROOF_RECORDED: bool = false`), `crates/local-api/src/wire.rs` (`CLUSTER_MOVE_HOME = "cluster.move_home"`, `CLUSTER_MOVE_NOW = "cluster.move.now"`, node-only `MOVE_FREEZE`, `MOVE_STOP`, `MOVE_RESUME`, `MOVE_UNDO`), `crates/engine/src/cluster/policy.rs`, `crates/engine/src/cluster/sim/` (scenario op `MoveHome`)

**Interfaces:**
- Produces:
  - `pub enum MoveStep { Prepare, CatchUp, Freeze, FinalSnapshot, StopOld, Switch, Resume, Announce, Done, Undo(String) }`; `pub struct MoveSaga { pub from: NodeId, pub to: NodeId, pub step: MoveStep, pub started_ms: i64, pub moving: Vec<String>, pub staying: Vec<(String, String)>, pub now: bool }`; `pub fn next(s: &MoveSaga, ev: MoveEvent, proof: bool) -> (MoveSaga, Vec<MoveAction>)`
  - Prepare: `to` is a ready Can Be Master member, online, `compat(Promote)` Ok, `proof` true (else `"Moving the primary needs the resume proof first"`); the main sessions split into `moving` (Yes in the newest set) and `staying` (with the reason)
  - CatchUp: `to` gets the newest set now if its acknowledgement is behind
  - Freeze: the old primary holds Notchling's replies and answers to `moving` sessions and waits for each to reach a turn end, up to 10 minutes (a turn started from the terminal keeps it waiting; the sheet shows which ones). **Move Now** (`cluster.move.now`) ends the wait: working moving sessions are stopped mid-turn; their cut steps become plan E's `paused_step` items once E Tasks 4 and 10 land, and until then the sheet names them
  - FinalSnapshot: one `Final` set, acknowledged by `to`; a session no longer movable in it joins `staying`
  - StopOld: `claude stop <id>` for each moving Claude Code session and `codex.stop` for Codex ones (`control::session` steps), each confirmed ended by the session monitor
  - Switch: `Term { n: n + 1, node: to }`, `home = to`; journaled on both before either acts as the other. The old primary stays a Standby; `staying` sessions keep running there and the new primary lists them as remote rows (plan B Task 12): worker-hosted sessions
  - Resume: `to` restores the moving sessions (Task 8) and starts each Claude Code one with `claude --bg --resume <id>` and its saved flags, Codex ones on the next message; none is sent a message
  - Announce: an inbox `notice` "`<to>` is primary now" and a `cluster.changed` event; held commands flush to `to` (Task 10)
  - Undo before Switch: unfreeze, start stopped sessions again on the old primary with the same resume command, raise `notice` "Couldn't move the primary: `<reason>`"; after Switch there is no undo (a failed Resume raises an `action_failed` item per session)
  - `cluster.move_home {to}` (`OpSpec::APP`, forwarded to the primary). Used by the cluster view's Move to <name> Now while the primary is reachable (Task 13) and by plan E's Take Back
- [ ] **Step 1: Write the failing tests:**
  - pure: `prepare_needs_proof`, `prepare_splits_moving_and_staying`, `freeze_waits_for_turn_ends`, `move_now_ends_the_wait_and_names_cut_sessions`, `session_unmovable_in_final_set_stays`, `failure_before_switch_compensates`, `switch_bumps_term_once`, `no_undo_after_switch`, `resume_never_sends_a_message`
  - daemon (fake launcher, two in-memory nodes): `move_stops_then_resumes_with_same_flags`, `unpushed_session_stays_as_worker_hosted` (it keeps running on the old primary and shows as a remote row on the new one), `inbox_survives_move` (an open item's id answers on the new primary)
  - simulator: `move_under_partitions_keeps_one_home_per_term`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::move_home`, `cargo test -p notchling-notchlingd --lib cluster_move`, `cargo test -p notchling-engine --lib cluster::sim`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all three. Expected: PASS.
- [ ] **Step 5: Commit** `cluster: hand the primary over to a Standby, synced`.

### Task 10: Phone continuity on the Macs: relay and the held queue

**Zone:** maintainer (`protocol/`, `crates/engine/src/bridge/` with its pairing, `notchlingd`). CAP-NETWORK (a non-primary node serves the bridge to a phone), CAP-FILEWRITE (the held queue).

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- Modify: `protocol/agentconsole.v1.schema.json` (hello `cluster: {primary, term, nodes: [{id, name, addrs, fingerprint, canBeMaster, online}]}`, optional; an op result may carry `held: {id, at}` or `relayed: true`; ops `held.list {}` and `held.cancel {id}`; topic `held` (`held.changed`); error code `primary_unreachable` with `primary`; all additive), fixtures, regenerate (`python3 tools/protocol/gen.py`)
- Create: `crates/engine/src/cluster/held.rs` (pure), `crates/notchlingd/src/cluster_relay.rs`
- Test: `crates/engine/src/cluster/held_tests.rs`, `crates/notchlingd/src/cluster_relay_tests.rs`, `crates/engine/src/bridge/router_relay_tests.rs`
- Modify: `crates/engine/src/bridge/` (hello carries `cluster`; on a Can Be Master machine that isn't the primary every op but `hello`, `inbox.list` and `held.*` goes through `cluster_relay`; the paired devices merged from the newest set are accepted, Task 8), `crates/notchlingd/src/bridge_backend.rs`, `crates/engine/src/cluster/policy.rs` (from `Client` or a Can Be Master member, origin `App`, to the primary: `session.new` and `cluster.dispatch` relayed for a phone), `crates/engine/src/cluster/persist.rs` (a `held` table), the bridge's identity load (a node with no bridge identity serves its node certificate, and `nodes[].fingerprint` names the one it serves)

**Interfaces:**
- Produces:
  - `pub struct Held { pub id: String, pub op: String, pub args: Value, pub target: HeldTarget, pub device: String, pub request: String, pub at_ms: i64 }`, `pub enum HeldTarget { Primary, Session(String) }`
  - `pub struct HeldQueue` (pure): `fn hold(&mut self, h: Held) -> String` (a repeat of the same device and request id is the same entry), `fn cancel(&mut self, id: &str) -> bool`, `fn take_all(&mut self) -> Vec<Held>` (in order), `fn list(&self) -> Vec<Held>`
  - relay, on a Can Be Master machine that isn't the primary: a phone op on a session whose owner is a Worker that this machine reaches goes straight to it (plan B's data path); else to the primary when it is reachable (the reply carries `relayed: true`); else it is held, answered `{held: {id, at}}` at once, and `held.changed` goes to the phone. Ops that only read (lists, chat windows) are answered from this machine's own mirrors and newest set, with `stale` set to the set's time
  - flush: when the primary answers again, every held entry goes to it in order, before any new op from this machine; after a promote of this machine (plan E) they run here once the restore is done. A held answer whose prompt is no longer open is dropped and the phone gets "`<primary>` closed that question before your answer arrived"; a held command for a session that didn't move stays held until its machine is back
  - `held.list` and `held.cancel` (paired phone only; a cancel of a delivered entry answers `delivered`)
- [ ] **Step 1: Write the failing tests:** `hello_lists_can_be_master_machines`, `old_phone_without_cluster_field_still_connects`, `standby_relays_to_reachable_primary`, `worker_session_goes_straight_to_worker`, `unreachable_primary_holds_in_order`, `same_request_twice_is_held_once`, `held_survives_restart`, `held_command_is_delivered_once_on_return`, `held_goes_before_new_ops`, `held_runs_here_after_promote`, `held_answer_to_a_closed_prompt_is_dropped_and_reported`, `cancel_removes_a_held_command`, `reads_are_answered_from_the_set_marked_stale`, `replicated_devices_are_accepted`, `phone_session_new_relays_to_primary`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::held`, `cargo test -p notchling-notchlingd --lib cluster_relay`, `cargo test -p notchling-engine --lib bridge::router_relay_tests`, `python3 tools/protocol/gen.py --check`. Expected: FAIL.
- [ ] **Step 3: Implement** schema first, regenerate, then the queue, the relay and the bridge.
- [ ] **Step 4: Run** the four. Expected: PASS.
- [ ] **Step 5: Commit** `bridge: a Standby relays the phone's commands or holds them (additive)`.

### Task 11: Phone continuity on the phone

**Zone:** maintainer for `apps/flutter/lib/features/pairing/`; auto for `apps/flutter/lib/features/connection/`, `sessions/` and new Flutter tests. No CAP.

**Files:**
- Modify: `apps/flutter/lib/features/connection/` (keeps every Can Be Master machine from the last hello with the fingerprint learned over the trusted link; on a lost link or `primary_unreachable` it tries the primary, then each Can Be Master machine, and stays on the first that answers; a line "Through m1 · m5 isn't reachable"; back to the primary when it answers), `apps/flutter/lib/features/pairing/` (stores the learned pins beside the paired Mac's), `apps/flutter/lib/features/sessions/` (a held command shows "Held on m1" with Cancel until it is delivered or dropped)
- Test: `apps/flutter/test/follow_can_be_master_test.dart`
- [ ] **Step 1: Write the failing tests:** `falls_back_to_a_reachable_can_be_master_machine`, `refuses_a_machine_whose_cert_doesnt_match`, `returns_to_primary_when_it_answers`, `held_command_shows_and_cancels`, `old_mac_without_cluster_field_keeps_one_machine`.
- [ ] **Step 2: Run** `~/flutter/bin/flutter test test/follow_can_be_master_test.dart` in `apps/flutter`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it and `~/flutter/bin/dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: fall back to a Can Be Master machine and show held commands`.

### Task 12: Can Be Master readiness

**Zone:** maintainer (`engine/src/cluster`, `engine/src/machines`, `notchlingd`). CAP-PROCESS (`fdesetup status`, `lsblk`: read only, on the node itself).

**Files:**
- Create: `crates/engine/src/cluster/ready.rs`, `crates/engine/src/machines/steps_master.rs`, `crates/notchlingd/src/cluster_host_facts.rs`
- Test: `crates/engine/src/cluster/ready_tests.rs`, `crates/engine/src/machines/tests_master_steps.rs`, `crates/notchlingd/src/cluster_host_facts_tests.rs`
- Modify: `crates/engine/src/cluster/versions.rs` (`Versions.host: Option<HostFacts>`), `crates/engine/src/machines/step.rs` (`MASTER_ENCRYPTED`, `MASTER_SAME_HOME`, `MASTER_SPACE`, `MASTER_TOOLS`), `crates/engine/src/machines/catalog.rs` (when the machine's member has Can Be Master on)

**Interfaces:**
- Produces:
  - `pub struct HostFacts { pub encrypted: Option<bool>, pub home: String, pub user: String, pub free_mb: u64 }`, reported by each node's own daemon in its `Versions` (macOS `fdesetup status` says On; Linux `lsblk -o NAME,TYPE,MOUNTPOINT -J` shows the home on a `crypt` device; at most every 10 minutes, 5 s timeout, through `FakeLauncher` in tests)
  - `pub fn ready(member: &Member, primary: &Member, last_set_mb: u64) -> Result<(), String>`: Can Be Master on; `encrypted == Some(true)` (else `"<name> has no disk encryption"`); `home` and `user` equal the primary's; `free_mb` at least twice the last set and 5 GB; Claude Code and Codex versions equal the primary's (version sync's step does the update)
  - Machines shows the same checks as steps, each failing one a HowTo (FileVault, the same user and home, space, versions); it reads the facts through `cluster.status` (agent tier), so it starts nothing on the node for them
  - Can Be Master may be on for a machine that isn't ready (the default for every Mac): it then gets no set, can't be promoted and isn't a failover target, and the cluster view says `"Not ready: <first failing check>"`
- [ ] **Step 1: Write the failing tests:** `filevault_on_passes`, `luks_home_passes`, `unencrypted_fails_with_howto`, `unknown_encryption_is_not_ready`, `different_home_path_fails`, `low_space_fails`, `can_be_master_is_inert_until_ready`, `host_facts_probe_runs_read_only_commands` (the fake launcher records only `fdesetup status` or `lsblk`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib cluster::ready`, `cargo test -p notchling-engine --lib machines::tests_master_steps`, `cargo test -p notchling-notchlingd --lib cluster_host_facts`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `cluster: Can Be Master readiness, shown in Machines`.

### Task 13: The app: Can Be Master, main sessions, replication, Move to <machine> Now

**Zone:** auto. No CAP.

**Files:**
- Create: `crates/app/src/cluster/move_sheet.rs`, `crates/app/src/cluster/replication.rs`
- Test: `crates/app/src/cluster/replication_tests.rs`, `crates/app/src/cluster/move_sheet_tests.rs`
- Modify: `crates/app/src/cluster/model.rs` and `view.rs` (a Standby's readiness and, per Can Be Master machine, its last set's age and size; a row menu Move to <name> Now while the primary is reachable), `crates/app/src/chat/view/` header menu or row menu (Keep on <machine>, a check that turns main off for that session, with "Other sessions continue on your Standby if this Mac goes away"), `crates/app/src/cluster/renders.rs`

**Interfaces:**
- Produces: `ReplicationRow::from(status) -> ReplicationRow { node, last_set_age_s, size_mb, behind: bool, ready: Result<(), String> }` (`behind` after 15 minutes without a set while a main session worked); the Move sheet: the sessions that move and the ones that stay with their reason, states Prepare → Done with each step's line, Move Now while freezing, Cancel before Switch only.
- [ ] **Step 1: Write the failing tests:** `standby_shows_readiness_failures`, `behind_after_fifteen_minutes`, `move_sheet_lists_moving_and_staying_sessions`, `move_sheet_offers_move_now_while_freezing`, `move_sheet_offers_cancel_only_before_switch`, `keep_on_machine_only_for_claude_and_codex_on_primary`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib cluster::replication_tests` and `cluster::move_sheet_tests`. Expected: FAIL.
- [ ] **Step 3: Implement** and add `ViewSpec`s `cluster-move`, `cluster-replication`.
- [ ] **Step 4: Run**, render both (release) and look.
- [ ] **Step 5: Commit** `app: Can Be Master, replication and Move to <machine> Now`.

### Task 14: Docs

**Zone:** maintainer. No CAP.

**Files:**
- Create: `docs/decisions/XXXX-cluster-standby-and-handover.md` (Proposed; the number is assigned at merge, in merge order: 0021 is search and 0022 localization): sets not live files, at every turn end, to every ready Can Be Master machine; code never in a set, only pushed sessions move (rule a); identical paths; no encryption by Notchling, disk encryption required; main sessions on by default; the inbox in `cluster.db`; the handover saga and its undo line; a Standby relays or holds the phone's commands; paired devices copied through sets (the same accepted risk as `devices.json` today)
- Modify: `docs/policies/privacy.md` (what a Can Be Master machine holds: transcripts, the cluster database with the inbox, paired phones, pages; no code, no secret values), `docs/local-api-ops.md`, AGENTS notes for engine, notchlingd, app, `apps/flutter/AGENTS.md`, `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** the full checks, plus `~/flutter/bin/flutter test` and `~/flutter/bin/dart analyze`. Expected: clean, PASS.
- [ ] **Step 3: Commit** `docs: Standby snapshots, the handover and phone continuity`.

### Checkpoint 2: the live check (maintainer only)

Gated by M5 (resume proof: flips `RESUME_PROOF_RECORDED`), M11 (m1 set up, its git access included), M12 (the phone reaches m1), M14 (disk encryption). With m5 primary: work a few turns in one session whose repository is pushed and one with uncommitted changes; check m1's set age in the cluster view; Move to m1 Now from the app. The pushed session continues on m1, the other keeps running on m5 as a worker-hosted session. Turn m5's network off: the phone says it goes through m1, a reply is held; turn it back on: the reply arrives once. Moving back to m5 is plan E's Take Back.
