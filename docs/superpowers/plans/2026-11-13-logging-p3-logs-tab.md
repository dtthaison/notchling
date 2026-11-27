# Logging, Phase 3: Settings > Logs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A fourth Settings tab, Logs, built from the registry: what each kind records and why, which are always on, retention per sink and per kind, the Audit's status with Check Now and Start New Chain, Clear Activity and Delete Old Audit Months. Every change is recorded; agents can't make one; settings edited on disk bring the defaults back.

**Architecture:** `notchling_engine::logs::settings` is the pure model of the choices, kept by the daemon as the `docs` row `daemon/log-settings` in `notchling.db`, with a hash recorded in each `log.setting_changed` and in the checkpoint. `notchlingd` serves `log.settings.get`, `log.settings.set` and `log.clear`, feeds the bus's `enabled` test and the sinks' retention from the model, and checks the row at start. The app's `logs/` area is a pure model plus a GPUI view embedded in the Settings sheet like Activity.

**Tech Stack:** Rust; `notchling_engine::store` (SQLite `docs` rows); GPUI 0.2.2 and `crates/kit` (menus, rows, tokens).

**Spec:** `docs/superpowers/specs/2026-11-13-logging-design.md`, sections 9, 10, 12 (Start New Chain), 16 (`log.settings.get`, `log.settings.set`, `log.clear`), 17 (Phase 3).

**Branch:** `agent/logging-p3-logs-tab`, from `agent/logging-p2-audit-activity` once Phase 2 is merged (or the integration branch that holds it).

**Calls:** C-numbers refer to "Calls these plans make" in `2026-11-13-logging-p1-registry.md`.

