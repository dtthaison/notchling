# Providers, Phase 2: Signals, One Finished Event Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The daemon notices when a turn, a subagent or a delegated job finishes, for every tool, and says so once on a new `signals` topic with a replayable ring (`signals.recent`). The notch peeks, the Details Panel reloads, the Inbox raises a done item for a background job, the phone is pushed the same events, held messages go out on the turn's end, and a new setting, Tell Me When Work Finishes, sends system banners (default: long turns and background jobs, only when Chat isn't in front).

**Architecture:** A pure step in the engine (`contract::signals::step(seen, snapshot) -> Vec<Signal>`) compares snapshots. The daemon builds snapshots from every tool's session list (Claude Code from the Monitor; Codex and Antigravity from `bridge::foreign`'s rows, read only, whatever the bridge switch and with or without an app, call 5 of the spec), the registry's subagents for sessions that work or worked lately, a cached finished-agents reader for Claude Code parents (spec call 6), and the delegation records. `SignalWatch` (`notchlingd/src/signal_watch.rs`, call C19) runs it on watch batches and every 5 s, publishes, and keeps the last 500 in a ring. Each listener stays where it lives today.

**Tech Stack:** Rust 1.99.1, `platform::watch`, the local API (`OpGroup`, topics), the phone protocol generator (`tools/protocol/gen.py`), Flutter for one phone alert.

**Spec:** sections 6.5, 7.2, 9 (all), 12 (phase 2), 14.1, calls 5 and 6. Phases 0 and 1 merged. Calls C15 to C19, C24, C25, C28 in the overview.

**Branch:** `agent/providers-p2-signals`. The phone part (Task 2.9) may go as `agent/providers-p2-phone` after 2.4.

## Global Constraints

- Phase 0 and 1 constraints hold.
- The first snapshot after a daemon start fires nothing. A finish fires once per (session, subagent) or (session, turn start) per daemon run. Quiet is not finished. A subagent first seen already finished fires only if it started after the baseline. A turn must have been seen Working.
- An idle session costs nothing: subagents are looked at only for a session that works or had a running subagent in the last 10 minutes, plus every delegation record not ended.
- The local API and the phone protocol change additively only: new ops, a new topic, new optional fields, a new event; `SubagentState` keeps `running` and `done`.
- No signal carries message text: ids, tool, status, times, a description (a subagent's description, a job's prompt line, as the Details Panel shows) and token counts.
- Banners go through the app's one notification worker (`inbox::system::Detached`), once per event `seq`, never for a subagent of a session that still works, and only when Chat isn't the key window.
- The setting is a new key in `settings/keys.rs`, user-facing words through the locale rule in force (call C25). If hybrid search P1 T8 has merged, the key takes `.effect(..)` as its siblings do.

## Review Focus

1. A daemon restart in the middle of six running Codex children: no burst of six finishes at start, and each child fires once when it ends. (Task 2.1 tests `the_first_snapshot_fires_nothing`, `a_finish_fires_once`.)
2. The finished-agents reader reads only what a transcript gained: a 40 MB parent costs 1 MB at first and the new bytes after. (Task 2.3 test `a_grown_transcript_reads_only_the_new_bytes`.)
3. `sessions.list` callers that send no argument get exactly what they got before (Claude Code rows only); the bridge, `notchling sessions` and the app's session source ask for more on purpose. (Task 2.2 test `no_argument_lists_claude_code_only`.)
4. The phone: an old phone that never subscribes with `finished: true` receives no new message type; a new one gets each finish once per connection, also after a reconnect within the ring. (Task 2.9 tests `an_old_subscription_gets_no_finish`, `a_cursor_resumes_after_a_reconnect`.)
5. A banner rule bug that floods: six Codex workers under a working parent make zero banners in every mode. (Task 2.1 test `a_working_parents_subagent_never_banners`.)

## Order and lanes

```
2.1 pure step and banner rule ─┬─► 2.4 SignalWatch, signals.recent ─┬─► 2.5 held on turn.finished
2.2 daemon lists every tool ───┤                                    ├─► 2.6 Inbox JobFinished
2.3 finished-agents reader ────┘                                    ├─► 2.7 app: notch, Details, feed ─► 2.8 setting and banners
                                                                    ├─► 2.9 phone
                                                                    ├─► 2.10 keep awake
                                                                    └─► 2.11 log kinds (only if logging P1 merged)
2.12 docs, matrix (last)
```

