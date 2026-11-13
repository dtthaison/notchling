# Logging, Phase 6: Bug-Report Bundle and Command Line Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export Bug Report… builds a scrubbed folder the user previews and saves where they choose; `notchling log` reads events, traces an `op_id`, checks the Audit, lists kinds, resolves a hash and shows Metrics, seeing only what an agent may see. First, the three parallel phases are joined and the settings that cross them are wired.

**Architecture:** The daemon builds the bundle (`log_bundle.rs`) under `<data>/app/reports/`, from Diagnostics files, Metrics, the Audit's check status, crash places and, if chosen, Activity with every `Ref` hashed; a pure scrub (`engine::logs::scrub`) runs over every file last. The app shows the preview and asks the daemon to save or discard it, so every file write stays in the daemon. `log.read` serves events from `events.db` with the agent rule; `log_cli.rs` is the command line's output file.

**Tech Stack:** Rust; GPUI (`cx.prompt_for_new_path`, `cx.reveal_path`); `notchling-events`.

**Spec:** `docs/superpowers/specs/2026-11-13-logging-design.md`, sections 6.5 (the canary through a bundle), 7.5 (`notchling log trace`), 11 (bundle hashing, `notchling log ref`), 15, 16 (`log.read`, `log.bundle`, the command line), 17 (Phase 6).

**Branch:** `agent/logging-p6-bundle-cli`, from `agent/logging-p5-app-metrics` with `agent/logging-p3-logs-tab` and `agent/logging-p4-diagnostics` merged in (call C25), or from the maintainer's integration branch that holds all three.

**Calls:** C-numbers refer to "Calls these plans make" in `2026-11-13-logging-p1-registry.md`.

## Global Constraints

- Every rule of the earlier phases' Global Constraints.
- Nothing is built or sent until the user presses Export Bug Report…; nothing is sent at all: the user picks a folder.
- The bundle is a plain folder (no zip crate is in the tree and `ditto` would be a process): `README.txt`, `system.json`, `diagnostics/*.jsonl`, `metrics.json`, `audit-check.json`, `crashes.txt`, and `activity.jsonl` only when chosen.
- Never in a bundle: Audit lines, transcripts, `notchling.db`, `events.db`, `log-salt`, anything from the Keychain, `permission-screen.txt`, the vault list, `devices.json`.
- Windows: last hour, 24 hours, 7 days. Options, both off by default: include Activity (names hashed), include crash messages.
- The scrub drops, or rewrites, any line holding a token shape (`sk-ant-`, `ghp_`, `Bearer `, a JWT, 40 or more hex characters, 60 or more base64 characters), an email address or a home folder path (the home folder becomes `~`), and counts each in `README.txt`. It is a backstop; the types are the defense. Hex ids the logs use (`op_id` 16, `boot` 8, `host` 16, `h:` 8) are under 40 and pass.
- `log.read` (`OpSpec::AGENT`): the agent tier gets only events whose `actor.session` is its own and whose kind has `agents_read: Own`; a caller with no session gets an empty list. The app gets everything.
- The command line is always agent tier.

## Review Focus

