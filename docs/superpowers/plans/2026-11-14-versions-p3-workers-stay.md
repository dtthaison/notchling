# Version sync, Phase 3: Workers Stay Put Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** No worker updates a tool on its own. Machines gets a **Self-Update Off** step per tool that sets one key in every place it knows (each Claude Code config folder, Codex's `config.toml`, the sd service files), with a backup and nothing else changed, and **Self-Update On** that takes back exactly what Notchling added. A worker whose version differs from the Mac's outside a rollout is **drift**: marked on its card and announced once in the Inbox.

**Architecture:** Pure edits on the Mac: `install::edit::set_member` (JSONC, beside `append`/`remove`) and a new `install::toml_edit` (one top-level key or one key under `[features]`), planned by `versions::knob` into a `KnobPlan` that shows one line and carries the SHA-256 of the text it was planned from. `machines::remote_edit` reads a worker's file over ssh (at most 1 MiB) and writes the planned text through stdin with a hash check, a 0600 backup and a rename. `machines::steps_selfupdate` puts it in Machines' plan, confirm, run. `versions::drift` compares facts; the daemon raises one notice per worker, tool and found version.

**Tech Stack:** Rust (engine, notchlingd, GPUI app), `ScriptedRunner` for every ssh call, `install`'s existing fixtures.

**Spec:** `docs/superpowers/specs/2026-11-13-version-sync-design.md`: sections 9 (all), 14 (Drift, ahead), 17.2 (Self-Update Off), 17.3 (`version_drift`), 19 (`selfupdate.changed`, `version.drift`), 21, 22 (Remote edit, TOML edit, Safety, Tool list and drift), 23 (Phase 3), coordinator's calls 3, 4 and 6.

**Branch:** `agent/versions-p3-workers-stay`, off Phase 1's merge.

**Depends on:** Phase 1; Phase 0's goals.md amendment must be **accepted before this phase merges** (it writes a key other than the hook's in a worker's `settings.json`). Runs beside Phase 2.

## Global Constraints

- Phase 1's constraints hold. In addition:
- **The Mac's tool configs are never written.** Every write in this phase goes to a worker, over ssh, after a plan the user saw and one click.
- **One key per place.** A write changes only the knob's key (adding `env` or `[features]` if missing); every other byte stays. A duplicate key refuses. The spliced text must equal the same change made to the parsed data, or the plan refuses.
- **Never show or log a settings file.** The plan shows the one line that changes; errors name the place and the reason (`too_big`, `not_utf8`, `unreadable`, `changed_since_plan`, `duplicate_key`), never content. Masks apply to every output.
- **Backup first:** `<file>.notchling-backup-<ms>`, mode 0600, the newest 5 kept; then a temp file in the same folder with the old mode; then a rename. The write refuses if the file's SHA-256 changed since the plan.
- Knob keys used: Claude Code `env.DISABLE_AUTOUPDATER = "1"` (never `DISABLE_UPDATES`, never `autoUpdates`); Codex top-level `check_for_update_on_startup = false`; Codex `[features] daemon_auto_start = false` behind `CODEX_DAEMON_KNOB_VERIFIED` (false until the builder confirms it, spec §25); agy's `AGY_CLI_DISABLE_AUTO_UPDATE=true` behind `AGY_KNOB_VERIFIED` (false: agy stays "detect and repair").
- `CAP-FILEWRITE` (remote writes) and the `service_files.rs` change go to the maintainer. A change to an existing service-file golden or test is a `WEAK-TEST-EDIT`.

## Review Focus

1. A worker's `settings.json` with tokens in `env`, hooks and comments: after Self-Update Off, every byte but the new key is the same, the backup holds the old bytes with mode 0600, and no token appears in the plan, the log or an error. (Task 2 `one_key_added_everything_else_byte_for_byte`, Task 3 `plan_display_never_holds_a_token`.)
2. The file changed between plan and run: the write refuses and nothing is written. (Task 3 `hash_changed_since_plan_refuses`.)
3. Self-Update On removes only the value Notchling wrote; a user's own `DISABLE_AUTOUPDATER: "0"` stays. (Task 2 `undo_removes_only_ours`.)
4. A place that can't be read is reported, never skipped, and the step doesn't pass. (Task 4 `unreadable_place_fails_the_step`.)
5. Absent tools are never drift, a held worker is Held not drift, and one notice per worker, tool and found version. (Task 6 `absent_tool_is_never_drift`, `one_notice_per_found_version`.)

---

### Task 1: A TOML splice, `install::toml_edit`

**Zone:** maintainer (`crates/engine/src/install`). No CAP.