Lanes: **A** engine pure (2.1, 2.3), **B** daemon lists (2.2), then **C** the watch (2.4) and its daemon listeners (2.5, 2.6, 2.10, 2.11), **D** app (2.7, 2.8), **E** phone (2.9: `protocol/**`, `crates/protocol/**`, `crates/engine/src/bridge/**`, `apps/flutter/**`).

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 2.1 | Maintainer (`contract/**`) | none | none |
| 2.2 | Maintainer (`notchlingd/**`, `bridge/**`, `contract/**`) | none | none: `sessions.list` keeps its default; `clients.rs` test module gains a test |
| 2.3 | Auto (`transcript/**`) | none | none |
| 2.4 | Maintainer (`notchlingd/**`, `local-api/**`) | none | none |
| 2.5 | Maintainer (`held_ops.rs`, `reply/**`) | none | none: the 1 s tick stays for the 20 s grace; `held_tests` unchanged |
| 2.6 | Maintainer (`inbox/**`, `notchlingd/**`) | none | none |
| 2.7 | Auto (`app/**`) | none | none |
| 2.8 | Maintainer (`settings/keys.rs`); auto (`app/**`) | none | in-crate, listed: `sheets/settings.rs` tests that list the General page's rows (line 448 area) if they pin the row list |
| 2.9 | Maintainer (`protocol/**`, `crates/protocol/**`, `bridge/**`, `apps/flutter/lib/app/**`, `apps/flutter/lib/core/protocol/**`) | `GUARD-PROTOCOL-GEN` passes after `gen.py`; `CAP-NETWORK` none | none: new `apps/flutter/test/finish_alerts_test.dart`; `message_alerts_test.dart` untouched |
| 2.10 | Maintainer (`platform/**` untouched; `contract/**`) | none | none |
| 2.11 | Maintainer (`notchlingd/**`, the logging registry) | none | none |
| 2.12 | Maintainer (`AGENTS.md`, feature map); auto (`docs/provider-matrix.md`, `docs/local-api-ops.md`) | `ZONE` | none |

## Maintainer only (Phase 2)

- The protocol schema, generated types and the phone's `lib/app/` alert (2.9).
- The new settings key `notifyFinished` (2.8) and the daemon's watch and ops (2.2, 2.4 to 2.6).
- Whether the quiet rule's 30 s window is right for Antigravity terminal runs (2.4; the builder starts at 30 s).

---

### Task 2.1: The pure step and the banner rule

**Files:**
- Create: `crates/engine/src/contract/signals.rs`
- Test: `crates/engine/src/contract/signals_tests.rs`