1. A Diagnostics line that somehow holds a home path or a token (a third-party crate's `tracing` event, a bug in a field type): the bundle never carries it, and the README says how many lines were removed. (Task 6.2 tests `drops_token_shapes` and `home_path_becomes_tilde`; Task 6.6 test `canary_never_in_a_bundle`.)
2. The user presses Cancel, or quits the app with the preview open: the folder under `<data>/app/reports/` is deleted, then or at the daemon's next start (reports older than a day are removed). (Task 6.3 tests `cancel_deletes_the_folder` and `stale_reports_removed_at_start`.)
3. Save to a folder that already holds a file of that name, or to a path an agent planted as a symlink: refused with a plain message, nothing overwritten, nothing followed. (Task 6.3 test `save_refuses_existing_and_symlink`.)
4. An agent runs `notchling log --json --kind vault.secret_used` from a session: it sees only its own session's rows of readable kinds; `--session <other>` is ignored for the agent tier. (Task 6.6 test `agent_log_shows_only_its_own_session`.)
5. `notchling log trace <op_id>` with an id that crosses midnight, or a malformed id: lines from both days' files in time order; a malformed id is a usage error, not a scan of every file. (Task 6.5 tests `trace_crosses_midnight` and `trace_refuses_malformed_id`.)

## Order and lanes

```
6.1 join and wire ──► 6.2 scrub ──► 6.3 log.bundle ──► 6.4 preview in the app ──► 6.6 canary and docs
                      6.5 log.read and notchling log (after 6.1, beside 6.2 to 6.4)
```

Lanes: **A** 6.2, 6.3 (`crates/engine/src/logs/scrub.rs`, `crates/notchlingd/src/log_bundle.rs`), **B** 6.4 (`crates/app/src/logs/bundle*.rs`, after 6.3's reply shape), **C** 6.5 (`crates/notchlingd/src/log_cli.rs`, `log_read_ops.rs`, `cli.rs`, `tools/guard/guard/lint.py`).

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited (why) |
| --- | --- | --- | --- |
| 6.1 | Mixed: daemon and local-api maintainer; app auto | none new | none |
| 6.2 | Maintainer (engine) | none | none |
| 6.3 | Maintainer (`notchlingd`) | `CAP-FILEWRITE` (report folder, `rename`, `remove_dir_all`) | none |
| 6.4 | auto (`crates/app/src/logs/**`) | none (`cx.prompt_for_new_path` and `cx.reveal_path` are not process launches) | none |
| 6.5 | Maintainer (`notchlingd`, `tools/guard`, `docs/rules.md`) | none | `tools/guard/tests/test_lint_log.py` gains a case for `log_cli.rs` (a file this effort added in Phase 1; still `WEAK-TEST-EDIT` by path, `tools/guard/tests/`) |
| 6.6 | Maintainer (`crates/notchlingd/tests/` new files, notes, `privacy.md`, `README.md`) | none | none |

## Maintainer only (Phase 6)

- Merge (or approve merging) Phases 3, 4 and 5 into the base this phase starts from.
- Approve the bundle's file writes and the `test_lint_log.py` edit.
- On a Mac with a display: Export Bug Report… from Settings > Logs, read the preview, Save to the Desktop, open the folder and read `README.txt` and one Diagnostics file by eye for anything personal.

---

### Task 6.1: Join the phases and wire what crosses them

**Files:**
- Modify: `crates/notchlingd/src/log_settings_ops.rs` (`detail` calls `diag::set_level`; `log.clear {sink: diagnostics, procs?}` removes those processes' files through the janitor's name rule and pushes `log.pruned{sink: diagnostics, reason: user}`), `crates/notchlingd/src/log_diag.rs` (keep days from `LogSettings::diag_keep_days`), `crates/local-api/src/presence.rs` (keyed by `(pid, boot)` when the hello has a `boot`), `crates/app/src/logs/view.rs` (Show Logs in Finder reveals the Logs folder; Clear Activity… gains Diagnostics), `crates/app/src/log_client/` (the app reads `detail` from `log.settings.get` at start and from the `log.settings` event, and calls `diag::set_level`; the app's unreported crashes go out as `process.crashed` through the log client, then `crash::mark_reported`)
- Test: `crates/notchlingd/src/log_join_tests.rs`, `crates/app/src/log_client/join_tests.rs`, `crates/local-api/src/presence_tests.rs` (new tests only)
- [ ] **Step 1: Write the failing tests:** `detailed_raises_daemon_level_for_24_hours`, `diag_keep_days_reach_the_janitor`, `clear_diagnostics_removes_files_and_records`, `presence_keys_by_boot_when_given` (a new app process with a reused pid and a new boot attaches again), `app_reports_unreported_crash_once`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_join`, `cargo test -p notchling-app log_client::join`, `cargo test -p notchling-local-api presence`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` and clippy on the Mac. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "logs: Detail and keep days reach Diagnostics; app crashes reported"`.

### Task 6.2: The scrub

**Files:**
- Create: `crates/engine/src/logs/scrub.rs`
- Test: `crates/engine/src/logs/scrub_tests.rs`

**Interfaces:**
- Produces: `pub struct Scrubbed { pub text: String, pub dropped: u64, pub rewritten: u64, pub why: BTreeMap<&'static str, u64> }`; `pub fn scrub(text: &str, home: Option<&Path>) -> Scrubbed` (line by line: a line with a token shape or an email is dropped; a line with the home folder's path has it replaced by `~`; `why` keys: `token`, `email`, `home`). Written with byte scans, no `regex` (the engine has `regex`; using it is fine if simpler).
- [ ] **Step 1: Write the failing tests:** `drops_token_shapes` (one line per shape: `sk-ant-…`, `ghp_…`, `Bearer abc`, a three-part JWT, 40 hex, 60 base64), `keeps_short_hex_ids` (`op_id`, `boot`, `h:3f9a0c1b`), `drops_emails`, `home_path_becomes_tilde`, `counts_by_reason`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine logs::scrub`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "engine: the bundle's scrub"`.

### Task 6.3: `log.bundle` and `log.bundle.finish`

**Files:**
- Create: `crates/notchlingd/src/log_bundle.rs`, `crates/notchlingd/src/log_bundle_system.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::LOG_BUNDLE`, `ops::LOG_BUNDLE_FINISH`), `crates/notchlingd/src/op_kinds.rs` (both `Called`), `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/log_start.rs` (reports older than a day removed at start), `crates/platform/src/` (`hw_model() -> Option<String>`, `sysctl hw.model` on macOS)
- Test: `crates/notchlingd/src/log_bundle_tests.rs`

**Interfaces:**
- Produces:
  - `log.bundle {window: "hour"|"day"|"week", include_activity?, include_crash_messages?}` (`OpSpec::APP`) → `{folder, files: [{name, bytes, lines, removed, head: [first 5 lines]}]}`. It runs on a worker thread and answers within the op's time; a build over 4 s answers `{building: true, folder}` at once and publishes `log.bundle` on topic `log` when done (the "work that can outlast 5 s" rule).
  - The folder: `<data>/app/reports/<yyyy-mm-dd-hhmmss>/`, 0700, files 0600. `system.json`: versions (daemon; app from the last `app.attached`; ptyhost from its newest Diagnostics header; macOS from `platform::os_version`; Claude Code, Codex, Antigravity from `tool.version_seen` rows), `platform::hw_model`, switch states, accounts per provider (counts), paired phones (count), kept pages (count), Free settings with values and other settings by name only (classes from `settings/keys.rs`). `diagnostics/<file>` copies of every process's files whose day overlaps the window, cut to the window's lines. `metrics.json`: `log.metrics` rows in the window. `audit-check.json`: the last `log.check` data. `crashes.txt`: entries in the window, with `at` and the stack; messages replaced by `(message removed)` unless chosen. `activity.jsonl` (when chosen): rows in the window, every field the kind declares as a `Ref` hashed with the salt (`FieldDesc::ref_kind`), and the row's `session`, `folder`, `account`, `device` columns hashed. `README.txt`: what is inside, what was removed and how many lines per reason, and the salt ids the hashes span (from each Diagnostics file's header). The scrub runs over every file last.
  - `log.bundle.finish {folder, action: "save", dest, remove?: [names]}` or `{folder, action: "cancel"}` (`OpSpec::APP`; call C32): `folder` must be a direct child of `<data>/app/reports/` (no `..`, no symlink); `remove` deletes those files first; `save` moves the folder to `dest` (an absolute path from the save panel), refusing a `dest` that exists or whose parent is a symlink, and copying then deleting when `rename` crosses volumes; `cancel` deletes it. Reply `{saved: dest}` or `{discarded: true}`.
- [ ] **Step 1: Write the failing tests** (a temp root with Diagnostics files, `events.db` rows and a `crash.log`): `bundle_has_the_listed_files`, `bundle_never_holds_audit_lines_salt_or_db` (no file's text contains a sealed line's `"seal":`, the salt's bytes as hex, `SQLite format 3`, or any file named `events.db`, `notchling.db`, `log-salt`, `devices.json`), `activity_refs_are_hashed`, `crash_messages_removed_unless_chosen`, `readme_counts_removed_lines`, `cancel_deletes_the_folder`, `save_moves_the_folder`, `save_refuses_existing_and_symlink`, `finish_refuses_folder_outside_reports`, `stale_reports_removed_at_start`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_bundle`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd: log.bundle builds a scrubbed folder; log.bundle.finish saves or discards it"`.

### Task 6.4: The preview in the app

**Files:**
- Create: `crates/app/src/logs/bundle.rs`, `crates/app/src/logs/bundle_view.rs`, `crates/app/src/logs/ref_lookup.rs`
- Modify: `crates/app/src/logs/view.rs` (the Export Bug Report… button), `crates/app/src/logs/wire.rs`, `crates/app/src/logs/renders.rs` (`settings-logs-bundle`)
- Test: `crates/app/src/logs/bundle_tests.rs`

**Interfaces:**
- Produces: `logs/ref_lookup.rs`: `pub fn look_up(hash: &str, salt: &Salt, known: &[(&'static str, String, String)]) -> Option<String>` (`known` is `(ref kind, id, name)` from what the app already holds: Chat's session rows, folders, accounts, paired devices, vault names); the Logs page gets a "Look Up" field that turns `h:3f9a0c1b` into a name, or says "Not known under the current salt" (the spec's 11). `pub struct BundleForm { pub window: Window, pub include_activity: bool, pub include_crash_messages: bool }` (defaults: day, false, false); `pub struct Preview { pub folder: String, pub files: Vec<PreviewFile>, pub removed: BTreeSet<String> }`; `PreviewFile { name, bytes, lines, removed, head }`; `Preview::toggle_remove(name)`; `pub fn size_text(bytes) -> String`. The view: the form, then the preview list (name, size, first lines in the kit's code type, "N lines removed"), Remove per file, Save… (`cx.prompt_for_new_path` with the default name `Notchling Bug Report <date>`, then `log.bundle.finish save`, then `cx.reveal_path(dest)`), Cancel (`log.bundle.finish cancel`; also on closing the sheet). Calls off the UI thread.
- [ ] **Step 1: Write the failing tests:** `look_up_finds_a_known_session`, `look_up_unknown_says_so`, `defaults_are_day_and_both_off`, `remove_toggles_and_goes_to_finish`, `size_text_formats`, `close_without_save_cancels` (the model's `on_close()` returns the cancel call).
- [ ] **Step 2: Run** `cargo test -p notchling-app logs::bundle`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-app`, clippy on the Mac, and `--render settings-logs-bundle` in release. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "app: Export Bug Report with a preview"`.

### Task 6.5: `log.read` and `notchling log`

**Files:**
- Create: `crates/notchlingd/src/log_read_ops.rs`, `crates/notchlingd/src/log_cli.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::LOG_READ`), `crates/notchlingd/src/op_kinds.rs` (`log.read` Quiet), `crates/notchlingd/src/cli.rs` (`Command::Log { args }`), `crates/notchlingd/src/lib.rs` (dispatch), `tools/guard/guard/lint.py` (`log_cli.rs` joins the output files of `LINT-LOG-ONE-WRITER`), `tools/guard/tests/test_lint_log.py`, `docs/rules.md` (the rule's row lists it)
- Test: `crates/notchlingd/src/log_read_ops_tests.rs`, `crates/notchlingd/src/log_cli_tests.rs`

**Interfaces:**
- Produces:
  - `log.read {since?, until?, kinds?, groups?, session?, limit?, before_seq?}` → `{events: [{seq, ts, kind, kv, group, outcome, actor, op_id, f, spooled}], more}`; limit default 200, max 1,000; the agent rule from Global Constraints (`session` is ignored for the agent tier).
  - `notchling log [--json] [--since 1h|today|<ms>] [--kind <name>]… [--limit n]` (rows as one line each: time, title, result, the kind's short detail; `--json` prints the reply), `notchling log trace <op_id>` (reads the Logs folder's files for the days the id's rows span, plus `log.read` rows with that `op_id`, merged by `ts`), `notchling log verify` (`log.verify`), `notchling log kinds [--markdown]` (`notchling_events::doc::markdown()`, or one name and title per line), `notchling log ref h:3f9a0c1b` (reads the salt file; hashes the ids this Mac still knows and an agent may see: session ids and folders from `sessions.list`, secret names from `secrets`; prints the match or "not known under the current salt"), `notchling log metrics [--json]` (`log.metrics`). Exit 0 on success, 2 on a usage error.
  - `log_cli.rs` holds every print of these commands.
- [ ] **Step 1: Write the failing tests:** `agent_read_is_own_session_and_readable_kinds`, `app_read_is_everything`, `no_session_caller_gets_empty`, `parse_since_forms` (`1h`, `today`, a number), `trace_crosses_midnight` (two days' fixture files), `trace_refuses_malformed_id`, `ref_resolves_known_session`, `kinds_markdown_matches_docs` (equals `docs/log-kinds.md`). In `test_lint_log.py`: `test_one_writer_skips_log_cli`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- log_read log_cli` and `tools/test-python.sh`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`, clippy, `tools/test-python.sh`, `python3 -m guard rules`. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd: log.read and notchling log"`.

### Task 6.6: The canary through a bundle; docs

**Files:**
- Create: `crates/notchlingd/tests/log_bundle_e2e.rs`, `crates/notchlingd/tests/log_cli_e2e.rs`
- Modify: `crates/notchlingd/tests/log_canary_e2e.rs` is not edited (it is Phase 2's; the bundle case goes in the new file), `docs/policies/privacy.md` (the bundle: built only on request, never sent, what it holds and never holds, `<data>/app/reports/` removed after a day), `README.md` (Logs table; `notchling log`), `docs/local-api-ops.md` (`log.read`, `log.bundle`, `log.bundle.finish`), `crates/AGENTS.md`, `crates/notchlingd/AGENTS.md` (the command line's modes list gains `log`), `crates/app/AGENTS.md`, `docs/feature-map.md`
- [ ] **Step 1: Write the failing tests:** `canary_never_in_a_bundle` (plant the Phase 2 markers, plus one in a Diagnostics point's field through a `Ref` and one in a crash message; build a bundle with both options on; no file in it holds a marker, the crash message included), `bundle_has_no_audit_lines_or_salt` (end to end), `agent_log_shows_only_its_own_session` (two fake sessions' processes: the CLI run as a child of session A's fake process sees A's rows only), `log_verify_cli_prints_intact`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --test log_bundle_e2e --test log_cli_e2e`; expected FAIL, then PASS once wired.
- [ ] **Step 3: Write** the docs.
- [ ] **Step 4: Run** the checkpoint below.
- [ ] **Step 5: Commit** by path, `git commit -m "logs: the canary through a bundle; notchling log end to end; docs"`.

## Checkpoint P6

- `cargo test --workspace --locked`, clippy on the Mac, `cargo fmt --all --check`, `cargo deny check`, `tools/test-python.sh`, `python3 -m guard lint`, `rules`, `claudemd`, `codeowners --check`.
- Render tests on a Mac with a display (`cargo test -p notchling-app -- --ignored`).
