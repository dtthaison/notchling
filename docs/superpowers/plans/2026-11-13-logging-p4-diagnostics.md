# Logging, Phase 4: Diagnostics and `op_id` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each process (app, daemon, hook, ptyhost) writes typed diagnostic lines to its own daily file in `~/Library/Logs/Notchling/` (or `<root>/notchling/logs/` under a data root), kept 7 days and capped; one `op_id` follows an action from the app or the hook through the daemon; the daemon and ptyhost get the app's panic hook and `crash.log` keeps the last 20 crashes; today's `eprintln!` lines become `diag!` points and the lint baseline shrinks.

**Architecture:** `notchling_events::diag` holds the `diag!` macro, the level filter (`tracing-subscriber`'s `EnvFilter`, so `NOTCHLING_LOG` works), the line format (the envelope's shape, `Ref`s hashed with the salt) and a non-blocking writer thread with a day file, a size cap and a janitor. `notchling_events::crash` is the shared panic hook. `local-api` carries `op_id` on requests, replies and `hello`'s `boot`; the client sends the current scope's `op_id`; the daemon enters it as scope and hands it to children as `NOTCHLING_OP_ID`. The daemon's bus gains a Diagnostics sink.

**Tech Stack:** Rust; `tracing` 0.1.44 (in `Cargo.lock` through GPUI), `tracing-subscriber` (new, MIT; features `std`, `registry`, `env-filter`, no defaults); `notchling-events` from Phases 1 and 2.

**Spec:** `docs/superpowers/specs/2026-11-13-logging-design.md`, sections 7.5, 8.3, 8.5, 10 (Diagnostics, Crash), 11 (hashing), 13 (hook budget), 17 (Phase 4).

**Branch:** `agent/logging-p4-diagnostics`, from `agent/logging-p2-audit-activity` once Phase 2 is merged. Runs beside Phases 3 and 5.

**Calls:** C-numbers refer to "Calls these plans make" in `2026-11-13-logging-p1-registry.md`.