**Shared files with Phases 4 and 5** (they may run at the same time): `crates/notchlingd/src/op_kinds.rs` (each phase adds its own ops' homes: append only), `crates/local-api/src/wire.rs` (`ops` constants: append only), `crates/notchlingd/src/daemon_wiring.rs` (one `with_*` line each). Rebase on whichever lands first; no other file is shared.

## Global Constraints

- Every rule of the Phase 1 and Phase 2 plans' Global Constraints.
- `log.settings.get` is `APP_QUIET`; `log.settings.set` and `log.clear` are `APP`. An agent-tier call is refused by the tier check and recorded as `api.refused`.
- An `Always` kind can't be turned off by any argument; its retention follows the Audit's.
- Choices: Audit keep 90 days, 1 year (default), 2 years; Activity per kind 7, 30 (default), 90, 365 days (`account.switched` default 90, `usage.daily` 365); Diagnostics 1, 3, 7 (default), 14 days; Detail Normal (default) or Detailed for 24 hours.
- The settings are not keys in `crates/engine/src/settings/keys.rs`; decision 0020's `settings.set` can't name them.
- At start, a `daemon/log-settings` row whose hash doesn't match the last recorded `settings_hash` is ignored: defaults come back, `log.settings_restored{kinds_changed}` is recorded and an Inbox item says so.
- Views use the kit's tokens, `menu_panel`/`MenuItem`, rows and buttons; every new view has a `.part("…")` tag; no `unwrap`/`expect` outside tests in `app`.
- User-facing text short and plain, titles in Title Case; the tab's top line is exactly "Logs stay on this Mac. They never hold your messages, prompts, files or secrets."

## Review Focus

1. An agent edits `notchling.db` to turn every Toggle kind off: at the next daemon start the row's hash doesn't match, defaults come back, `log.settings_restored` is sealed and the Inbox says so. (Task 3.3 test `edited_row_brings_defaults_back`.)
2. `log.settings.set {target: "vault.secret_used", on: false}` or `keep_days: 7` for an `Always` kind, from the app: refused with `bad_request`, nothing changes, nothing is recorded as changed. (Task 3.2 test `no_argument_turns_off_an_always_kind`.)
3. Two Settings windows changing the same kind at once: the daemon applies them in order and both windows show the last value after the `log` topic's `log.settings` event. (Task 3.2 test `concurrent_sets_end_in_last_value_and_publish`.)
4. The daemon down while the Logs tab is open: the tab shows the registry's rows with "Notchling's background service isn't running" in place of the controls, never an empty page. (Task 3.4 test `no_daemon_shows_rows_and_a_notice`.)
5. Start New Chain pressed while sealing is fine: the button isn't shown; pressed while unsealed, it asks for confirmation with the spec's words first. (Task 3.4 test `start_new_chain_only_when_unsealed_and_confirms`.)

## Order and lanes

```
Daemon lane: 3.1 settings model ──► 3.2 ops ──► 3.3 start check and retention feed
App lane:    3.4 Logs model ──► 3.5 Logs view and tab ──► 3.6 Inbox opens Logs; live status; docs
```

The two lanes run in parallel: the app lane builds against the reply shapes in Task 3.2's Interfaces with canned JSON, and 3.6's last step runs against the real daemon after 3.2 is in.

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited (why) |
| --- | --- | --- | --- |
| 3.1 | Maintainer (engine outside `transcript/`, `search/`) | none (`store.rs` already writes `docs` rows) | none |
| 3.2, 3.3 | Maintainer (`notchlingd`, `local-api/wire.rs`) | none new | none |
| 3.4, 3.5 | auto (`crates/app/**`) | none | `crates/app/src/sheets/agents_tests.rs:46,50` asserts `Page::ALL` is three pages; it becomes four (`WEAK-TEST-EDIT` by rule: a test edited, in `src/`, so the path check won't fire; the pull request names it) |
| 3.6 | auto for `crates/app/**` and `docs/local-api-ops.md`; maintainer for `crates/app/AGENTS.md`, `crates/AGENTS.md`, `docs/feature-map.md`, `docs/policies/privacy.md` | none | none |

## Maintainer only (Phase 3)

- Approve the `agents_tests.rs` edit, the daemon ops and the notes.
- On a Mac with a display, look at `--render settings-logs` and `settings-logs-unsealed` in light and dark, and click through Start New Chain on a dev profile (`tools/dev-profile.sh app`, never the real data folder).

---

### Task 3.1: The settings model

**Files:**
- Create: `crates/engine/src/logs/settings.rs`
- Test: `crates/engine/src/logs/settings_tests.rs`

**Interfaces:**
- Consumes: `notchling_events::registry::all`, `KindSpec`, `Lock`.
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)] pub struct LogSettings { pub kinds: BTreeMap<String, KindSetting>, pub audit_keep: AuditKeep, pub diag_keep_days: u16, pub detail: Detail, pub detail_until: Option<i64> }`; `pub struct KindSetting { pub on: bool, pub keep_days: u16, pub off_since: Option<i64> }`; `enum AuditKeep { Days90, Year1, Years2 }` with `days()`; `enum Detail { Normal, Detailed }`.
  - `LogSettings::defaults() -> LogSettings` (every kind at its registry default and `keep_days`; `audit_keep: Year1`; `diag_keep_days: 7`; `Normal`).
  - `pub enum Target { Kind(&'static KindSpec), Sink(SinkName), Detail }`; `pub fn parse_target(&str) -> Option<Target>` (a kind's name, `audit`, `activity`, `diagnostics`, or `detail`).
  - `LogSettings::apply(&mut self, target: Target, on: Option<bool>, keep_days: Option<u16>, detail: Option<Detail>, now_ms: i64) -> Result<Vec<Changed>, SetError>`; `SetError { Locked, BadValue, UnknownTarget }`; `Changed { target, on, keep_days, detail }` (one per real change; setting the current value changes nothing). Turning a `Toggle` kind off sets `off_since`; on clears it. A `Detailed` choice sets `detail_until = now + 24 h`.
  - `LogSettings::enabled(&self, spec: &KindSpec, now_ms: i64) -> bool` (an `Always` kind is always enabled whatever the row says); `keep_days_for(&self, spec) -> u16` (an `Always` kind: the Audit's days); `detail_at(&self, now_ms) -> Detail` (back to Normal after `detail_until`).
  - `LogSettings::hash(&self, salt: &Salt) -> Hex` (first 16 hex of SHA-256 over the salt and the row's JSON with sorted keys).
  - `pub const DOC_ID: &str = "daemon/log-settings"`; `load(store) -> Option<LogSettings>`, `save(store, &LogSettings)` through `store::put_doc`.
- [ ] **Step 1: Write the failing tests:** `defaults_follow_registry` (`phone.viewed` off; every `Always` kind on), `always_kind_refuses_off_and_keep`, `toggle_off_sets_off_since_and_on_clears_it`, `same_value_is_no_change`, `keep_days_only_from_the_choices` (`keep_days: 5` is `BadValue`), `detail_reverts_after_24h`, `hash_changes_with_any_field_and_salt`, `round_trip_through_store` (a temp `notchling.db` through `store::open`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine logs::settings`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and clippy. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/engine/src/logs/settings.rs crates/engine/src/logs/settings_tests.rs crates/engine/src/logs/mod.rs` then `git commit -m "engine: log settings, a pure model with a hash"`.

### Task 3.2: `log.settings.get`, `log.settings.set` and `log.clear`

**Files:**
- Create: `crates/notchlingd/src/log_settings_ops.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::LOG_SETTINGS_GET`, `LOG_SETTINGS_SET`, `LOG_CLEAR`; `topics::LOG_SETTINGS = "log.settings"` on topic `log`), `crates/notchlingd/src/op_kinds.rs` (homes: get Quiet, set `log.setting_changed`, clear `log.pruned`), `crates/notchlingd/src/log_bus.rs` (`enabled` reads a shared `Arc<RwLock<LogSettings>>`), `crates/notchlingd/src/daemon_wiring.rs`
- Test: `crates/notchlingd/src/log_settings_ops_tests.rs`

**Interfaces:**
- Consumes: Task 3.1's model, `EventsDb::clear`, `LogBus`, `AuditSink` checkpoint.
- Produces:
  - `log.settings.get` → `{kinds: [{kind, group, title, records, why, on, keep_days, locked, reason?, off_since?}], sinks: {audit: {keep: "year1"}, activity: {keep_days: 30}, diagnostics: {keep_days: 7}}, detail: "normal", detail_until?, settings_hash}`.
  - `log.settings.set {target, on?, keep_days?, detail?}` → the same reply as get. Each `Changed` pushes `log.setting_changed{target, sink, on, keep_days, detail, settings_hash}` (an `Always` kind, so sealed), saves the row, swaps the shared settings and publishes `log.settings` with the new reply. Writes are serialized by one `Mutex` held across the model change only; the save and the push happen after it.
  - `log.clear {sink: "activity", groups?: [..], include_locked?: bool}` → `{removed}`; pushes `log.pruned{sink: activity, entries, reason: user}`. `sink: "diagnostics"` answers `unimplemented` until Phase 4 wires it (Phase 6's Task 6.1 connects it).
  - The checkpoint's `settings_hash` is updated through the Audit sink's next checkpoint.
- [ ] **Step 1: Write the failing tests** (an `Ops` rig with `MemorySink`, a temp store and `EventsDb`): `get_lists_every_kind_with_lock_and_reason`, `set_turns_toggle_off_and_records`, `no_argument_turns_off_an_always_kind` (`on: false` and `keep_days: 7` on `vault.secret_used` are `bad_request`; nothing pushed, row unchanged), `agent_tier_is_refused` (through the server rig: `forbidden`, one `api.refused`), `concurrent_sets_end_in_last_value_and_publish`, `disabled_kind_is_not_written` (after turning `session.reply` off, a pushed `session.reply` reaches no sink), `clear_activity_removes_group_rows_and_records`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_settings_ops`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd` (lib; `op_kinds_tests` must pass with the three new ops) and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd: log.settings.get, set and log.clear"`.

### Task 3.3: The check at start, and settings feeding retention

**Files:**
- Modify: `crates/notchlingd/src/log_start.rs`, `crates/notchlingd/src/log_audit.rs` (retention reads `audit_keep`), `crates/notchlingd/src/log_activity.rs` (`expires` from `keep_days_for`), `crates/notchlingd/src/log_checks.rs` (the Inbox item for a restore)
- Test: `crates/notchlingd/src/log_settings_start_tests.rs`; create `crates/engine/src/settings/keys_no_log_tests.rs`

**Interfaces:**
- Produces:
  - At start: load the row; the expected hash is the last `log.setting_changed`'s `settings_hash` in the Audit's newest months, else the checkpoint's `settings_hash`, else none (a first run: defaults, saved, no event). A row with no expected hash, or a mismatch, is ignored: defaults are saved, `log.settings_restored{kinds_changed}` is pushed (`kinds_changed` counts kinds whose `on` differs from the row), and an Inbox `Notice` `log-warning:<n>` says "Logging Settings Were Reset", opening Settings > Logs.
  - The Audit's retention uses `audit_keep`; Activity's `expires` uses `keep_days_for`; a keep change applies to new rows and to the next hourly prune (rows already written keep their `expires` unless shorter keep: the prune also deletes rows older than the new keep).
  - `keys_no_log_tests.rs`: `log_settings_are_not_settings_keys` (no key in `settings::keys` starts with `log` or names a kind; if one ever does, its class must be Never).
- [ ] **Step 1: Write the failing tests:** `matching_row_is_kept`, `edited_row_brings_defaults_back` (write a row with every Toggle off and a wrong hash into the temp store: after `log_start::start`, the shared settings are defaults, one `log.settings_restored` with `kinds_changed` equal to the number of Toggle kinds that default on, and one Inbox item), `first_run_saves_defaults_silently`, `shorter_activity_keep_prunes_older_rows`, `audit_keep_90_days_prunes_older_months`, plus the engine test.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd log_settings_start` and `cargo test -p notchling-engine keys_no_log`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`, `cargo test -p notchling-engine` and clippy. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "notchlingd: settings on disk don't count; retention follows the Logs settings"`.

### Task 3.4: The Logs model

**Files:**
- Create: `crates/app/src/logs/mod.rs`, `crates/app/src/logs/model.rs`, `crates/app/src/logs/wire.rs`
- Modify: `crates/app/src/lib.rs` (or the area list in `main.rs`, as `activity` is declared)
- Test: `crates/app/src/logs/model_tests.rs`

**Interfaces:**
- Consumes: `notchling_events::registry` (the app depends on `notchling-events`: add it to `crates/app/Cargo.toml`, a maintainer-zone line), Task 3.2's reply, Phase 2's `log.check` data.
- Produces:
  - `pub struct LogsModel { pub header: Header, pub keep: Keep, pub groups: Vec<GroupRows>, pub service: Service }`; `pub enum Header { Intact { checked_ms, entries_since: Option<(u64, i64)> }, Warning { result: String, from_n: u64, to_n: u64 }, Unsealed { why: String }, Unknown }`; `pub struct Keep { pub audit: AuditKeep, pub activity_days: u16, pub diag_days: u16, pub detail: Detail, pub detail_until: Option<i64> }`; `pub struct GroupRows { pub title: &'static str, pub always: bool, pub rows: Vec<KindRow> }`; `pub struct KindRow { pub kind: &'static str, pub title: &'static str, pub records: &'static str, pub why: &'static str, pub on: bool, pub locked: Option<&'static str>, pub keep_days: u16, pub off_since: Option<i64> }`; `pub enum Service { Up, Down }`.
  - `LogsModel::build(reply: Option<&Value>, check: Option<&Value>, now_ms: i64) -> LogsModel`: rows come from the registry when there is no reply (daemon down, `Service::Down`, every row read-only); `perf.*` rows are grouped into Performance with the line "Counts and times only, kept 7 days.".
  - `pub fn header_text(&Header, now_ms) -> (String, Tone)` with the spec's words: "Audit log intact. Checked 2 minutes ago. 12,480 entries since Nov 1."; broken, gap, tail_missing, file_missing, unanchored: "The audit log was changed outside Notchling. Entries 4,790 to 4,812 can't be trusted."; unsealed: "The audit log isn't sealed. Notchling can't read its key in your Keychain (it is missing). New entries are written but can't be checked." with "(it is missing)", "(macOS refused)", "(waiting for you to answer macOS's prompt)".
  - `pub fn off_since_text(ms) -> String` ("Off since Nov 13, 14:02"); `pub fn can_start_new_chain(&Header) -> bool` (only `Unsealed` and `Warning`); `pub const START_NEW_CHAIN_CONFIRM: &str = "Start a new audit log? The old entries stay, but they can't be checked against the new key. This is recorded."`.
  - `wire.rs`: `get() -> (op, args)`, `set(target, on, keep_days, detail)`, `verify(full)`, `chain_start()` (`{confirm: true}`), `clear(groups, include_locked)`, `audit_delete(through_month)`; calls go through `actions.rs` off the UI thread.
- [ ] **Step 1: Write the failing tests:** `rows_follow_registry_groups`, `locked_rows_carry_reason`, `no_daemon_shows_rows_and_a_notice`, `header_texts_match_the_spec`, `start_new_chain_only_when_unsealed_and_confirms`, `off_since_text_formats`, `performance_group_line`, `delete_old_months_choices_exclude_last_30_days` (`pub fn deletable_months(months: &[u32], now_ms) -> Vec<u32>`).
- [ ] **Step 2: Run** `cargo test -p notchling-app logs::`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `cargo clippy -p notchling-app --all-targets -- -D warnings` on the Mac. Expected: PASS.
- [ ] **Step 5: Commit** `git add crates/app/src/logs crates/app/src/lib.rs crates/app/Cargo.toml Cargo.lock` then `git commit -m "app: the Logs tab's model"`.

### Task 3.5: The Logs view and tab

**Files:**
- Create: `crates/app/src/logs/view.rs`, `crates/app/src/logs/renders.rs`
- Modify: `crates/app/src/sheets/agents.rs` (`Page::Logs`, `Page::ALL` of four), `crates/app/src/sheets/settings_view.rs` (one branch: the Logs page body is `LogsView`, like Activity), `crates/app/src/sheets/agents_tests.rs` (protected edit), `crates/app/src/mac/views.rs` (render arms)
- Test: `crates/app/src/logs/model_tests.rs` (view-free checks), the render names

**Interfaces:**
- Consumes: Task 3.4's model and wire.
- Produces: `LogsView` (a GPUI entity owning `LogsModel`, loading on open and on the `log` topic's `log.settings` and `log.check` events through the app's existing topic feed pattern, `switches/feed.rs`), tagged `.part("Logs")`, `.part("Logs Header")`, `.part("Logs Keep")`, `.part("Logs Kinds")`, `.part("Logs Actions")`. Keep choices are kit menus; a locked row is greyed and shows its reason on hover and click; buttons: Check Now (`log.verify`), Start New Chain… (confirm sheet), Clear Activity… (a menu of groups with a "Include locked kinds" check), Delete Old Audit Months… (a menu of `deletable_months`, with the line "Months older than 30 days. The deletion is recorded for good."), Show Logs in Finder (`cx.reveal_path(<data>/audit)`; Phase 6 points it at the Logs folder). Render specs `settings-logs` and `settings-logs-unsealed` (`NOTCHLING_RENDER_LOGS_HEADER=unsealed` forces the second).
- [ ] **Step 1: Edit the failing test** `agents_tests.rs`: `Page::ALL == [General, Agents, Activity, Logs]`, `Page::Logs.title() == "Logs"`.
- [ ] **Step 2: Run** `cargo test -p notchling-app sheets::`; expected FAIL.
- [ ] **Step 3: Implement** the view, the page and the renders.
- [ ] **Step 4: Run** `cargo test -p notchling-app`, `cargo build --release -p notchling-app`, then `target/release/notchling-app --render settings-logs --out /tmp/logs.png` and `NOTCHLING_RENDER_APPEARANCE=dark target/release/notchling-app --render settings-logs-unsealed --out /tmp/logs-red.png`; look at both. Expected: PASS; four tabs; the red header in the second.
- [ ] **Step 5: Commit** by path, `git commit -m "app: Settings > Logs"`.

### Task 3.6: The Inbox opens Logs; notes and docs

**Files:**
- Modify: `crates/app/src/inbox/` (the item's `open` value `{"settings": "logs"}` opens Settings on the Logs page: find where `open` is handled with `grep -rn '"open"' crates/app/src/inbox`), `docs/local-api-ops.md`, `crates/app/AGENTS.md`, `crates/AGENTS.md` (Architecture: the Logs tab in one paragraph), `docs/feature-map.md`, `docs/policies/privacy.md` (the `daemon/log-settings` row, "agents can't change it")
- Test: `crates/app/src/inbox/` model tests (new test only)
- [ ] **Step 1: Write the failing test** `open_settings_logs_target` (the parsed open target of `{"settings": "logs"}` is `Open::Settings(Page::Logs)`).
- [ ] **Step 2: Run** `cargo test -p notchling-app inbox::`; expected FAIL.
- [ ] **Step 3: Implement**, then write the docs.
- [ ] **Step 4: Run** the checkpoint below; with a dev profile, run the daemon over a fixture copy and the app with `--data-root`, open Settings > Logs, turn a kind off and on, and see two `log.setting_changed` rows in Activity. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "app: an Audit warning opens Settings > Logs; notes"`.

## Checkpoint P3

- `cargo test --workspace --locked`, clippy on the Mac, `cargo fmt --all --check`, `cargo deny check`, `tools/test-python.sh`, `python3 -m guard lint` (0 new), `rules`, `claudemd`.
- `cargo test -p notchling-app -- --ignored` (render tests) on a Mac with a display; `bench_chat` needn't change (the tab is in the Settings sheet).