**Files:**
- Create: `crates/engine/src/install/toml_edit.rs`, fixtures `crates/engine/src/install/fixtures/codex-config-*.toml` (new files)
- Test: `crates/engine/src/install/toml_edit_tests.rs`
- Modify: `crates/engine/src/install/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum TomlEditError { Parse(&'static str), Duplicate, NotABool }`.
  - `pub fn get_bool(text: &str, table: Option<&str>, key: &str) -> Result<Option<bool>, TomlEditError>`.
  - `pub fn set_bool(text: &str, table: Option<&str>, key: &str, value: bool) -> Result<String, TomlEditError>`: top level (`table: None`) puts a new key before the first `[table]` line, after any leading comments; under a table it goes after that table's last key, creating `[features]` at the end when missing; an existing key has only its value replaced.
  - `pub fn remove(text: &str, table: Option<&str>, key: &str) -> Result<String, TomlEditError>`: removes the one line.
  - It reads only what it needs: bare keys, `=`, `true`/`false`, `[table]` headers, comments, strings (to skip `#` inside them) and multi-line strings and arrays (to not mistake their lines for keys). Anything it can't place refuses with `Parse`. The engine's `toml` crate (in `Cargo.lock`) may be used to re-parse the result and check it equals the intended change; adding it to `crates/engine/Cargo.toml` is a protected change the PR names.