**Shared files with Phases 3 and 5:** `crates/local-api/src/wire.rs` (Phase 4 changes `Request` and `Hello`; 3 and 5 only append `ops` constants), `crates/local-api/src/conn.rs` (Phase 4: `op_id` into scope; Phase 5: `perf.*` and `log.emit` limits; rebase, the hunks don't overlap), `crates/notchlingd/src/daemon.rs` (Phase 4: diagnostics init; Phase 5: metrics rollup timer). The Detail setting (Phase 3) and the Diagnostics keep days (Phase 3) reach the writer in Phase 6's Task 6.1, since 3 and 4 run at the same time; until then the level is `info` (`warn` for the hook) and the keep is 7 days.

## Global Constraints

- Every rule of the Phase 1 and Phase 2 plans' Global Constraints.
- Files: `<logs>/<proc>-YYYY-MM-DD.jsonl` for `app`, `daemon`, `hook`, `ptyhost`; 0600 in a 0700 folder; at most 20 MB a day per process (past it, lines are dropped and counted, and one `log.dropped`-shaped point says how many at the day's end); a janitor at start and hourly deletes files past the keep (7 days) and keeps the folder under 300 MB (oldest first).
- `<logs>` is `~/Library/Logs/Notchling` on macOS, `$XDG_STATE_HOME/notchling/logs` (else `~/.local/state/notchling/logs`) on Linux, `%LOCALAPPDATA%\Notchling\Logs` on Windows; under `NOTCHLING_DATA_ROOT` or `--data-root` it is `<root>/notchling/logs`, never the real folder.
- Each file's first line names the salt id it hashes with: `{"v":1,"kind":"diag.file","proc":…,"salt_id":…,"ts":…}`. Every `Ref` in a Diagnostics line is `h:` + 8 hex; no line holds free text: a point's message is a string literal, its fields are `LogField`s.
- Levels: `error`, `warn`, `info`, `debug`; no `trace` in release builds. Defaults `info` for app, daemon, ptyhost and `warn` for the hook. `NOTCHLING_LOG=debug,notchling_engine::chat=trace` overrides in development.
- `op_id`: 16 lowercase hex; a malformed one is replaced with a new id and noted at `warn`. It is a hint, never an identity or a permission.
- The hook adds at most 2 ms: one `O_APPEND` write of one line, no thread, no janitor (the daemon's janitor cleans the hook's files).
- `crash.log` stays at `<data>/logs/crash.log`, keeps the last 20 crashes, at most 1 MB a crash, each entry naming its process.
- `notchlingd.log` stays the daemon's catch file for what is printed before the logger starts, with today's `log_rotate.rs`; the `dev.notchling.daemon` plist doesn't change. ptyhost's stderr goes where its `local.agentconsole.hostd` LaunchAgent says, and Notchling doesn't install that agent, so its catch file and rotation wait until it does (call C34).

## Review Focus

1. A run with `NOTCHLING_DATA_ROOT` set and a fake `HOME`: nothing is created under `$HOME/Library/Logs`. (Task 4.8 test `nothing_written_to_library_logs_under_a_data_root`.)
2. Two app processes, or the app and the daemon, writing the same day's file name: they can't, since the file name has the process; but two daemons over two data roots must not share a file either: the folder is per data root under a fixture and the real one is single-daemon. (Task 4.4 test `fixture_root_gets_its_own_logs_folder`.)
3. A disk that fills while Diagnostics are written: the writer drops and counts, never blocks a caller, and the daemon keeps serving. (Task 4.1 test `write_error_drops_and_counts_without_blocking`.)
4. An agent sets `NOTCHLING_OP_ID` to a 2 KB string or `../../x`: it is replaced by a new id and nothing longer than 16 hex is written. (Task 4.3 test `bad_op_id_is_replaced`.)
5. A panic inside the panic hook's own write (disk full, folder gone): the process still aborts or unwinds as before, and the hook never panics twice. (Task 4.5 test `hook_survives_unwritable_crash_log`.)

## Order and lanes

```
4.1 line, writer, janitor ──► 4.2 diag!, filter, init ──┬─► 4.4 daemon diagnostics ──► 4.6 daemon, engine, ptyhost prints ──► 4.8 e2e and docs
4.3 op_id on the wire (beside 4.1, 4.2) ───────────────┘   4.5 crash hook (beside 4.4)        4.7 app prints (beside 4.6)
```

Lanes: **A** 4.1, 4.2 (`crates/events/src/diag/`), **B** 4.3 (`crates/local-api`, `crates/engine/src/control/launcher.rs`, `crates/engine/src/tools/`), **C** 4.5 (`crates/events/src/crash.rs`, `crates/app/src/crash_log.rs`, `crates/ptyhost/src/main.rs`); 4.6 and 4.7 run in parallel (different crates).

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited (why) |
| --- | --- | --- | --- |
| 4.1, 4.2 | Maintainer (`crates/events`, its `Cargo.toml`, `Cargo.lock`) | `CAP-FILEWRITE` (day files, janitor `remove_file`, `create_dir_all`) | none |
| 4.3 | Maintainer (`local-api`, engine `control/`, `tools/`) | `CAP-PROCESS` may fire on the changed lines in `launcher.rs` and the spawners (they already start processes; an env var is added) | the two `Request { .. }` literals in `crates/app/src/actions.rs` (near 438, 445) gain `op_id: None` (tests by rule if they are in a test module) |
| 4.4 | Maintainer (`notchlingd`, `platform`) | `CAP-FILEWRITE` | none |
| 4.5 | Maintainer for `crates/events`, `crates/notchlingd`, `crates/ptyhost`; auto for `crates/app/src/crash_log.rs` | `CAP-FILEWRITE` (crash file, `crash-seen.json`) | the inline tests in `crates/app/src/crash_log.rs` (`an_entry_names_the_place_and_keeps_the_message_and_backtrace` gains the process in the header; `the_log_is_private_appends_and_rolls_over` becomes "keeps the last 20"): `WEAK-TEST-EDIT` by rule |
| 4.6 | Maintainer | none new; `GUARD-BASELINE` shrinks (allowed) | none |
| 4.7 | auto (`crates/app/**`) | none | none |
| 4.8 | Maintainer (`crates/notchlingd/tests/` new file; notes) | `WEAK-TEST-EDIT` if `tests/hook_cli.rs` is edited (avoid: the new checks go in a new file) | none |

## Maintainer only (Phase 4)

- Approve `tracing-subscriber` in `Cargo.toml`/`Cargo.lock` (`cargo deny check` passes: MIT) and the `crash_log.rs` test edits.
- On the installed build, open Console.app and check `~/Library/Logs/Notchling/` lists the four files after a day of use, and that a forced daemon panic (a dev build with a test switch, never the installed one) lands in `crash.log` with `daemon` in its header.

---

### Task 4.1: The Diagnostics line, writer and janitor

**Files:**
- Create: `crates/events/src/diag/mod.rs`, `crates/events/src/diag/line.rs`, `crates/events/src/diag/writer.rs`, `crates/events/src/diag/janitor.rs`
- Test: `crates/events/src/diag/line_tests.rs`, `crates/events/src/diag/writer_tests.rs`, `crates/events/src/diag/janitor_tests.rs`

**Interfaces:**
- Consumes: `write_body`, `Envelope`, `Event`, `RefMode::Hashed`, `Salt`, `scope::current`.
- Produces:
  - `pub struct Point { pub level: Level, pub msg: &'static str, pub target: &'static str, pub at: CodeLoc, pub fields: Fields }`
  - `line.rs`: `pub fn point_line(p: &Point, proc: Proc, pid: u32, boot: &Hex, ts: i64, op_id: Option<&Hex>, salt: &Salt) -> String` (`{"v":1,"kind":"diag","level":"warn","msg":"reload took long","target":…,"at":…,"ts":…,"proc":…,"pid":…,"boot":…,"op_id":…,"f":{…}}`, `f` hashed); `pub fn event_line(env: &Envelope, ev: &Event, salt: &Salt) -> String` (the envelope body, hashed, in braces); `pub fn file_header(proc: Proc, salt: &Salt, ts: i64) -> String`.
  - `writer.rs`: `pub struct DiagWriter`; `DiagWriter::start(dir: PathBuf, proc: Proc, salt: Salt, day_cap: u64, clock: Arc<dyn Fn() -> i64 + Send + Sync>) -> DiagWriter` (one thread, a bounded channel of 4,096 lines; `send(line)` never blocks: a full channel counts a drop); `DiagWriter::write_now(dir, proc, salt, line)` (the hook's path: one `O_APPEND` open, write, close, no thread; header written when the file is new); `DiagWriter::dropped() -> u64`; `close(wait)`.
  - `janitor.rs`: `pub fn sweep(dir: &Path, now_ms: i64, keep_days: u16, cap_bytes: u64) -> Swept { files_removed, bytes_removed, from, to }` (only names matching `^(app|daemon|hook|ptyhost)-\d{4}-\d{2}-\d{2}\.jsonl$`).
- [ ] **Step 1: Write the failing tests:** `point_line_hashes_refs_and_names_salt` (a `Ref<Session>` field is `h:` + 8 hex), `event_line_matches_envelope_order`, `header_is_first_line_of_a_new_file`, `day_rollover_opens_a_new_file`, `day_cap_drops_and_counts` (cap 1 KB), `write_error_drops_and_counts_without_blocking` (a read-only folder, unix: `send` returns at once and `dropped()` grows), `write_now_appends_one_line`, `sweep_keeps_seven_days`, `sweep_caps_folder_oldest_first`, `sweep_ignores_other_files`.
- [ ] **Step 2: Run** `cargo test -p notchling-events diag::`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/events/src/diag crates/events/src/lib.rs` then `git commit -m "events: Diagnostics lines, writer and janitor"`.

### Task 4.2: `diag!`, the level filter and `init`

**Files:**
- Create: `crates/events/src/diag/macros.rs`, `crates/events/src/diag/filter.rs`, `crates/events/src/diag/init.rs`, `crates/events/src/diag/foreign.rs`
- Modify: `crates/events/Cargo.toml` (`tracing = "0.1"`, `tracing-subscriber = { version = "0.3", default-features = false, features = ["std", "registry", "env-filter"] }`), `Cargo.lock`
- Test: `crates/events/src/diag/init_tests.rs`

**Interfaces:**
- Produces:
  - `diag!(Level::Warn, "reload took long", ms = Millis(took), rows = Count(n))` (`#[macro_export]`): the message must be a string literal (the macro takes `$msg:literal`), fields are `name = expr` where `expr: LogField`; it expands to `if $crate::diag::enabled($level, module_path!()) { $crate::diag::point(Point { .. }) }` with `at: CodeLoc::new(file!(), line!())`.
  - `filter.rs`: `pub fn enabled(level: Level, target: &str) -> bool` over a global `EnvFilter`-backed table, set by `init` and by `set_level(Level)`; in release builds `Level::Debug` passes only when Detail is Detailed or `NOTCHLING_LOG` asks.
  - `init.rs`: `pub struct DiagInit { pub proc: Proc, pub dir: PathBuf, pub salt: Salt, pub default_level: Level, pub hook_mode: bool }`; `pub fn init(cfg: DiagInit) -> DiagGuard` (starts the writer, or for `hook_mode` uses `write_now`; installs the `tracing` registry with `foreign::Layer` as the global default); `pub fn point(p: Point)` (adds `scope::current().op_id`); `pub fn event(env: &Envelope, ev: &Event)` (the bus's Diagnostics sink and non-daemon processes write events through this); `pub fn set_level(Level)`.
  - `foreign.rs`: a `tracing_subscriber::Layer` that writes a `tracing` event from another crate as `{"kind":"diag","level":…,"target":…,"msg":"foreign"}`: target and level only, never its message or fields (they are free text).
- [ ] **Step 1: Write the failing tests:** `diag_writes_literal_message_and_typed_fields` (an `init` over a temp folder; one `diag!` line read back), `level_filter_drops_debug_by_default`, `notchling_log_env_raises_a_target` (`NOTCHLING_LOG=debug,notchling_events=debug` set before `init` in a child test process: `std::process::Command::new(std::env::current_exe())` with a filter argument, so the env var doesn't leak to other tests), `foreign_tracing_event_keeps_no_message`, `op_id_from_scope_is_in_the_line`.
- [ ] **Step 2: Run** `cargo test -p notchling-events diag::init`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-events`, clippy and `cargo deny check`. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/events Cargo.lock` then `git commit -m "events: diag!, the level filter and init"`.

### Task 4.3: `op_id` on the wire and in children

**Files:**
- Modify: `crates/local-api/src/wire.rs` (`Request.op_id: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`; `Hello.boot: Option<String>` the same), `crates/local-api/src/conn.rs` (validate and enter scope; echo `op_id` in the reply's extras), `crates/local-api/src/client.rs` (`call` adds `scope::current().op_id`; `hello` sends the process's boot), `crates/notchlingd/src/hook.rs` (a new `op_id` per hook event, in scope before `hook.request`), `crates/notchlingd/src/hook_ops.rs` (`prompt.pending` data gains `op_id`), `crates/notchlingd/src/main.rs` (`NOTCHLING_OP_ID` read once for CLI calls), `crates/engine/src/control/launcher.rs` and the Codex and Antigravity spawners under `crates/engine/src/tools/` (`NOTCHLING_OP_ID` in the child's env when the scope has one), `crates/app/src/actions.rs` (`Request` literals; an answer to a prompt reuses the `prompt.pending`'s `op_id`), `crates/app/src/` where `prompt.pending` is parsed (`grep -rn "prompt.pending" crates/app/src`)
- Test: `crates/local-api/src/op_id_tests.rs`, `crates/notchlingd/src/hook_op_id_tests.rs`, `crates/engine/src/control/launcher_env_tests.rs`

**Interfaces:**
- Produces: `notchling_events::ids::parse_op_id(&str) -> Option<Hex>` (16 lowercase hex only); `conn.rs`: a request with a valid `op_id` runs in that scope; a missing one gets a new id (the daemon's own); a malformed one is replaced and a `diag!(Level::Warn, "malformed op_id replaced")` point written; every reply carries `op_id`. `Launcher` and spawners: `env("NOTCHLING_OP_ID", id)` when `scope::current().op_id` is set.
- [ ] **Step 1: Write the failing tests:** `op_id_round_trips_through_a_call` (a real server in a short `/tmp` folder: the reply's `op_id` equals the one the client sent in scope), `missing_op_id_gets_one`, `bad_op_id_is_replaced` (a 2 KB string and `../../x`: the reply's `op_id` is 16 hex and differs), `old_client_without_op_id_works` (a raw line with no `op_id` gets a normal reply); `hook_sends_op_id_and_prompt_pending_carries_it` (the hook rig of `hook_ops_tests.rs`); `launcher_passes_op_id_env` (the `FakeLauncher` records the env).
- [ ] **Step 2: Run** `cargo test -p notchling-local-api op_id` and `cargo test -p notchling-notchlingd hook_op_id` and `cargo test -p notchling-engine launcher_env`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "local-api, hook, launcher: one op_id across processes"`.

### Task 4.4: The daemon's Diagnostics

**Files:**
- Create: `crates/platform/src/logs_dir.rs`, `crates/notchlingd/src/log_diag.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/notchlingd/src/layout.rs` (`logs_dir()`), `crates/notchlingd/src/daemon.rs` (init first thing after the lock), `crates/notchlingd/src/log_start.rs` (the Diagnostics sink after the Activity sink; hourly salt watch)
- Test: `crates/platform/src/logs_dir_tests.rs`, `crates/notchlingd/src/log_diag_tests.rs`

**Interfaces:**
- Produces:
  - `platform::logs_dir_in(os: Os, env: Env) -> Option<PathBuf>` and `platform::logs_dir() -> Option<PathBuf>`, as `data_dir_in` does.
  - `Layout::logs_dir() -> PathBuf`: `<root>/notchling/logs` under a data root, else `<state>/logs` when `--socket` was given, else `platform::logs_dir()`.
  - `log_diag.rs`: `pub struct DiagSink` implementing Phase 2's `Sink`: writes every stamped event whose kind has `DIAG` through `diag::event`; `tick` runs the janitor at start and hourly and pushes `log.pruned{sink: diagnostics, …}` when it removed files, and re-reads the salt file's id hourly, pushing `log.salt_rotated{old_id, new_id}` when it changed (another process replaced a lost salt).
- [ ] **Step 1: Write the failing tests:** `logs_dir_per_os` (macOS, Linux with and without `XDG_STATE_HOME`, Windows), `fixture_root_gets_its_own_logs_folder` (`Layout` over a temp root), `diag_sink_writes_diag_kinds_only`, `janitor_tick_records_log_pruned`, `salt_change_records_salt_rotated`.
- [ ] **Step 2: Run** `cargo test -p notchling-platform logs_dir` and `cargo test -p notchling-notchlingd log_diag`; expected FAIL.
- [ ] **Step 3: Implement**; the daemon calls `diag::init(DiagInit { proc: Daemon, dir: layout.logs_dir(), salt, default_level: Info, hook_mode: false })` and writes `process.started`-shaped point `diag!(Level::Info, "daemon started", version = Version::new(env!("CARGO_PKG_VERSION")))`.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`, `cargo test -p notchling-platform` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd: Diagnostics in the Logs folder"`.

### Task 4.5: One panic hook, the last 20 crashes, `process.crashed`

**Files:**
- Create: `crates/events/src/crash.rs`
- Modify: `crates/app/src/crash_log.rs` (calls into `notchling_events::crash`; its inline tests edited), `crates/app/src/main.rs` (unchanged call site), `crates/notchlingd/src/daemon.rs` (install for the daemon; at start, report the daemon's unreported crashes as `process.crashed`), `crates/ptyhost/src/main.rs` (install; at start write its unreported crashes as `process.crashed` lines in its own Diagnostics file: ptyhost has no app-tier connection to the daemon, call C18), `crates/ptyhost/Cargo.toml` (`notchling-events`)
- Test: `crates/events/src/crash_tests.rs`

**Interfaces:**
- Produces: `pub fn install(data_dir: PathBuf, proc: Proc)` (the app's hook moved: same file, 0600, `O_NOFOLLOW`; the entry header becomes `=== {when} {proc} pid {pid} boot {boot} thread '{thread}' panicked at {location}`; a write error is ignored); `pub fn entries(text: &str) -> Vec<CrashEntry>` with `CrashEntry { ts: i64, proc: String, boot: String, thread: String, at: String, message: String }`; `pub fn unreported(data_dir: &Path, proc: Proc) -> Vec<CrashEntry>` and `pub fn mark_reported(data_dir: &Path, proc: Proc, ts: i64)` (state in `<data>/logs/crash-seen.json`, 0600); `pub fn keep_last(text: &str, n: usize) -> String` (applied before each append: the last 19 plus the new one; one entry at most 1 MB, cut). `process.crashed{proc, at: CodeLoc, message_hash: Hex (SHA-256 of the message with the salt, 16 hex), main_thread}` (Phase 1 declared `main_thread: bool` in place of the spec's `thread`, a name being free text).
- [ ] **Step 1: Write the failing tests:** `entry_names_the_process` (in `crash_tests.rs`), `keeps_last_twenty`, `one_entry_capped_at_one_megabyte`, `unreported_then_marked`, `hook_survives_unwritable_crash_log` (a panic under `std::panic::catch_unwind` with the crash folder made read-only, unix: the hook returns, no second panic). Edit the two inline tests in `crash_log.rs` as the Zones table says.
- [ ] **Step 2: Run** `cargo test -p notchling-events crash` and `cargo test -p notchling-app crash_log`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "events: one panic hook for app, daemon and ptyhost; crash.log keeps 20"`.

### Task 4.6: Daemon, engine, ptyhost and hook prints become points

**Files:**
- Modify: every non-test, non-output file in `crates/notchlingd/src`, `crates/engine/src`, `crates/ptyhost/src`, `crates/platform/src` that `python3 -m guard lint` reports in the baseline for `LINT-LOG-ONE-WRITER` (start from `grep '"LINT-LOG-ONE-WRITER|crates/\(notchlingd\|engine\|ptyhost\|platform\)' tools/guard/lint-baseline.json`), `crates/notchlingd/src/hook.rs` (`diag::init(.. hook_mode: true, default_level: Warn ..)` and one line per decided event), `tools/guard/lint-baseline.json` (shrinks)
- Test: `crates/notchlingd/src/hook_diag_tests.rs`

**Interfaces:**
- Each `eprintln!("notchling bridge: audit log: {e}")`-style line becomes `diag!(Level::Warn, "<fixed words>", <fields>)` with the error reduced to a choice (`io_kind`: `not_found`, `permission_denied`, `other`; `sqlite`: `busy`, `corrupt`, `other`), never its message; lines that announced state changes become `Level::Info`; startup lines printed before `diag::init` (in `main.rs` before the logger exists) stay as prints and stay in the baseline, since `notchlingd.log` is their catch file. `notchlingd/src/lib.rs` and `main.rs` are already output files for the rule (Phase 1).
- [ ] **Step 1: Write the failing tests** `hook_pass_writes_header_only_at_default_level` (run `notchling hook` over a fixture event with no daemon, `NOTCHLING_DATA_ROOT` at a temp copy: `<root>/notchling/logs/hook-<today>.jsonl` holds the header line only, since a pass is `info` and the hook's level is `warn`) and `hook_refusal_writes_a_warn_line_with_op_id` (a daemon that refuses the event as not descendant: one `warn` point carrying the event's `op_id`). The hook's 2 ms budget is measured by Phase 5's `bench_log`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd hook_diag`; expected FAIL.
- [ ] **Step 3: Replace** the prints; then `PYTHONPATH=tools/guard python3 -m guard lint --update-baseline` (it only shrinks).
- [ ] **Step 4: Run** `cargo test --workspace --locked`, clippy, `PYTHONPATH=tools/guard python3 -m guard lint` (0 new). Expected: PASS, with the baseline count lower by the number of lines moved.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd, engine, ptyhost: prints become diag! points (baseline N to M)"`.

### Task 4.7: The app's Diagnostics and prints

**Files:**
- Modify: `crates/app/src/main.rs` (`diag::init(DiagInit { proc: App, dir: <logs dir by the app's data root rules>, .. })` before the first window), `crates/app/src/mac/shell.rs`, and every app file in the baseline for `LINT-LOG-ONE-WRITER` except the render, bench and probe output files; `tools/guard/lint-baseline.json`
- Test: `crates/app/src/logs_init_tests.rs`

**Interfaces:**
- Consumes: `diag!`, `platform::logs_dir`. The app's logs folder follows the same rule as the daemon's (`--data-root` or `NOTCHLING_DATA_ROOT` → `<root>/notchling/logs`); a `--render` run writes no Diagnostics (it reads files only).
- [ ] **Step 1: Write the failing tests:** `app_logs_dir_follows_data_root`, `render_mode_writes_no_diagnostics` (the pure function that decides: `pub fn diag_dir(args: &Args) -> Option<PathBuf>` is `None` under `--render`).
- [ ] **Step 2: Run** `cargo test -p notchling-app logs_init`; expected FAIL.
- [ ] **Step 3: Replace** the prints (`switches/feed.rs`, `inbox/system.rs`, `mac/window.rs`, `welcome/window.rs`, `mac/restart_notice.rs`, `mac/vault_prompts.rs`, `chat/view/panels.rs`, `sheets/settings_view.rs`, `search/panel.rs`, `machines/window.rs`, …); `--update-baseline`.
- [ ] **Step 4: Run** `cargo test -p notchling-app`, clippy on the Mac, `python3 -m guard lint`; then in release on a Mac with a display `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1` and compare the counts with the last run on the base branch. Expected: PASS, counts unchanged (no point is on a draw path).
- [ ] **Step 5: Commit** by path, `git commit -m "app: Diagnostics file; prints become diag! points"`.

### Task 4.8: End to end, and the notes

**Files:**
- Create: `crates/notchlingd/tests/log_diag_e2e.rs`
- Modify: `docs/policies/privacy.md` (the Logs folder, what a Diagnostics line holds, hashing, `crash.log` last 20 for every process, `crash-seen.json`), `README.md` (Logs table), `crates/AGENTS.md` (Architecture: Diagnostics and `op_id`; Build, run, test: `NOTCHLING_LOG`), `crates/events/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `crates/local-api/AGENTS.md`, `crates/app/AGENTS.md`, `crates/ptyhost/AGENTS.md`, `docs/local-api-ops.md` (`op_id` on requests and replies, `hello.boot`), `docs/feature-map.md`
- [ ] **Step 1: Write the failing tests:** `one_op_id_across_client_daemon_and_hook` (a client call with an `op_id` in scope makes the daemon's file hold it; a hook event's `op_id` is in the hook's file, the daemon's file and the `prompt.pending` data), `nothing_written_to_library_logs_under_a_data_root` (fake `HOME`: `$HOME/Library/Logs` doesn't exist after a run), `diag_files_hold_no_marker` (the Phase 2 canary marker planted in a reply's text and a hook event's tool input is absent from every file under `<root>/notchling/logs`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --test log_diag_e2e`; expected FAIL, then PASS once wired.
- [ ] **Step 3: Write** the notes and docs.
- [ ] **Step 4: Run** the checkpoint below.
- [ ] **Step 5: Commit** by path, `git commit -m "logs: Diagnostics end to end; notes"`.

## Checkpoint P4

- `cargo test --workspace --locked`, clippy on the Mac and Linux, `cargo fmt --all --check`, `cargo deny check` (`tracing-subscriber` is MIT), `tools/test-python.sh`, `python3 -m guard lint` (0 new; the baseline shrank), `rules`, `claudemd`.
- `bench_chat` in release on a Mac with a display: counts unchanged (maintainer, if the builder has no display).
