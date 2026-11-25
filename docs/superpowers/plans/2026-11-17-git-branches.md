# Git Branches Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Details Panel's Git section shows one line of numbers per worktree of the session's repository (this session's first, a running dot where a session or subagent works, a "main and 39 branches ›" row), and "›" opens a read-only Branches panel (Working, Not Merged, Merged; a filter; Worktrees and No Worktree groups; Changes, Open Session, Show in Finder) in the slot Changes uses. Every number is against the branch's nearest base.

**Architecture:** The engine's `git/` gains pure parsers (worktree list, refs, counts, shortstat, remote hosts), the nearest-base and branch-row rules, and a `Reader` that runs the new read-only commands through the existing sandboxed `command`/`run`. The daemon's `GitOps` gains two app-tier quiet ops (`git.worktrees`, `git.branch_list`), optional `worktree`/`ref`/`pick` arguments on the existing ops, an argument guard checked against what `git` lists, and in-memory caches (numbers between two commits, each worktree's work) whose lock never covers a `git` run. The app builds pure models from the replies and the live session list, asks through two more gates, redraws the Git section, gives Changes a target, and adds a `BranchesPanel` entity.

**Tech Stack:** Rust (engine, notchlingd, local-api, kit, GPUI app), git 2.41+ (with a `rev-list` fallback), Fluent locale files.

**Spec:** `docs/superpowers/specs/2026-11-17-git-branches-design.md` (approved 2026-11-17) and its mockup `2026-11-17-git-branches-mockup.html`. Every implementer reads the sections its task names. Builder's calls B1 to B25 are settled and not reopened. The maintainer's seven answers are in the spec's "Needs the maintainer" and the ledger's first ruling; all were "as recommended", and answer 2 (every row's right side is vs its base; the current row's uncommitted part moves into its sub-line) is written into 7.1, 7.2 and the mockup.

**Base and branch:** build on `agent/git-branches` from the latest `origin/agent-run-oss`. Never commit to `agent-run-oss` or `main`; never force-push; agents never merge. Log each task's commit and review in `2026-11-17-git-branches.ledger.md` (append-only).

## Decisions this plan makes (within the spec)

- **D1 Merged from the counts.** A branch is merged into a base exactly when it is 0 ahead of it, which is what `for-each-ref --merged` and `merge-base --is-ancestor` answer. The counts are read anyway for the nearest base, so the plan runs neither command (fewer processes, a shorter allowlist).
- **D2 Delegated jobs' folder.** The delegation record has no `folder` (only `parent_cwd`, the caller's folder). A delegated Codex job is placed by its rollout's `session_meta` `cwd`, as a Codex child is; a delegated Antigravity job has no glance and is not placed in v1 (its worktree still shows "last … ago").
- **D3 Subagent folders travel with the glance.** `Subagent` already has `started`; its folder is read with the glance the loader already makes off the main thread (`Glance.cwd` → `Facts.cwd` → `SubagentRow.cwd`), so `Subagent` and its literals stay as they are.
- **D4 Bases are revs.** Every `base` in a reply is a rev git accepts (`origin/main`, `agent-run-oss`, `origin/agent-run-oss`). The app shows a base by its short name with `worktrees::shown_base` (strips `origin/` from the default only).
- **D5 No common history.** `%(ahead-behind)` and `rev-list --left-right --count` still print counts for unrelated histories (git 2.54: `1 1`), so a pick made from counts alone is unconfirmed. A merged pick (0 ahead) needs no check: 0 ahead means the head is in the base. Any other pick is confirmed before its numbers are used: in `git.branch_list` by the row's own `lines` read (`diff --shortstat` exits 128 "no merge base", so `Reader::lines` returns `None`), and before a worktree's work read or a `pick: "nearest"` stat by `Reader::related` (`merge-base <base> <head>` exits 1). Both answers are cached by the two commits. An unrelated mark is sticky (`Numbers::mark_unrelated`; a later `put_counts` never clears it), and the row is picked again without that candidate. A row that had candidates of which none is related has `base: null` and `unrelated: true` (an added reply field) and shows "no common history".
- **D6 `Agent.worktree_path`.** It breaks every `Agent { … }` literal (about 54, the compiler lists them); Task 6 adds `worktree_path: None` to each. See Merge notes.

## Global Constraints

