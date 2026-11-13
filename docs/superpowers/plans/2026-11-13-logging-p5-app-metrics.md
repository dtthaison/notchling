# Logging, Phase 5: App, Hook and Metrics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The app, the hook and `notchling delegate` send their events to the daemon with `log.emit` under tier rules and limits; the app keeps a queue and a spool for when the daemon is away; the remaining kinds get their emitters; counters and timings are rolled up into Metrics and shown on the Logs tab's Performance section; `bench_log` holds the budget.

**Architecture:** `notchling_events::parse` turns a wire event into a typed `Event` against the registry, so nothing unchecked reaches a sink. `notchlingd`'s `log.emit` checks each event's `emitters` against the caller and the tier, rate-limits per connection, and hands it to the bus with its origin (`proc`, `pid`, `ts_rx`, `spooled`); the bus never sends an event from another process to the Audit. `notchling_events::metrics` folds `perf.*` observations into fixed-bucket histograms; the daemon writes minute rows to `events.db`'s `metrics` table and folds them into hours. The app's `log_client/` is a non-blocking queue with a sender thread and a spool file.

**Tech Stack:** Rust; `notchling-events`; GPUI (the app's stall watch, the Performance section).

**Spec:** `docs/superpowers/specs/2026-11-13-logging-design.md`, sections 6.3 (Agents, Diagnostics, Performance kinds), 7.3, 7.6, 8.4, 9 (Performance), 13, 16 (`log.emit`), 17 (Phase 5).

**Branch:** `agent/logging-p5-app-metrics`, from `agent/logging-p2-audit-activity` once Phase 2 is merged. Runs beside Phases 3 and 4. Task 5.9 needs Phase 3's Logs tab and Phase 4's `DiagWriter` (the hook's half of `bench_log`): start it after both are merged into the base (rebase), or move it to Phase 6. The app's `process.crashed` needs Phase 4's crash reader: Phase 6's Task 6.1 wires it.

**Calls:** C-numbers refer to "Calls these plans make" in `2026-11-13-logging-p1-registry.md`.

**Shared files with Phases 3 and 4:** `crates/local-api/src/conn.rs` (Phase 4's `op_id` hunk; this phase's `perf.*` and connection id hunks), `crates/local-api/src/wire.rs` (append-only `ops`), `crates/notchlingd/src/op_kinds.rs` (append-only homes), `crates/app/Cargo.toml` (Phase 3 adds `notchling-events`; add the same line if Phase 3 isn't in the base yet), `crates/notchlingd/src/daemon.rs`.

## Global Constraints

- Every rule of the Phase 1 and Phase 2 plans' Global Constraints.
- `log.emit` is `OpSpec::AGENT`, always a push; args `{events: [{kind, kv, ts, op_id?, f, spooled?}], metrics?: [..]}` at most 256 KB; reply `{accepted, dropped}`.
- An agent-tier caller may emit only kinds that list it (`Hook` for `ClientKind::Hook`, `Cli` for `ClientKind::Cli`), never an `Always` kind, never a kind with the Audit sink; at most 20 events a second and 1,000 an hour per connection. A refused event is counted in `dropped` and recorded once per second as `api.refused{op: log.emit}` (folded).
- The app (app tier) may emit kinds that list `App`. Its events, spooled or not, never reach the Audit (the coordinator's call 5): the bus drops the Audit sink for every event whose origin isn't the daemon.
- The app's queue holds 2,048 events; the sender sends every 500 ms or at 64 events, on its own thread and connection; UI threads never touch the socket. The spool `<data>/app/log-spool.jsonl` is 0600, at most 1 MB (oldest dropped and counted), and holds only `process.*` and `daemon.unreachable`; spooled events are sent with `spooled: true` and go to Activity and Diagnostics only.
- Before quitting, the app sends `process.stopped` (waiting at most 500 ms), and the daemon records `app.detached{cause: quit}` for that process.
- Metrics: fixed buckets 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000 ms and more; labels are choices and ops only; the daemon writes minute rows every 60 s, keeps them 24 hours, folds them into hourly rows kept 7 days; the app sends its rollup with `log.emit` every 60 s.
- `perf.stall` is main-thread work over 50 ms; `bench_chat`'s counts must not change.
- Budgets (section 13): `push()` under 5 µs at p99; `commit()` under 50 ms at p99; the writer keeps up at 5,000 events a second; the hook adds under 2 ms.

## Review Focus

1. An agent replays a captured `log.emit` with `kind: "vault.secret_used"` or `"switch.changed"`: refused, counted in `dropped`, recorded as `api.refused`, and nothing reaches the Audit or Activity. (Task 5.2 test `agent_cannot_emit_always_or_audited_kinds`.)
2. An agent writes a forged `log-spool.jsonl` full of `process.crashed` lines, and the app sends it on reconnect: the rows show as spooled and never enter the Audit. (Task 5.2 test `spooled_events_never_reach_the_audit`; Task 5.4 test `spool_is_sent_spooled`.)
3. The daemon gone for an hour while the app runs: the queue keeps the newest 2,048, the spool keeps 1 MB of `process.*` and `daemon.unreachable`, the UI never waits, and both arrive when it's back. (Task 5.4 tests `queue_full_drops_oldest_and_counts`, `push_never_blocks_with_daemon_down`.)
4. A long `perf.stall` storm (a stuck main thread): one `perf.stall` per stall, folded into Metrics, never an Activity row. (Task 5.6 test `stalls_go_to_metrics_only`.)
5. An app connection that sends `log.emit` 10,000 times a second: the limit holds at 20 a second, and other connections' events are unaffected. (Task 5.2 test `rate_limit_is_per_connection`.)

## Order and lanes

```
Daemon lane: 5.1 parse ──► 5.2 log.emit ──► 5.3 metrics ──► 5.7 hook and delegate ──► 5.8 remaining daemon kinds
App lane:    5.4 log client and spool (after 5.2's wire shape) ──► 5.5 app emitters ──► 5.6 stall watch and reload
Last:        5.9 Performance section, bench_log, docs (after 5.3, 5.6, Phase 3 and Phase 4)
```

5.4 to 5.6 (`crates/app/**`) and 5.7 to 5.8 (`crates/notchlingd`, `crates/engine`) touch different files and run in parallel once 5.2 is in.

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited (why) |
| --- | --- | --- | --- |
| 5.1 | Maintainer (`crates/events`) | none | none |
| 5.2, 5.3 | Maintainer (`notchlingd`, `local-api`, `crates/events`, engine `activity/db.rs`) | none new | none expected; `op_kinds_tests` passes once `log.emit` and `log.metrics` have homes |
| 5.4 | auto (`crates/app/**`) but the spool is a new file writer | `CAP-FILEWRITE` (spool: `OpenOptions`, `rename`): `maintainer-gate` holds it | none |
| 5.5, 5.6 | auto | none | none; `bench_chat` counts are run, not edited |
| 5.7, 5.8 | Maintainer (`notchlingd`, engine `bridge/`, `tools/`, `cost/`, `limits/`) | none new | none expected |
| 5.9 | auto for `crates/app/src/logs/**`; maintainer for `crates/notchlingd/tests/bench_log.rs` (new file) and the notes | `WEAK-SKIP` fires on `bench_log`'s `#[ignore]` (it is a release-only bench, like `bench_chat`) | none |

## Maintainer only (Phase 5)

- Approve the spool writer (`CAP-FILEWRITE`), the `#[ignore]` on `bench_log` (`WEAK-SKIP`) and the daemon changes.
- Run on a Mac with a display, in release, one at a time: `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1` and compare the counts with the base branch; `cargo test --release --locked -p notchling-notchlingd --test bench_log -- --ignored --nocapture` and record p99 `push`, p99 `commit` (`F_FULLFSYNC`) and the hook's p99 in decision 0021.

---

### Task 5.1: Parsing a wire event

**Files:**
- Create: `crates/events/src/parse.rs`
- Test: `crates/events/src/parse_tests.rs`

**Interfaces:**
- Produces: `pub enum Invalid { UnknownKind, Retired, KvMismatch, MissingField(&'static str), BadField(&'static str), UnknownField, TooBig }`; `pub fn event_from_wire(kind: &str, kv: u32, ts: i64, f: &serde_json::Value, known_ops: &[&'static str]) -> Result<Event, Invalid>`. Per `FieldDesc`: a choice string not in the list becomes `other`; a flag must be a bool; counts and sizes are non-negative integers; `Percent` clamps; `Version`, `Exe`, `Tool`, `CodeLoc`, `Hex` validate as in Phase 1 (an invalid one becomes `unknown`); `Seal` is refused (`BadField`): no wire event carries a seal; a `Ref` is a string cut as `Ref::new` cuts it; lists over 16 are cut; a missing non-optional field is `MissingField`; a field name the kind doesn't declare is `UnknownField`; an event over 4 KB is `TooBig`. The `outcome` comes from the kind's `result` field when it has one (`ok`, `failed`, `refused` names map; others `info`), else `info`.
- [ ] **Step 1: Write the failing tests:** `parses_a_queue_stuck_event`, `unknown_choice_becomes_other`, `text_in_a_count_is_bad_field` (`"chars": "hello"`), `unknown_field_is_refused` (`"note": "hi"`), `seal_from_wire_is_refused`, `kv_mismatch_is_refused`, `retired_kind_is_refused`, `too_big_is_refused`.
- [ ] **Step 2: Run** `cargo test -p notchling-events parse`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "events: parse a wire event against the registry"`.

### Task 5.2: `log.emit`

**Files:**
- Create: `crates/notchlingd/src/log_emit_ops.rs`, `crates/notchlingd/src/log_limits.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::LOG_EMIT`), `crates/events/src/scope.rs` (`Scope.conn: Option<u64>`), `crates/local-api/src/conn.rs` (sets `conn` to the session's id), `crates/local-api/src/server.rs` (`ServerHandle::note_app_quit(pid: u32)` calls `Presence::on_quit`), `crates/notchlingd/src/log_bus.rs` (`LogBus::push_origin(origin: Origin, event: Event) -> bool`; `pub struct Origin { pub proc: Proc, pub pid: Option<u32>, pub ts_rx: i64, pub spooled: bool }`; the Audit sink is skipped for every event whose origin isn't the daemon), `crates/notchlingd/src/op_kinds.rs` (`log.emit` Quiet: every event it carries is itself recorded), `crates/notchlingd/src/daemon_wiring.rs`
- Test: `crates/notchlingd/src/log_emit_ops_tests.rs`, `crates/notchlingd/src/log_limits_tests.rs`

**Interfaces:**
- Consumes: `event_from_wire`, `op_kinds::known_ops`, `scope::current` (tier, via, conn).
- Produces: `LogEmitOps` (`with_events(Arc<LogBus>)`, `with_app_quit(Arc<dyn Fn(u32) + Send + Sync>)`); `log_limits.rs`: `pub struct Limits` with `allow(conn: u64, now_ms: i64) -> bool` (20 a second and 1,000 an hour per connection, token buckets, a connection's state dropped when it closes or after an hour idle). The caller's `Proc` is `App` for the app tier, `Hook` for `ClientKind::Hook`, `Cli` for `ClientKind::Cli`, else refused. `spooled: true` is accepted only from the app. A `process.stopped` from the app calls `app_quit(pid)`.
- [ ] **Step 1: Write the failing tests:** `app_emits_queue_stuck` (accepted 1, a row in Activity), `agent_cannot_emit_always_or_audited_kinds` (a hook-tier `vault.secret_used` and `switch.changed`: dropped, one `api.refused`, no Audit line, no Activity row), `hook_emits_hook_decided`, `cli_emits_delegate_job`, `spooled_events_never_reach_the_audit` (an app-tier spooled `process.crashed`: Activity row with `spooled`, no Audit line), `rate_limit_is_per_connection` (connection A sends 100 in a second: 20 accepted; connection B's 5 all accepted), `oversized_args_are_bad_request` (257 KB), `process_stopped_detaches_with_quit` (through the server rig: `app.detached{cause: quit}`, not `dropped`).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd -- log_emit log_limits`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`, `cargo test -p notchling-local-api` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd: log.emit with tier rules and limits"`.

### Task 5.3: Metrics

**Files:**
- Create: `crates/events/src/metrics.rs`, `crates/notchlingd/src/log_metrics.rs`
- Modify: `crates/engine/src/activity/db.rs` (`insert_metrics`, `fold_hours`, `prune_metrics`, `read_metrics`), `crates/local-api/src/conn.rs` (push `perf.op{op, code, ms}` for every op and `perf.poll{op}` for a quiet success), `crates/notchlingd/src/log_bus.rs` (per-kind skipped counters for kinds turned off), `crates/notchlingd/src/log_emit_ops.rs` (`metrics` rows from the app), `crates/local-api/src/wire.rs` (`ops::LOG_METRICS`), `crates/notchlingd/src/log_ops.rs` (`log.metrics`), `crates/notchlingd/src/op_kinds.rs` (`log.metrics` Quiet)
- Test: `crates/events/src/metrics_tests.rs`, `crates/notchlingd/src/log_metrics_tests.rs`, `crates/engine/src/activity/db_metrics_tests.rs`

**Interfaces:**
- Produces:
  - `pub const BUCKETS_MS: [u64; 13] = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000]`; `pub struct Histogram { pub buckets: [u64; 14], pub count: u64, pub sum: u64 }` with `observe(ms)`; `pub struct Metrics` with `observe(&Event)` (for `perf.op`: name `perf.op`, labels `op` and `code`; `perf.poll`: a count per `op`; `perf.stall`: label `part`, histogram of `ms`; `perf.reload`: histogram of `ms`, sums of `bytes_parsed` and `rows_built` as separate names `perf.reload.bytes` and `perf.reload.rows`), `skipped(kind: &'static str, n: u64)`, `take(start_ms: i64, proc: Proc) -> Vec<Rollup>`; `pub struct Rollup { pub start: i64, pub span: Span, pub proc: String, pub name: String, pub labels: String, pub count: u64, pub sum: u64, pub buckets: Option<String> }` (`labels` and `buckets` as JSON text with sorted keys); `pub fn rollup_from_wire(&Value) -> Option<Rollup>` (validates names are `perf.*` kinds with `App` in `emitters` and label values against their choice lists).
  - `MetricsSink` implementing `Sink`: observes `METRICS` kinds; every 60 s writes the minute's rollups to `events.db`; hourly folds minute rows older than 24 hours into hourly rows and prunes hourly rows older than 7 days.
  - `log.metrics` (`OpSpec::AGENT`; args `{since?, span?: "minute"|"hour"}`; reply `{rows: [Rollup]}`), for the Logs page and `notchling log metrics`.
- [ ] **Step 1: Write the failing tests:** `histogram_buckets_match_the_spec`, `perf_op_labels_are_op_and_code`, `take_resets_the_minute`, `rollup_from_wire_refuses_unknown_names_and_labels`; `minute_rows_written_every_sixty_seconds` (fake clock), `minutes_fold_into_hours_after_a_day`, `hours_pruned_after_seven_days`, `skipped_kinds_are_counted`; `conn_times_every_op` (server rig: one `perf.op` per call, one `perf.poll` for a quiet success, no Activity rows).
- [ ] **Step 2: Run** `cargo test -p notchling-events metrics`, `cargo test -p notchling-notchlingd log_metrics`, `cargo test -p notchling-engine db_metrics`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "logs: Metrics rolled up into events.db; log.metrics"`.

### Task 5.4: The app's log client and spool

**Files:**
- Create: `crates/app/src/log_client/mod.rs`, `crates/app/src/log_client/queue.rs`, `crates/app/src/log_client/spool.rs`, `crates/app/src/log_client/sender.rs`
- Modify: `crates/app/src/main.rs` (start the client for real roots and fixture roots, not for `--render`), `crates/app/src/mac/shell.rs` (`process.started` at start, `process.stopped{cause: quit}` before quit)
- Test: `crates/app/src/log_client/queue_tests.rs`, `crates/app/src/log_client/spool_tests.rs`, `crates/app/src/log_client/sender_tests.rs`

**Interfaces:**
- Produces: `pub fn log(event: Event)` (global, non-blocking: `try_send` into a 2,048 queue; full → the oldest is dropped and counted); `pub struct Sender` on its own thread: every 500 ms or 64 events, one `log.emit` over its own `Client` (app tier hello); on a failed send the batch goes back to the queue's front; `process.*` and `daemon.unreachable` are also written to the spool when the daemon is away; after a reconnect the spool is sent first, with `spooled: true`, then truncated. `pub fn flush_for_quit(wait: Duration)` sends `process.stopped` and waits at most 500 ms. `spool.rs`: `append(path, line)`, `drain(path) -> Vec<String>`, `cap(path, 1 MB)` (oldest lines dropped, the count kept for a `log.dropped`-shaped Diagnostics point), 0600, never through a symlink. Every 60 s the sender also sends the app's `Metrics::take` rollups in `log.emit`'s `metrics`.
- [ ] **Step 1: Write the failing tests:** `queue_full_drops_oldest_and_counts`, `push_never_blocks_with_daemon_down` (10,000 pushes with no daemon finish in under 50 ms), `batch_by_time_and_size`, `spool_keeps_only_process_and_unreachable`, `spool_capped_at_one_megabyte`, `spool_is_sent_spooled` (a real local-api server in a short `/tmp` folder with a recording handler: the spooled lines arrive with `spooled: true`, then the spool is empty), `spool_refuses_symlink` (unix).
- [ ] **Step 2: Run** `cargo test -p notchling-app log_client::`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-app` and clippy on the Mac. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "app: a log client that never blocks, with a spool for when the daemon is away"`.

### Task 5.5: The app's own kinds

**Files:**
- Create: `crates/app/src/chat/queue_watch.rs`
- Modify: `crates/app/src/sheets/settings.rs` (`SettingsView::apply` logs `settings.changed` through `notchling_engine::settings::log_value`), `crates/app/src/chat/loader.rs` (runs the queue watch off the main thread), `crates/app/src/session_source.rs` and `crates/app/src/actions.rs` (`daemon.unreachable{why, count}` once a minute at most while it lasts; `op.failed` only for client-side failures: a session call's 60 s timeout or a reply that isn't JSON, with `why: timeout` or `other`)
- Test: `crates/app/src/chat/queue_watch_tests.rs`, `crates/app/src/log_client/kinds_tests.rs`

**Interfaces:**
- Produces: `pub struct QueueWatch`; `QueueWatch::check(&mut self, session: &str, items: &[QueuedItem], working: bool, pid_changed: bool, now_ms: i64) -> Vec<SessionQueueStuck>`: once per entry Queued for over 2 minutes; `guess`: `bash_input_unmatched` when the entry starts with `!`, `claude_restarted` when the session's pid changed since it was queued, `still_working` when the session works, else `no_dequeue_seen`; an entry is reported once for the app's life. `QueuedItem` is whatever `transcript::conversation::queue` exposes (an id and a time; never its text, which the watch doesn't keep). `pub fn unreachable_why(&ClientError) -> UnreachableWhy` (`no_socket`, `refused`, `timeout`, `untrusted_server`, `version`).
- [ ] **Step 1: Write the failing tests:** `stuck_after_two_minutes_once`, `guesses_in_order`, `text_never_kept` (the watch's state holds ids and times only: a marker in the item's text is absent from `format!("{:?}", watch)`), `settings_changed_uses_log_value`, `unreachable_why_maps_each_error`, `unreachable_logged_at_most_once_a_minute`.
- [ ] **Step 2: Run** `cargo test -p notchling-app -- queue_watch log_client::kinds`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-app` and clippy on the Mac. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "app: settings.changed, session.queue_stuck, daemon.unreachable"`.

### Task 5.6: Stalls and reloads

**Files:**
- Create: `crates/app/src/mac/stall_watch.rs`, `crates/app/src/stall.rs` (pure)
- Modify: `crates/app/src/chat/view/pane.rs` and `crates/app/src/chat/view/root.rs` (note the part being drawn and the draw's time, through `stall::Part` guards), `crates/app/src/notch/` view root (the same, part `notch`), `crates/app/src/chat/loader.rs` (`perf.reload{bytes_parsed, rows_built, ms}` per load, from the counts the bench already reads), `crates/app/src/mac/shell.rs` (start the watch)
- Test: `crates/app/src/stall_tests.rs`

**Interfaces:**
- Produces: `stall.rs`: `pub enum Part { ChatRoot, Conversation, Sidebar, Details, Notch, Other }`; `pub struct Draw` guard (`Draw::begin(Part)` sets a thread-local and an atomic; dropping it measures and, over 50 ms, logs `perf.stall{part, ms}`); `pub fn late_tick(expected_ms: i64, got_ms: i64) -> Option<u64>` (lateness over 50 ms). `mac/stall_watch.rs`: a GPUI timer on the main thread every 500 ms; a tick late by more than 50 ms logs `perf.stall{part: <the atomic's part, else other>, ms: lateness}`. No frame is requested by either: the timer only reads a clock.
- [ ] **Step 1: Write the failing tests:** `draw_over_fifty_ms_logs_a_stall`, `draw_under_fifty_ms_logs_nothing`, `late_tick_lateness`, `stalls_go_to_metrics_only` (through Task 5.3's `Metrics` and a test sink: `perf.stall` has no Activity sink, so no row).
- [ ] **Step 2: Run** `cargo test -p notchling-app stall`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-app`, clippy on the Mac, then in release on a Mac with a display `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1`. Expected: PASS and the same counts as the base (root, conversation, Sidebar, Details Panel draws; rows built).
- [ ] **Step 5: Commit** by path, `git commit -m "app: perf.stall and perf.reload; bench counts unchanged"`.

### Task 5.7: The hook and `notchling delegate`

**Files:**
- Modify: `crates/notchlingd/src/hook_ops.rs` (`hook.decided{session, event, tool, outcome, waited_ms}` for every `hook.request`: `asked_app`, `passed_no_app`, `passed_acting_off`, `refused_not_descendant`), `crates/notchlingd/src/hook.rs` (a pass it decides alone while the daemon is up: `passed_too_big` for an event over 1 MiB, `passed_timeout` at its 290 s deadline, sent with `log.emit` on the connection it has), `crates/notchlingd/src/hook_pages.rs` (`pages.saved{page, action, account}` through `log.emit` when a daemon answers, else a Diagnostics point only), `crates/notchlingd/src/delegate_cli.rs` (`delegate.job{tool, model, write, result, duration_ms}` after the job)
- Test: `crates/notchlingd/src/hook_decided_tests.rs`, `crates/notchlingd/tests/delegate_job_e2e.rs` (new)

**Interfaces:**
- `hook.decided.tool` is a `Tool` (the hook event's tool name, validated), never the tool input. `pages.saved.page` is a `Ref<Page>` (the artifact id).
- [ ] **Step 1: Write the failing tests:** `each_outcome_records_hook_decided` (the `hook_ops_tests.rs` rig), `too_big_pass_is_emitted_by_the_hook` (the built `notchling hook` over a fixture event padded past 1 MiB with a daemon running: one `hook.decided{passed_too_big}` row), `tool_input_never_recorded` (a marker in the tool input is absent from Activity and the hook's Diagnostics file), `delegate_job_recorded` (the e2e runs `notchling delegate` against the fake Codex binary the existing `tests/delegate_cli.rs` uses).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd hook_decided` and `cargo test -p notchling-notchlingd --test delegate_job_e2e`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` (all targets) and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "hook, delegate: hook.decided, pages.saved, delegate.job"`.

### Task 5.8: The remaining daemon kinds

**Files:**
- Create: `crates/engine/src/bridge/conn_fold.rs`, `crates/notchlingd/src/version_seen.rs`
- Modify: `crates/notchlingd/src/codex_ops.rs`, `crates/notchlingd/src/agy_ops.rs` (`tool.turn{tool, session, model, result, duration_ms, tokens_in, tokens_out}` when a turn ends), the limits announcer in `crates/notchlingd/src/limits_ops.rs` (`limits.crossed` at 50, 75, 90, 100), `crates/notchlingd/src/cost_ops.rs` (`usage.daily` for yesterday, per account and model, at the first tick after local midnight, once: `meta.usage_daily_last`), `crates/engine/src/bridge/net/bridge.rs` and `access.rs` (`bridge.tunnel{action, result}` through `Bridge::with_events`), `crates/engine/src/bridge/hub.rs` (`phone.connected` with `conn_fold`), `crates/engine/src/bridge/router.rs` (`phone.viewed{device, view, session}` on reads; off by default), `crates/notchlingd/src/daemon.rs` (the sessions monitor feeds `version_seen`)
- Test: `crates/engine/src/bridge/conn_fold_tests.rs`, `crates/notchlingd/src/version_seen_tests.rs`, `crates/notchlingd/src/remaining_kinds_tests.rs`

**Interfaces:**
- Produces: `conn_fold.rs`: `pub struct ConnFold` (`connected(device, via, now) -> Option<PhoneConnected>`, `disconnected(device, ops, now) -> Option<PhoneConnected>`): a reconnect within 5 minutes folds into the open row; the closed row carries `duration_ms` and `ops`. `version_seen.rs`: `pub struct VersionSeen` (`offer(tool, version) -> Option<ToolVersionSeen>`, the seen set kept in `events.db` `meta` as `versions_seen`); the version comes from what the daemon already reads (Claude Code's session files and transcript lines, Codex rollout meta, `agy` replies), never by running a tool.
- [ ] **Step 1: Write the failing tests:** `reconnect_within_five_minutes_folds`, `long_gap_is_a_new_row`, `version_seen_once_per_tool_and_version`, `tool_turn_recorded_on_codex_done` (the `FakeSpawner` rig of `codex_ops_tests.rs`), `limits_crossed_at_each_threshold_once`, `usage_daily_once_per_day`, `phone_viewed_off_by_default` (a router read with defaults writes nothing; with the kind on, one row).
- [ ] **Step 2: Run** `cargo test -p notchling-engine conn_fold` and `cargo test -p notchling-notchlingd -- version_seen remaining_kinds`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "logs: tool.turn, limits.crossed, usage.daily, bridge.tunnel, phone.connected, phone.viewed, tool.version_seen"`.

### Task 5.9: Performance on the Logs tab, `bench_log`, docs

**Files:**
- Create: `crates/app/src/logs/performance.rs`, `crates/notchlingd/tests/bench_log.rs`
- Modify: `crates/app/src/logs/model.rs`, `crates/app/src/logs/view.rs`, `crates/app/src/logs/wire.rs` (`log.metrics`), `crates/app/src/logs/renders.rs` (`settings-logs-performance`), `docs/local-api-ops.md`, `crates/AGENTS.md`, `crates/app/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `docs/policies/privacy.md` (the spool file, Metrics), `README.md`, `docs/feature-map.md`
- Test: `crates/app/src/logs/performance_tests.rs`

**Interfaces:**
- Produces: `pub struct Slowest { pub op: String, pub p95_ms: u64, pub count: u64 }`; `pub fn slowest_today(rows: &[Value], now_ms: i64) -> Vec<Slowest>` (top 5 `perf.op` by p95, from the buckets); `pub fn stalls_today(rows) -> Vec<(String, u64)>` (count of `perf.stall` per part). The section shows "Slowest operations today" and "Slow moments in Chat", and three toggles (`perf.op`, `perf.stall`, `perf.reload`).
- `bench_log.rs` (`#[ignore]`, release): 100,000 mixed events (10% `Always`, 1% commits) from 8 threads into a `LogBus` over a temp root with the file key store; asserts no `Always` event lost, p99 `push` under 5 µs, p99 `commit` under 50 ms, the writer keeping up at 5,000 events a second; also times 1,000 `DiagWriter::write_now` calls and asserts p99 under 2 ms. Prints one `bench-log:` line.
- [ ] **Step 1: Write the failing tests:** `slowest_from_buckets`, `stalls_by_part`, `empty_metrics_say_none_yet`.
- [ ] **Step 2: Run** `cargo test -p notchling-app logs::performance`; expected FAIL.
- [ ] **Step 3: Implement** the section and `bench_log.rs`; write the docs.
- [ ] **Step 4: Run** `cargo test -p notchling-app`, `--render settings-logs-performance` in release on a Mac, and `cargo test --release --locked -p notchling-notchlingd --test bench_log -- --ignored --nocapture`. Expected: PASS; the bench line within budget.
- [ ] **Step 5: Commit** by path, `git commit -m "logs: Performance on the Logs tab; bench_log"`.

## Checkpoint P5

- `cargo test --workspace --locked`, clippy on the Mac, `cargo fmt --all --check`, `cargo deny check`, `tools/test-python.sh`, `python3 -m guard lint`, `rules`, `claudemd`.
- `bench_chat` and `bench_log` in release on a Mac with a display (maintainer, if the builder has no display); the numbers go in the pull request.
