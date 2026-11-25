# Providers, Phase 3: Delegate Everywhere Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `notchling delegate` works from every tool and to every tool: Claude Code becomes a target (`claude -p` on the main login, `sonnet` at `medium` by default); the caller's session is found innermost first (`NOTCHLING_SESSION`, `CODEX_THREAD_ID`, `CLAUDE_CODE_SESSION_ID`, the process tree); every job is recorded (version 2), can run in the background, be waited on, looked at, listed and stopped; the `notchling-delegate` skill says which tool for what and what it costs.

**Architecture:** The engine's `delegate` module gains a Claude runner beside `codex_run.rs` and `agy_run.rs`, behind the same `Deps` seams; a pure caller chain in `contract::caller` over the environment and a `Procs` fake; the record grows to version 2 and is written for every job; background jobs are the same command re-run detached by `notchling` itself. Everything still runs without the daemon and without the `acting` switch, as `notchling delegate` does today.

**Tech Stack:** Rust 1.99.1, `std::process` behind `Deps` (no new crate), `platform::watch` for `--wait`, `platform::files::write_private` for records and `.out` files.

**Spec:** sections 10 (all), 12 (phase 3), 14.3, call 3. Phase 0 merged; Phase 1 (`Subagent` shape, `delegate-jobs` in the engine) merged; Phase 2 (`delegate.finished`) merged or landing beside: the skill's line about the finish signal ships only once Phase 2 is in. Calls C4, C17, C26 in the overview.

**Branch:** `agent/providers-p3-delegate`.

## Global Constraints

- Phase 0 to 2 constraints hold.
- **Call C4:** the `claude -p` flags for read-only and write runs and for effort come from the installed `claude --help` read in Task 3.1, not from this plan. The plan names candidates; the task writes down what the help says and builds on that.
- No test starts a real `claude`, `codex` or `agy`, uses a real login, or costs a turn: every runner test uses a fake script in `<root>/.local/bin` (as `crates/notchlingd/tests/delegate_cli.rs` does for Codex) or `FakeSpawner`. The first real Claude delegation is the maintainer's (it costs a turn on the Claude account).
- A job's environment never inherits `CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID`, `NOTCHLING_SESSION` or `CLAUDE_CONFIG_DIR`; it gets `NOTCHLING_SESSION=job:<id>` and `NOTCHLING_DELEGATE_DEPTH`. The depth limit stays 2.
- Records and `.out` files are 0600 in `<data>/delegations/`, kept 30 days; version 1 records still read, as Claude Code parents.
- The skill's two pinned sentences stay word for word: "Nested delegation stops at two levels." and "It sees the folder, not your context."
- Logging P5 5.7 (`delegate.job` kind), version sync P1 T10 (a warning in `delegate_cli.rs`) and cluster B T13 (`--machine`) touch the same files: rebase on whichever merged first; none changes the same lines this plan does except the usage string (merge both flags into it).

## Review Focus

1. A job that delegates again names its own job as parent, never the session above it, whichever variables it inherited. (Task 3.3 test `every_combination_of_the_three_variables`, Task 3.4 test `a_jobs_environment_holds_only_its_own_id`.)
2. `--stop` kills only a process that is still `notchling delegate` with that job id (pids are reused). (Task 3.5 test `stop_refuses_a_reused_pid`.)
3. The Claude target runs on the main login: `CLAUDE_CONFIG_DIR` is gone from its environment even when the caller runs in a lane. (Task 3.2 test `the_claude_job_drops_the_lane`.)
4. A read-only Claude job cannot write: the flags from `claude --help` deny edits without `--write`. (Task 3.2 test `without_write_the_claude_job_is_read_only`, which asserts the exact argv the help supports.)
5. `--wait` on a job whose pid vanished without an end in its record answers within a bounded time with "stopped", not forever. (Task 3.5 test `a_lost_pid_ends_the_wait`.)

## Order and lanes

```
3.1 claude --help, Claude options ─► 3.2 Claude runner ─┐
3.3 caller chain ─► 3.4 record v2, env scrub, agy runner env ─► 3.5 background, wait, status, jobs, stop
3.6 skill text (after 3.1; its finish line after Phase 2)
3.7 delegate-models claude (after 3.1)
3.8 cells, matrix, notes (last)
```