- Packages: `notchling-engine`, `notchling-notchlingd`, `notchling-local-api`, `notchling-kit`, `notchling-app`. One test filter per `cargo test` run.
- No comments in code, doc comments included (`python3 tools/strip_comments.py --check`); the why goes in the crate's `AGENTS.md` (Task 19).
- Files under 1,000 lines (600 warns): `inspector.rs` (572), `view/mod.rs` (572), `changes_panel.rs` (527) gain only the few lines named; new code goes in new files. Engine tests in `git/<module>_tests.rs` registered in `git/mod.rs`; daemon tests in `<module>_tests.rs` registered in `lib.rs`; app tests `#[path = "<module>_tests.rs"]`.
- The app runs no process and reads no repository. Every `git.*` call is off the UI thread (`cx.background_executor()`), its answer goes into a model as an `Arc`, and an unchanged one is dropped before any part hears of it. A model compute and a draw do no file I/O (`chat/worktrees.rs`, `chat/branch_list.rs`, `chat/changes_target.rs`, `chat/git_gates.rs` take data only).
- Read only, sandboxed: every new command goes through `git/read.rs::command` (`--no-optional-locks`, the nulled `-c` options, the filter drivers blanked) and `run`/`run_capped` (3 s, capped). Diffs carry every `DIFF_FLAGS` option (`--no-color --no-ext-diff --no-textconv --ignore-submodules=all -M`) and a trailing `--`. A rev is either a 40-hex commit from git's own output or a name that passed `clean_ref`, and always comes before `--`.
- **The allowlist** (Task 12's test): subcommands `config` (only `-z --get-regexp`), `status`, `diff`, `rev-parse`, `merge-base`, `symbolic-ref`, `for-each-ref`, `worktree` (only `list`), `rev-list`, `log`. Never `fetch`, `pull`, `push`, `remote`, `ls-remote`, `gc`, `worktree add|remove|prune|repair|move|lock|unlock`.
- Values (spec B11 to B13, 7.9): `MAX_WORKTREES = 50`, `SHOWN_WORKTREES = 6`, `MAX_REFS = 300`, `SUBJECT_CHARS = 120`, `NUMBERS_CAP = 2_000` (least recently used), `IDLE_DROP_MS = 600_000`, `LIST_FOR_MS = 5_000`, `STALE_MS = 15_000`, `MISSING_READS = 4`, `FRESH_READS = 2`, `READ_STOP_MS = 1_500`, `LINES_BUDGET_MS = 1_000`, `SLOW_EVERY_MS = 15_000`, `PENDING_AGAIN_MS = 2_000`; the existing `EVENT_FLOOR_MS = 1_000`, `EVERY_MS = 5_000`, `MAX_FILES = 500`.
- The local API changes additively: two new ops, new optional arguments and reply fields; `git.diff` stays. Both new ops are `OpSpec::APP_QUIET` and `Home::Quiet`. `agentconsole.v1` and `protocol/` do not change.
- A remote's URL is parsed for its host inside `Reader::remotes` and dropped there: it is in no reply, error, log or Audit line.
- Tests never touch the real `~/.claude`, `~/.codex`, Keychain or the network. Git tests use real temp repositories (`branch_real_tests.rs`'s `git()` helper: `GIT_CONFIG_GLOBAL=/dev/null`, no network) or `FakeLauncher`.
- Words in `locales/en/mac/chat.ftl`, in a new `## Details Panel, Branches` block placed right after `chat-changes-uncommitted` (not at the end of the file, where other lanes add theirs); `python3 tools/locale/gen.py` then `--check`. A message with variables is formatted in a model compute, never in a draw. Buttons and tabs in Title Case.
- Never run `./build.sh --install` or `tools/bundle-next.sh --install`; only the maintainer installs and merges.

## Review Focus

1. **Changes on another worktree or branch, then back.** Open Changes from a lane row, pick another base there, close it: the "this session" row keeps its own numbers and base the whole time, and Changes for the session again uses its own base choice. (Task 14 `closing_other_target_keeps_own_numbers`, `other_target_base_choice_is_its_own`.)
2. **Hostile names and paths.** A `worktree` argument that is a link into another repository, has `..`, or names a removed worktree; a `ref` like `-c`, `refs/tags/v1` or one not listed; a branch name `clean_ref` refuses; a commit subject with control characters, a bidi override and 500 characters. Each is refused, listed with Changes off, or shown as one plain cut line. (Task 9 `worktree_link_to_elsewhere_refused`, `ref_starting_with_dash_refused`; Task 2 `plain_label_drops_controls_and_cuts_120_on_char`; Task 17 `changes_off_for_refused_name`.)
3. **A big, slow repository.** 50 worktrees, a main checkout whose `status` takes 1 s, 300 refs: a call starts no read after 1.5 s, a call the app times out still fills the cache for the next one, and Details keeps its last good rows meanwhile. (Task 10 `no_read_starts_after_one_and_a_half_seconds`, `timed_out_reads_fill_the_cache`; Task 16 `failed_call_keeps_last_rows`.)
4. **A session that ran EnterWorktree** (`worktreePath` set, `cwd` the main checkout): its Git section, `git.status` and Changes read the lane; the lane is "this session", the main checkout an other row, and the session appears once. (Task 6 `job_worktree_path_reaches_agent`; Task 13 `entered_worktree_is_this_session_not_an_other_row`; Task 16 B17 wiring.)
5. **A remote URL with a user and token.** It never reaches a reply, an error or the Audit, and "GitHub only" still shows. (Task 4 `remotes_keep_only_the_host`; Task 11 `remote_url_never_in_reply_or_audit`.)

## Merge notes (shared files)

The Ask lane (`agent/ask`) and the SSH join lane (`agent/ssh-join`) run in parallel and touch some of the same files. Before a task that touches one, merge the latest `origin/agent-run-oss` into `agent/git-branches` and keep both sides' additions. Every edit here to a shared file is additive and placed beside the existing git lines, not at a file's end.

| File | Tasks here | Other lanes |
| --- | --- | --- |
| `crates/local-api/src/wire.rs` | 10, 11 (two consts right after `GIT_BRANCHES`) | Ask (`ask.*` block), SSH join (`cluster.invite`) |
| `crates/notchlingd/src/op_kinds.rs`, `op_kinds_tests.rs` | 10, 11 (`HOMES` after `GIT_BRANCHES`; `OWN_QUIET` grows to 10) | Ask, SSH join (`NEVER_QUIET` length) |
| `crates/notchlingd/src/lib.rs` | 8 to 12 (`mod git_*` beside `git_ops`) | Ask (`mod ask_*`), SSH join (`cluster_*`) |
| `crates/engine/src/sessions/agent.rs` and every `Agent { … }` literal | 6 | any lane that adds an `Agent` literal (Ask's tests may): add `worktree_path: None` when merging |
| `sessions.list` rows | 6 (optional `worktreePath`) | Ask's `caller_session.rs` reads rows; the field is additive |
| `locales/en/mac/chat.ftl`, `crates/locale/src/generated.rs` | 13, 14, 17 (block after `chat-changes-uncommitted`) | Ask (`chat-ask-*`): rerun `python3 tools/locale/gen.py` after a merge, never hand-merge `generated.rs` |
| `crates/app/src/chat/renders.rs`, `crates/app/src/mac/views.rs`, `crates/app/src/chat/view/mod.rs`, `crates/app/src/chat/mod.rs` | 13, 15 to 18 | Ask (`NOTCHLING_RENDER_ASK`, `ask_cards`, `asks`) |
| `docs/local-api-ops.md`, `docs/feature-map.md`, `crates/*/AGENTS.md`, `docs/notes/*` | 19 | every lane documents its work |

## Task order

| Wave | Tasks (parallel within a wave) | Needs |
| --- | --- | --- |
| 1 | 1, 2, 5, 6, 7, 15 | nothing |
| 2 | 3, 4, 9, 13 | 3: 1, 2; 4: 1, 2; 9: 1, 2; 13: 6, 7 |
| 3 | 8, 14, 17 | 8: 1, 2, 3; 14: 13; 17: 13 |
| 4 | 10, 16 | 10: 3, 4, 6, 8, 9; 16: 13, 14, 15 |
| 5 | 11, 12, 18 | 11: 10; 12: 5, 10; 18: 14, 15, 16, 17 |
| 6 | 19 | all |
| 7 | 20 (the maintainer) | all, installed |

The app tasks test pure models over reply JSON written from Tasks 10 and 11's Interfaces, so they need the daemon's ops only for the renders' real data and the live check.

---

### Task 1: The worktree list parser

Spec §6, §7.4, §7.11, *B11*.

**Zone:** protected (maintainer merges). **Files:**
- Create: `crates/engine/src/git/worktrees.rs`, `crates/engine/src/git/worktrees_tests.rs`
- Modify: `crates/engine/src/git/mod.rs` (`pub mod worktrees;`, the test module)

**Interfaces:**
- Produces: `pub const MAX_WORKTREES: usize = 50;` `#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)] pub struct WorktreeEntry { pub path: String, pub head: Option<String>, pub branch: Option<String>, pub detached: bool, pub bare: bool, pub locked: bool, pub prunable: bool }` (`head` the full 40-hex commit, `None` for the all-zero id an unborn branch prints; `branch` without `refs/heads/`); `pub fn parse_worktrees(text: &str) -> (Vec<WorktreeEntry>, bool)` over `git worktree list --porcelain -z` (attributes NUL-terminated, an empty attribute ends an entry; `locked`/`prunable` may carry a reason), cut at 50 with `true`.

- [ ] **Step 1: Write the failing tests:** `main_linked_and_detached_entries`, `bare_main_is_marked`, `locked_and_prunable_with_and_without_reasons`, `path_with_space_and_newline_is_kept` (`-z` keeps the newline), `branch_without_refs_heads_prefix`, `fifty_one_entries_cut_at_fifty`, `garbage_attribute_is_ignored`, `unborn_head_is_none` (`HEAD 0000000000000000000000000000000000000000`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib git::worktrees_tests`. Expected: FAIL (no module).
- [ ] **Step 3: Implement** `parse_worktrees`.
- [ ] **Step 4: Run** the same. Expected: PASS.
- [ ] **Step 5: Commit** `git: parse the worktree list`.

### Task 2: Ref, count, shortstat and remote parsers

Spec §7.4, §7.5, §7.8, §7.2 (plain labels), *B5*, *B10*, *B11*.

**Zone:** protected. **Files:**
- Create: `crates/engine/src/git/refs.rs`, `refs_tests.rs`
- Modify: `crates/engine/src/git/mod.rs`

**Interfaces:**
- Produces, in `git::refs`:
  - `pub const REF_FORMAT: &str = "%(refname)%00%(objectname)%00%(committerdate:unix)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(subject)";` one record per line.
  - `pub const MAX_REFS: usize = 300;` `pub const SUBJECT_CHARS: usize = 120;`
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)] pub struct Counts { pub ahead: u64, pub behind: u64 }`, `pub struct Track { pub ahead: u64, pub behind: u64, pub gone: bool }`, `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)] pub struct Lines { pub added: u64, pub removed: u64, pub files: u64 }`
  - `pub struct RefRow { pub refname: String, pub commit: String, pub committed_ms: Option<i64>, pub upstream: Option<String>, pub track: Track, pub subject: String }` (`committed_ms` is `%(committerdate:unix)` seconds × 1000; `subject` already `plain_label`ed)
  - `pub fn parse_refs(text: &str) -> (Vec<RefRow>, bool)`: skips `refs/remotes/<r>/HEAD`, cut at 300 after the skip.
  - `pub fn parse_track(text: &str) -> Track` (`"ahead 2, behind 1"`, `"ahead 2"`, `"behind 3"`, `"gone"`, `""`).
  - `pub fn parse_ahead_behind(text: &str) -> HashMap<String, Counts>` over lines `%(refname)%00%(ahead-behind:<base>)` (`"<ahead> <behind>"`).
  - `pub fn parse_left_right(text: &str) -> Option<Counts>` over `rev-list --left-right --count <base>...<head>`, which prints `<behind>\t<ahead>`.
  - `pub fn parse_shortstat(text: &str) -> Lines` (`LC_ALL=C`; empty text is all zeros).
  - `pub fn remote_hosts(config_z: &str) -> Vec<(String, Option<String>)>` over `config -z --get-regexp ^remote\.[^.]*\.url$` records (`remote.<name>.url\n<url>\0`): the remote's name and its URL's host, lowercased, from `https://[user[:secret]@]host[:port]/…`, `ssh://[user@]host[:port]/…` or `user@host:path`; nothing else of the URL survives the function.
  - `pub fn plain_label(text: &str, max_chars: usize) -> String`: one line, control and bidi-control characters dropped, cut on a `char` boundary.

- [ ] **Step 1: Write the failing tests:** `ref_record_fields`, `track_forms`, `origin_head_skipped`, `refs_cut_at_300`, `ahead_behind_atom_lines`, `left_right_prints_behind_first` (`"3\t2"` is ahead 2, behind 3), `both_count_forms_agree_for_one_pair`, `shortstat_insertions_only`, `shortstat_deletions_only`, `shortstat_both`, `shortstat_one_file`, `shortstat_binary_only` (`" 1 file changed"` is 1 file, 0, 0), `remote_host_https_ssh_and_scp`, `remote_host_drops_user_and_token` (the fixture `https://me:ghp_x@github.com/o/r.git`: no part of `me` or `ghp_x` in the output's `Debug`), `plain_label_drops_controls_and_cuts_120_on_char` (a 500-character subject with `\u{202e}`, `\t`, `\u{1b}` and multi-byte letters at the cut).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib git::refs_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `git: parse refs, counts, shortstat and remote hosts`.

### Task 3: The nearest base and the branch rows (pure)

Spec §6, §7.4 (`git.branch_list` rows, *B25*), §7.5, §7.6, *B18*, *B19*, D1, D4, D5.

**Zone:** protected. **Files:**
- Create: `crates/engine/src/git/nearest.rs`, `nearest_tests.rs`, `crates/engine/src/git/branch_list.rs`, `branch_list_tests.rs`
- Modify: `crates/engine/src/git/mod.rs`

**Interfaces:**
- Consumes: Task 1 `WorktreeEntry`; Task 2 `RefRow`, `Counts`, `Lines`.
- Produces, `git::nearest`:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Candidate { pub rev: String, pub commit: String }`
  - `pub fn short_name<'a>(rev: &'a str, remotes: &[String]) -> &'a str` (strips `<remote>/` for a known remote)
  - `pub fn candidates(own: Option<&str>, upstream: Option<&Candidate>, default: Option<&Candidate>, integration: Option<&Candidate>, remotes: &[String]) -> Vec<Candidate>`: order default, integration, upstream; never one whose short name is `own`; the upstream only when its short name differs from `own`.
  - `pub fn pick(scored: &[(Candidate, Option<Counts>)]) -> Option<(Candidate, Counts)>`: fewest ahead, then fewest behind, then order; `None` counts (unrelated, D5) never win.
- Produces, `git::branch_list`:
  - `#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)] pub struct Work { pub added: u64, pub removed: u64, pub files: u64, pub uncommitted_files: u64, pub changed_at: Option<i64>, pub read_at: i64 }`
  - `#[derive(Serialize)] pub struct WorktreeOf { pub path: String, pub real: String, pub main: bool, pub current: bool, pub missing: bool, pub work: Option<Work> }`
  - `#[derive(Serialize)] pub struct BranchRow { pub name: Option<String>, #[serde(rename = "ref")] pub reference: Option<String>, pub local: bool, pub remote: Option<String>, pub remote_only: bool, pub on_github: bool, pub is_base: bool, pub worktree: Option<WorktreeOf>, pub head: Option<String>, pub detached: bool, pub committed_at: Option<i64>, pub subject: Option<String>, pub upstream: Option<String>, pub upstream_ahead: u64, pub upstream_behind: u64, pub upstream_gone: bool, pub base: Option<String>, pub ahead: Option<u64>, pub behind: Option<u64>, pub unrelated: bool, pub merged_into: Option<String>, pub lines: Option<Lines>, pub pending: bool }` (`head` 7 characters)
  - `pub struct Bases { pub default: Option<Candidate>, pub integration: Option<Candidate> }`
  - `pub struct ListInput<'a> { pub refs: &'a [RefRow], pub worktrees: &'a [(WorktreeEntry, WorktreeOf)], pub remotes: &'a [(String, bool)], pub bases: &'a Bases, pub detached_facts: &'a HashMap<String, (Option<i64>, String)> }`
  - `pub fn rows(input: &ListInput, counts: &dyn Fn(&Candidate, &str) -> Option<Option<Counts>>, lines: &dyn Fn(&Candidate, &str) -> Option<Option<Lines>>) -> Vec<BranchRow>`: `counts(candidate, head_commit)` is `None` when not known yet (row `pending`), `Some(None)` when marked unrelated (D5: such a candidate is skipped; when every candidate is, `base: null`, `unrelated: true`); `lines(candidate, head_commit)` likewise, keyed by the candidate's commit. One row per local branch, per remote-tracking ref whose name minus its remote is no local branch's name, and per detached worktree (`name` and `ref` null). `is_base` for the default (by short name) and the integration branch; a base gets no `merged_into`, no `lines`. `merged_into`: the integration branch, else the default, that the branch is 0 ahead of (D1), never one with the branch's own short name. `lines` only for a non-base, not merged row. `on_github` from `remotes`.
  - `pub fn no_worktree(rows: &[BranchRow]) -> u64` (the "›" row's count).

- [ ] **Step 1: Write the failing tests:** nearest: `fewest_ahead_wins`, `then_fewest_behind`, `then_candidate_order`, `branch_made_at_main_gets_main` (0 ahead of both, 1,482 behind the integration branch), `never_its_own_base`, `upstream_with_own_name_is_no_candidate`, `default_compared_by_short_name` (`origin/main` vs local `main`), `unrelated_candidates_never_win`, `no_candidates_no_base`; rows: `local_and_remote_only_rows`, `remote_duplicate_of_local_dropped`, `remote_split_on_known_remote_with_slash_in_branch` (`refs/remotes/origin/agent/ask` is `origin`, `agent/ask`), `detached_worktree_is_a_row`, `bases_never_classified`, `merged_into_integration_before_default`, `never_merged_into_own_short_name` (`agent-run-oss` vs `origin/agent-run-oss`), `fresh_lane_with_no_commits_is_merged`, `lines_only_for_not_merged`, `unknown_counts_are_pending`, `unrelated_candidate_skipped_for_the_next`, `every_candidate_unrelated_says_so`, `on_github_from_remote`, `no_worktree_counts_rows_without_one`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib git::nearest_tests` and `git::branch_list_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `git: the nearest base and the branch rows`.

### Task 4: The repository reader

Spec §7.4 (new subcommands), §7.8, §7.10, §10 (engine, real repositories; hardening), *B10*, D1, D5.

**Zone:** protected. **Files:**
- Create: `crates/engine/src/git/reader.rs`, `reader_real_tests.rs` (`#[cfg(all(test, unix))]`)
- Modify: `crates/engine/src/git/mod.rs`, `crates/engine/src/git/branch.rs` (`remote_default` and `FALLBACK_BASES` become `pub(super)` so `Reader::default_rev` shares today's rule), `read_hardening_tests.rs`, `sandbox_tests.rs` (the new commands)

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces, `git::reader`:
  - `pub struct Reader<'a>`; `pub fn open(cwd: &str, launcher: &'a dyn Launcher) -> Result<Reader<'a>, Failure>` (one `config -z --get-regexp ^filter\.` probe); `pub fn at(&self, cwd: &str) -> Reader<'a>` (another worktree of the same repository, same drivers); `pub fn cwd(&self) -> &str`.
  - `pub fn common_dir(&self) -> Result<PathBuf, Failure>` (`rev-parse --path-format=absolute --git-common-dir`; `NotRepo` outside one)
  - `pub fn worktrees(&self) -> Result<(Vec<WorktreeEntry>, bool), Failure>` (`worktree list --porcelain -z`)
  - `pub fn refs(&self) -> Result<(Vec<RefRow>, bool), Failure>` (`for-each-ref --sort=-committerdate --format=REF_FORMAT refs/heads refs/remotes`, no `--count`: the `<remote>/HEAD` rows the parser skips would make it cut short; `run_capped` bounds the output and the parser cuts at 300)
  - `pub fn ahead_behind_all(&self, base_rev: &str) -> Result<Option<HashMap<String, Counts>>, Failure>` (`for-each-ref --format=%(refname)%00%(ahead-behind:<base_rev>) refs/heads refs/remotes`; `Ok(None)` when git refuses the atom, B10)
  - `pub fn left_right(&self, base_rev: &str, head: &str) -> Result<Option<Counts>, Failure>` (`rev-list --left-right --count <base_rev>...<head> --`; `None` for an unknown rev)
  - `pub fn lines(&self, base: &str, head: &str) -> Result<Option<Lines>, Failure>` (`diff --shortstat` + `DIFF_FLAGS` + `<base>...<head> --`; `None` when there is no merge base, D5)
  - `pub fn related(&self, base_commit: &str, head_commit: &str) -> Result<bool, Failure>` (`merge-base <base> <head>`: exit 0 `true`, exit 1 `false`, anything else `Failed`; D5)
  - `pub fn commit_facts(&self, commit: &str) -> Result<(Option<i64>, String), Failure>` (`log -1 --format=%ct%x00%s <commit> --`, for a detached head; the time in ms (`%ct` × 1000), the subject `plain_label`ed)
  - `pub fn remotes(&self) -> Result<Vec<(String, bool)>, Failure>` (name, host is `github.com`)
  - `pub fn default_rev(&self, local: &[String]) -> Option<String>` (today's rule: `origin/HEAD`'s target, else `main`, else `master`)
  - `pub fn fetched_at(common_dir: &Path) -> Option<i64>` (`FETCH_HEAD`'s mtime in ms, one `stat`)
  - Every `commit`/`head` argument must be 40 lowercase hex and every `base_rev` pass `clean_ref`, else `Failure::Refused` before anything runs.

- [ ] **Step 1: Write the failing tests** over a temp repository built like `branch_real_tests.rs` (a main checkout on `main`, an integration branch `int` 3 commits ahead, two linked worktrees `lane-a` (2 commits on `int`, an untracked file) and `lane-b` (detached), a remote-only ref made with `update-ref refs/remotes/origin/far <commit>`, a merged branch, `remote.origin.url = https://me:secret@github.com/o/r.git`, a `FETCH_HEAD` file): `common_dir_is_the_main_git_dir_from_a_lane`, `worktrees_list_main_linked_detached`, `refs_list_heads_and_remotes`, `ahead_behind_matches_rev_list` (for every ref against `int`, compare with `git rev-list --left-right --count` run by the test), `lines_match_git_shortstat`, `unrelated_history_has_no_lines` (an orphan branch), `related_false_for_an_orphan_true_for_a_lane`, `commit_facts_for_a_detached_head`, `remotes_keep_only_the_host` (`secret` and `me` absent from the result's `Debug`), `fetched_at_from_fetch_head`, `odd_rev_or_commit_refused_before_running` (`-c`, `a..b`, a 39-hex string; `FakeLauncher` records no call); in `read_hardening_tests.rs` and `sandbox_tests.rs`, the filter, hook, fsmonitor, textconv and external-diff traps stay silent for `worktrees`, `refs`, `ahead_behind_all`, `left_right`, `lines`, `related` and `commit_facts`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib git::reader_real_tests`, `git::read_hardening_tests`, `git::sandbox_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** all three and `cargo test -p notchling-engine --lib git::`. Expected: PASS.
- [ ] **Step 5: Commit** `git: read worktrees, refs, counts and remotes`.

### Task 5: Stat and patch for a branch without a worktree; `changed_at`

Spec §7.3, §7.4 (`ref`, `changed_at`), *B21*.

**Zone:** protected. **Files:**
- Modify: `crates/engine/src/git/branch.rs` (`read_ref_stat`; `merge_base` takes the head rev instead of a fixed `HEAD`), `branch_file.rs` (`read_ref_diff`), `read.rs` (`changed_at` in `read_status`; `pub(super) fn newest_change(root: &Path, paths: &[&str]) -> Option<i64>`), `porcelain.rs` (`Status.changed_at: Option<i64>`), `branch_parse.rs` (`BranchStat.changed_at: Option<i64>`), `mod.rs` (exports)
- Test: `branch_real_tests.rs`, `read_tests.rs`, `read_hardening_tests.rs` (additions); `read_tests.rs` and `branch_tests.rs` (each `calls.len()` assertion over a status with changes grows by one: the root read)

**Interfaces:**
- Produces: `pub fn read_ref_stat(cwd: &str, base: Option<&str>, rev: &str, launcher: &dyn Launcher) -> Result<BranchStat, Failure>`: `rev` must pass `clean_ref` and start with `refs/heads/` or `refs/remotes/`; runs no `status` and reads no working tree; with no `base` (no related candidate, Task 12) the reply has `branch` and `head` only, no base and no files; with one: `merge-base <base> <rev>`, then `diff --name-status|--numstat -z` + `DIFF_FLAGS` + `<merge base> <rev> --`; `branch` is `rev` without its `refs/heads/` or `refs/remotes/` prefix, `detached: false`, `head` the rev's 7-character commit (`rev-parse --verify --quiet <rev>^{commit}`), `uncommitted_files: 0`.
- Produces: `pub fn read_ref_diff(cwd: &str, base: &str, rev: &str, path: &str, from: Option<&str>, launcher: &dyn Launcher) -> Result<Diff, Failure>`: `path` must be in that ref's name-status list against that base; the patch is `diff` + `DIFF_FLAGS` + `<merge base> <rev> -- :(literal)<path> [:(literal)<from>]`; no untracked-file path.
- Produces: `changed_at` (ms) on `git.status` and on `read_branch_stat`'s result: the newest mtime (`symlink_metadata`, links not followed) of the changed files that exist, at most the 500 listed, each `clean_path`-checked under `rev-parse --show-toplevel`. The root is read only when there are changes, after every command today's code runs (so scripted `FakeLauncher` answers stay in order), and reused when `read_branch_stat` already read it for untracked files; a failed or empty root read leaves `changed_at` `None` and never fails the read. `None` when no changed file exists. The field is `#[serde(skip_serializing_if = "Option::is_none")]`, so a reply without it is byte for byte today's.

- [ ] **Step 1: Write the failing tests:** `ref_stat_counts_committed_work_only` (a branch whose worktree has uncommitted edits: the ref stat ignores them), `ref_stat_runs_no_status` (`FakeLauncher`: no recorded `status`), `ref_is_before_double_dash`, `ref_outside_heads_and_remotes_refused`, `remote_ref_stat_branch_name` (`refs/remotes/origin/far` is `origin/far`), `ref_diff_path_must_be_in_the_stat`, `ref_diff_patch_matches_git`, `changed_at_is_newest_existing_change`, `changed_at_none_when_only_deleted`, `changed_at_skips_paths_leaving_the_root`, `root_read_failure_leaves_changed_at_none`, `ref_stat_without_base_has_no_files`, `existing_stat_and_diff_unchanged` (every reply field of today's tests unchanged; only the call counts above grow), and the textconv trap silent for `read_ref_stat` and `read_ref_diff`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib git::branch_real_tests`, `git::read_tests`, `git::read_hardening_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** them, `cargo test -p notchling-engine --lib git::` and `cargo test -p notchling-notchlingd --lib git_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `git: committed-only stat and patch for a ref; when files last changed`.

### Task 6: `worktreePath` on sessions

Spec §5, §7.4 (`sessions.list`), §7.7 (*B14*), §7.10, *B17*, D6.

**Zone:** protected (engine `sessions/`, daemon), auto (the app's literals). **Files:**
- Modify: `crates/engine/src/sessions/agent.rs` (`#[serde(default, skip_serializing_if = "Option::is_none")] pub worktree_path: Option<String>`, serialized `worktreePath` by the struct's `camelCase`), `crates/engine/src/sessions/jobs.rs` (`agent_from_job` fills it from the job state's non-empty `worktreePath`), `crates/notchlingd/src/git_ops.rs` (`live_folders` adds each row's `worktreePath`), every `Agent { … }` literal in the workspace (`worktree_path: None`)
- Test: `crates/engine/src/sessions/jobs_tests.rs`, `agent_tests.rs`, `crates/notchlingd/src/git_ops_tests.rs`

- [ ] **Step 1: Write the failing tests:** `job_worktree_path_reaches_agent`, `empty_worktree_path_is_none`, `agent_without_worktree_path_serializes_without_it`, `agent_from_older_peer_reads` (JSON with no `worktreePath`), `a_sessions_worktree_path_is_a_known_folder` (a `cwd` that is only some row's `worktreePath` is allowed by `git.status`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib sessions::` and `cargo test -p notchling-notchlingd --lib git_ops_tests`. Expected: FAIL (does not compile until the field exists, then the new tests fail).
- [ ] **Step 3: Implement**, then add `worktree_path: None` to every literal `cargo build --workspace --tests` names.
- [ ] **Step 4: Run** both filters and `cargo test --workspace --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `sessions: carry a job's worktreePath; it is a known git folder`.

### Task 7: Subagent folders and start times

Spec §7.7, D2, D3.

**Zone:** protected (`engine/src/contract/`), auto (`engine/src/transcript/`, the app). **Files:**
- Modify: `crates/engine/src/transcript/glance.rs` (`Peeked.cwd: Option<String>`: the newest line's `"cwd"` among the lines the windows already read; no extra read), `crates/engine/src/contract/subagents.rs` (`Glance.cwd: Option<String>`), `crates/engine/src/contract/claude/subagents.rs` (from `Peeked.cwd`), `crates/engine/src/contract/codex/subagents.rs` (from `codex::meta(&s.path)`'s non-empty `cwd`), `crates/engine/src/contract/antigravity/subagents.rs` (`None`), `crates/app/src/chat/subagent_rows.rs` (`Facts.cwd`; `Disk::read` fills it), `crates/app/src/chat/inspector.rs` (`SubagentRow.cwd: Option<String>`, `SubagentRow.started_ms: i64` from `Subagent.started` seconds × 1000), the other `SubagentRow`/`Glance` literals
- Test: `crates/engine/src/transcript/glance_tests.rs`, `crates/engine/src/contract/registry_subagents_tests.rs`, `crates/app/src/chat/subagent_rows_tests.rs`

- [ ] **Step 1: Write the failing tests:** `peek_keeps_newest_cwd` (two lines with different `cwd`: the later wins), `peek_without_cwd_is_none`, `codex_glance_cwd_from_session_meta`, `delegated_codex_job_glance_has_its_rollout_cwd`, `row_carries_cwd_and_started_ms`, `row_facts_cached_with_cwd` (the stamp cache returns the same `cwd` without a second read).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib transcript::glance` (the tests are `glance::tests`), `cargo test -p notchling-engine --lib contract::`, `cargo test -p notchling-app --lib chat::subagent_rows`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three and `cargo test -p notchling-app --test subagent_reads`. Expected: PASS, the read counts unchanged.
- [ ] **Step 5: Commit** `subagents: each row knows its folder and start time`.

### Task 8: The daemon's caches and the work-read choice (pure)

Spec §7.9, *B11*, *B12*, *B13*, *B20*.

**Zone:** protected. **Files:**
- Create: `crates/notchlingd/src/git_cache.rs`, `git_cache_tests.rs`
- Modify: `crates/notchlingd/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 `WorktreeEntry`; Task 2 `Counts`, `Lines`; Task 3 `Work`.
- Produces (the constants of Global Constraints live here):
  - `pub struct Numbers` (LRU of `NUMBERS_CAP`): `pub fn counts(&mut self, base: &str, head: &str) -> Option<Option<Counts>>` (`Some(None)` once the pair is marked unrelated), `pub fn put_counts(&mut self, base: &str, head: &str, counts: Option<Counts>)` (never clears a mark), `pub fn lines(&mut self, base: &str, head: &str) -> Option<Option<Lines>>`, `pub fn put_lines(&mut self, base: &str, head: &str, lines: Option<Lines>)` (`None` marks the pair unrelated), `pub fn related(&mut self, base: &str, head: &str) -> Option<bool>`, `pub fn put_related(&mut self, base: &str, head: &str, related: bool)` (`false` marks it); keys are 40-hex commits.
  - `pub struct RepoCache { pub listed: Option<(i64, Listed)>, pub numbers: Numbers, pub work: HashMap<String, Work>, pub facts: HashMap<String, (Option<i64>, String)>, pub reading: HashSet<String>, pub asked_ms: i64 }` (`facts`: a detached head's commit to its `commit_facts`) with `pub struct Listed { pub worktrees: Vec<WorktreeEntry>, pub truncated: bool, pub remotes: Vec<(String, bool)>, pub default: Option<String>, pub fetched_at: Option<i64> }`; `pub fn listed_fresh(&self, now_ms: i64) -> Option<&Listed>` (`LIST_FOR_MS`).
  - `pub struct GitCache`: `pub fn repo(&mut self, common_dir: &Path, now_ms: i64) -> &mut RepoCache` (updates `asked_ms`, drops repositories idle `IDLE_DROP_MS`).
  - `pub fn pick_reads(eligible: &[String], work: &HashMap<String, Work>, reading: &HashSet<String>, fresh: &[String], now_ms: i64) -> Vec<String>`: the missing ones first (up to `MISSING_READS`, in `eligible` order); when none is missing, the stalest one older than `STALE_MS`; then up to `FRESH_READS` of `fresh` not already chosen; never one in `reading`.
  - `pub struct Budget`: `pub fn new(start_ms: i64, stop_ms: i64) -> Budget`, `pub fn may_start(&self, now_ms: i64) -> bool`.

- [ ] **Step 1: Write the failing tests** (a fake clock: plain `now_ms` values): `numbers_keep_the_2000_most_recent`, `unrelated_pair_is_remembered`, `unrelated_mark_survives_put_counts`, `lines_none_marks_unrelated`, `listed_reused_for_5s_then_stale`, `idle_repo_dropped_after_10_minutes`, `missing_first_up_to_four`, `stalest_older_than_15s_when_none_missing`, `nothing_when_all_fresh`, `fresh_adds_up_to_two`, `reading_paths_skipped`, `budget_stops_at_1500ms`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib git_cache_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: git caches and which worktrees to read`.

### Task 9: The argument guard

Spec §7.10.

**Zone:** protected. **Files:**
- Create: `crates/notchlingd/src/git_args.rs`, `git_args_tests.rs`
- Modify: `crates/notchlingd/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 `WorktreeEntry`, Task 2 `RefRow`.
- Produces: `pub fn worktree_arg(raw: &str, listed: &[WorktreeEntry]) -> Result<PathBuf, ApiError>` (absolute, no `..`, canonicalizes to exactly the canonical path of one listed entry that is not bare and not prunable; `forbidden` "not a worktree of this repository" otherwise); `pub fn ref_arg<'a>(raw: &'a str, listed: &[RefRow]) -> Result<&'a str, ApiError>` (`clean_ref`, `refs/heads/` or `refs/remotes/`, and listed; `forbidden` otherwise); `pub fn fresh_args(raw: Option<&Value>, listed: &[WorktreeEntry]) -> Vec<PathBuf>` (each matched as `worktree_arg`; an unknown, odd or non-string one is ignored; at most `FRESH_READS`).

- [ ] **Step 1: Write the failing tests** (temp folders and links, listed entries made by hand): `listed_worktree_accepted_as_canonical`, `worktree_not_listed_refused`, `relative_worktree_refused`, `worktree_with_dotdot_refused`, `worktree_link_to_elsewhere_refused`, `prunable_or_bare_worktree_refused`, `ref_outside_heads_and_remotes_refused` (`refs/tags/v1`, `HEAD`), `ref_starting_with_dash_refused`, `ref_not_listed_refused`, `fresh_unknown_ignored`, `fresh_capped_at_two`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib git_args_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: check worktree, ref and fresh against what git lists`.

### Task 10: `git.worktrees`

Spec §7.1, §7.4, §7.5, §7.9, §7.11, *B10*, *B12*, *B14*, *B15*.

**Zone:** protected. **Files:**
- Create: `crates/notchlingd/src/git_view.rs` (the shared snapshot: `Listed` from the cache or the reader, the refs, the bases, the counts filled per candidate under the budget), `git_worktrees.rs` (the op), `git_testkit.rs` (`#[cfg(test)]`: `Recorded`, a `Launcher` over `RealLauncher` (sandboxed on macOS, as `branch_real_tests.rs`) that records every argv and can delay one subcommand; `RepoRig`, the layout of Task 4's repository built with the same `git()` helper, plus the known-folder list), `git_ops_worktrees_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`GIT_WORKTREES = "git.worktrees"` right after `GIT_BRANCHES`), `crates/notchlingd/src/git_ops.rs` (`GIT_OPS` gains it; `GitOps.cache: Mutex<GitCache>`; the route), `op_kinds.rs` (`HOMES` `Home::Quiet` after `GIT_BRANCHES`), `op_kinds_tests.rs` (`OWN_QUIET`), `git_ops_branch_tests.rs` (`GIT_OPS.len()`), `lib.rs`

**Interfaces:**
- Consumes: Tasks 3, 4, 8, 9; Task 6 (known folders).
- Produces: `git.worktrees {cwd, fresh?: [path]}` →

```
{repo: false}
| {repo: true, name, default, integration, fetched_at, no_worktree, truncated,
   worktrees: [{path, real, main, current, branch, head, detached, locked, missing,
                upstream, upstream_ahead, upstream_behind, upstream_gone,
                base, ahead, behind, unrelated, committed_at,
                work: {added, removed, files, uncommitted_files, changed_at, read_at} | null}]}
```

  `name` the main checkout's folder name (for a bare repository, the common dir's name without `.git`); `default` a rev (D4); `integration` the main checkout's branch unless it is the default's short name, `null` when bare or detached; `current` the entry whose `path` or `real` is the longest component-wise prefix of `cwd`; the current entry's `work` always `null`; a bare entry left out; `missing` for a prunable one (no numbers). Work reads: `pick_reads` over the non-current, non-missing entries, each a `read_branch_stat(<real>, Some(<base>))` summed into `Work`, run outside the cache lock with the path in `reading`, stored with `read_at`; no read starts once `Budget::may_start` is false. A detached entry's counts come from `left_right` per candidate and its `committed_at` from `commit_facts`, both cached (`Numbers`, `RepoCache.facts`). Counts per candidate: one `ahead_behind_all` for the default and one for the integration branch, each skipped when every pair it would answer is cached; `Ok(None)` (old git) falls back to `left_right` per branch, cached, under the budget. An upstream candidate (a branch's upstream whose short name differs from the branch's) is counted with `left_right` for that one pair, cached, under the budget. Before a non-current entry's work read, its pick is confirmed with `Reader::related` (cached; D5): an unrelated one is marked and the entry is picked again, its work read waiting for the next call; with no related candidate left, `base` is `null`, `unrelated` `true` and no work is read.
- Produces (for Tasks 11, 12): `GitOps::snapshot(&self, reader: &Reader, cache_key: &Path, now_ms: i64) -> Result<Snapshot, Failure>` in `git_view.rs`, with `Snapshot { listed, refs, refs_truncated, bases, counts, facts }` (`facts` the detached heads' `commit_facts`, for `ListInput.detached_facts`) and `fn base_for(&self, head_commit: &str, own: Option<&str>, upstream: Option<&str>) -> Option<(String, Counts)>` (confirmed with `related` as above; `None` when no candidate is related).

- [ ] **Step 1: Write the failing tests:** `the_op_is_app_quiet_and_in_git_ops`, `cwd_not_a_session_folder_is_refused`, `not_a_repository_is_repo_false`, `lists_main_and_linked_with_branch_head_and_base`, `lane_base_is_the_integration_branch` (lane made from `int`: base `int`, 2 ahead), `current_is_the_longest_path` (a lane folder nested in the main checkout), `current_work_is_null`, `missing_work_read_first_up_to_four`, `second_call_inside_15s_runs_no_status` (`Recorded`), `fresh_path_is_read_again`, `unknown_fresh_ignored`, `detached_worktree_row`, `prunable_folder_is_missing_without_numbers`, `bare_main_left_out_without_integration` (a `clone --bare` with two linked worktrees), `unrelated_lane_has_no_base_and_no_work_read` (a lane on an orphan branch: `base` null, `unrelated: true`, no `status` recorded in it), `upstream_candidate_counted_by_rev_list` (a lane tracking `origin/int`), `no_worktree_counts_after_dedup`, `fetched_at_from_fetch_head`, `old_git_falls_back_to_rev_list` (`Recorded` answers the `ahead-behind` call as git 2.40 does: exit 128 "unknown field name"), `no_read_starts_after_one_and_a_half_seconds` (`status` delayed 800 ms, three missing worktrees: two reads), `timed_out_reads_fill_the_cache` (the first call's reads, finished after it returned, are the second call's `work`), `cache_lock_never_held_across_git` (a delayed `status` in one thread while a second call returns from the cache).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib git_ops_worktrees_tests` and `--lib op_kinds` (the tests are `op_kinds::tests`). Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both and `cargo test -p notchling-notchlingd --lib git_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: git.worktrees`.

### Task 11: `git.branch_list`

Spec §7.2, §7.4, §7.6, §7.8, §7.9, *B5*, *B13*, *B19*, *B20*, *B25*.

**Zone:** protected. **Files:**
- Create: `crates/notchlingd/src/git_branch_list.rs`, `git_ops_branch_list_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`GIT_BRANCH_LIST = "git.branch_list"` after `GIT_WORKTREES`), `git_ops.rs`, `op_kinds.rs`, `op_kinds_tests.rs`, `git_ops_branch_tests.rs`, `lib.rs`

**Interfaces:**
- Consumes: Task 10's `snapshot`; Task 3 `rows`.
- Produces: `git.branch_list {cwd}` →

```
{repo: false}
| {repo: true, name, default, integration, fetched_at, truncated, pending,
   branches: [BranchRow]}
```

  (`BranchRow` as Task 3; `truncated` when the worktrees or the refs were cut; `pending` the count of rows still `pending`). Missing line counts are read for at most `LINES_BUDGET_MS` per call (`Reader::lines`, cached by commits); a read that finds no merge base marks the pair unrelated (D5), and the rows are computed again after the reads, so such a row moves to its next candidate (its `lines` then read in the same call when the budget allows, else `pending`). Missing work is read within the call's `READ_STOP_MS` by Task 10's rule over every non-missing worktree, current included (B20). A detached worktree's row takes its time and subject from `Snapshot.facts`.

- [ ] **Step 1: Write the failing tests:** `the_op_is_app_quiet_and_in_git_ops`, `one_row_per_local_and_remote_only`, `origin_head_skipped_and_remote_duplicate_dropped`, `bases_never_classified`, `merged_into_integration_or_default`, `never_merged_into_own_short_name`, `lines_null_for_merged_and_bases`, `lines_pending_past_one_second_budget` (`diff` delayed 400 ms, four missing line counts: `pending > 0`), `second_call_same_heads_runs_no_diff_or_rev_list`, `worktree_rows_carry_work_from_the_cache`, `unrelated_branch_has_no_base` (an orphan branch with candidates `main` and `int`: neither is its base after its first `lines` read, `unrelated: true`), `detached_worktree_row_has_its_commit_time_and_subject`, `on_github_from_host`, `subject_plain_and_cut`, `remote_url_never_in_reply_or_audit` (the rig's user and token strings are absent from the reply JSON and from the message of every error the git ops return over the rig, which is what the Audit records for a refusal or failure).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib git_ops_branch_list_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it, `--lib op_kinds` and `--lib git_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: git.branch_list`.

### Task 12: `worktree`, `ref` and `pick` on the existing ops; the allowlist

Spec §7.3, §7.4 (changed ops), §7.5 (the stat's cache), §7.10, §10 (no fetch), *B21*, *B22*, answer 4.

**Zone:** protected. **Files:**
- Create: `crates/notchlingd/src/git_targets.rs` (argument handling for the three ops), `git_ops_targets_tests.rs`, `git_ops_allowlist_tests.rs`
- Modify: `crates/notchlingd/src/git_ops.rs` (route through `git_targets`), `lib.rs`

**Interfaces:**
- Consumes: Tasks 5, 9, 10.
- Produces:
  - `git.branch_diff_stat {cwd, base?, worktree?, ref?, pick?: "nearest"}`: `worktree` reads that worktree (its canonical path); `ref` reads `read_ref_stat` (reply `includes_uncommitted: false` and B21's fields; with `pick: "nearest"` and no related candidate, `read_ref_stat(cwd, None, rev)`); `pick: "nearest"` without `base` uses `Snapshot::base_for` (the choice cached by the head commit and every candidate's commit, so an unchanged repository costs the one `refs` read); a given `base` always wins; with none of the three, the reply is today's byte for byte. A worktree stat with its nearest base stores its totals as that worktree's `work` in the cache (so `git.branch_list` reads the current worktree no second time).
  - `git.branch_diff_file {cwd, base, path, from?, worktree?, ref?}`; `git.branches {cwd, worktree?}`; `git.status` passes `changed_at` through.
  - `worktree` and `ref` together are `bad_request`.

- [ ] **Step 1: Write the failing tests:** `stat_in_another_worktree_reads_there`, `stat_pick_nearest_names_the_base`, `explicit_base_wins_over_pick`, `pick_nearest_skips_an_unrelated_candidate`, `pick_nearest_cached_runs_no_rev_list`, `no_new_args_reply_unchanged`, `ref_stat_runs_no_status`, `ref_before_double_dash` (every recorded argv), `ref_stat_reply_fields` (B21), `ref_file_path_must_be_in_the_stat`, `branches_in_a_worktree`, `worktree_and_ref_together_refused`, `nearest_stat_fills_work_cache` (through a `#[cfg(test)] GitOps::cached_work(path)` accessor), `status_has_changed_at`; allowlist: `every_git_command_is_in_the_allowlist` (runs every `git.*` op with every argument form over `RepoRig` with `Recorded`: each subcommand in the Global Constraints list, `worktree` only with `list`, `config` only with `-z --get-regexp`, none of the forbidden ones), `diff_shortstat_carries_every_diff_flag`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib git_ops_targets_tests` and `--lib git_ops_allowlist_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, `--lib git_ops` and `cargo test -p notchling-local-api`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: Changes for another worktree or branch, the nearest base, no fetch`.

### Task 13: The Git section's model (pure)

Spec §7.1 (with answer 2), §7.7, §8, *B1*, *B2*, *B9*, *B14*, *B17*, D4.

**Zone:** auto. **Files:**
- Create: `crates/app/src/chat/worktrees.rs`, `worktrees_tests.rs`, `crates/app/src/chat/changes_target.rs` (`Target` and its requests only; Task 17 adds the state), `changes_target_tests.rs`
- Modify: `crates/app/src/chat/mod.rs`, `crates/app/src/chat/git.rs` (`GitModel` gains the reply's raw `upstream: Option<String>`, `ahead: u64`, `behind: u64`, `initial: bool` and `changed_at: Option<i64>`, parsed; `tracking` stays for today's view until Task 16), `crates/app/src/chat/git_tests.rs` and `crates/app/src/chat/details_words_tests.rs` (their `GitModel { … }` literals gain the fields), `locales/en/mac/chat.ftl`

**Interfaces:**
- Consumes: the `git.worktrees` reply (Task 10); `Session` (with Task 6's `agent.worktree_path`); `SubagentRow` (Task 7's `cwd`, `started_ms`); `GitModel`; `BranchModel`.
- Produces, `chat::worktrees`:
  - `pub const WORKTREES_OP: &str = "git.worktrees";` `pub const SHOWN_WORKTREES: usize = 6;`
  - `Work`, `Worktree` (every reply field), `Worktrees { name, default, integration, fetched_at, no_worktree, truncated, list }`, all `Clone, Debug, PartialEq, Eq`; `pub fn parse_worktrees(reply: &Value) -> Option<Worktrees>`; `pub fn parse_work(value: &Value) -> Option<Work>`; `pub fn worktrees_request(cwd: &str, fresh: &[String]) -> Request`.
  - `pub fn shown_base(base: &str, default: Option<&str>) -> String`; `pub fn session_folder(agent: &Agent) -> &str` (`worktree_path` when set, else `cwd`); `pub fn owner(folder: &str, paths: &[(&str, &str)]) -> Option<usize>` (over each worktree's `(path, real)`: the longest component-wise prefix of either).
  - `pub enum Open { Session(String), Subagent(String) }`; `pub enum Occupancy { Running { since_ms: i64, open: Open, more: usize }, NeedsInput { open: Open, more: usize }, Idle { open: Open } }`; `pub fn occupancy(paths: &[(&str, &str)], sessions: &[Session], subagents: &[SubagentRow]) -> Vec<Option<Occupancy>>` (one per `(path, real)`, so the Branches panel uses it for its worktree rows too; no remote rows (`agent.node`); a Working session runs since `since_ms`, a running subagent since `started_ms`; the longest running is named, `more` the other matches).
  - `pub enum SubPart { Faint(String), Added(u64), Removed(u64), Running(String) }`; `pub struct CurrentRow { pub branch: String, pub right: Option<(u64, u64)>, pub sub: Vec<SubPart> }`; `pub struct OtherRow { pub path: String, pub branch: String, pub right: Option<(u64, u64)>, pub sub: Vec<SubPart>, pub working: bool, pub missing: bool }`; `pub struct GitSection { pub title: String, pub current: CurrentRow, pub others: Vec<OtherRow>, pub more: Option<String> }`.
  - `pub fn section(status: &GitModel, stat: Option<&BranchModel>, worktrees: Option<&Worktrees>, occupancy: &[Option<Occupancy>], minute_ms: i64) -> GitSection` (`minute_ms` is now floored to the minute, so ages change once a minute). Current row: right side the stat's totals; sub-line `N uncommitted +a −r` (left out when clean; "500+" when truncated) · `last … ago` (newer of the current entry's `committed_at` and `status.changed_at`) · the upstream part · `vs <shown_base>` (left out until the stat answers). The upstream part comes from the current entry when the worktrees reply is there, else from `status`'s raw fields (no "upstream gone" then). With `status.initial` (no commit yet), the row is the name and the uncommitted part only. Other rows: running first, then by path, at most 6 (B1); right side `work`; sub-line `N files · 2 ahead[, 1 behind]` then `running 41m [+1]` / `needs input` / `last … ago`; an unread work row has no right side and no file count (its sub-line is `2 ahead` then the running or last part); an `unrelated` row says "no common history" in place of the files and ahead part; a missing one "folder missing". "›" row: `{default} and N branches` when no worktree has the default's short name, else `N other branches`; prefixed `M more worktrees · ` past 6; `None` when `no_worktree` is 0.
  - `pub fn title(running_others: usize, worktrees: usize) -> String` (B2: one worktree is "Git").
- Produces, `chat::changes_target`: `#[derive(Clone, Debug, PartialEq, Eq)] pub enum Target { Own, Worktree(String), Ref(String) }`; `pub fn stat_request(cwd: &str, target: &Target, base: Option<&str>) -> Request` (`pick: "nearest"` when `base` is `None`); `pub fn file_request(cwd: &str, target: &Target, base: &str, path: &str, from: Option<&str>) -> Request`; `pub fn branches_request(cwd: &str, target: &Target) -> Request` (a `Ref` target asks in `cwd`, B22).
- Words (chat.ftl block, Global Constraints): `chat-git-this-session` "this session", `chat-git-uncommitted` "{ $count } uncommitted", `chat-git-last-ago` "last { $age } ago", `chat-git-last-now` "last just now", `chat-git-even-with` "even with { $upstream }", `chat-git-to-push` "{ $count } to push", `chat-git-only-behind` "{ $count } behind", `chat-git-push-and-behind` "{ $ahead } to push, { $behind } behind", `chat-git-no-upstream` "no upstream", `chat-git-upstream-gone` "upstream gone", `chat-git-base-ahead` "{ $count } ahead", `chat-git-base-ahead-behind` "{ $ahead } ahead, { $behind } behind", `chat-git-running-for` "running { $age }", `chat-git-more-matches` "+{ $count }", `chat-git-needs-input` "needs input", `chat-git-folder-missing` "folder missing", `chat-git-no-common` "no common history", `chat-git-default-and` "{ $default } and { $count -> [one] { $text } branch *[other] { $text } branches }", `chat-git-other-branches` "{ $count -> [one] { $text } other branch *[other] { $text } other branches }", `chat-git-more-worktrees` "{ $count -> [one] { $text } more worktree *[other] { $text } more worktrees } · { $rest }"; ages from `format::age`; file counts reuse `chat-changes-file-count`; "vs …" reuses `chat-git-vs-base`; the title reuses `counted_title`'s words.

- [ ] **Step 1: Write the failing tests:** `parse_reply_fields_and_repo_false`, `current_right_side_is_the_stat_vs_base` (answer 2), `current_sub_line_every_part_present`, `current_sub_line_parts_absent` (clean, no times, no upstream), `uncommitted_500_plus_when_truncated`, `upstream_forms` (even with origin, 2 to push, 3 behind, both, no upstream, gone, the full upstream when its name differs), `vs_base_short_name_and_absent_before_stat`, `other_rows_files_ahead_running_or_last`, `unread_work_shows_ahead_only`, `missing_folder_row`, `unrelated_row_says_no_common_history`, `upstream_from_status_before_the_worktrees_reply`, `rows_running_first_then_path_capped_at_six`, `more_row_three_forms_and_absent`, `title_git_alone_running_and_total`, `owner_longest_prefix_over_path_and_real`, `component_prefix_not_string_prefix` (`/r/lane` is not inside `/r/lane-2`), `session_folder_worktree_path_over_cwd`, `entered_worktree_is_this_session_not_an_other_row`, `remote_rows_not_matched`, `subagent_cwd_matches_with_started_ms`, `two_matches_longest_named_plus_one`, `needs_input_word`, `idle_match_keeps_open_without_dot`, `same_minute_same_section`, `unmatched_session_change_keeps_section_equal`, `no_commit_yet_shows_name_and_uncommitted_only`, `shown_base_strips_origin_only_for_default`; `changes_target_tests`: `own_target_asks_pick_nearest`, `explicit_base_drops_pick`, `worktree_target_args`, `ref_target_args`, `ref_branches_ask_in_cwd`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib chat::worktrees` and `--lib chat::changes_target`. Expected: FAIL.
- [ ] **Step 3: Implement**; add the words and run `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** both filters, `--lib chat::git` and `python3 tools/locale/gen.py --check`. Expected: PASS.
- [ ] **Step 5: Commit** `app: the Git section's model, every worktree in one line`.

### Task 14: Changes targets

Spec §7.3, *B22*, *B23*, Review Focus 1.

**Zone:** auto. **Files:**
- Modify: `crates/app/src/chat/changes_target.rs` (the state), `changes_target_tests.rs`, `crates/app/src/chat/view/repo.rs` (`State.other: Option<TargetState>`, `State.changes_from_branches: bool`; each Gate round still asks `git.status` and the own stat (`Target::Own`) whenever Details or Changes shows, as today, plus the other target's stat while Changes shows one; `diff_via` gains the target), `crates/app/src/chat/branch.rs` (`pub fn choices_for(&self, target: Option<&str>) -> Vec<String>`: leaves out `target` wherever it appears, remote default included; `None` is today's rule, and `choices()` stays as `choices_for(None)`; B22), `crates/app/src/chat/branch_tests.rs`, `crates/app/src/chat/view/changes.rs` (open, close, base menu, file patches and the refused fallback per target; `open_changes_for(target, from_branches: bool)`; `open_branch_diff` gains `window: &mut Window` to call `offer_page`), `crates/app/src/chat/view/changes_panel.rs` (header `<branch> → <base>` from the shown model; a `Ref` target's counts line says no uncommitted files; the menu calls `choices_for` with a `Ref` target's name as its stat names it (B21); the file row's "Open in File Panel" click passes its `window` through a window-carrying variant of `on_chat`), `crates/app/src/chat/view/html_panel.rs` (`offer_page` takes the Changes row's `ChangedFile`: only for `Target::Own`, `uncommitted` and new (`A` or `?`), B23), `locales/en/mac/chat.ftl` (`chat-changes-committed-only` "Committed work only")

**Interfaces:**
- Consumes: Task 13 `Target` and requests; Task 12's reply fields.
- Produces: `pub struct TargetState { pub target: Target, pub branch: Option<Arc<BranchModel>>, pub base_choice: Option<String> }`; `pub fn shown_branch<'a>(own: Option<&'a Arc<BranchModel>>, other: Option<&'a TargetState>) -> Option<&'a Arc<BranchModel>>`; `pub fn on_close(other: &mut Option<TargetState>, from_branches: &mut bool) -> bool` (clears both; returns whether Branches shows again, for the own target too); `ChatView::open_changes_for(&mut self, target: Target, from_branches: bool, cx)` (`Target::Own` leaves `other` empty). `repo::State::branch` and `base_choice` keep feeding Details; `TargetState` is cleared when Changes closes or the selected folder changes.

- [ ] **Step 1: Write the failing tests:** `closing_other_target_keeps_own_numbers` (own `BranchModel` and `base_choice` unchanged after an other target's stat arrives and Changes closes), `other_target_base_choice_is_its_own`, `refused_target_falls_back_to_own` (an error reply: "Nothing to compare." with the reason, and the next round asks the own folder), `folder_change_clears_other_target`, `file_request_carries_the_target`, `offer_page_only_for_own_new_uncommitted_html`, `close_from_branches_returns_there`, `own_target_from_branches_returns_there`; in `branch_tests.rs`: `ref_base_menu_leaves_out_the_ref_not_cwds_branch` (cwd on `main`, target `feature`: `main` listed, `feature` not), `remote_ref_leaves_out_the_remote_default`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib chat::changes_target` and `--lib chat::branch`. Expected: FAIL.
- [ ] **Step 3: Implement**; the existing Changes renders (`--render chat-git` with `NOTCHLING_RENDER_CHANGES=1|expanded|menu`) look as before for the own target.
- [ ] **Step 4: Run** the filter, `cargo test -p notchling-app --lib chat::`, and the three Changes renders (look at each PNG). Expected: PASS, unchanged pictures.
- [ ] **Step 5: Commit** `app: Changes for another worktree or branch`.

### Task 15: The two gates (pure)

Spec §7.9, *B24*.

**Zone:** auto. **Files:**
- Create: `crates/app/src/chat/git_gates.rs`, `git_gates_tests.rs`
- Modify: `crates/app/src/chat/mod.rs`

**Interfaces:**
- Produces: `pub const SLOW_EVERY_MS: i64 = 15_000;` `pub const PENDING_AGAIN_MS: i64 = 2_000;` `#[derive(Debug, Default)] pub struct SlowGate`; `pub fn ask(&mut self, why: git::Why, folder: Option<&str>, visible: bool, now_ms: i64) -> bool` (Opened and Folder: a new folder or 1 s since the last; TurnEnded: 1 s; Tick: a new folder, 15 s, or a due pending retry; never while one is in flight or not visible); `pub fn done(&mut self, pending: bool, now_ms: i64)` (a pending reply makes the next Tick due after 2 s); `pub fn reset(&mut self)`; `pub fn fresh_after<T: PartialEq>(before: &[(String, T)], after: &[(String, T)]) -> Vec<String>` (the worktree paths whose value changed, at most 2; the caller passes each worktree's occupancy kind, so this file needs nothing from Task 13).

- [ ] **Step 1: Write the failing tests:** `tick_every_15s`, `opened_and_folder_at_once_with_1s_floor`, `turn_end_1s_floor`, `one_in_flight`, `nothing_while_hidden_or_without_folder`, `pending_retry_after_2s`, `fresh_after_names_changed_worktrees_at_most_two`, `reset_lets_the_next_ask_through`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib chat::git_gates`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `app: gates for the worktree and branch lists`.

### Task 16: The Details Git section

Spec §7.1, §7.9, §7.11, §8, §10 (renders, bench), *B3*, *B17*, *B23*.

**Zone:** auto. **Files:**
- Create: `crates/app/src/chat/view/worktrees_ask.rs` (the off-thread `git.worktrees` call, its gate, the fresh paths from turn ends and occupancy changes; asked only after `git.status` said `repo: true`; no call under a fixture root)
- Modify: `crates/app/src/chat/view/repo.rs` (`selected_folder` is `worktrees::session_folder` (B17); the own stat through `changes_target::stat_request(…, &Target::Own, base_choice)`; `State.worktrees: Option<Arc<Worktrees>>`, kept on a failed call), `crates/app/src/chat/view/repo_section.rs` (rewritten: the current row, the other rows, the "›" row; the look of today's Changes row: `mx(-4) px(4) py(2)`, `RADIUS_SM`, Raised at 0.6 on hover, `MARK_BOX` icon column, `ROW_LINE`; two-line rows like today's file rows; the working dot is the Sidebar's; "this session" a pill like Changes' "Large diff" badge; a click opens Changes for `Target::Own` or `Target::Worktree(path)`, "›" opens Branches (Task 18 wires it; until then it is inert), a missing row does nothing), `crates/app/src/chat/view/inspector.rs` (`DetailsModel.git: Option<Arc<GitSection>>` replaces `git` and `changes`; `model` builds it with no I/O), `crates/app/src/chat/git.rs` (`title`, `hidden`, `hidden_text`, `SHOWN_FILES` and the file rows' code go; `preview_model` gains `changed_at`), `crates/app/src/chat/worktrees.rs` (`pub fn preview() -> Worktrees`: three lanes, one running, one unread, one missing), `crates/app/src/chat/view/load.rs` (the tick, the turn end and a sessions update call `worktrees_ask`), `crates/app/src/chat/view/mod.rs` (`mod worktrees_ask;`), `crates/app/src/chat/view/html_panel.rs` (`open_git_row` goes), `docs/notes/` untouched (Task 19)
- Test: `crates/app/src/chat/git_tests.rs` and `crates/app/src/chat/details_words_tests.rs` (the tests of `title`, `hidden_text` and `SHOWN_FILES` go with that code), `crates/app/tests/git_models.rs` (new)

- [ ] **Step 1: Write the failing tests:** `failed_call_keeps_last_rows` (pure: `repo::State` keeps `worktrees` on an error), `no_worktrees_call_before_status_says_repo`, `stat_asks_pick_nearest_unless_base_chosen`, `selected_folder_follows_worktree_path`; `crates/app/tests/git_models.rs` `models_take_no_io` (the source of `chat/worktrees.rs`, `chat/branch_list.rs`, `chat/changes_target.rs`, `chat/git_gates.rs` names none of `std::fs`, `std::process`, `Command`, `File::`, `read_dir`, `metadata(`, `actions`).
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib chat::` and `cargo test -p notchling-app --test git_models`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both; `python3 tools/locale/gen.py --check`; `notchling-app --render chat-git --out /tmp/git.png` (light and dark) and look: the section matches the mockup's option C; render `chat-git` on `origin/agent-run-oss` and here with the same fixture and check the Tasks, Session Facts, Subagents and Background Shells parts are pixel-identical (Git is the last section); `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1` with every part's draw count unchanged. Expected: PASS.
- [ ] **Step 5: Commit** `app: the Git section shows every worktree in one line`.

### Task 17: The Branches panel's model (pure)

Spec §7.2, §8, *B4*, *B5*, *B6*, *B8*, *B19*, *B20*, answer 2.

**Zone:** auto. **Files:**
- Create: `crates/app/src/chat/branch_list.rs`, `branch_list_tests.rs`
- Modify: `crates/app/src/chat/mod.rs`, `locales/en/mac/chat.ftl`

**Interfaces:**
- Consumes: the `git.branch_list` reply (Task 11); Task 13 `Work`, `Occupancy`, `SubPart`, `shown_base`, `Target`.
- Produces:
  - `pub const BRANCH_LIST_OP: &str = "git.branch_list";` `Branch` (every reply row field), `BranchList { name, default, integration, fetched_at, truncated, pending, branches }`; `pub fn parse_branch_list(reply: &Value) -> Option<BranchList>`; `pub fn branch_list_request(cwd: &str) -> Request`.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Tab { Working, NotMerged, Merged }`, `pub enum Group { Worktrees, NoWorktree }`; `pub fn in_tab(b: &Branch, tab: Tab) -> bool` (Working: a worktree, or a base without one; Not Merged: not a base and (`merged_into` null or a worktree with uncommitted files); Merged: not a base, `merged_into` set, and no worktree or one whose read work has no uncommitted file); `pub fn tab_counts(list: &BranchList) -> [usize; 3]`.
  - `pub enum Right { Lines(u64, u64), Pending, Empty }`; `pub struct Buttons { pub changes: bool, pub open_session: bool, pub finder: bool }`; `pub struct PanelRow { pub key: String, pub cloud: bool, pub name: String, pub right: Right, pub sub: Vec<SubPart>, pub current: bool, pub working: bool, pub buttons: Buttons, pub target: Option<Target>, pub open: Option<Open>, pub reveal: Option<String> }`.
  - `pub enum PanelState { Loading, Ready, Failed(String) }`; `pub struct PanelInput { pub list: Option<Arc<BranchList>>, pub failed: Option<String>, pub occupancy: Vec<(String, Option<Occupancy>)>, pub session_names: HashMap<String, String>, pub minute_ms: i64 }` (`session_names` keyed by the `Open` id: a session's title, a subagent's description); `pub struct PanelModel { pub repo: String, pub fetched: String, pub tabs: [(Tab, Option<usize>); 3], pub groups: Vec<(Group, Vec<PanelRow>)>, pub note: Option<String>, pub state: PanelState, pub truncated: bool }`; `pub fn panel(input: &PanelInput, tab: Tab, query: &str) -> PanelModel` (with a `list`, its rows show and `failed` is ignored, so a failure after a good reply keeps the last rows; `Failed` only without one).
  - Rows: right side a worktree's `work`, else its `lines`, else Pending; a branch without a worktree its `lines`; Empty for merged and bases (answer 2). Sub-line parts in the spec's order (where: "main checkout", the worktree path relative to the main checkout's path when inside it (component-wise), else the absolute path, "local", or "GitHub only"/"<remote> only"; the session "“name” running", "“name” needs input" or "“name” idle"; "last commit … ago “subject”" ("just now" under a minute); push state; "not merged" or "merged into …"; "N files · 2 ahead", or "no common history" for an `unrelated` row; "5 uncommitted" for a worktree with uncommitted files). Groups: Worktrees (this session's, running, then by path), No Worktree (newest commit first). Filter: every word, any case, anywhere in the name, the worktree path, the session name or the subject (reuse `branch::matches` per field). Changes off for a name `clean_ref` refuses (B8) and for a missing folder; Open Session only with a match; Show in Finder only for a worktree that is not `missing`.
- Words: `chat-branches-title` "Branches", `chat-branches-tab-working` "Working", `chat-branches-tab-not-merged` "Not Merged", `chat-branches-tab-merged` "Merged", `chat-branches-tab-count` "{ $tab } ({ $count })", `chat-branches-filter` "Filter Branches", `chat-branches-worktrees` "Worktrees", `chat-branches-no-worktree` "No Worktree", `chat-branches-main-checkout` "main checkout", `chat-branches-local` "local", `chat-branches-github-only` "GitHub only", `chat-branches-remote-only` "{ $remote } only", `chat-branches-session` "“{ $name }” { $state }", `chat-branches-state-running` "running", `chat-branches-state-idle` "idle" (needs input reuses `chat-git-needs-input`), `chat-branches-last-commit` "last commit { $age } ago", `chat-branches-last-commit-subject` "last commit { $age } ago “{ $subject }”", `chat-branches-last-commit-now` "last commit just now", `chat-branches-last-commit-now-subject` "last commit just now “{ $subject }”", `chat-branches-merged-into` "merged into { $base }", `chat-branches-not-merged` "not merged", `chat-branches-detached` "detached", `chat-branches-fetched` "Remote branches as of { $age } ago", `chat-branches-fetched-now` "Remote branches as of just now", `chat-branches-never-fetched` "Never fetched", `chat-branches-reading` "Reading the branches…", `chat-branches-cant-read` "Can't read the branches.", `chat-branches-no-other-worktrees` "No other worktrees.", `chat-branches-empty` "No branches here.", `chat-branches-no-match` "No branches match.", `chat-branches-truncated` "Not all branches are shown.", `chat-branches-changes` "Changes", `chat-branches-open-session` "Open Session"; Show in Finder reuses `chat-show-in-finder`, Close `chat-changes-close`, "no common history" `chat-git-no-common`.

- [ ] **Step 1: Write the failing tests:** `parse_reply_fields`, `tabs_are_views_not_a_partition`, `bases_only_in_working_without_worktree`, `uncommitted_worktree_never_merged`, `unread_worktree_waits_out_of_merged`, `right_side_work_then_lines_then_pending`, `merged_and_base_rows_have_no_numbers`, `groups_order`, `filter_every_word_any_field_any_case`, `remote_only_github_or_remote_name`, `cloud_for_remote_only`, `changes_off_for_refused_name`, `changes_and_finder_off_for_missing_folder`, `open_session_only_with_a_match`, `detached_row_name`, `fetched_words`, `where_main_relative_or_absolute` (`.claude/worktrees/lane-ask`; a lane outside the main checkout by its absolute path; `/r/main-2` is not inside `/r/main`), `session_part_running_needs_input_idle_and_subagent_name`, `ages_under_a_minute_say_just_now`, `unrelated_row_says_no_common_history`, `failed_after_good_keeps_rows`, `states_loading_failed_empty_no_match`, `no_other_worktrees_note`, `truncated_note`, `tab_counts_absent_while_loading`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib chat::branch_list`. Expected: FAIL.
- [ ] **Step 3: Implement**; add the words; `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the filter and `python3 tools/locale/gen.py --check`. Expected: PASS.
- [ ] **Step 5: Commit** `app: the Branches panel's model`.

### Task 18: The Branches panel

Spec §7.2, §7.3 (close order), §8, *B4*, *B7*, *B16*.

**Zone:** auto. **Files:**
- Create: `crates/app/src/chat/view/branches.rs` (ChatView's side: `open_branches`, `close_branches`, the gated off-thread `git.branch_list` call (15 s, 2 s while `pending`), pushing `PanelInput`, B7 on session switch, Changes from a row with `from_branches`, Open Session (`select` for a session, `open_subagent` for a subagent), Show in Finder (`cx.reveal_path`)), `crates/app/src/chat/view/branches_panel.rs` (`BranchesPanel` entity, which keeps its last good list when a call fails: header 36 pt with the branch icon, "Branches", the repository pill and Close; the fetched line; tabs as Ghost buttons with a lit selected state; a single-line `TextEditor` filter; the groups; a click opens a row's button line, one at a time; cached part), `crates/kit/assets/icons/cloud.svg` (`lucide-static` v0.460.0's `cloud`, with the license line `git-branch.svg` carries), `crates/app/src/chat/branches_preview.rs` (`pub fn preview(state: &str) -> PanelInput`)
- Modify: `crates/kit/src/icon.rs` (`Icon::Cloud`: SF Symbol `cloud`, text `☁`, the SVG; `ALL` and `SVGS` lengths grow by one), `crates/app/src/chat/view/pane.rs` (`Parts.branches: Entity<BranchesPanel>`; `redraw_parts` also redraws it), `crates/app/src/chat/view/load.rs` (`tick` runs the Branches gate; `select` reloads or closes Branches (B7) and clears its open row and filter; `apply` pushes the new occupancy into the panel), `root.rs` (`.children(branches::render(…))` beside `changes::render`; `.window_region` before `.part_at("Branches Panel", …)`), `repo.rs` (`details_visible` also needs Branches closed; `State.branches_shown`), `crates/app/src/chat/view/mod.rs` (the modules; `toggle_details` closes the page, then Changes (back to Branches when it came from there), then Branches, then hides), `crates/app/src/chat/panels.rs` (`pub enum HideStep { ClosePage, CloseChanges, CloseBranches, HideDetails }`, `pub fn hide_step(page: bool, changes: bool, branches: bool) -> HideStep`), `repo_section.rs` (the "›" row opens Branches), `crates/app/src/chat/renders.rs` and `crates/app/src/mac/views.rs` (`NOTCHLING_RENDER_BRANCHES=working|not-merged|merged|open-row|loading|error|empty` on `chat-git`)
- Test: `crates/app/src/chat/panels_tests.rs` (additions)

**Interfaces:**
- Consumes: Tasks 14, 15, 16, 17.
- Produces: `BranchesPanel::set_input(&mut self, input: Arc<PanelInput>, cx) -> bool` (false and no redraw when equal); `ChatView::open_branches(&mut self, cx)`, `close_branches(&mut self, cx)`, `branches_visible(&self) -> bool`.

- [ ] **Step 1: Write the failing tests:** `hide_step_order` (page, then Changes, then Branches, then Details), `changes_from_branches_closes_back_to_branches`, `session_switch_reloads_or_closes` (B7, through a pure helper in `branches.rs` over the selected folder), `open_row_and_filter_clear_on_switch`, `equal_input_is_no_redraw`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib chat::panels` and `--lib chat::`. Expected: FAIL.
- [ ] **Step 3: Implement.** Kit styles only (`KitInk`, `KitType`, the kit's buttons), pointer cursor on every clickable row; hover, sizes and layout of every other part unchanged.
- [ ] **Step 4: Run** `cargo test -p notchling-app --lib chat::`, `cargo test -p notchling-kit --lib icon`, `python3 tools/locale/gen.py --check`, every `NOTCHLING_RENDER_BRANCHES` state over `--render chat-git` (light and dark) and look at each PNG against the mockup's option B panel, and the chat bench as in Task 16. Expected: PASS, counts unchanged.
- [ ] **Step 5: Commit** `app: the Branches panel`.

### Task 19: Docs

**Zone:** maintainer for `docs/notes/`, `docs/feature-map.md`, the crates' `AGENTS.md`; auto for `docs/local-api-ops.md`, `changes/`. **Files:**
- Modify: `docs/local-api-ops.md` (the `git_ops.rs` section: `git.worktrees`, `git.branch_list`, the new arguments and reply fields, the guard, the caches and cadence; the quiet list gains both ops), `docs/notes/chat-details.md` (Changes gains targets; a Branches entry; the Git section's rows), `docs/feature-map.md` (the two Git rows: new files and tests, `--render chat-git` with `NOTCHLING_RENDER_BRANCHES`), `crates/engine/AGENTS.md` (`git/worktrees.rs`, `refs.rs`, `nearest.rs`, `branch_list.rs`, `reader.rs`: the formats, D1, D5, the rev rule), `crates/notchlingd/AGENTS.md` (`git_cache.rs`, `git_args.rs`, `git_view.rs`, `git_worktrees.rs`, `git_branch_list.rs`, `git_targets.rs`: the lock rule, the read choice, the guard, why no fetch), `crates/app/AGENTS.md` (`chat/worktrees.rs`, `branch_list.rs`, `changes_target.rs`, `git_gates.rs`, `view/branches*.rs`: the two states, the gates, minute ages)
- Create: `changes/git-branches.md` (`kind: feature`; one paragraph: every worktree in the Details Panel's Git section, a Branches panel, numbers against each branch's nearest base)

- [ ] **Step 1: Write.**
- [ ] **Step 2: Run** `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings` (on the Mac), `cargo fmt --all --check`, `cargo deny check`, `tools/test-python.sh`, `PYTHONPATH=tools/guard python3 -m guard lint` (and `featuremap`, `claudemd`, `codeowners --check`), `python3 tools/strip_comments.py --check`, `python3 tools/locale/gen.py --check`. Expected: clean, PASS.
- [ ] **Step 3: Commit** `docs: git branches`.

### Task 20: Live check (the maintainer)

Not an implementer task. After the maintainer merges and installs. This repository: the main checkout on `agent-run-oss`, lanes under `.claude/worktrees/`.

**Details:**
- [ ] Select a session started inside a lane (`isolation: "worktree"` or `cd` first): the Git section's first row is that lane, "this session", its right side against `agent-run-oss` (tens of lines, not the 1,493-commit diff), its sub-line `N uncommitted +a −r · last … ago · even with origin · vs agent-run-oss` (or "no upstream" before the lane is pushed).
- [ ] The other lanes are rows with `N files · K ahead`; a lane whose session or subagent runs shows the orange dot and "running …" within a second of the Sidebar's dot; others "last … ago". `git -C <lane> rev-list --count agent-run-oss..HEAD` matches each "K ahead".
- [ ] The last row reads "main and N branches" (or "N other branches"); N matches `git for-each-ref refs/heads refs/remotes` minus worktrees, the remote duplicates and `origin/HEAD`.
- [ ] A session that ran EnterWorktree: its Git section and Changes read the worktree, and the main checkout is an other row.
- [ ] Click a lane row: Changes shows that lane against its base; pick another base, close: the "this session" row's numbers and base did not change.

**Branches:**
- [ ] "›" opens Branches in Changes' slot; "Remote branches as of …" matches `stat -f %m "$(git rev-parse --git-common-dir)/FETCH_HEAD"`.
- [ ] Merged's count matches `git for-each-ref --merged=agent-run-oss refs/heads refs/remotes` plus those merged only into `origin/main`, minus the bases and the dirty worktrees; a clean, merged lane is in Merged's Worktrees group.
- [ ] A GitHub-only branch: "GitHub only", the cloud icon; Changes shows its committed work; closing Changes returns to Branches; Show in Finder and Open Session work on a lane row.
- [ ] The filter finds a lane by a word of its last commit's subject.
- [ ] Hide Details closes the file panel, then Changes, then Branches, in that order.

**Cost and safety:**
- [ ] With Details and Branches open for 10 minutes: `FETCH_HEAD`'s mtime is unchanged; the Activity shows no `git.worktrees` or `git.branch_list` successes; `notchling` stays near idle in Activity Monitor (compare with Details hidden).
- [ ] `grep -r "github.com/" ~/Library/Application\ Support/Notchling/*.jsonl` finds no remote URL from these ops.
