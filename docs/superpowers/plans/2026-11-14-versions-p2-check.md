# Version sync, Phase 2: Check on the Mac Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When the Mac's version of a tool changes, the daemon checks it with files and flags only (user's decision 2): `--version`, `claude doctor`, `codex exec --help`, and Notchling's readers in strict mode over the transcripts already on the Mac. A passed check makes that version the **reference**, always labelled Partly Checked; a failed one is marked failed, with an Inbox notice. The soak keeps counting unknown lines on real sessions and is shown, never gating (user's decision 3). Settings > Agents gets Latest, Update, Check Again and Go Back.

**Architecture:** Pure parts in `engine::versions` (`check.rs` plans and judges a check, `reference.rs` keeps references and failed versions, `soak.rs` counts, `latest.rs` parses the vendors' latest files, `update.rs` plans the Mac's update command) and a `Strict` mode in `engine::transcript` that counts what the lenient readers skip. The daemon runs the commands through `Launcher` (`versions_check.rs`), stores results in `notchling.db` (`versions/references`, `versions/failed`), fills `versions.get`'s `reference`, `usable`, `failed` and `check`, and serves `versions.check`, `versions.latest` and `versions.update_local`.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app), the daemon's `Launcher` (`FakeLauncher` in tests), `curl` through the same `Launcher` for the Latest reads (the pattern of `welcome/updates.rs`'s `CurlFetcher`).

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: sections 7 (Latest), 10, 11 (all), 15 (Rolling back the Mac, A failed version), 17.1, 17.3 (`version_check_failed`), 18 (`versions.check`, `versions.latest`, `versions.update_local`), 19 (`version.checked`, `version.changed` causes), 21, 22 (Smoke check, Reference), 23 (Phase 2), user's decisions 2 and 3, coordinator's calls 1 and 3.

**Branch:** `agent/versions-p2-check`, off Phase 1's merge.

**Depends on:** Phase 1 (`engine::versions`, `versions.get`, `Recorder`). Runs beside Phase 3 (overview, lanes); the two share only `crates/local-api/src/wire.rs`, `crates/engine/src/inbox/model.rs`, `crates/engine/src/versions/record.rs`, `crates/engine/src/versions/mod.rs` and the docs, which are changed by one task at a time.

## Global Constraints

- Phase 1's constraints hold. In addition:
- **The check starts no session, opens no pty, sends no turn and writes nothing** to `~/.claude`, `~/.codex`, `~/.gemini` or `~/.claude.json`. Its only processes are one-shot commands that print and exit: `<tool> --version`, `claude doctor`, `codex exec --help`. It needs no switch.
- **Every built result is Partly Checked.** `CheckResult::Passed` exists in the type for the later additions (spec 11.2) and no code in this phase produces it.
- The soak never changes a reference, a result or (from Phase 4) a rollout. It is shown only.
- `versions.update_local` is app tier and needs the `install` switch on; with it off it answers `forbidden` with the switch's off message.
- Latest is read only when Settings > Agents opens (at most once an hour per tool) or on Check for Updates, never in the background; each read sends no identifier; https only.
- New network reads (`CAP-NETWORK`) and new processes (`CAP-PROCESS`) go to the maintainer; `docs/policies/privacy.md` changes in the same pull request (protected).
- Existing transcript tests are not edited: strict mode is new code beside the lenient readers, with new fixtures.

## Review Focus

1. The check writes nothing in the user's folders: a test snapshots every file's bytes under a temp home before and after a full check. (Task 4 `check_writes_nothing_under_home`.)
2. Only one-shot commands run: the launcher fake sees exactly `--version`, `doctor` and `exec --help`, never a pty, `-p`, `exec` with a prompt, `install` or `update`. (Task 4 `check_runs_only_one_shot_commands`.)
3. A reference is usable only for the binary it ran against: the same version string with another SHA-256 is stale, and Machines then shows "The Mac's check is out of date". (Task 3 `same_version_other_binary_is_stale`.)
4. A failed version is never made the reference again until Check Again passes or the Mac moves on, even if auto-update brings it back. (Task 3 `failed_version_stays_failed_when_it_returns`.)
5. Update with the `install` switch off does nothing and says so; Go Back is offered only for a method that installs an exact version. (Task 7 `update_refused_with_install_off`, `go_back_only_for_exact_methods`.)

---

### Task 1: Strict counters for the readers

**Zone:** auto (`crates/engine/src/transcript/**`). No CAP.

**Files:**
- Create: `crates/engine/src/transcript/strict.rs`, fixtures `crates/engine/tests/fixtures/versions/claude-unknown-block.jsonl`, `codex-unknown-event.jsonl`, `claude-mixed-versions.jsonl`
- Test: `crates/engine/src/transcript/strict_tests.rs`
- Modify: `crates/engine/src/transcript/mod.rs`, the places the lenient readers skip (`transcript/conversation/parser.rs`, `conversation/queue.rs`, `transcript/codex/`, `transcript/acp.rs`): each skip also calls `Skips::note` when a `Strict` is passed in. The lenient path's behavior and output don't change.

**Interfaces:**
- Produces:
  - `pub enum SkipKind { UnknownLine, UnknownBlock, UnknownQueueOp, ParseFailed, UnknownAcpUpdate, UnknownCodexEvent }` (serde `snake_case`).
  - `pub struct Skips { pub by_version: BTreeMap<String, BTreeMap<SkipKind, u32>>, pub lines_by_version: BTreeMap<String, u32>, pub turns_by_version: BTreeMap<String, u32> }` with `fn note(&mut self, version: Option<&str>, kind: SkipKind)`, `fn line(&mut self, version: Option<&str>)`, `fn turn(&mut self, version: Option<&str>)`; a missing version counts under `""`. Only kinds, never text.
  - `pub fn strict_claude(path: &Path, budget: u64) -> Skips`, `pub fn strict_codex(path: &Path, budget: u64) -> Skips`, `pub fn strict_acp(lines: &[String]) -> Skips` (a read only pass over a file's tail, using the same parsers; each Claude Code line's `version` field and each rollout's `session_meta.cli_version` tell the version; agy logs may carry none, spec §25).
- [ ] **Step 1: Write the failing tests:** `unknown_block_counts_under_its_version`, `unknown_codex_event_counts`, `old_lines_count_under_their_own_version` (the mixed fixture: an unknown block in a `2.1.323` line and none in `2.1.324` lines), `strict_pass_never_holds_text` (a planted marker in the unknown block's text is in no field of `Skips`' JSON), `lenient_reader_output_is_unchanged` (the conversation from `read` over the fixtures equals the same read before strict existed: compare with a golden made from the current code in this task's first commit).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib transcript::strict`. Expected: FAIL.
- [ ] **Step 3: Implement.** The strict pass opens files read only and holds no cursor cache (it never touches `Cursors`).
- [ ] **Step 4: Run** `cargo test -p notchling-engine --lib transcript::` and `cargo test -p notchling-engine --tests`. Expected: PASS.
- [ ] **Step 5: Commit** `transcript: a strict pass that counts what the readers skip`.

### Task 2: The check's plan and its judgment

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/check.rs`
- Test: `crates/engine/src/versions/check_tests.rs`
- Modify: `crates/engine/src/tools/codex/plan.rs` (export `pub const FLAGS_USED: &[&str]`, every flag the turn builder can pass; a test in `check_tests.rs` builds each kind of turn and asserts its flags are all listed), `crates/engine/src/versions/mod.rs`

**Interfaces:**
- Consumes: `Skips` (Task 1), `Tool`, `Version`.
- Produces:
  - `pub enum CheckPart { Version, Doctor, ExecFlags, Transcript, SelfTest }` (serde `snake_case`).
  - `pub struct Command { pub part: CheckPart, pub argv: Vec<String>, pub timeout_secs: u64 }`; `pub fn commands(tool: Tool, binary: &Path) -> Vec<Command>`: every tool `--version` (10 s); Claude Code `doctor` (30 s); Codex `exec --help` (10 s); Notchling none (its self test is in-process: Task 4).
  - `pub enum CheckResult { Passed, PartlyChecked, Failed, CantTell }` (serde `snake_case`: `passed`, `partly`, `failed`, `cant_tell`).
  - `pub struct Outcome { pub result: CheckResult, pub failed_part: Option<CheckPart>, pub why: Option<String>, pub unknown_count: u32, pub unchecked: Vec<&'static str>, pub duration_ms: u64 }`; `unchecked` is always `["screens", "dialogs", "turns"]` here. `why` is fixed text Notchling writes ("`codex exec` no longer takes `--json`"), never tool output.
  - `pub fn judge(tool: Tool, installed: &Version, outputs: &[(CheckPart, Option<(i32, String)>)], skips: &Skips) -> Outcome`: a `--version` that isn't exactly `installed` fails `Version`; `doctor` must exit 0 and contain a line starting `Auto-update` (case-insensitive; the exact text is checked against a real run by the builder, spec §25); `exec --help` must list every `FLAGS_USED`; any skip kind above zero under the installed version fails `Transcript` with the kind in `why`; skips under older versions are counted in `unknown_count` only; a command that didn't start or timed out gives `CantTell`; everything passing gives `PartlyChecked`.
- [ ] **Step 1: Write the failing tests:** `all_parts_passing_is_partly_checked_never_passed`, `version_text_mismatch_fails_version`, `doctor_nonzero_fails_doctor`, `help_missing_json_fails_exec_flags` (asserts `why` names `--json`), `unknown_block_in_new_version_fails_transcript`, `unknown_block_in_old_version_only_counts`, `timeout_is_cant_tell`, `flags_used_covers_every_turn_kind`, `outcome_why_never_quotes_output` (a planted marker in every output appears in no field).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::check`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `cargo test -p notchling-engine --lib tools::codex`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: the files-and-flags check and how it is judged`.

### Task 3: References and failed versions

**Zone:** maintainer (`crates/engine`). No CAP (rows in `notchling.db` through `store`).

**Files:**
- Create: `crates/engine/src/versions/reference.rs`
- Test: `crates/engine/src/versions/reference_tests.rs`

**Interfaces:**
- Consumes: Task 2; `Identity` (Phase 1); `store::{doc, update_doc}`.
- Produces:
  - `pub struct Reference { pub tool: Tool, pub version: Version, pub sha256: String, pub result: CheckResult, pub at_ms: i64, pub unchecked: Vec<String> }` (serde camelCase; `partly` in the wire reply is `result == PartlyChecked`).
  - `pub struct Book { refs: BTreeMap<Tool, Reference>, failed: BTreeMap<Tool, BTreeMap<String, Failed>> }`, `Failed { part: CheckPart, at_ms: i64 }`.
  - `fn apply(&mut self, tool: Tool, version: &Version, sha256: &str, outcome: &Outcome, now_ms: i64)`: `PartlyChecked` or `Passed` sets the reference and clears that version from `failed`; `Failed` adds it to `failed` and leaves the reference as it was; `CantTell` changes nothing (the daemon retries, Task 4).
  - `fn usable(&self, tool: Tool, installed: &Version, sha256: &str) -> bool`: a reference with the same version and SHA-256 that isn't in `failed`.
  - `fn is_failed(&self, tool: Tool, v: &Version) -> bool`; `fn check_again(&mut self, tool: Tool, v: &Version)` (clears its failed mark before a new check).
  - `pub fn load(data: &Path) -> Book`, `pub fn save(data: &Path, book: &Book) -> Result<(), String>` (docs `versions/references` and `versions/failed`; a bad doc loads empty and is logged through the recorder as `versions.load_failed`).
- [ ] **Step 1: Write the failing tests:** `partly_checked_becomes_the_reference`, `failed_keeps_the_old_reference`, `same_version_other_binary_is_stale`, `failed_version_stays_failed_when_it_returns` (fail 2.1.324, move to 2.1.325, come back to 2.1.324: still failed, not usable), `check_again_then_pass_clears_it`, `round_trips_through_the_store`, `bad_doc_loads_empty`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::reference`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same. Expected: PASS.
- [ ] **Step 5: Commit** `engine: version references, usable only for the binary they checked`.

### Task 4: The daemon runs the check

**Zone:** maintainer (`notchlingd`, `local-api`). `CAP-PROCESS`.

**Files:**
- Create: `crates/notchlingd/src/versions_check.rs`
- Test: `crates/notchlingd/src/versions_check_tests.rs`
- Modify: `crates/notchlingd/src/versions_ops.rs` (hold a `Book`; fill `reference`, `usable`, `failed`, `check`; run a check after a rescan sees a new version or identity; `versions.check`), `crates/local-api/src/wire.rs` (op `VERSIONS_CHECK = "versions.check"`, event `VERSIONS_CHECKED = "versions.checked"`), `crates/engine/src/versions/record.rs` (`Fact::Checked { tool, version, tier: CheckTier::FilesFlags, result, failed_part, unknown_count, duration_ms }`; `CheckTier` is new in `check.rs`, named so it never meets local-api's `Tier`), `docs/local-api-ops.md`

**Interfaces:**
- Consumes: Tasks 1 to 3; `Launcher`; Phase 1's `VersionsOps`.
- Produces:
  - `pub struct Checker { launcher: Arc<dyn Launcher>, roots: Roots, data: PathBuf, clock }`, `fn run(&self, tool: Tool, installed: &Installed) -> Outcome`: the commands of `check::commands` against the resolved binary path (never a name looked up on `PATH`), then the strict pass over the newest 20 Claude Code transcripts under the root's `.claude/projects` (each at most the chat tail's default 4 MB) or the newest 20 Codex rollouts; Notchling's own check is `hello` and `sessions.list` against the running daemon's `Ops` in process plus the strict pass over the fixtures compiled in with `include_str!` (`crates/engine/tests/fixtures/versions/self-test-*.jsonl`).
  - `versions.check {tool}` (`OpSpec::APP`) → at once `{"started": true}`, then `versions.checked {tool, version, result, failedPart, why, unknownCount, unchecked, at}` on the `versions` topic. A check already running for that tool answers `{"started": false, "running": true}`.
  - `CantTell` is tried again once after 2 minutes (clock), then counts as `Failed` with part `Version` and why "it didn't start".
  - `Fact::Checked` recorded for each finished check.
- [ ] **Step 1: Write the failing tests** (temp home with a native Claude Code layout and fake transcripts; `FakeLauncher` scripted per argv; a fake clock):
  - `check_runs_only_one_shot_commands` (the launcher saw `[<bin> --version]`, `[<bin> doctor]` and nothing else for Claude Code; never `run_pty`)
  - `check_writes_nothing_under_home` (hash every file under the temp home before and after; equal; no new file)
  - `passing_check_sets_a_partly_checked_reference` (`versions.get` then has `reference.result == "partly"` and `usable: true`)
  - `failing_help_marks_the_version_failed`, `cant_tell_retries_once_then_fails`
  - `new_binary_same_version_rechecks` (rescan with another identity starts a check)
  - `check_is_app_tier`, `second_check_while_running_is_not_started`
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_check`. Expected: FAIL.
- [ ] **Step 3: Implement.** The check runs on its own thread; no lock is held across a command.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd --lib versions_check` and `cargo test -p notchling-notchlingd --lib versions_ops`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: check a new tool version with files and flags`.

### Task 5: The soak, shown and never gating

**Zone:** maintainer (`crates/engine`, `notchlingd`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/soak.rs`
- Test: `crates/engine/src/versions/soak_tests.rs`, `crates/notchlingd/src/versions_soak_tests.rs`
- Modify: `crates/notchlingd/src/versions_ops.rs` (every 10 minutes, a strict pass over transcripts written since the reference's `at_ms` adds to the soak; `versions.get` gains `soak: {since, turns, unknown}`)

**Interfaces:**
- Consumes: Tasks 1 and 3.
- Produces: `pub struct Soak { pub since: Version, pub turns: u32, pub unknown: u32 }`, `fn add(&mut self, skips: &Skips)` (only counts under `since`), `fn line(&self) -> String` ("Since 2.1.324: 14 turns read, nothing unknown" / "…, 2 unknown lines"); docs row `versions/soak`. A new reference starts a new soak.
- [ ] **Step 1: Write the failing tests:** `counts_only_the_new_version`, `line_reads_plainly`, `new_reference_restarts_the_soak`, `soak_never_changes_the_reference` (an unknown count of 50 leaves `Book` equal and `usable` true).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::soak` and `cargo test -p notchling-notchlingd --lib versions_soak`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `versions: the soak counts unknown lines on real sessions and gates nothing`.

### Task 6: A notice when a check fails

**Zone:** maintainer (`crates/engine/src/inbox`, `notchlingd`). No CAP.

**Files:**
- Modify: `crates/engine/src/inbox/model.rs` (`Kind::VersionCheckFailed`, additive), `crates/notchlingd/src/versions_ops.rs` (raise through `InboxOps::raise` with no responder), `crates/notchlingd/src/daemon_wiring.rs` (hand `VersionsOps` the `Arc<InboxOps>`), `crates/app/src/inbox/model.rs` (the kind's row; `open` `{"settings": "agents", "tool": "<tool>"}` opens Settings > Agents)
- Test: `crates/notchlingd/src/versions_inbox_tests.rs`, `crates/app/src/inbox/versions_rows_tests.rs`

**Interfaces:**
- Produces: item `version-check:<random>` (fresh per event; crates/AGENTS.md, Inbox), kind `version_check_failed`, title "Claude Code 2.1.324 Didn't Pass", detail the outcome's `why` and "Workers stay on 2.1.323." when a reference exists, action `[Open "Open Settings" primary]`. The item resolves Withdrawn when the version passes on Check Again or the Mac moves on.
- [ ] **Step 1: Write the failing tests:** `failed_check_raises_one_item`, `pass_after_check_again_withdraws_it`, `ids_are_fresh_per_failure`, `row_opens_settings_agents` (app model).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_inbox` and `cargo test -p notchling-app --lib inbox::versions_rows`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both, and `cargo test -p notchling-engine --lib inbox::`. Expected: PASS.
- [ ] **Step 5: Commit** `inbox: a notice when a new tool version fails its check`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib versions::`, `cargo test -p notchling-notchlingd --lib versions_`, clippy, fmt, `guard lint`. Push. The check works end to end without the app's new controls.

### Task 7: Latest, Update and Go Back on the Mac

**Zone:** maintainer (`crates/engine`, `notchlingd`). `CAP-PROCESS` (the update commands), `CAP-NETWORK` (Latest reads through `curl`).

**Files:**
- Create: `crates/engine/src/versions/latest.rs`, `crates/engine/src/versions/update.rs`, `crates/notchlingd/src/versions_update.rs`
- Test: `crates/engine/src/versions/latest_tests.rs`, `crates/engine/src/versions/update_tests.rs`, `crates/notchlingd/src/versions_update_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (ops `VERSIONS_LATEST = "versions.latest"`, `VERSIONS_UPDATE_LOCAL = "versions.update_local"`; event `VERSIONS_UPDATED = "versions.updated"`), `crates/notchlingd/src/versions_ops.rs`, `crates/notchlingd/src/quiet_ops.rs` (`versions.latest` quiet), `crates/engine/src/versions/record.rs` (`Changed` gets `cause: UserUpdate` or `Rollback` when the daemon ran it), `docs/local-api-ops.md`, `docs/policies/privacy.md` (what is fetched, when, that nothing identifying is sent)

**Interfaces:**
- Produces:
  - `latest.rs`: `pub fn source(tool: Tool) -> Option<&'static str>` (Claude Code `https://downloads.claude.ai/claude-code-releases/latest`; Codex `https://registry.npmjs.org/@openai/codex/latest`; agy's manifest URL from spec §7 for the Mac's platform; Notchling none: its update check stays in `welcome/updates.rs`); `pub fn parse(tool: Tool, body: &str) -> Option<Version>` (Claude: the body is the version; npm: its `version` field; agy: the manifest's version field, named from the installer by the builder).
  - `update.rs`: `pub fn update_argv(tool: Tool, method: Method, to: Option<&Version>) -> Result<Vec<String>, String>`: Claude native `claude update`, or `claude install <v>` with `to`; npm `npm install -g @anthropic-ai/claude-code@latest` or `@<v>`; brew `brew upgrade --cask <cask>` (no `to`); Codex standalone the installer pipe with `--release <v>` only for `to`, else `codex update` only when the builder confirms it exists (spec §26), else the installer; agy `agy update` (no `to`). A `to` for a method that can't pin is `Err("Homebrew can't install an exact version")`.
  - `versions.latest {tool}` (`OpSpec::APP_QUIET`) → `{latest, at}`; cached per tool for an hour; a fetch through `curl --proto =https -fsSL --max-time 10 <url>` with the launcher; errors give `{latest: null, at, error: "couldn't reach <host>"}`.
  - `versions.update_local {tool, to?}` (`OpSpec::APP`, behind the `install` switch) → at once `{started}`, then `versions.updated {tool, from, to, ok}`; after it the watcher sees the change and the check runs (Task 4). Go Back passes `to` = the version before the current one, known from `versions/seen`.
- [ ] **Step 1: Write the failing tests:**
  - `latest_tests.rs`: `each_body_parses` (recorded bodies as new fixtures), `junk_body_is_none`, `sources_are_https_on_vendor_hosts`
  - `update_tests.rs`: `argv_per_method`, `exact_only_where_it_can_pin`, `to_must_be_a_checked_version`
  - `versions_update_tests.rs`: `update_refused_with_install_off`, `update_runs_the_methods_argv` (FakeLauncher), `go_back_only_for_exact_methods`, `latest_is_cached_for_an_hour`, `latest_error_names_only_the_host`, `update_is_app_tier`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::latest`, `cargo test -p notchling-engine --lib versions::update`, `cargo test -p notchling-notchlingd --lib versions_update`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the three. Expected: PASS.
- [ ] **Step 5: Commit** `versions: Latest, Update and Go Back on the Mac`.

### Task 8: Settings > Agents: Latest, Update, the Check line and the soak

**Zone:** auto (`crates/app/src/sheets`). No CAP.

**Files:**
- Modify: `crates/app/src/sheets/client_model.rs`, `crates/app/src/sheets/client_view.rs`, `crates/app/src/sheets/settings.rs` (ask `versions.latest` when the page opens; send `versions.check` and `versions.update_local` off the UI thread; follow `versions.checked` and `versions.updated`), `crates/app/src/sheets/renders.rs` (`settings-agents-client-failed`, `settings-agents-client-update`)
- Test: `crates/app/src/sheets/client_model_check_tests.rs`

**Interfaces:**
- Produces: `ClientPart` gains `latest: Option<String>` ("Latest 2.1.325"), `update: Option<UpdateButton { enabled, note }>` (shown only when Latest is newer and the method can update; with the `install` switch off `enabled: false`, note "Turn on Install in Settings first"), `check: CheckLine { text, failed, details }` ("Partly checked 14:03 · version, doctor, transcripts" and, under it, "Not checked: screens, dialogs and turns"; a failure "Failed 14:03: `codex exec` no longer takes `--json`. Workers stay on 0.166.0."), `soak: Option<(String, bool)>` (the line and red when unknown > 0), `go_back: Option<String>` ("Go Back to 2.1.323"; its confirm says "Claude Code will update itself again at its next check. Workers stay on 2.1.323.").
- [ ] **Step 1: Write the failing tests:** `update_shows_only_when_newer_and_possible`, `install_off_disables_update_with_a_note`, `check_line_says_partly_and_what_wasnt_checked`, `failed_check_line_is_red_with_details`, `soak_line_red_when_unknown`, `go_back_only_with_an_exact_method_and_a_previous`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib sheets::client_model_check`. Expected: FAIL.
- [ ] **Step 3: Implement** the model, then the view.
- [ ] **Step 4: Run** the tests; render the two new states in light and dark and look.
- [ ] **Step 5: Commit** `settings: Latest, Update, Check Again and the soak in the Client part`.

### Task 9: Machines uses the reference

**Zone:** auto (`crates/app/src/machines`). No CAP.

**Files:**
- Modify: `crates/app/src/machines/versions_model.rs` (compare with `reference` only; no usable reference gives "The Mac's check is out of date" on the banner and `Unknown` marks; drop Phase 1's unchecked fallback), `crates/app/src/machines/versions_feed.rs` (map `reference`, `usable`)
- Test: `crates/app/src/machines/versions_model_ref_tests.rs`

- [ ] **Step 1: Write the failing tests:** `marks_follow_the_usable_reference`, `stale_reference_shows_out_of_date_and_no_marks`, `failed_version_is_never_the_mark_target`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib machines::versions_model_ref`. Expected: FAIL.
- [ ] **Step 3: Implement.** Phase 1's `unchecked_mac_version_is_labelled` test stays as written: it asserts the label when `reference` is absent, which still happens before a first check.
- [ ] **Step 4: Run** `cargo test -p notchling-app --lib machines::`. Expected: PASS.
- [ ] **Step 5: Commit** `machines: compare workers with the Mac's checked version`.

### Task 10: Docs and checks

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/AGENTS.md` (the Versions entry: the check, Partly Checked, the soak never gates, `install` switch for Update), `crates/engine/AGENTS.md` (`versions/check.rs`, `reference.rs`, the strict pass), `crates/notchlingd/AGENTS.md` (`versions_check.rs`, `versions_update.rs`), `crates/app/AGENTS.md`, `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Run** fmt, clippy (on the Mac), `cargo test --workspace --locked`, `guard lint`, `guard featuremap`, `guard rules`, `python3 tools/strip_comments.py --check`. Expected: clean.
- [ ] **Step 2: Write** the docs.
- [ ] **Step 3: Commit** `docs: tool versions, phase 2`. The pull request body lists the `CAP-PROCESS` and `CAP-NETWORK` lines and the privacy.md change.

### Checkpoint 2 (maintainer, on the Mac)

After the maintainer's install: Settings > Agents > Claude Code shows "Partly checked" for the installed version and the soak line; Check Again re-runs it in seconds; with Install on, Update runs `claude update` and the new version is checked by itself. The builder's verifications of spec §25 that this phase relies on (`claude doctor`'s auto-update line, `codex exec --help`'s flag list, Codex's standalone layout, Claude Code's per-line `version` on a fresh transcript) are recorded in the ledger with what each printed (no account details).