Lanes: **A** the Claude target (3.1, 3.2, 3.7: `delegate/options.rs`, `delegate/claude_run.rs`, `command.rs`), **B** the caller and the record (3.3, 3.4: `contract/caller.rs`, `delegate/record.rs`, `tools/agy/runner.rs`), then **C** the job commands (3.5: `notchlingd/src/delegate_cli.rs`, `delegate/jobs.rs`), **D** the skill (3.6).

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 3.1 | Maintainer (`delegate/**`) | none | in-crate, listed: `delegate/options_tests.rs` `errors` (`claude` was refused, now parses; the line moves to a tool name that still fails, `gemini`); `delegate/command_tests.rs` `a_command_line_that_cannot_run_exits_2_and_starts_nothing` (the message becomes `Name the tool first: claude, codex or agy.` and the case uses `gemini`). Tripped: `crates/notchlingd/tests/delegate_cli.rs` `a_bad_command_line_prints_the_usage_and_exits_2` (its `["delegate", "claude", "x"]` case becomes `["delegate", "gemini", "x"]`) |
| 3.2 | Maintainer | `CAP-PROCESS` (a new `Command` for `claude`, through `Deps`) | none |
| 3.3 | Maintainer (`contract/**`) | `CAP-PROCESS` if the real `Procs` reads `lsof` (reuse `accounts::procs`' runner; no new spawn if it already exists) | none |
| 3.4 | Maintainer (`delegate/**`, `tools/agy/**`) | `CAP-FILEWRITE` (record v2 written for every job, through the existing `Recorder`) | none: `record_tests` keep passing (v1 reads); new tests in `record_v2_tests.rs` |
| 3.5 | Maintainer (`notchlingd/**`, `delegate/**`) | `CAP-PROCESS` (detached re-run, `setsid`/`process_group`), `CAP-FILEWRITE` (`.out` files) | none: new `crates/notchlingd/tests/delegate_jobs.rs` |
| 3.6 | Maintainer (`delegate/skill.rs`) | none | none: the two pinned sentences stay; new tests only |
| 3.7 | Maintainer | none | none |
| 3.8 | Maintainer (`AGENTS.md`, feature map); auto (matrix) | `ZONE` | none |

## Maintainer only (Phase 3)

- Every task (the zone is `delegate`, `notchlingd`, `contract`), and the `CAP-PROCESS` and `CAP-FILEWRITE` hits in 3.2, 3.4, 3.5.
- The `WEAK-TEST-EDIT` on `crates/notchlingd/tests/delegate_cli.rs` (3.1): one case's tool name changes because `claude` is now a target.
- The first real `notchling delegate claude` run (a turn on the user's Claude account).

---

### Task 3.1: Read `claude --help`; the Claude target's options

**Files:**
- Modify: `crates/engine/src/delegate/options.rs` (`Tool::Claude`, raw `claude`, title `Claude Code`; `Tool::wire()` gives `claude`, `codex`, `antigravity`; `USAGE` names `<claude|codex|agy>`; `DEFAULT_CLAUDE_MODEL = "sonnet"`, `DEFAULT_CLAUDE_EFFORT = "medium"`), `crates/engine/src/delegate/command.rs` (the "Name the tool first" message), `crates/engine/AGENTS.md` (the flags found)
- Test: `crates/engine/src/delegate/options_claude_tests.rs` (new); the edits listed in the zones table

**Interfaces:**
- Produces: `options::Tool::{Claude, Codex, Agy}`; `impl From<options::Tool> for contract::Tool` (P6 removes the enum, call C23); `pub struct ClaudeFlags { pub read_only: Vec<&'static str>, pub write: Vec<&'static str>, pub effort: EffortFlag }`, `pub enum EffortFlag { Flag(&'static str), Settings(&'static str), None }`, `pub const CLAUDE_FLAGS: ClaudeFlags` (filled from Step 1).

- [ ] **Step 1: Read the help (call C4).** On the Mac: `claude --version` and `claude --help` (reading help starts no session and spends nothing). Write down, in the commit message and in `crates/engine/AGENTS.md`'s delegate notes: the version; how `-p` takes the prompt (argument or stdin); whether `--output-format stream-json` needs `--verbose` with `-p`; the model flag; the effort flag, or else a settings key passed with `--settings`, or none; the flags that limit tools (`--permission-mode`, `--allowedTools`/`--allowed-tools`, `--disallowedTools`, `--tools`, `--add-dir`, whichever exist) for a read-only run (no Edit, Write, NotebookEdit, no Bash that writes: the simplest is a read-only tool list) and for `--write` (edits in `--folder` only, permission mode that does not ask). If the help has no way to set effort, `--effort` on a Claude job is refused with `Claude Code takes no effort setting here.` and the default is the model's own.
- [ ] **Step 2: Write the failing tests** in `options_claude_tests.rs`: `claude_parses_with_sonnet_and_medium_by_default`, `claude_takes_a_model_and_an_effort`, `claude_is_read_only_without_write`, `an_unknown_tool_is_still_refused` (`gemini`), `the_usage_names_claude`. Then make the zones-table edits to the three existing tests.
- [ ] **Step 3: Run** `cargo test -p notchling-engine delegate::options`; expected FAIL.
- [ ] **Step 4: Implement**; `command::run` answers `Claude Code isn't wired yet.` for a Claude job until 3.2 (never merged in that state: 3.2 lands in the same pull request).
- [ ] **Step 5: Run** `cargo test -p notchling-engine delegate`, `cargo test -p notchling-notchlingd --test delegate_cli`. Expected: PASS.
- [ ] **Step 6: Commit** `git commit -m "delegate: claude is a target name; flags read from claude --help <version>"`.

### Task 3.2: The Claude runner

**Files:**
- Create: `crates/engine/src/delegate/claude_run.rs`, `crates/engine/src/delegate/claude_run_tests.rs`
- Modify: `crates/engine/src/delegate/command.rs` (dispatch `Tool::Claude`), `crates/engine/src/delegate/deps.rs` (`Deps::claude: Option<PathBuf>` found like `codex`: `<root>/.local/bin/claude` under a fixture root, else `PATH`), `crates/engine/src/delegate/real.rs`

**Interfaces:**
- Produces: `claude_run::run(options: &Options, deps: &Deps) -> Outcome`: argv `claude -p --output-format stream-json [--verbose if 3.1 says] --model <m> <effort per EffortFlag> <read-only or write flags>`, the prompt on stdin (or as the help says), cwd `--folder`; environment: the job's (3.4) minus `CLAUDE_CONFIG_DIR`; parses the stream: the `system` init line's `session_id` (the job's session), each `assistant` message's text, the final `result` line (`is_error`, `result`, `usage`); the reply is the `result` text; timeout and stop as `codex_run` (`CANCEL_GRACE`, `TERMINATE_AFTER`); `NO_BINARY: &str = "Couldn't find the claude command. Install Claude Code first."`.
- `notchling hook` already passes on a `claude -p` session, so the job never asks the Mac.

- [ ] **Step 1: Write the failing tests** (fake `claude` scripts that print a canned stream-json and record their argv and environment to files): `a_claude_job_prints_the_result`, `the_claude_job_drops_the_lane` (`CLAUDE_CONFIG_DIR` set in the caller, absent in the recorded environment), `without_write_the_claude_job_is_read_only` (the recorded argv holds exactly the read-only flags from 3.1), `with_write_it_edits_in_the_folder`, `an_error_result_exits_1`, `a_timeout_exits_124`, `no_claude_binary_says_so`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine delegate::claude_run`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine delegate`. Expected: PASS. `python3 -m guard lint`: `CAP-PROCESS` on the new spawn, expected.
- [ ] **Step 5: Commit** `git commit -m "delegate: notchling delegate claude runs claude -p on the main login"`.

### Task 3.3: The caller chain

**Files:**
- Create: `crates/engine/src/contract/caller.rs`, `crates/engine/src/contract/caller_tests.rs`
- Modify: `contract/codex/support.rs` (`delegate.caller` Native: `CODEX_THREAD_ID in every command Codex runs`), `contract/antigravity/support.rs` (`delegate.caller` Fallback `ProcessTree`: `agy exports no id; Notchling sets NOTCHLING_SESSION or walks the process tree`), `contract/*/mod.rs` (`WIRED` gains `delegate.caller` for all three), `gaps.json` (−2), `unwired.json` (−1), `contract/conformance_delegate.rs` (new)

**Interfaces:**
- Produces: `pub enum Via { Notchling, CodexThread, ClaudeSession, ProcessTree }` (wire `notchling`, `codex_thread`, `claude_session`, `process_tree`); `pub struct Caller { pub session: String, pub tool: Tool, pub via: Via }`; `pub fn find(env: &dyn Fn(&str) -> Option<String>, procs: &dyn Procs, roots: &Roots, me: u32) -> Option<Caller>`: `NOTCHLING_SESSION` (`job:<id>` is a job; any other row id is that session), then `CODEX_THREAD_ID` (`codex:<id>`), then `CLAUDE_CODE_SESSION_ID` (the Claude Code session), then the process tree: ancestors of `me` to the first `claude`, `codex` or `agy`; `claude` maps through `sessions/<pid>.json` in each lane's config folder; `codex` through the rollout it holds open; `agy` through the brain folder it holds open (`elsewhere`'s `lsof` reading). `Procs` is `accounts::procs::Procs`, faked by `FakeProcs`.

- [ ] **Step 1: Write the failing tests:** `every_combination_of_the_three_variables` (8 cases: the innermost set wins), `a_job_id_names_a_job`, `the_process_tree_finds_a_claude_session`, `the_process_tree_finds_a_codex_rollout`, `the_process_tree_finds_an_agy_brain_folder`, `nothing_found_is_none`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::caller_tests`; expected FAIL.
- [ ] **Step 3: Implement**; declarations, `WIRED`, the two JSON files in this commit.
- [ ] **Step 4: Run** `cargo test -p notchling-engine`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "contract: the caller chain, innermost first"`.

### Task 3.4: Record version 2, the scrubbed environment, the agy runner's id

**Files:**
- Modify: `crates/engine/src/delegate/record.rs` (`VERSION = 2`; new fields `job: String`, `parent_tool: Option<String>`, `parent_via: Option<String>`, `background: bool` (read since Phase 2), `result: Option<String>`; `tool` written by `Tool::wire()` so a new Antigravity record is `antigravity-<session>.json`; `read_all` keeps versions 1 and 2; a v1 record's parent is a Claude Code session), `crates/engine/src/delegate/command.rs` (`Context.record` is written for every job, with or without a parent), `crates/notchlingd/src/delegate_cli.rs` (`contract::caller::find` replaces the `PARENT_VARIABLE` read; the job's environment: remove the three ids and `CLAUDE_CONFIG_DIR`, set `NOTCHLING_SESSION=job:<id>`), `crates/engine/src/tools/agy/runner.rs` (each `agy acp` started for a session loaded by id gets `NOTCHLING_SESSION=antigravity:<id>`; a brand-new session's first `agy acp` gets none, call C26)
- Test: `crates/engine/src/delegate/record_v2_tests.rs` (new), `crates/engine/src/tools/agy/runner_env_tests.rs` (new), a new test in `delegate_cli.rs`'s inline module

**Interfaces:**
- Produces: `Delegation::job_id(&self) -> &str`, `Delegation::parent(&self) -> Option<(String, Tool, caller::Via)>`; `delegate::env::scrubbed(base: &[(String, String)], job: &str, depth: u32) -> Vec<(String, String)>`.

- [ ] **Step 1: Write the failing tests:** `a_v2_record_round_trips`, `a_v1_record_reads_as_a_claude_parent`, `an_antigravity_record_is_written_as_antigravity_and_agy_still_reads`, `a_job_without_a_parent_is_recorded`, `a_jobs_environment_holds_only_its_own_id` (in `delegate_cli.rs`'s module), `a_loaded_session_gets_notchling_session`, `a_new_session_gets_none`.
- [ ] **Step 2: Run**; expected FAIL.
- [ ] **Step 3: Implement.** The app's delegated rows (`flows::delegate_jobs`, Phase 1) read v2 parents: a job's parent may be a Codex or Antigravity session or another job.
- [ ] **Step 4: Run** `cargo test -p notchling-engine -p notchling-notchlingd -p notchling-app`. Expected: PASS (`record_tests` unchanged).
- [ ] **Step 5: Commit** `git commit -m "delegate: record v2 for every job; a job names itself to the jobs it starts"`.

### Task 3.5: `--background`, `--wait`, `--status`, `--jobs`, `--stop`

**Files:**
- Create: `crates/engine/src/delegate/jobs.rs`, `crates/engine/src/delegate/jobs_tests.rs`, `crates/notchlingd/tests/delegate_jobs.rs` (new e2e)
- Modify: `crates/engine/src/delegate/options.rs` (the five forms in `USAGE`), `crates/notchlingd/src/delegate_cli.rs` (dispatch; `--background` re-runs `notchling delegate` itself with the same arguments minus `--background`, detached in a new session (`process_group(0)` plus `setsid` through `pre_exec`, or what `std` offers), stdout and stderr to `<data>/delegations/<job>.out` (0600), prints `{"job": "<id>", "wait": "notchling delegate --wait <id>"}` and exits 0)

**Interfaces:**
- Produces: `jobs::wait(folder: &Path, job: &str, timeout: Option<Duration>, alive: &dyn Fn(u32) -> bool, watch: &dyn Fn(&Path, Duration) -> bool) -> Waited` (`Done(Outcome)`, `Failed(Outcome)`, `Stopped`, `Unknown`, `TimedOut`): returns when the record has `ended`, or when its pid is gone without an end (then Stopped, after one more read); exit codes 0 done, 1 failed, 2 stopped, 3 unknown job, 124 timeout; `jobs::status(folder, job) -> Option<Delegation>`; `jobs::list(folder, caller: Option<&Caller>, all: bool) -> Vec<Delegation>`; `jobs::stop(job: &Delegation, procs: &dyn Procs, signal: &dyn Fn(u32, Signal) -> bool) -> Result<(), StopError>` (SIGINT, then SIGTERM after `CANCEL_GRACE`, then SIGKILL after `TERMINATE_AFTER`, only while the pid's command line is `notchling delegate` holding that job id: the job id is in the detached run's arguments as `--job <id>`, a hidden flag).
- `--wait` prints what a foreground run prints, from the `.out` file and the record's `result`. None of the five needs a daemon or `acting`.

- [ ] **Step 1: Write the failing tests:** `jobs_tests.rs`: `wait_returns_on_an_end`, `a_lost_pid_ends_the_wait`, `wait_times_out_with_124`, `an_unknown_job_is_3`, `stop_refuses_a_reused_pid`, `stop_escalates_to_kill`, `jobs_lists_the_callers_session_only`, `all_lists_every_job`. `tests/delegate_jobs.rs` (fake `codex` and `claude` in `<root>/.local/bin`, `NOTCHLING_DATA_ROOT` and `HOME` in a scratch folder): `a_background_job_runs_and_wait_prints_its_result`, `a_background_job_that_fails_waits_to_1`, `a_job_that_delegates_again_shows_under_the_job`, `status_of_a_running_job`, `out_files_are_private`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine delegate::jobs`, `cargo test -p notchling-notchlingd --test delegate_jobs`; expected FAIL.
- [ ] **Step 3: Implement**; every daemon or child a test starts is stopped in a guard.
- [ ] **Step 4: Run** `cargo test -p notchling-engine -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "delegate: background jobs, wait, status, jobs and stop"`.

### Task 3.6: The delegate skill's new text

**Files:**
- Modify: `crates/engine/src/delegate/skill.rs` (`TEMPLATE`, `tool_note` per `SkillTool`), `crates/notchlingd/src/install_ops.rs` (nothing, unless the Claude Code copy needs a note per tool)
- Test: `crates/engine/src/delegate/skill_guidance_tests.rs` (new)

**Interfaces:**
- The text gains spec 10.4's five parts: which tool for what (Claude for review and the hardest work; Codex or Antigravity for images and for a second opinion from a different model; a session never asks its own tool for one); model and effort (`delegate-models`; cheap and low for lookups, stronger and high for review, the strongest only for the hardest; Claude's default `sonnet` at `medium`); cost (each Claude job is a turn on the user's Claude account and counts in its limits; Codex and Antigravity jobs spend those accounts the same way); waiting (`--background`, then `--wait <job>`; `--status`, `--jobs`); results (a finished job shows under the caller's session in the Details Panel, peeks the notch, raises an Inbox item for a background job; `--wait` prints the result).
- If decision 0019 has moved `TEMPLATE` to `crates/engine/data/instructions/notchling-delegate.md`, edit that file instead and bump its version as 0019 says; the text is the same in every copy (spec 10.4).

- [ ] **Step 1: Write the failing tests:** `the_skill_says_which_tool_for_what`, `the_skill_says_what_a_claude_job_costs`, `the_skill_says_how_to_wait`, `the_claude_default_is_named`, `every_copy_says_the_same` (the three `SkillTool` copies differ only in their tool note).
- [ ] **Step 2: Run** `cargo test -p notchling-engine delegate::skill`; expected FAIL. The existing inline tests (`Nested delegation stops at two levels.`, `It sees the folder, not your context.`) keep passing untouched.
- [ ] **Step 3: Write** the text: short, plain, no more than the five parts.
- [ ] **Step 4: Run** `cargo test -p notchling-engine`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "delegate skill: which tool, which model, what it costs, how to wait"`.

### Task 3.7: `delegate-models claude`

**Files:**
- Modify: `crates/engine/src/delegate/command.rs` (`models_listing`, `models_listing_json` take `claude`: the catalog's Claude Code models and efforts, `catalog::Catalog` from `data/catalog.json`, default marked)
- Test: in `command_tests.rs`'s sibling, new `delegate/models_claude_tests.rs`: `claude_models_come_from_the_catalog`, `the_default_is_sonnet_at_medium`, `json_lists_them_too`

- [ ] **Steps 1 to 5:** failing tests, run (FAIL), implement, run `cargo test -p notchling-engine delegate` (PASS), commit `git commit -m "delegate: delegate-models lists Claude Code's models"`.

### Task 3.8: Cells, matrix, notes

**Files:**
- Modify: `contract/claude/support.rs` (`delegate.target` Native: `notchling delegate claude: claude -p on the main login`), `contract/*/mod.rs` (`WIRED` gains `delegate.target` for all three), `gaps.json` (−1), `unwired.json` (−2), `contract/conformance_delegate.rs` (target checks over fake binaries), `docs/provider-matrix.md`, `crates/engine/AGENTS.md` (delegate: the Claude runner, record v2, `jobs.rs`, the caller chain), `crates/notchlingd/AGENTS.md` (`delegate_cli.rs`: background re-run, `--job`), `crates/AGENTS.md` (Data: delegations v2, `.out` files), `docs/feature-map.md`

- [ ] **Step 1:** declarations and files; **Step 2:** `cargo test -p notchling-engine` (conformance `*::delegate_*` run; summary `gaps 21`, `unwired 58`); **Step 3:** regenerate the matrix; **Step 4:** notes, `guard claudemd`, `strip_comments --check`; **Step 5:** commit `git commit -m "contract: delegate targets and callers wired for every tool"`.

## Checkpoint P3

- `cargo test --workspace --locked`, clippy, fmt, deny; `tools/test-python.sh`; `python3 -m guard lint` (the expected `CAP-*` hits only).
- Conformance summary `gaps 21`, `unwired 58`.
- By hand, over a fixture root with fake tools: `notchling delegate codex --background -- x`, then `--jobs`, `--status`, `--wait`; a job started from inside another job shows nested under it in the Details Panel.
- The pull request names the `WEAK-TEST-EDIT`, the `CAP-*` hits and asks the maintainer for the first real Claude delegation.
