# Providers, Phase 1: Subagents in One Shape Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every session's subagents, of every tool, native and delegated, come from one call, `Registry::subagents`, and show as a tree in the Details Panel. Codex child rollouts stop being sessions (the bug) and become subagents of their parent; Antigravity child conversations show at all; Search files a child under its parent; the wire name of Antigravity becomes `antigravity`, read both ways.

**Architecture:** `transcript::subagents::Subagent` takes the spec's 8.1 shape (`Origin { tool, via }`, `session`, `parent`, `depth`). A `Subagents` port (list, glance, open) arrives with an implementation per tool: Claude Code over today's `subagents::list`, Codex over a child source (the daemon's SQLite child index, asked through `codex.children`; two day folders without a daemon, call C2), Antigravity over the summaries copy and the child's log. The `delegate-jobs` flow moves into the engine and every row is merged by `Registry::subagents`, which recurses to depth 10. Codex readers learn one fact, `own_start`, the byte where a child's own turn begins, and ignore the forked parent history before it.

**Tech Stack:** Rust 1.99.1, `rusqlite` (already the engine's, for the summaries copy and `search.db`), `platform::watch` (FSEvents), GPUI through the kit.

**Spec:** `docs/superpowers/specs/2026-11-13-provider-contract-design.md`, sections 4.1, 4.2, 6.3, 6.4, 6.5, 8 (all), 12 (phase 1), calls 3, 4 and 8. Phase 0's plan (`2026-11-14-providers-p0-contract.md`) must be merged. Calls C2, C3, C6, C7, C12 to C14, C29 to C31 are in the overview.

**Branch:** `agent/providers-p1-subagents`, from the integration branch after Phase 0. Tasks 1.11 and 1.12 may go as their own pull request, `agent/providers-p1-antigravity-name`, after 1.3 (call C3 keeps 1.12 a maintainer review on its own).

## Global Constraints

- Everything from Phase 0's constraints holds (no comments, one-line declaration lines, `gaps.json` and `unwired.json` only shrink, no count pinned in a test).
- A model compute and a draw read no file (`crates/AGENTS.md`, Details Panel rows): new reads happen in the loader, off the main thread, cached by size and mtime; a collapsed row's children are listed, never glanced or drawn. `bench_subagents` keeps `parsed_bytes` at most 1 MB.
- No surface names a tool to decide a subagent question: the app asks the registry. `Tool::of_row` is the only place a row id's prefix is read (call C30).
- Tests never touch `~/.codex`, `~/.gemini`, `~/.claude`: every root is a parameter; Codex rollouts and the summaries db are built in the test or under `crates/engine/tests/fixtures/conformance/`, made up, no real transcript.
- Each task that makes a cell Native or wires one edits `support.rs`, `WIRED`, `gaps.json` and `unwired.json` in the same commit and adds its conformance check in `crates/engine/src/contract/conformance_subagents.rs` (call C7). The registry test from Phase 0 fails otherwise.
- User-facing words follow the locale rule in force (call C25); after L4a arms `chat`, the Details Panel's new words are messages.

## Review Focus

1. A Codex rollout whose forked block holds the parent's `token_count` lines: the cost tracker must count them once, in the parent. (Task 1.2 test `a_forked_block_is_not_counted_twice`.)
2. The child index never reads a rollout whose size and mtime are unchanged, and never scans per parent. (Task 1.4 tests `a_changed_rollout_is_read_once_and_an_unchanged_one_never`, `children_are_found_by_the_parent_thread`.)
3. `Registry::subagents` with a loop in the data (a job whose parent is its own child) stops; depth past 10 is cut. (Task 1.8 tests `a_loop_is_listed_once`, `depth_stops_at_ten`.)
4. The Details Panel's steady state reads nothing: a collapsed group's children cost no glance and no draw. (Task 1.9 test `a_collapsed_group_is_never_glanced`; `bench_subagents` unchanged.)
5. Stored row ids written `agy:` before this phase keep matching after it (closed sessions, used sessions, drafts, the selection, Search hits), with or without the key rewrite having run. (Task 1.11 tests `a_stored_agy_id_matches_the_new_row`, Task 1.12 `the_rewrite_runs_once_and_keeps_every_value`.)

## Order and lanes

```
1.1 first session_meta ──► 1.2 own_start in every Codex reader
1.3 Subagent shape ──┬─► 1.6 Codex port ─┐
1.4 child index ─► 1.5 daemon op ───────┘ │
                     ├─► 1.7 Antigravity port ─┤
                     └─► 1.8 registry, Claude port, delegate-jobs ─► 1.9 Details Panel tree
1.1 ─► 1.10 Search children
1.3 ─► 1.11 wire name ─► 1.12 key rewrite (maintainer-reviewed)
1.13 docs, matrix (last)
```

Lanes: **A** Codex readers (1.1, 1.2: `transcript/codex/**`, `tools/codex/list.rs`, `cost/**`), **B** the index (1.4, 1.5: `tools/codex/children.rs`, `notchlingd/src/codex_children*.rs`), **C** Antigravity (1.7: `tools/agy/**`), **D** the shape and the registry (1.3, 1.8, 1.9: `transcript/subagents.rs`, `contract/**`, `app/src/chat/**`), **E** Search (1.10), **F** the name (1.11, 1.12). A and B meet in 1.6; C and D meet in 1.8.

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 1.1, 1.2 | Auto (`transcript/codex/**`); maintainer (`tools/codex/list.rs`, `cost/**`) | none of `CAP-*` | `tools/codex/list.rs` inline test module: no edit, new tests only |
| 1.3 | Auto (`transcript/**`, `app/**`) | `WEAK-TEST-EDIT` on `crates/engine/tests/transcript_subagents.rs` (a struct literal gains fields, no assertion changes) | `crates/engine/tests/transcript_subagents.rs` (tripped); in-crate, not tripped, listed for the reviewer: `app/src/chat/inspector.rs` tests, `subagent_view.rs` tests, `subagent_rows_tests.rs`, `delegated_tests.rs` (literals and `Origin::Codex` becoming `Origin::delegated(Tool::Codex)`) |
| 1.4 | Maintainer (`tools/codex/**`) | `CAP-FILEWRITE` (`fs::create_dir_all` for the index's folder) | none |
| 1.5 | Maintainer (`notchlingd/**`, `local-api/**`) | none beyond 1.4's | none |
| 1.6 | Maintainer (`contract/**`, `tools/codex/**`); auto (`app/**`) | none | none |
| 1.7 | Maintainer (`tools/agy/**`) | none (reads the copy `rows_cached` already makes) | none: new `summaries_children_tests.rs`; `summaries_tests.rs` untouched |
| 1.8 | Maintainer (`contract/**`, `delegate/**`); auto (`app/**`) | none | `app/src/chat/delegated_tests.rs` keeps its assertions: `delegated::subagents` stays as a wrapper (call C14) |
| 1.9 | Auto (`app/**`) | none | in-crate, listed: `app/src/chat/paging` tests unchanged; `inspector.rs` test literal of `SubagentRow` gains `depth`, `children`, `open` |
| 1.10 | Auto (`search/**`) | none | in-crate, listed: `SessionMeta` literals in `search/meta.rs`, `search/index/scan.rs`, `search/fts.rs`, `search/tests.rs`, `search/index/mod.rs`, `search/index/tests.rs` gain `parent: None`; `index/tests.rs` asserting `agy:ag1`, `agy:a`, `agy:b` keys change to `antigravity:` in 1.11 |
| 1.11 | Mixed; maintainer for `notchlingd/**`, `tools/**`, `bridge/**`, `delegate/**` | none | in-crate, listed: id-output assertions (`app/src/tools/foreign.rs:148` `agy:abc`, `search/index/tests.rs`, `bridge/foreign_tests.rs` row ids, `delegate/record_tests.rs:62,128` `agy:c0ffee` and `agy:s2`; `foreign_tests.rs:69` `tool == "agy"` stays, call C31); `crates/notchlingd/tests/*` only if one asserts an `agy:` id it prints (check with `grep -rn '"agy:' crates/*/tests`) |
| 1.12 | Maintainer (`store.rs`, `notchlingd/src/daemon.rs`), call C3 | none of `CAP-*` (SQL updates through `store`) | none |
| 1.13 | Maintainer (`AGENTS.md` files, `docs/feature-map.md`); auto (`docs/provider-matrix.md`) | `ZONE` | none |

## Maintainer only (Phase 1)

- The child index and its op (1.4, 1.5: `CAP-FILEWRITE`, a new SQLite file under `<data>/daemon/`, a new app-tier op).
- `tools/codex`, `tools/agy`, `cost`, `contract` changes.
- The key rewrite (1.12), reviewed on its own (call C3).
- `WEAK-TEST-EDIT` on `crates/engine/tests/transcript_subagents.rs` (1.3) and any `crates/*/tests/` id assertion 1.11 changes.

---

### Task 1.1: The first `session_meta` decides; a child is not a session

**Files:**
- Modify: `crates/engine/src/transcript/codex/mod.rs` (`read_meta` keeps the first `session_meta`, not the last; new `lineage`), `crates/engine/src/tools/codex/list.rs` (`session` answers `None` for a child; new `child`)
- Test: `crates/engine/src/transcript/codex/lineage_tests.rs` (new), `crates/engine/tests/codex_children.rs` (new) with fixtures `crates/engine/tests/fixtures/codex-children/` (new: a parent rollout and a child rollout with the forked block, made up)

**Interfaces:**
- Produces: `transcript::codex::Lineage { pub parent: String, pub depth: u8, pub nickname: Option<String>, pub role: Option<String>, pub agent_path: Option<String> }`; `pub fn lineage(path: &str) -> Option<Lineage>` (read from the first `session_meta` line's `parent_thread_id`, else `source.subagent.thread_spawn.parent_thread_id`; `thread_source: "subagent"` with neither is a child of unknown parent, `parent` empty; cached by `Stamp` like `meta`); `tools::codex::list::child(path: &Path) -> Option<(Agent, Lineage)>`.
- `Meta` keeps its fields (call C29): no struct literal changes.

- [ ] **Step 1: Write the failing tests.** `lineage_tests.rs`: `the_first_session_meta_names_the_child` (a child file: `meta(path).id` is the child's thread, not the parent's), `a_top_level_rollout_has_no_lineage`, `lineage_reads_the_spawn_source_when_the_top_field_is_missing`, `lineage_is_read_once_per_stamp`. `tests/codex_children.rs`: `a_child_rollout_is_not_a_session` (`list::session(child) == None`, `list::child(child)` gives the parent's thread and depth 1), `a_parent_is_still_a_session`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine lineage_tests` and `cargo test -p notchling-engine --test codex_children`; expected FAIL (a child reports the parent's id; `lineage` missing).
- [ ] **Step 3: Implement.** `read_meta` stops at the first `session_meta` for the id and cwd, and takes the title from the child's own first `user_message` after `own_start` (Task 1.2 fills `own_start`; until then 0). `list::rollouts` keeps listing files; `session` returns `None` when `lineage` is `Some`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine`. Existing callers (`app/src/notch/source.rs` foreign rows, `notchlingd/src/git_ops.rs`, `codex_ops.rs`) now skip children: run `cargo test -p notchling-app -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "codex: the first session_meta decides; a child rollout is not a session"`.

### Task 1.2: Every Codex reader skips the forked block

**Files:**
- Create: `crates/engine/src/transcript/codex/fork.rs`
- Modify: `transcript/codex/mod.rs` (the parser and `turn_open` start at `own_start`), `transcript/codex/glance.rs` (`peek`, `context_tokens`), `crates/engine/src/cost/other.rs` and its Codex caller in `cost/tracker.rs` (skip bytes before `own_start`), `crates/engine/src/search/index/read.rs` (Codex text from `own_start`)
- Test: `crates/engine/src/transcript/codex/fork_tests.rs`, `crates/engine/src/cost/fork_tests.rs` (new)

**Interfaces:**
- Produces: `transcript::codex::fork::own_start(path: &str) -> u64`: 0 for a top-level rollout; for a child, the byte offset of the first `task_started` after the second `session_meta`; read from at most the first 3 MB, cached by `Stamp`; a child whose own turn is not in the first 3 MB yet answers the offset after the last line read (nothing of the fork shows).
- Every reader of a Codex file that reads a tail compares its window start with `own_start` and drops lines before it.

- [ ] **Step 1: Write the failing tests** (fixtures from 1.1, plus one where the forked block holds two `token_count` lines and a `task_complete`):
  - `fork_tests.rs`: `a_top_level_rollout_starts_at_zero`, `a_child_starts_at_its_own_task_started`, `the_childs_conversation_has_only_its_own_turn` (the parsed conversation's first item is the child's task, no parent message), `turn_open_ignores_the_forked_turn_aborted` (a child whose own turn is running reads open), `glance_ignores_the_forked_token_count`, `a_window_starting_inside_the_fork_drops_the_rest_of_it` (a tail window whose start lies before `own_start`).
  - `cost/fork_tests.rs`: `a_forked_block_is_not_counted_twice` (parent plus child: tokens equal the parent's plus the child's own).
  - A search test in `search/index/tests.rs`'s sibling, new file `search/index/fork_tests.rs`: `a_childs_text_has_no_parent_words`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine fork_tests`; expected FAIL.
- [ ] **Step 3: Implement** `own_start` and the cuts.
- [ ] **Step 4: Run** `cargo test -p notchling-engine` and `cargo test -p notchling-app`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "codex: readers start at a child's own turn; the forked block counts nowhere"`.

### Task 1.3: `Subagent` in one shape

**Files:**
- Modify: `crates/engine/src/transcript/subagents.rs`, every constructor and reader of `Subagent` and `Origin`: `crates/app/src/chat/delegated.rs`, `subagent_rows.rs`, `loader.rs`, `inspector.rs`, `subagent_view.rs`, `cleared.rs`
- Test: `crates/engine/src/transcript/subagents_shape_tests.rs` (new); the literal edits listed in the zones table

**Interfaces:**
- Produces (spec 8.1): `pub struct Origin { pub tool: Tool, pub via: Via }`, `pub enum Via { Native, Delegated }`, `Origin::native(Tool)`, `Origin::delegated(Tool)`, `Default` is `native(Tool::Claude)`; `Subagent` gains `session: String`, `parent: String`, `depth: u8` and its `id` becomes the row id (`claude:<agent id>`, `codex:<thread>`, `antigravity:<conversation>`, `job:<delegation id>`); `Subagent::own_id(&self) -> &str` (after the first `:`); `Subagent::as_parent(&self) -> SessionRef` (Phase 1.8). `status_of` and `list` look up `finished` by `own_id()`. `peek(origin: Origin, ...)` matches on `origin.tool`.
- The old enum's three variants map: `Claude` to `native(Claude)`; `Codex` and `Antigravity` (used only for delegated rows on the base) to `delegated(..)`. `subagent_rows.rs`: `handed_over` becomes `origin.via == Via::Delegated`; the mark comes from `origin.tool`.

- [ ] **Step 1: Write the failing tests** in `subagents_shape_tests.rs`: `claude_rows_carry_their_session_and_depth_one` (over `crates/engine/tests/fixtures` Claude subagent folder copied to a temp root: `session` is the transcript's id, `parent == session`, `depth == 1`, `id` starts `claude:`), `status_comes_from_the_finished_map_by_the_own_id`, `the_default_origin_is_native_claude`. In the app, `subagent_rows_tests.rs` gains `a_native_codex_child_is_not_handed_over` (new test, existing ones only get literal changes).
- [ ] **Step 2: Run** `cargo test -p notchling-engine subagents_shape_tests`; expected FAIL (no such fields).
- [ ] **Step 3: Implement**, then fix every literal (`cargo build --workspace --all-targets` lists them). Other `Origin` types in `install/command.rs`, `bridge/extras.rs`, `transcript/chat/images.rs` are unrelated: leave them.
- [ ] **Step 4: Run** `cargo test --workspace`, `cargo test -p notchling-app --release --test subagent_reads`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "subagents: one shape, origin is the tool and the way it started"`.

### Task 1.4: The Codex child index

**Files:**
- Create: `crates/engine/src/tools/codex/children.rs`
- Modify: `crates/engine/src/tools/codex/mod.rs`
- Test: `crates/engine/src/tools/codex/children_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Head { pub path: PathBuf, pub thread: String, pub parent: Option<String>, pub depth: u8, pub nickname: Option<String>, pub role: Option<String>, pub started: f64 }`; `pub fn head(path: &Path) -> Option<Head>` (reads at most `HEAD_BYTES = 65_536`, the first `session_meta` only, through `lineage`'s parser).
  - `pub struct Index` over SQLite: `open(path: &Path) -> Result<Index, Error>` (creates the parent folder, `PRAGMA journal_mode=WAL`, the table `rollouts(path TEXT PRIMARY KEY, size INTEGER, mtime REAL, thread TEXT, parent TEXT, depth INTEGER, nickname TEXT, role TEXT, started REAL)` and `CREATE INDEX rollouts_parent ON rollouts(parent)`), `open_in_memory()`, `stamp(&self, path) -> Option<(u64, f64)>`, `note(&self, head: &Head, size: u64, mtime: f64)`, `children(&self, thread: &str) -> Vec<Head>`, `is_child(&self, thread: &str) -> bool`, `sweep(&self, day: &Path, present: &[PathBuf]) -> usize`.
  - `pub fn refresh(index: &Index, paths: &[PathBuf]) -> Refreshed` with `Refreshed { pub read: usize, pub kept: usize }`: a path whose size and mtime match the row is kept unread.
  - `pub fn day_folders(sessions: &Path, now_ms: i64, days: u32) -> Vec<PathBuf>` (newest first; reuses `list::days_from_civil`), `BACKFILL_DAYS: u32 = 30`.
  - `pub trait ChildSource: Send + Sync { fn children(&self, parent_thread: &str, parent_path: &Path) -> Vec<Head>; }`; `impl ChildSource for Index`; `pub struct DayFolders` (call C2: reads the parent's start day folder and the next one, two at most, with `head` on each file; no cache beyond `head`'s stamp).

- [ ] **Step 1: Write the failing tests** (temp folders, rollouts written by the test): `a_top_level_rollout_is_a_row_with_no_parent`, `children_are_found_by_the_parent_thread`, `a_changed_rollout_is_read_once_and_an_unchanged_one_never` (`refresh` twice: `read` 2 then 0; append to one: `read` 1), `a_gone_rollout_is_swept_when_its_day_is_read_again`, `a_head_past_64_kb_is_not_read` (a first line over 64 KB: no row, no panic), `the_backfill_reads_thirty_days_newest_first`, `day_folders_reads_two_folders_at_most` (`DayFolders` over a parent started at 23:59 finds a child written the next day, and never opens a third folder).
- [ ] **Step 2: Run** `cargo test -p notchling-engine tools::codex::children`; expected FAIL.
- [ ] **Step 3: Implement.** No lock is held across a file read (`crates/AGENTS.md`): `refresh` reads heads first, then writes rows in one transaction.
- [ ] **Step 4: Run** `cargo test -p notchling-engine`, `cargo clippy -p notchling-engine --all-targets -- -D warnings`. Expected: PASS. `python3 -m guard lint` reports `CAP-FILEWRITE` on the `create_dir_all`: expected, maintainer.
- [ ] **Step 5: Commit** `git commit -m "codex: a child index keyed by rollout path, size and mtime"`.

### Task 1.5: The daemon keeps the index; `codex.children`

**Files:**
- Create: `crates/notchlingd/src/codex_children.rs`, `crates/notchlingd/src/codex_children_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::CODEX_CHILDREN = "codex.children"`), `crates/notchlingd/src/daemon_wiring.rs` (an `OpGroup` added with `Ops::with_group`; started in `Wired::start`, stopped on close), `docs/local-api-ops.md` (the op's shape)

**Interfaces:**
- Produces: `CodexChildren::new(sessions: PathBuf, db: PathBuf) -> CodexChildren`, `start(&self)` (a thread: `platform::watch` on `sessions`, each batch's `.jsonl` paths go to `refresh`; then the backfill over `day_folders(.., BACKFILL_DAYS)` newest first, sweeping each day; every 5 s nothing unless a batch came), `close(&self)`; `OpGroup` serving `codex.children {"thread": "<id>"}` → `{"children": [{"path", "thread", "parent", "depth", "nickname", "role", "started"}]}`, `OpSpec::APP_QUIET` (asked on every load; successes not audited), no switch (read only, like `git.*`), an unknown or empty thread answers `{"children": []}`. `is_child(thread)` is public for Phase 2's session list.
- The db is `<data>/daemon/codex-children.db`; deleting it costs one backfill.

- [ ] **Step 1: Write the failing tests** in `codex_children_tests.rs` (a real server in a short `/tmp` folder, a temp `sessions` root, as `server_tests.rs` does): `children_are_answered_for_a_thread`, `the_op_is_app_tier_and_quiet` (an agent-tier client is refused; `audit.jsonl` gains no line for a success), `a_rollout_written_after_start_is_found` (write a child, wait for the batch with a 5 s deadline), `a_missing_db_folder_is_made`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd codex_children`; expected FAIL.
- [ ] **Step 3: Implement**; the watcher thread holds no lock across a read.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd -p notchling-local-api`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "daemon: the Codex child index and codex.children"`.

### Task 1.6: The Codex `Subagents` port; the app asks the daemon

**Files:**
- Create: `crates/engine/src/contract/subagents.rs` (the port trait, `SessionRef`, `Roots`, `Glance`), `crates/engine/src/contract/codex/subagents.rs`, `crates/engine/src/contract/conformance_subagents.rs`, `crates/app/src/chat/codex_children.rs`, fixtures `crates/engine/tests/fixtures/conformance/codex/` (an idle session, a working session, a session with one running and one finished child; the finished child's fork holds a `turn_aborted`)
- Modify: `contract/provider.rs` (`fn subagents(&self) -> &dyn Subagents`, default the shared `Unported`, call C6), `contract/codex/support.rs` (`subagent.list`, `subagent.status`, `subagent.glance`, `subagent.open` become Native), `contract/codex/mod.rs` (`WIRED` gains the four), `gaps.json` (−4), `contract/mod.rs` (`conformance::check` sends `Subagent*` to `conformance_subagents`)
- Test: `crates/engine/src/contract/codex/subagents_tests.rs`, `crates/app/src/chat/codex_children_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct SessionRef { pub id: String, pub tool: Tool, pub transcript: Option<PathBuf>, pub name: String, pub finished: Arc<HashMap<String, String>>, pub session: String }` (`session` is the top session's row id; equal to `id` at depth 0).
  - `pub struct Roots { pub sessions: sessions::Roots, pub codex_sessions: PathBuf, pub antigravity: PathBuf, pub antigravity_copy: PathBuf, pub delegations: PathBuf }`.
  - `pub struct Glance { pub tokens: Option<u64>, pub doing: Option<String> }`.
  - `pub trait Subagents: Send + Sync { fn list(&self, parent: &SessionRef, roots: &Roots, now: Timestamp) -> Result<Vec<Subagent>, Unsupported>; fn glance(&self, s: &Subagent) -> Result<Glance, Unsupported>; fn open(&self, s: &Subagent) -> Result<Arc<Conversation>, Unsupported>; }`, every method defaulting to `Unsupported::not_native(..)`.
  - `CodexSubagents::new(source: Arc<dyn ChildSource>)`; status from the child's tail after `own_start`: Running (open turn, written in the last 10 min), Completed (`task_complete` with `error` null), Failed (`error` set), Stopped (`turn_aborted`), Quiet (open, silent 10 min); description: nickname and role, else the first line of the child's first task; `glance` through `codex::peek`; `open` through `codex::read_shared` (which starts at `own_start`).
  - App: `DaemonChildren` implements `ChildSource` by `codex.children` through the session source's client (the loader is off the main thread), and falls back to `DayFolders` when no daemon answers (a fixture root, a `--render`), call C2.
- Declaration lines: Native `how` for Codex `subagent.list` `child rollouts whose first session_meta names the parent`, `subagent.status` `the child's task_complete, error or turn_aborted`, `subagent.glance` `the child's token_count and newest call`, `subagent.open` `the rollout from the child's own turn`.

- [ ] **Step 1: Write the failing tests:** `subagents_tests.rs`: `two_children_with_their_statuses` (Running and Completed, `origin == native(Codex)`, ids `codex:`, `parent` set), `a_failed_child_says_failed`, `an_aborted_child_says_stopped`, `glance_has_tokens_and_a_doing_line_for_the_running_one`, `open_shows_only_the_childs_turn`. `conformance_subagents.rs` holds the shared expectations (spec 11.3) as `pub fn check_list/check_glance/check_open(registry, tool, root) -> Checked`; with the four cells wired, `cargo test -p notchling-engine --test conformance codex::subagent_` runs them. `codex_children_tests.rs`: `no_daemon_reads_two_day_folders`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::codex` and `--test conformance`; expected FAIL.
- [ ] **Step 3: Implement**; edit `support.rs`, `WIRED`, `gaps.json` in this commit.
- [ ] **Step 4: Run** `cargo test -p notchling-engine` (registry tests and conformance), `cargo test -p notchling-app`. Expected: PASS; conformance summary shows `wired 4`.
- [ ] **Step 5: Commit** `git commit -m "codex: subagents of a session, from the child index"`.

### Task 1.7: The Antigravity `Subagents` port

**Files:**
- Create: `crates/engine/src/contract/antigravity/subagents.rs`, fixtures `crates/engine/tests/fixtures/conformance/antigravity/` (brain folders with `.system_generated/logs/transcript.jsonl` for a parent and two children; the summaries db is built by the test from a SQL file `summaries.sql` in that folder)
- Modify: `crates/engine/src/tools/agy/summaries.rs` (new `children`), `crates/engine/src/tools/agy/elsewhere.rs` (new `peek`), `contract/antigravity/support.rs`, `contract/antigravity/mod.rs` (`WIRED`), `gaps.json` (−4)
- Test: `crates/engine/src/tools/agy/summaries_children_tests.rs` (new), `crates/engine/src/tools/agy/elsewhere_peek_tests.rs` (new), `crates/engine/src/contract/antigravity/subagents_tests.rs`

**Interfaces:**
- Produces: `summaries::ChildRow { pub id: String, pub agent_name: String, pub status: String, pub killed: bool, pub depth: u8, pub modified: f64 }`, `pub fn children(copy: &Path, parent: &str) -> Vec<ChildRow>` (`SELECT … WHERE parent_conversation_id = ?1 ORDER BY last_modified_time`, over the copy `rows_cached` keeps; a missing column answers empty); `elsewhere::peek(path: &Path, with_recent: bool) -> Option<Peeked>` (last `PLANNER_RESPONSE`'s tokens, newest tool call or content; reads at most the last 256 KB).
- `AntigravitySubagents`: status `killed` → Stopped; `RUNNING` → Running; `IDLE` with a `send_message` call in the child's log or a parent `[Message] … sender=<child>` step → Completed; an `ERROR_MESSAGE` last step → Failed; `IDLE` otherwise → Quiet; description: the role from the parent's `invoke_subagent` call when the parent's log is at hand, else `agent_name`; `glance` through `elsewhere::peek`; `open` through `elsewhere::from_log_file`.
- Lines: `subagent.list` `summaries rows whose parent_conversation_id is the session`, `subagent.status` `summaries status and killed, send_message, the parent's [Message]`, `subagent.glance` `the child log's tokens and newest step`, `subagent.open` `the child's log`.

- [ ] **Step 1: Write the failing tests:** `summaries_children_tests.rs`: `children_of_a_conversation`, `a_child_of_a_child_is_found_by_its_own_id`, `an_old_db_without_the_column_answers_none`. `elsewhere_peek_tests.rs`: `peek_reads_tokens_and_the_newest_step`, `peek_of_a_missing_log_is_none`. `subagents_tests.rs`: `a_killed_child_is_stopped`, `idle_after_send_message_is_completed`, `idle_with_neither_is_quiet`, `an_error_step_is_failed`, `two_children_with_their_statuses` (shared expectation).
- [ ] **Step 2: Run** `cargo test -p notchling-engine tools::agy contract::antigravity`; expected FAIL.
- [ ] **Step 3: Implement**; the four cells Native and wired, `gaps.json` −4.
- [ ] **Step 4: Run** `cargo test -p notchling-engine` (conformance `antigravity::subagent_*` run). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "antigravity: child conversations are subagents"`.

### Task 1.8: `Registry::subagents`, the Claude Code port and `delegate-jobs`

**Files:**
- Create: `crates/engine/src/contract/claude/subagents.rs`, `crates/engine/src/contract/flows/mod.rs`, `crates/engine/src/contract/flows/delegate_jobs.rs`, fixtures `crates/engine/tests/fixtures/conformance/claude/`
- Modify: `contract/registry.rs` (`subagents`, `glance`, `open`), `contract/claude/mod.rs` (`WIRED` gains the four `subagent.*`), `unwired.json` (−4), `crates/app/src/chat/data.rs` (`ChatData::subagents` calls the registry), `crates/app/src/chat/delegated.rs` (its functions become wrappers over `flows::delegate_jobs`, call C14), `crates/app/src/chat/loader.rs` (one registry per loader, built with `DaemonChildren`)
- Test: `crates/engine/src/contract/registry_subagents_tests.rs`, `crates/engine/src/contract/flows/delegate_jobs_tests.rs`

**Interfaces:**
- Produces: `Registry::with_sources(children: Arc<dyn ChildSource>) -> Registry`; `Registry::subagents(&self, top: &SessionRef, roots: &Roots, now: Timestamp) -> Vec<Subagent>` (spec 6.4: native rows by declaration, plus `delegate_jobs` for every parent, sorted by `started`, `seen` by id, recursing through `Subagent::as_parent`, `MAX_DEPTH: u8 = 10`); `Registry::glance(&self, s: &Subagent) -> Option<Glance>` and `open(&self, s: &Subagent) -> Option<Arc<Conversation>>` route by `s.origin.tool` (a job by its tool). `flows::delegate_jobs::subagents(records: &[Delegation], parent: &SessionRef, roots: &Roots, now, alive: &dyn Fn(u32) -> bool) -> Vec<Subagent>`; `parent_ids` (the `/clear` and continued predecessors, `MAX_BACK` 4) moves with it.
- `ClaudeSubagents` wraps `subagents::list` plus `cleared::running_before` (the latter moves from the app into `transcript::cleared` if it is pure; otherwise the app passes the predecessors in `SessionRef`).
- `ChatData::subagents(session, conversation, delegations, now)` keeps its signature and returns `registry.subagents(..)`; it now answers for every tool.

- [ ] **Step 1: Write the failing tests:** `registry_subagents_tests.rs`: `a_codex_child_with_a_job_of_its_own_is_a_tree` (depths 1 and 2, `parent` of the job is the child), `a_loop_is_listed_once`, `depth_stops_at_ten`, `a_gap_without_a_flow_lists_only_jobs`, `glance_routes_by_the_rows_tool`. `delegate_jobs_tests.rs`: the six cases of `app/src/chat/delegated_tests.rs` ported as new engine tests (the app file stays and keeps passing through the wrappers).
- [ ] **Step 2: Run** `cargo test -p notchling-engine registry_subagents_tests flows`; expected FAIL.
- [ ] **Step 3: Implement**; cells wired, `unwired.json` −4; `cargo test -p notchling-engine --test conformance claude::subagent_` runs.
- [ ] **Step 4: Run** `cargo test --workspace`, then on the Mac `cargo test --release --locked -p notchling-app --test bench_subagents -- --ignored --nocapture --test-threads=1`: `parsed_bytes` at most 1 MB. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "contract: one subagent list for every session, native and delegated, as a tree"`.

### Task 1.9: The Details Panel's tree

**Files:**
- Modify: `crates/app/src/chat/paging.rs` (a `Tree` over `Page`), `crates/app/src/chat/subagent_rows.rs` (`SubagentRow` gains `depth: u8`, `children: usize`, `open: bool`), `crates/app/src/chat/loader.rs` (`open_groups: HashSet<String>` in the load request; only shown rows are glanced), `crates/app/src/chat/inspector.rs` (`DetailsModel` carries the tree), `crates/app/src/chat/view/inspector.rs` and `crates/app/src/chat/view/paged_rows.rs` (indent by depth with `metric` tokens, a disclosure for a row with children, a per-group Load N More), `crates/app/src/chat/view/mod.rs` (`toggle_group`, which asks for a load)
- Test: `crates/app/src/chat/tree_tests.rs` (new)

**Interfaces:**
- Produces: `paging::Tree::show(&mut self, session: Option<&str>, rows: &[Subagent], open: &HashSet<String>, more: &HashMap<String, usize>) -> Vec<Shown>` (depth 1 paged as today: every running row, else the 3 newest, then Load N More by 10; a row with children starts collapsed; an open row shows its children under it, paged the same way; collapsed children are counted, never shown); `ChatView::toggle_group(id)`, `ChatView::load_more_under(id)`.
- Sizes and hover of existing rows do not change (memory: UI changes keep hover and size); a nested row is the same row indented.

- [ ] **Step 1: Write the failing tests:** `depth_one_pages_as_before`, `a_row_with_children_starts_collapsed_and_counts_them`, `opening_a_row_shows_its_children_paged`, `a_collapsed_group_is_never_glanced` (the loader's glance list holds only shown rows), `another_session_starts_closed`.
- [ ] **Step 2: Run** `cargo test -p notchling-app tree_tests`; expected FAIL.
- [ ] **Step 3: Implement**; tag the disclosure `.part("Subagent Group", …)`.
- [ ] **Step 4: Run** `cargo test -p notchling-app`, `cargo clippy -p notchling-app --all-targets -- -D warnings` on the Mac, the release `bench_subagents` and `bench_chat`; render `NOTCHLING_RENDER_*` over a fixture with a nested job and look at it in light and dark. Expected: PASS, counts unchanged.
- [ ] **Step 5: Commit** `git commit -m "chat: subagents as a tree in the Details Panel, collapsed past depth one"`.

### Task 1.10: Search files a child under its parent

**Files:**
- Modify: `crates/engine/src/search/meta.rs` (`SessionMeta.parent: Option<String>`, `#[serde(default)]`), `search/index/scan.rs` (a Codex candidate whose `lineage` is `Some` is indexed with `parent`), `search/index/read.rs` (the child's text from `own_start`), `search/index/mod.rs`, `crates/app/src/search/` (a hit with a parent opens the parent's row and selects the subagent row, as delegated hits do)
- Test: `crates/engine/src/search/index/children_tests.rs` (new), `crates/app/src/search/children_tests.rs` (new)

**Interfaces:**
- Produces: `SessionMeta { …, parent: Option<String> }`; a child's hit carries `parent` (the parent's row id) and its own id; the app's open action takes `(row: parent, select: Some(child id))`.

- [ ] **Step 1: Write the failing tests:** `a_child_is_indexed_under_its_parent`, `a_child_hit_opens_the_parent_with_the_row_selected`, `old_rows_without_a_parent_still_read` (a stored `SessionMeta` JSON without the key).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::index::children_tests`, `cargo test -p notchling-app search::children_tests`; expected FAIL.
- [ ] **Step 3: Implement**; fix the `SessionMeta` literals listed in the zones table with `parent: None`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine -p notchling-app`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "search: a Codex child is a subagent hit under its parent"`.

### Task 1.11: The wire name `antigravity`, read both ways

**Files:**
- Create: `crates/engine/src/contract/ids.rs`
- Modify: every writer of an `agy:` id or `agy` tool name: `crates/engine/src/tools/agy/summaries.rs:30` (`row_id`), `crates/engine/src/bridge/foreign.rs` (`Tool::Agy` prefix `antigravity:`; `parse` accepts both), `crates/engine/src/delegate/record.rs:91` (`row_id`), `crates/engine/src/search/index/mod.rs:220`, `search/meta.rs` (`SessionTool` key `antigravity`, reads `agy`), `crates/notchlingd/src/agy_ops.rs:397`, `crates/notchlingd/src/agy_sink.rs:35`, `crates/app/src/tools/requests.rs` (`AGY_PREFIX`, `AGY_DRAFT_PREFIX` become `antigravity:` and `antigravity-new:`), `crates/app/src/tools/agy_context.rs:9`, `crates/app/src/chat/sidebar.rs:108,251,256`, `crates/app/src/chat/used.rs`, `crates/app/src/notch/source.rs:286`, `crates/app/src/chat/data.rs` (`Tool::of` through `contract::Tool::of_row`)
- Test: `crates/engine/src/contract/ids_tests.rs`, `crates/app/src/chat/ids_tests.rs` (new)

**Interfaces:**
- Produces: `contract::ids::normalize(row_id: &str) -> Cow<'_, str>` (`agy:` to `antigravity:`, `agy-new:` to `antigravity-new:`, anything else unchanged); `ids::same(a: &str, b: &str) -> bool`. Every place that compares a stored id (settings lists `chatClosedSessions`, `chatUsedSessions`, `hiddenAgents`, `dismissed`, `chatSelection`; the `drafts` table; Search keys; the phone's prompt ids `agy-perm:agy:<id>:<rpc>`) normalizes on load. The daemon's ops accept `agy:` and `antigravity:` ids for Antigravity sessions; replies carry `antigravity:`.
- The phone protocol's `Session.tool` keeps the value `agy` (call C31): its schema documents `claude, codex or agy`, no phone reads it, and protocol changes are additive only. Row ids on the phone become `antigravity:` like everywhere; the `agy-perm:` prompt prefix stays (it is opaque to the phone).

- [ ] **Step 1: Write the failing tests:** `ids_tests.rs`: `agy_ids_normalize_to_antigravity`, `other_ids_are_unchanged`, `same_compares_across_the_names`. App `ids_tests.rs`: `a_stored_agy_id_matches_the_new_row` (a closed-sessions list holding `agy:c1` hides the row `antigravity:c1`; a draft saved under `agy:c1` loads for it), `a_selection_saved_as_agy_reopens`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::ids_tests`, `cargo test -p notchling-app ids_tests`; expected FAIL.
- [ ] **Step 3: Implement**; then `grep -rn '"agy:\|agy-new:\|"agy"' crates/*/src` shows only readers, aliases, the command's name and the phone's `Session.tool`.
- [ ] **Step 4: Run** `cargo test --workspace`; fix the id-output assertions listed in the zones table (they now expect `antigravity:`). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "antigravity: the wire name is antigravity; agy is still read"`.

### Task 1.12: The one-time key rewrite (maintainer-reviewed, call C3)

**Files:**
- Create: `crates/engine/src/contract/rewrite.rs`
- Modify: `crates/notchlingd/src/daemon.rs` (call it after the lock and before `server.spawn()`, beside `Wired::load`)
- Test: `crates/engine/src/contract/rewrite_tests.rs`

**Interfaces:**
- Produces: `pub fn rewrite_antigravity_ids(store: &Store, search_db: Option<&Path>) -> Result<Rewritten, Error>` with `Rewritten { pub settings: usize, pub drafts: usize, pub search: usize, pub skipped: bool }`: in one transaction per database, rewrites `agy:` and `agy-new:` to `antigravity:` and `antigravity-new:` in the settings keys `chatClosedSessions`, `chatUsedSessions`, `hiddenAgents`, `dismissed`, `chatSelection` (JSON values parsed and written back, other entries untouched), the `drafts.row_id` column (a row whose new id already exists keeps the newer `updated_ms`), and `search.db` keys; then writes the marker `meta('antigravityIds', '1')`. With the marker present it does nothing (`skipped`). An error is logged and the daemon starts anyway: the readers normalize (1.11), so the rewrite is clean-up, never needed for correctness.

- [ ] **Step 1: Write the failing tests** (a store in a temp folder): `the_rewrite_runs_once_and_keeps_every_value`, `a_second_run_is_skipped_by_the_marker`, `a_draft_under_both_names_keeps_the_newer`, `other_ids_and_keys_are_untouched`, `a_missing_search_db_is_fine`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::rewrite_tests`; expected FAIL.
- [ ] **Step 3: Implement**; the daemon logs `Rewritten` counts (no ids) and goes on.
- [ ] **Step 4: Run** `cargo test -p notchling-engine -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "daemon: rewrite stored agy ids to antigravity once"`. The pull request names this task for the maintainer's review.

### Task 1.13: Matrix, notes, feature map

**Files:**
- Modify: `docs/provider-matrix.md` (regenerated), `crates/AGENTS.md` (Details Panel rows: the tree; Data: the child index; "Things that bit before": Codex's forked block and the last `session_meta`), `crates/engine/AGENTS.md` (`contract/subagents.rs`, `flows/`, `tools/codex/children.rs`, `transcript/codex/fork.rs`), `crates/notchlingd/AGENTS.md` (`codex_children.rs`), `crates/app/AGENTS.md` (`codex_children.rs`, the tree), `docs/local-api-ops.md`, `docs/feature-map.md` (rows for Chat's subagents and the child index)

- [ ] **Step 1: Regenerate** `NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-engine --test provider_matrix`; read the diff: 8 cells from `GAP:` to `N:`, 4 Claude Code cells lose ` · unwired`.
- [ ] **Step 2: Write** the notes.
- [ ] **Step 3: Run** `PYTHONPATH=tools/guard python3 -m guard claudemd`, `python3 tools/strip_comments.py --check`. Expected: PASS.
- [ ] **Step 4: Commit** `git commit -m "docs: subagents in one shape, the child index, the matrix"`.

## Checkpoint P1 (before the pull request)

- `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings` (and on the Mac), `cargo fmt --all --check`, `cargo deny check`.
- Conformance summary: `wired 4` (Claude Code) plus the 8 new Native cells, `gaps 26`, `unwired 68`.
- On the Mac: `bench_subagents` (`parsed_bytes` at most 1 MB), `bench_chat` counts unchanged.
- By hand over a copy of a real Codex session folder in a fixture root (copied to `/tmp`, never printed): the child is gone from the Sidebar and the notch, shows under its parent with its status, opens to its own turn.
- `GUARD-PROVIDER-GAPS` passes (both files shrank). The pull request lists `CAP-FILEWRITE` (1.4), the `WEAK-TEST-EDIT` (1.3) and the key rewrite (1.12) for the maintainer.