- [ ] **Step 1: Write the failing tests:** `top_level_key_goes_before_the_first_table`, `features_key_under_its_table`, `features_table_made_when_missing`, `comments_and_order_kept`, `existing_value_replaced_in_place`, `already_set_is_unchanged_text`, `duplicate_key_refuses`, `hash_inside_a_string_is_not_a_comment`, `multiline_array_lines_are_not_keys`, `remove_takes_one_line`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib install::toml_edit`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `cargo test -p notchling-engine --lib install::`. Expected: PASS.
- [ ] **Step 5: Commit** `install: a small TOML splice for one key`.

### Task 2: Knob plans

**Zone:** maintainer (`crates/engine`). No CAP.

**Files:**
- Create: `crates/engine/src/versions/knob.rs`
- Test: `crates/engine/src/versions/knob_tests.rs`
- Modify: `crates/engine/src/install/edit.rs` (`pub fn set_member(text: &str, path: &[Seg], key: &str, value: &Json) -> Result<String, EditError>`: replace an existing member's value or append it, refusing a repeated key), `crates/engine/src/versions/mod.rs`

**Interfaces:**
- Consumes: `install::{jsonc, edit}`, Task 1.
- Produces:
  - `pub enum Knob { ClaudeDisableAutoupdater, CodexUpdateCheck, CodexDaemonAutoStart, AgyEnv }` (serde `snake_case`), `fn tool(self) -> Tool`, `fn verified(self) -> bool` (the two consts above).
  - `pub enum Place { ConfigDir(String), LaneConfigDir(String), CodexHome(String), ServiceFile(String) }` (serde, the remote path; every path checked by `is_safe_machine_path`).
  - `pub enum KnobState { Set, NotSet, Other(String), CantRead(&'static str) }` (`Other` holds only `"0"`, `"true"`, `"false"` or `"other"`).
  - `pub struct KnobPlan { pub knob: Knob, pub place: Place, pub shown: String, pub from_sha256: String, pub new_text: String }`; `shown` is exactly one line like `settings.json: "env": {"DISABLE_AUTOUPDATER": "1"}` or `config.toml: check_for_update_on_startup = false`.
  - `pub fn state(knob: Knob, text: &str) -> KnobState`.
  - `pub fn plan_off(knob: Knob, place: &Place, text: &str) -> Result<Option<KnobPlan>, String>` (`None` when already set; JSON: add `env` if missing, set the key, refuse a duplicate `env` or key, re-parse and compare with the intended value tree), `pub fn plan_on(knob: Knob, place: &Place, text: &str) -> Result<Option<KnobPlan>, String>` (removes the key only when its value is Notchling's; else `None`).
- [ ] **Step 1: Write the failing tests** (the `install` fixtures, plus new ones with an `env` block holding a fake token `sk-ant-oat01-PLANTED`, hooks and comments):
  - `one_key_added_everything_else_byte_for_byte` (diff of old and new is one inserted line or one inserted `"env"` member)
  - `env_made_when_missing`, `already_set_plans_nothing`, `duplicate_env_refuses`, `undo_removes_only_ours`, `undo_leaves_a_users_zero`
  - `shown_line_never_holds_file_content` (the planted token is not in `shown`)
  - `codex_check_is_top_level`, `codex_daemon_knob_only_when_verified`, `agy_knob_only_when_verified`
  - `set_member_replaces_or_appends` (in `knob_tests.rs`, over `edit::set_member`)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::knob`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `cargo test -p notchling-engine --lib install::`. Expected: PASS.
- [ ] **Step 5: Commit** `versions: plan the self-update knob as one key`.

### Task 3: Remote read and write of one file

**Zone:** maintainer (`crates/engine/src/machines`). `CAP-FILEWRITE` (on a worker).

**Files:**
- Create: `crates/engine/src/machines/remote_edit.rs`
- Test: `crates/engine/src/machines/tests_remote_edit.rs`
- Modify: `crates/engine/src/machines/mod.rs`

**Interfaces:**
- Consumes: Task 2; `step::run::script`, `FixContext`, `shell` quoting.
- Produces:
  - `pub fn read_script(path: &str) -> String`: prints `size=`, `sha256=` (`shasum -a 256` on macOS, `sha256sum` on Linux, picked in the script), then the file between nonce markers only when it is at most 1 MiB; `missing=1` for no file.
  - `pub enum Read { Text { text: String, sha256: String }, Missing, TooBig, NotUtf8, Unreadable }`, `pub fn parse_read(out: &RemoteOutput, nonce: &str) -> Read` (checks the sha of the bytes it got equals the reported one, else `Unreadable`).
  - `pub fn write_script(path: &str) -> String`: reads stdin's first line as the expected SHA-256 and the rest as the new text; refuses with exit 3 and `why=changed_since_plan` if the file's hash differs; copies the file to `<path>.notchling-backup-<ms>` with mode 0600; keeps the newest 5 backups; writes `<path>.notchling-tmp-<ms>` with the old file's mode; `mv` over the file; prints `ok=1`. A missing file (Codex's `config.toml` on a fresh worker) is created with mode 0600 and no backup.
  - `pub fn apply(ctx: &FixContext, plan: &KnobPlan) -> FixOutcome`: `run::script(ctx, &write_script(..), Some(stdin), 30)`; the outcome's message is fixed text ("Self-update off in ~/.claude/settings.json") or the reason.
- [ ] **Step 1: Write the failing tests** (`ScriptedRunner`; scripts checked by their text; outputs recorded):
  - `read_parses_text_and_hash`, `too_big_is_refused`, `not_utf8_is_refused`, `hash_of_bytes_must_match`
  - `write_sends_text_only_on_stdin` (argv holds no file text and no token; stdin starts with the hash line)
  - `hash_changed_since_plan_refuses` (exit 3 → a failed outcome naming `changed_since_plan`)
  - `backup_comes_before_the_rename` (in the script text: the `cp` with `chmod 600` comes before `mv`)
  - `plan_display_never_holds_a_token` (a full Fix's shown plan and its log, with the planted token in the file)
  - `script_never_uses_sudo_or_admin`
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_remote_edit`. Expected: FAIL.
- [ ] **Step 3: Implement.** Paths are quoted with the existing `shell` helpers; the nonce comes from the engine's existing nonce source.
- [ ] **Step 4: Run** the same. Expected: PASS.
- [ ] **Step 5: Commit** `machines: read and write one file on a worker, with a backup and a hash check`.

### Task 4: Self-Update Off and On steps

**Zone:** maintainer (`crates/engine/src/machines`). `CAP-FILEWRITE`; `WEAK-TEST-EDIT` if an existing test counts the catalog.

**Files:**
- Create: `crates/engine/src/machines/steps_selfupdate.rs`
- Test: `crates/engine/src/machines/tests_selfupdate.rs`
- Modify: `crates/engine/src/machines/step.rs` (`StepId::self_update(tool: Tool) -> StepId` = `self-update:<tool>`), `crates/engine/src/machines/catalog.rs` (one step per tool the worker has, after Tool Versions; a tool `NotInstalled` has no step), `crates/engine/src/machines/engine_fix.rs` (removing a machine first shows Self-Update On's plan for each place Notchling set, run on the same click as Remove; see call V6 in the overview)

**Interfaces:**
- Consumes: Tasks 2 and 3; Phase 1's `Detected.tools`.
- Produces:
  - `pub fn places(m: &Machine, home: &str, tool: Tool) -> Vec<Place>`: Claude Code `<home>/.claude/settings.json`, plus each lane's `CLAUDE_CONFIG_DIR` (none on workers today, the list is ready for when Machines makes lanes), plus the sd service file when `m.sd_worker()` is set; Codex `<home>/.codex/config.toml` (or `$CODEX_HOME` when the worker's probe reported one).
  - `pub fn step(m: &Machine, tool: Tool) -> SetupStep`: title "Self-Update Off"; `Cadence::Slow`; the probe reads every place with `read_script` and also reports `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE` from the login shell's environment, `minimumVersion` from each Claude Code settings file, and a running `app-server daemon pid-update-loop` for Codex (rows only, never content); passes only when every place is `Set`; a `CantRead` place is a failing row "Can't read ~/.claude/settings.json"; the Fix plans `plan_off` for each place and shows each one's `shown` line; `replace` is "Self-Update On", its own click, never in Set Up All.
  - The probe reports agy as "Not managed yet: Notchling finds and repairs drift" while `AGY_KNOB_VERIFIED` is false.
  - Phase 1's Tool Versions step adds `agy --version` to its `Slow` probe for a worker where this step's agy knob is `Set` (never otherwise).
- [ ] **Step 1: Write the failing tests** (World with scripted reads):
  - `off_sets_every_known_place`, `already_set_runs_no_write`, `unreadable_place_fails_the_step`
  - `on_is_its_own_click_and_not_in_set_up_all`, `remove_machine_shows_and_runs_self_update_on`
  - `package_manager_auto_update_is_reported`, `minimum_version_is_reported`, `codex_update_loop_is_reported`
  - `not_installed_tool_has_no_step`, `agy_is_detect_and_repair_until_verified`
  - `refresh_runs_only_reads` (every batched script in a refresh holds no `mv`, `cp`, `>` or `tee`)
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_selfupdate`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine --lib machines::`. Expected: PASS (list any edited existing test for the maintainer).
- [ ] **Step 5: Commit** `machines: Self-Update Off and On per tool, in every place Machines knows`.

### Task 5: The sd service files carry the knob

**Zone:** maintainer (`crates/engine/src/machines/service_files.rs`). `CAP-FILEWRITE` (the files Machines already writes); `WEAK-TEST-EDIT` if a service-file golden changes.

**Files:**
- Modify: `crates/engine/src/machines/service_files.rs` (`render` adds `Environment=DISABLE_AUTOUPDATER=1` beside the `PATH` line in the unit, and `<key>DISABLE_AUTOUPDATER</key><string>1</string>` in the plist's `EnvironmentVariables`; agy's variable only when `AGY_KNOB_VERIFIED`), `crates/engine/src/machines/steps_sd_service.rs` (a service file without the line counts as changed, and its restart waits for `WorkerStatus.current` empty, as today's restarts do)
- Test: `crates/engine/src/machines/tests_sd_service_versions.rs`

- [ ] **Step 1: Write the failing tests:** `unit_has_the_variable_beside_path`, `plist_has_the_variable`, `old_file_without_it_is_changed`, `restart_waits_for_no_job`, `agy_variable_only_when_verified`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib machines::tests_sd_service_versions`. Expected: FAIL.
- [ ] **Step 3: Implement.** Run `cargo test -p notchling-engine --lib machines::` and list any existing service-file test whose expected text changed (`WEAK-TEST-EDIT`); update those in their own commit `machines: service file tests carry DISABLE_AUTOUPDATER`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine --lib machines::`. Expected: PASS.
- [ ] **Step 5: Commit** `machines: the sd service files turn Claude Code's self-update off`.

### Checkpoint 1

- [ ] `cargo test -p notchling-engine --lib machines::`, `cargo test -p notchling-engine --lib versions::`, `cargo test -p notchling-engine --lib install::`, clippy, fmt, `guard lint`. Push.

### Task 6: Drift and its notice

**Zone:** maintainer (`crates/engine`, `notchlingd`, `crates/engine/src/inbox`); the app's rows are auto. No CAP.

**Files:**
- Create: `crates/engine/src/versions/drift.rs`, `crates/notchlingd/src/versions_drift.rs`
- Test: `crates/engine/src/versions/drift_tests.rs`, `crates/notchlingd/src/versions_drift_tests.rs`
- Modify: `crates/engine/src/inbox/model.rs` (`Kind::VersionDrift`), `crates/engine/src/versions/record.rs` (`Fact::Drift { tool, machine, expected, found }`, `Fact::SelfUpdate { tool, machine, knob, place, action, result }`), `crates/notchlingd/src/versions_ops.rs` (on each rescan and every minute, read `versions/workers`, compute drift, raise notices), `crates/engine/src/machines/steps_selfupdate.rs` (record `SelfUpdate` after each Fix; Machines writes the fact to `versions/selfupdate-log`, a doc of the last 200 facts, which the daemon reads into the recorder: Machines has no audit log of its own beyond `deps.audit`, so it uses that one), `crates/app/src/inbox/model.rs`

**Interfaces:**
- Produces:
  - `pub struct Drift { pub machine: String, pub tool: Tool, pub expected: Version, pub found: Version, pub ahead: bool }`.
  - `pub fn drift(reference: &BTreeMap<Tool, Version>, workers: &BTreeMap<String, WorkerTools>, held: &BTreeSet<(String, Tool)>, rolling: &BTreeSet<(String, Tool)>) -> Vec<Drift>`: only tools a worker has, only with a usable reference, never a held pair or one in a running rollout (empty until Phase 4).
  - Notice: id `version-drift:<random>`, kind `version_drift`, title "vps-sing-1 Runs Another Codex", detail "vps-sing-1 runs Codex 0.167.0, the Mac 0.166.0.", action `[Open "Open Machines" primary]`, `open: {"machines": "<machine>"}`. Raised once per (machine, tool, found) — kept in `versions/drift-noticed` — and withdrawn when that pair matches again.
- [ ] **Step 1: Write the failing tests:** `behind_and_ahead_are_drift`, `absent_tool_is_never_drift`, `held_is_not_drift`, `no_usable_reference_is_no_drift`, `one_notice_per_found_version`, `matching_again_withdraws_the_notice`, `notice_text_names_both_versions`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine --lib versions::drift` and `cargo test -p notchling-notchlingd --lib versions_drift`. Expected: FAIL.
- [ ] **Step 3: Implement.** Opening Machines from an inbox row uses the app's existing Machines menu path (the app starts `notchling-machines` beside it the way its menu does); if no such path exists yet, the row says "Open Machines from the menu bar" and the reviewer is told.
- [ ] **Step 4: Run** both, and `cargo test -p notchling-engine --lib inbox::`. Expected: PASS.
- [ ] **Step 5: Commit** `versions: drift on workers, marked and announced once`.

### Task 7: Machines shows Self-Update and drift

**Zone:** auto (`crates/app/src/machines`). No CAP.

**Files:**
- Modify: `crates/app/src/machines/versions_model.rs` (rows: "Self-update off" or "Self-update on in ~/.claude/settings.json"; a drift row says "Drifted: 0.167.0, the Mac 0.166.0"; ahead adds "Moved down only by its own click (Phase 4)"), `crates/app/src/machines/cards.rs`, `crates/app/src/machines/renders.rs` (`machines-selfupdate`, `machines-drift`)
- Test: `crates/app/src/machines/versions_model_selfupdate_tests.rs`

- [ ] **Step 1: Write the failing tests:** `selfupdate_rows_per_place`, `cant_read_place_is_flagged`, `drift_row_names_both_versions`, `ahead_says_never_moved_down_automatically`.
- [ ] **Step 2: Run** `cargo test -p notchling-app --lib machines::versions_model_selfupdate`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests; render both states and look.
- [ ] **Step 5: Commit** `machines: self-update and drift on each card`.

### Task 8: Docs and checks

**Zone:** maintainer. No CAP.

**Files:**
- Modify: `crates/AGENTS.md` (Versions: what Self-Update Off writes and where; never the Mac; undo; drift notices), `crates/engine/AGENTS.md` (`install/toml_edit.rs`, `versions/knob.rs`, `machines/remote_edit.rs`: the read size cap and the hash check), `docs/feature-map.md`, `CHANGELOG.md`
- [ ] **Step 1: Run** fmt, clippy (on the Mac), `cargo test --workspace --locked`, `guard lint`, `guard featuremap`, `guard rules`, `strip_comments --check`. Expected: clean.
- [ ] **Step 2: Write** the docs. **Step 3: Commit** `docs: tool versions, phase 3`. The PR names the `CAP-FILEWRITE` lines, any `WEAK-TEST-EDIT`, and that it waits for the goals.md amendment (Phase 0).

### Checkpoint 2 (maintainer, a real worker)

On one test worker with `--live`: Self-Update Off shows one line per place; after Run, `~/.claude/settings.json` has the key, a 0600 backup sits beside it, `claude doctor` there reports auto-updates disabled; Self-Update On takes it out. Then the builder's open checks (spec §25), done by the maintainer on that worker and recorded in the ledger: whether `AGY_CLI_DISABLE_AUTO_UPDATE=true` stops agy's background update, whether `agy --version` counts as a run that updates, and whether Codex's `daemon_auto_start` key exists. Each "yes" is a one-line PR flipping its `*_VERIFIED` const, with the evidence in `crates/engine/AGENTS.md`.