**Interfaces:**
- Produces:
  - `pub struct Snapshot { pub at: f64, pub sessions: Vec<SessionState>, pub subagents: Vec<SubagentState>, pub jobs: Vec<JobState> }`; `SessionState { id, tool: Tool, working: bool, needs_you: bool, turn_started: Option<f64> }`; `SubagentState { session, parent, id, depth: u8, tool: Tool, via: Via, description, status: Status, started: f64, updated: f64, tokens: Option<u64> }`; `JobState { job, session: Option<String>, parent: Option<String>, tool: Tool, description, state: String, background: bool, started: f64, ended: Option<f64> }`.
  - `pub enum Signal { TurnFinished { session, tool, outcome: Outcome, started, ended }, SubagentFinished { session, parent, subagent, depth, tool, via, description, status, started, ended, tokens }, DelegateFinished { job, session: Option<String>, parent: Option<String>, tool, description, status, background, started, ended } }`; `pub enum Outcome { Done, Failed, Stopped, NeedsYou }` (wire `done`, `failed`, `stopped`, `needs_you`); `Signal::to_json(&self, seq: u64) -> Value` in the spec 9.1 shapes; `Signal::event(&self) -> &'static str` (`turn.finished`, `subagent.finished`, `delegate.finished`).
  - `pub struct Seen` (the baseline flag, fired keys `(session, turn start)`, `(session, subagent)`, jobs; working sessions and their turn starts) with `Seen::default()`; `pub fn step(seen: &mut Seen, now: &Snapshot) -> Vec<Signal>`.
  - `pub enum Notify { Off, Long, All }` (`parse` of the setting's value, default `Long`), `pub const LONG_TURN_SECS: f64 = 60.0`, `pub fn banner(mode: Notify, s: &Signal, chat_in_front: bool, parent_working: bool) -> bool`.
  - `pub fn busy(now: &Snapshot) -> bool` (any working session, running subagent or open job; for keep awake, Task 2.10).

- [ ] **Step 1: Write the failing tests:** `the_first_snapshot_fires_nothing`, `a_turn_seen_working_then_idle_fires_done`, `a_turn_never_seen_working_fires_nothing`, `needs_you_is_an_outcome`, `a_finish_fires_once` (same snapshot twice), `quiet_is_not_finished`, `a_subagent_first_seen_finished_fires_only_if_it_started_after_the_baseline`, `a_job_without_a_session_fires_delegate_finished`, `a_job_with_a_parent_fires_delegate_finished_with_its_parent`; banners: `off_never_banners`, `long_banners_a_61_second_turn_not_a_59_second_one`, `long_banners_a_background_job_not_a_foreground_one`, `all_banners_every_finish`, `chat_in_front_never_banners`, `a_working_parents_subagent_never_banners` (every mode).
- [ ] **Step 2: Run** `cargo test -p notchling-engine contract::signals_tests`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "signals: the pure step, once only, and the banner rule"`.

### Task 2.2: The daemon lists every tool's sessions

**Files:**
- Create: `crates/engine/src/contract/sessions.rs` (the `Sessions` port's listing slice, call C6), `crates/notchlingd/src/every_session.rs`
- Modify: `contract/provider.rs` (`fn sessions(&self) -> &dyn Sessions`), `contract/{claude,codex,antigravity}/mod.rs` (`WIRED` gains `session.list`), `unwired.json` (−3), `crates/notchlingd/src/daemon.rs` (`Lister` answers `sessions.list {"tools": "all"}` with the Monitor's rows plus `every_session`'s), `crates/notchlingd/src/bridge_backend.rs` (`DaemonSources::foreign_rows` from the same list: today it returns none, so the phone through the daemon lists Claude Code only), `crates/notchlingd/src/clients.rs` (`notchling sessions` asks with `tools: "all"`), `docs/local-api-ops.md`
- Test: `crates/notchlingd/src/every_session_tests.rs`, a new test in `clients.rs`'s test module

**Interfaces:**
- Produces: `pub trait Sessions: Send + Sync { fn list(&self, roots: &Roots, now: Timestamp) -> Result<Vec<SessionRow>, Unsupported>; }` with `SessionRow { id: String, tool: Tool, name: String, folder: String, working: bool, modified: f64 }`; Codex lists `list::rollouts(.., 14 days, 30)` minus children (`is_child` from the index when the daemon has it, else `lineage`); Antigravity lists `summaries::rows_cached` (`nesting_depth = 0`, as now); Claude Code's rows stay the Monitor's. `EverySession::new(layout, children: Arc<CodexChildren>)`, `rows(&self, now) -> Vec<ForeignRow>` (cached 2 s, read only, no switch, no app needed).
- `sessions.list` takes an optional `{"tools": "all" | "claude"}`; absent means `claude` (call C28). The reply's rows carry `tool` (`claude`, `codex`, `antigravity`).

- [ ] **Step 1: Write the failing tests** (a fixture root with Claude Code, Codex and Antigravity sessions, the bridge switch off, no app attached): `no_argument_lists_claude_code_only`, `all_lists_every_tool_with_the_bridge_off`, `a_codex_child_is_not_listed`, `foreign_rows_reach_the_bridge_backend`; in `clients.rs`: `sessions_asks_for_every_tool`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd every_session`; expected FAIL.
- [ ] **Step 3: Implement**; `session.list` wired for the three, `unwired.json` −3, conformance checks in `contract/conformance_sessions.rs` (new).
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd -p notchling-engine`, the e2e `cargo test -p notchling-notchlingd --test '*_e2e'`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "daemon: every tool's sessions, read only, for signals, the phone and notchling sessions"`.

### Task 2.3: The finished-agents reader

**Files:**
- Create: `crates/engine/src/transcript/finished.rs`
- Test: `crates/engine/src/transcript/finished_tests.rs`

**Interfaces:**
- Produces: `pub struct Finished` (a cache keyed by transcript path: offset read up to, size, mtime, and the map id to status), `Finished::new()`, `statuses(&self, transcript: &Path) -> Arc<HashMap<String, String>>`: the first read covers the last `FIRST_READ: u64 = 1_048_576` bytes; later reads only the bytes past the offset, when size or mtime changed; a file that shrank (rewritten) is read again from its last 1 MB; each line's `<task-notification>` blocks through `subagents::notification_statuses`. `evict(&self, keep: &HashSet<PathBuf>)`.

- [ ] **Step 1: Write the failing tests:** `the_first_read_is_the_last_megabyte`, `a_grown_transcript_reads_only_the_new_bytes` (count bytes through a test hook `Finished::read_bytes()`), `an_unchanged_transcript_reads_nothing`, `a_shrunk_transcript_is_read_again`, `a_line_cut_at_the_window_start_is_skipped`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine transcript::finished_tests`; expected FAIL.
- [ ] **Step 3: Implement.** Chat keeps reading statuses from the conversation it parses (`ChatData`), call 6 of the spec: this reader is the daemon's.
- [ ] **Step 4: Run** the same. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "transcript: a cached reader of a parent's finished subagents"`.

### Task 2.4: `SignalWatch`, the `signals` topic and `signals.recent`

**Files:**
- Create: `crates/engine/src/contract/signals_port.rs` (`Signals` port: `turn_finished`, `subagent_finished` statuses for a snapshot), `crates/notchlingd/src/signal_watch.rs`, `crates/notchlingd/src/signals_ops.rs`, `crates/notchlingd/src/signal_watch_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`topics::SIGNALS = "signals"`, events `turn.finished`, `subagent.finished`, `delegate.finished`; `ops::SIGNALS_RECENT = "signals.recent"`), `crates/notchlingd/src/daemon_wiring.rs` (start and stop the watch; the `OpGroup`), `contract/codex/support.rs` and `contract/antigravity/support.rs` (`signal.subagent_finished` Native), `contract/*/mod.rs` (`WIRED` gains `signal.turn_finished` for all three and `signal.subagent_finished` for all three), `gaps.json` (−2), `unwired.json` (−4), `contract/conformance_signals.rs` (new; each tool's `before/` and `after/` fixture pair under `tests/fixtures/conformance/<tool>/signals/`)
- Test: `signal_watch_tests.rs`

**Interfaces:**
- Produces: `SignalWatch::new(sources: WatchSources, room: Arc<dyn Room>) -> Arc<SignalWatch>`, `start(self: &Arc<Self>)` (one thread: `platform::watch` batches under the Monitor's folders, the Codex day folders, the Antigravity summaries db's folder and `<data>/delegations`; and every 5 s), `stop()`, `recent(&self, after: Option<u64>) -> Vec<Value>` (ring of `RING: usize = 500`), `subscribe(&self, f: Box<dyn Fn(&Signal) + Send + Sync>)` for in-daemon listeners (held queue, Inbox, keep awake, log). `seq` starts at 1 per run.
- An Antigravity session with no `agy acp` of Notchling's: `turn.finished` when its summaries row goes `RUNNING` to `IDLE`; where it never showed `RUNNING`, the quiet rule: the log's last step is a `PLANNER_RESPONSE` with status `DONE` and the log has not grown for `QUIET_SECS: f64 = 30.0`.
- `signals.recent {"after": <seq>?}` → `{"events": [...], "last": <seq>}`, `OpSpec::APP`. Topic tier app.

- [ ] **Step 1: Write the failing tests** (a fixture root copied to a short `/tmp` folder, a real server, fake clocks through `WatchSources::now`): `a_claude_turn_finishes_once`, `a_codex_child_finishing_is_a_subagent_signal`, `an_antigravity_terminal_run_finishes_by_the_quiet_rule`, `an_ended_job_is_a_delegate_signal`, `recent_replays_after_a_seq`, `the_ring_keeps_500`, `nothing_fires_with_the_bridge_off_and_no_app_until_a_change` (the baseline), `an_agent_tier_client_cannot_read_signals`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd signal_watch`; expected FAIL.
- [ ] **Step 3: Implement**; the watch holds no lock across a read; a session idle for an hour is never read past its row.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd -p notchling-engine` (conformance `*::signal_*` run). Expected: PASS; summary `gaps 24`, `unwired 61`.
- [ ] **Step 5: Commit** `git commit -m "daemon: SignalWatch, the signals topic and signals.recent"`.

### Task 2.5: Held messages go out on `turn.finished`

**Files:**
- Modify: `crates/notchlingd/src/held_ops.rs` (a `SignalWatch` listener calls `flush_held` for the finished session's id; the 1 s tick stays only to release a hold whose turn never started within `START_GRACE_MS`), `crates/engine/src/reply/held.rs` (no change to `ready`)
- Test: `crates/notchlingd/src/held_signal_tests.rs` (new)

**Interfaces:**
- Consumes: `SignalWatch::subscribe`. Produces: `ActingOps::on_turn_finished(&self, session: &str, now_ms: i64)`.

- [ ] **Step 1: Write the failing tests:** `a_held_message_goes_out_on_the_finish_event`, `the_grace_still_releases_a_turn_that_never_started`, `acting_off_sends_nothing_on_a_finish`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd held_signal_tests`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` (`held_tests` unchanged). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "held: send on turn.finished; the tick only keeps the start grace"`.

### Task 2.6: Inbox `JobFinished`

**Files:**
- Modify: `crates/engine/src/inbox/model.rs` (`Kind::JobFinished`, additive, serde `job_finished`), `crates/engine/src/delegate/record.rs` (`Delegation.background: bool`, `#[serde(default)]`, read only in this phase, call C17), `crates/notchlingd/src/inbox_ops.rs` (a `SignalWatch` listener raises one item per background `delegate.finished`), `crates/app/src/inbox/` (the row's Open goes to the parent's row)
- Test: `crates/notchlingd/src/inbox_job_tests.rs` (new), `crates/engine/src/inbox/job_tests.rs` (new)

**Interfaces:**
- Produces: item id `job:<job id>` (fresh per event), title `<Tool> job finished` / `failed` / `stopped`, detail the job's description, no responder, expires by the clock like a limit notice; `open` carries the parent's row id when there is one.

- [ ] **Step 1: Write the failing tests:** `a_background_job_raises_a_done_item`, `a_foreground_job_raises_none`, `the_item_expires_by_the_clock`, `an_old_book_without_the_kind_loads` (a stored row of another kind still loads; a stored `job_finished` row loads in this build).
- [ ] **Step 2: Run**; expected FAIL.
- [ ] **Step 3: Implement.** Cluster plans B T11, C T8 and E T8 also add `Kind` values: whoever merges second rebases the enum (additive, no renumbering).
- [ ] **Step 4: Run** `cargo test -p notchling-engine -p notchling-notchlingd -p notchling-app`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "inbox: a background job's finish is an item"`.

### Task 2.7: The app hears signals: the notch, the Details Panel

**Files:**
- Create: `crates/app/src/signals/mod.rs` (`feed.rs`: follows the topic and reads `signals.recent` after a reconnect, off the UI thread, as `inbox/feed.rs` does)
- Modify: `crates/app/src/notch/model.rs` (new `NotchModel::peek_finished(&mut self, s: &FinishedPeek, now_ms) -> Option<String>`, call C15), `crates/app/src/notch/panel.rs:233` (after `update` returns `None`, ask `peek_finished` with the newest unseen signal), `crates/app/src/chat/view/load.rs` (a `subagent.finished` or `delegate.finished` for the selected session asks for a load; the row's status comes from the load, never the event)
- Test: `crates/app/src/signals/feed_tests.rs`, `crates/app/src/notch/finished_tests.rs` (new)

**Interfaces:**
- Produces: `FinishedPeek { session, text, working_parent: bool }` built from a signal; the peek order stays: a question, needs you, a turn finished, then a subagent or job finished; a subagent of a session that still works never peeks.

- [ ] **Step 1: Write the failing tests:** `a_job_finish_peeks_its_parent_after_the_existing_order`, `a_working_parents_subagent_does_not_peek`, `a_finish_for_the_selected_session_asks_for_a_load`, `a_reconnect_reads_recent_after_the_last_seq`.
- [ ] **Step 2: Run** `cargo test -p notchling-app signals notch::finished_tests`; expected FAIL.
- [ ] **Step 3: Implement**; no socket call on the UI thread.
- [ ] **Step 4: Run** `cargo test -p notchling-app`; on the Mac `bench_chat` (the feed adds no part draw while idle). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "app: the notch and the Details Panel hear finish signals"`.

### Task 2.8: Tell Me When Work Finishes

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`choice("notifyFinished", "long", &["off", "long", "all"], false, "Tell Me When Work Finishes").free(..)`), `crates/app/src/sheets/settings.rs` (a row on the General page, `choice(read, "notifyFinished", "Tell Me When Work Finishes", &NOTIFY_FINISHED)` with titles `Off`, `Long Turns and Jobs`, `Everything`), `crates/app/src/signals/mod.rs` (each signal: `banner(mode, s, chat_in_front, parent_working)`; a yes goes to `inbox::system::Detached`, once per `seq` for the app's life), `locales/en/mac/settings.ftl` if localization L1 has merged
- Test: `crates/app/src/signals/banner_tests.rs`, a new test in `settings/keys.rs`'s test module

**Interfaces:**
- Consumes: `contract::signals::{Notify, banner}`. Produces: `signals::Banners { notified: HashSet<u64> }`, `Banners::consider(&mut self, s: &Signal, seq: u64, mode: Notify, chat_in_front: bool, parent_working: bool) -> Option<Notification>`.
- Text: `<Tool> finished · <session name>` for a turn; `<Tool> job finished · <description>` for a job; a subagent `<description> finished · <session name>`.

- [ ] **Step 1: Write the failing tests:** `the_key_defaults_to_long`, `each_seq_banners_once`, `off_stops_banners_but_not_the_notch` (the notch peek still asked), `chat_in_front_holds_banners`.
- [ ] **Step 2: Run**; expected FAIL.
- [ ] **Step 3: Implement**; `Chat in front` is the Chat window being the key window and the app active (read on the UI thread when the signal arrives, passed to the pure rule).
- [ ] **Step 4: Run** `cargo test -p notchling-engine -p notchling-app`; render `--render settings` and look at the new row in light and dark. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "settings: Tell Me When Work Finishes, banners for long turns and jobs"`.

### Task 2.9: The phone: schema, the Hub's cursor, subagents, an alert

**Files:**
- Modify: `protocol/agentconsole.v1.schema.json` (on `Subagent`: optional `tool`, `via`, `outcome`, `parent`; a new event `Finished` with `seq`, `kind` (`turn`, `subagent`, `job`), `session`, `subagent`, `job`, `tool`, `status`, `description`, `ended`; `SubscribeRequest` gets optional `finished: boolean`), `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart` (regenerated by `python3 tools/protocol/gen.py`), `protocol/fixtures/` (new files for the event and a subscribe with `finished`), `crates/engine/src/bridge/hub.rs` (`Subscription::finished_after: Option<u64>`; `tick` reads `BridgeBackend::signals_after` per connection that subscribed with `finished: true`), `crates/engine/src/bridge/live.rs` (`Sources::signals_after(&self, after: u64) -> Vec<Value>` defaulting to none; `Sources::subagents(&self, session: &str) -> Vec<SessionSubagent>` defaulting to none), `crates/notchlingd/src/bridge_backend.rs` (both from `SignalWatch` and the registry), `crates/engine/src/bridge/model.rs` (`Session.subagents` filled), `apps/flutter/lib/app/message_alerts.dart` (a finish alert, muted like other alerts), `apps/flutter/lib/core/protocol/models.dart` (read the fields with fallbacks)
- Test: `crates/engine/src/bridge/hub_finished_tests.rs` (new), `apps/flutter/test/finish_alerts_test.dart` (new)

**Interfaces:**
- Follow `.claude/skills/add-protocol-field` for steps 1, 2, 4, 6, 7; its Swift fixture step is replaced by hand-written files in `protocol/fixtures/` (the Swift app is retired; never edit `legacy/macos/Tests/`). The values of `Subagent.tool` follow the phone's existing vocabulary (`claude`, `codex`, `agy`, call C31); `SubagentState` keeps `running` and `done` (a finished subagent is `done`, its `outcome` says how).

- [ ] **Step 1: Write the failing tests:** `hub_finished_tests.rs`: `an_old_subscription_gets_no_finish`, `a_finished_subscription_gets_each_finish_once`, `a_cursor_resumes_after_a_reconnect`, `subagents_are_filled_with_parent_and_depth`. Flutter: `a finish alert shows once and respects mute`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine bridge::hub_finished_tests`, `cd apps/flutter && flutter test test/finish_alerts_test.dart`; expected FAIL.
- [ ] **Step 3: Implement**, schema first, then `python3 tools/protocol/gen.py`.
- [ ] **Step 4: Run** `python3 tools/protocol/gen.py --check`, `python3 -m unittest discover -s tools/protocol/tests -t tools/protocol`, `cargo test --workspace --locked`, `cd apps/flutter && flutter analyze && flutter test`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "phone: finish events from signals.recent, subagents filled"`.

### Task 2.10: Keep awake counts running subagents and jobs

**Files:**
- Modify: `crates/engine/src/contract/signals.rs` (`busy`, written in 2.1), `crates/notchlingd/src/signal_watch.rs` (publishes `busy` changes as a field of each snapshot's tick, no new topic)
- Test: in `signals_tests.rs`: `a_running_job_with_an_idle_parent_is_busy`, `a_quiet_subagent_is_not_busy`

On the base nothing consumes keep awake in the Rust app (`platform::awake::AwakeGuard` has no caller; the app's `SetAwake` only stores the choice, call C18). This task adds the pure answer and nothing that holds the Mac awake; the keep-awake work that wires `AwakeGuard` reads `busy` when it lands.

- [ ] **Step 1: Write the failing tests** above. **Step 2: Run**; expected FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test -p notchling-engine`. **Step 5: Commit** `git commit -m "signals: busy counts running subagents and jobs"`.

### Task 2.11: Activity log kinds (only if logging P1 has merged)

**Files:**
- Modify: the logging registry (`crates/engine/src/log/kinds.rs` or where logging P1 put it), `crates/notchlingd/src/signal_watch.rs` (log each signal: `session`, `tool`, `via`, `status`, `duration_ms`, `tokens`, no text)
- Test: in the logging registry's tests, new: `turn_finished_and_subagent_finished_are_toggles_on_by_default`

Call C24: kinds `turn.finished` and `subagent.finished` (Toggle, on; Activity and Diagnostics). Logging P5 5.8's `tool.turn` (Codex and Antigravity) is retired in favor of `turn.finished`; its `tool` values use `antigravity`. If logging P1 has not merged, skip this task and note it in the pull request; logging's plan takes the two kinds instead.

- [ ] **Step 1 to 5** as above; commit `git commit -m "log: turn.finished and subagent.finished"`.

### Task 2.12: Matrix, notes, ops doc

**Files:**
- Modify: `docs/provider-matrix.md` (regenerated), `docs/local-api-ops.md` (`sessions.list {tools}`, `signals.recent`, the `signals` topic), `crates/AGENTS.md` (Architecture: Signals bullet; Phone inbox: finish pushes), `crates/notchlingd/AGENTS.md` (`signal_watch.rs`, `signals_ops.rs`, `every_session.rs`, `signals.rs` is still SIGTERM), `crates/engine/AGENTS.md` (`contract/signals.rs`, `transcript/finished.rs`), `apps/flutter/AGENTS.md` (the finish alert), `docs/feature-map.md`

- [ ] **Step 1: Regenerate** the matrix; the diff shows 2 Gaps to Native and 7 cells losing ` · unwired`.
- [ ] **Step 2: Write** the notes. **Step 3: Run** `PYTHONPATH=tools/guard python3 -m guard claudemd`, `python3 tools/strip_comments.py --check`. **Step 4: Commit** `git commit -m "docs: signals"`.

## Checkpoint P2

- `cargo test --workspace --locked`, clippy (Linux and the Mac), fmt, deny; `tools/test-python.sh`; Flutter analyze and test.
- Conformance summary `gaps 24`, `unwired 61`.
- By hand over a fixture root (`cp -R crates/app/fixtures/notch /tmp/nl-root`), daemon and app at `NOTCHLING_DATA_ROOT=/tmp/nl-root`: touch a fixture Codex child to finish it; the notch peeks the parent, the Details Panel row says Completed, `notchling sessions --json` lists every tool.
- The pull request lists the schema change and the new key for the maintainer.
