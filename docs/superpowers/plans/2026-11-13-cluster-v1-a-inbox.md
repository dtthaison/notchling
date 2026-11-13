# Cluster v1, plan A: Notifications Inbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One inbox of actionable notifications, kept by the daemon, shown in Chat (bell + panel), the notch (badge), macOS notifications and the phone, where each item is answered in place (Allow / Deny / Continue / Cancel / Done) and answering anywhere clears it everywhere.

**Architecture:** A pure `engine::inbox` (items, actions, first-answer-wins, expiry, pruning) persisted in `notchling.db`, served by a new `InboxOps` op group in `notchlingd` on a new `inbox` topic. Existing waits (vault approvals and requests, limit notices) become producers: they raise an item and resolve it when their own wait ends; answering an item calls the producer back. The app follows the topic like it follows `vault`; the phone gets additive bridge ops.

**Tech Stack:** Rust (engine, notchlingd, local-api, GPUI app with `crates/kit`), SQLite via `notchling_engine::store`, `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`, sections "Notifications (in-app inbox)" and "Scope: v1 first" (step 0).

## Global Constraints

- No new comments in code, doc comments included (maintainer's rule, branch `agent/no-code-comments`); the why goes in `crates/AGENTS.md` and `docs/feature-map.md`.
- Views use the kit (`KitInk`/`Ink`, `TypeRole`, kit button styles), never raw colors or sizes; every new view gets a `.part("…")` tag.
- Answer ops are app tier (0007); agents can't answer the inbox.
- First answer wins; a later answer gets `{"answered": false, "state": <current>}` and changes nothing.
- `agentconsole.v1` changes are additive only, schema first (`.claude/skills/add-protocol-field`).
- Tests never touch Keychain, real logins, ssh or `gh`.
- Only crates allowed by `deny.toml`; no new dependency is needed.
- User-facing text short and plain; titles in Title Case.

## Review Focus

1. The same item answered on the Mac and the phone at once: one wins, the other UI shows the winner's answer, the producer acts once. (Task 3 test `second_answer_is_refused_and_reports_state`.)
2. Daemon restart with open items whose waiter is gone: they become `Withdrawn`, never stay clickable. (Task 2 test `open_items_from_a_previous_run_load_as_withdrawn`.)
3. A vault approval that times out: its item turns `Expired`, buttons disabled, the agent gets the existing "no" answer. (Task 4 test `approval_timeout_expires_the_item`.)
4. No app following `vault` (even with a phone connected, in plan A): vault approvals keep today's immediate refusal; the inbox must not make an agent wait for nobody. Letting the phone alone approve is a later change. (Task 4 test `no_follower_still_refuses_at_once`.)
5. A flood of notices: at most 200 open items (oldest notice-only items dropped first), done items pruned after 7 days, titles cut at 120 chars and details at 400. (Task 1 tests `caps_open_items_at_200_dropping_oldest_notices`, `prunes_done_after_seven_days`, `cuts_long_text`.)

---

### Task 1: Pure inbox model

**Files:**
- Create: `crates/engine/src/inbox/mod.rs`, `crates/engine/src/inbox/model.rs`, `crates/engine/src/inbox/book.rs`
- Test: `crates/engine/src/inbox/tests_book.rs`
- Modify: `crates/engine/src/lib.rs` (add `pub mod inbox;`)

**Interfaces:**
- Produces:
  - `enum Kind { SecretApproval, SecretRequest, LimitNotice, ActionFailed, Notice }` (serde `snake_case`)
  - `enum ActionId { Allow, AllowOnce, AllowAlways, Deny, Continue, Cancel, Done, Open }` (serde `snake_case`)
  - `struct Action { id: ActionId, label: String, primary: bool }`
  - `enum State { Open, Answered, Expired, Withdrawn }`
  - `struct Source { node: Option<String>, session: Option<String>, job: Option<String> }`
  - `struct Item { id: String, kind: Kind, title: String, detail: String, source: Source, created_ms: i64, expires_ms: Option<i64>, actions: Vec<Action>, state: State, answer: Option<ActionId>, answered_by: Option<String>, seen: bool, open: Option<serde_json::Value> }`
  - `struct Book` with:
    - `fn raise(&mut self, item: Item) -> Change`
    - `fn answer(&mut self, id: &str, action: ActionId, by: &str, now_ms: i64) -> Result<Change, AnswerError>`
    - `fn resolve(&mut self, id: &str, state: State, answer: Option<ActionId>) -> Option<Change>`
    - `fn expire(&mut self, now_ms: i64) -> Vec<Change>`
    - `fn mark_seen(&mut self, ids: &[String]) -> Vec<Change>`
    - `fn prune(&mut self, now_ms: i64)`
    - `fn items(&self) -> Vec<Item>` (newest first)
    - `fn unread(&self) -> usize`
  - `enum Change { Raised(Item), Updated(Item), Removed(String) }`
  - `enum AnswerError { Unknown, NotOpen(State), NotAnAction }`
  - constants `MAX_OPEN = 200`, `DONE_KEEP_MS = 7 * 86_400_000`, `TITLE_MAX = 120`, `DETAIL_MAX = 400`

- [ ] **Step 1: Write the failing tests** in `tests_book.rs`:
  - `answer_sets_state_and_first_wins`: raise an item with actions [Allow, Deny]; `answer(id, Allow, "app", t)` gives `Updated` with `state == Answered`, `answer == Some(Allow)`, `answered_by == Some("app")`; a second `answer(id, Deny, "phone", t)` is `Err(NotOpen(Answered))`.
  - `answer_rejects_action_not_offered`: an item with only [Done], answered with `Allow`, gives `Err(NotAnAction)`.
  - `expire_turns_open_past_deadline_to_expired`: `expires_ms = Some(1000)`; `expire(999)` is empty, `expire(1000)` has one `Updated` with `State::Expired`.
  - `caps_open_items_at_200_dropping_oldest_notices`: raise 200 `Notice` items and then 1 `SecretApproval`; there are 200 open items, the oldest Notice is gone, and the approval is kept.
  - `prunes_done_after_seven_days`: an answered item older than `DONE_KEEP_MS` is gone after `prune`, an open one stays.
  - `cuts_long_text`: a 500-char title becomes 120 chars ending in `…`, and a detail of 1000 chars becomes 400.
  - `unread_counts_open_unseen_only`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine inbox::` and confirm it fails because the module doesn't exist.
- [ ] **Step 3: Implement** the types and `Book` in `model.rs` and `book.rs`. `Book` keeps a `Vec<Item>`. Cutting text happens in `raise`, and the cap is applied in `raise`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine inbox::`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: pure notifications inbox`.

### Task 2: Persist the inbox

**Files:**
- Modify: `crates/engine/src/store.rs` (schema: `CREATE TABLE IF NOT EXISTS inbox (id TEXT PRIMARY KEY, value TEXT NOT NULL, updated_ms INTEGER NOT NULL)`)
- Create: `crates/engine/src/inbox/persist.rs`
- Test: `crates/engine/src/inbox/tests_persist.rs`

**Interfaces:**
- Consumes: `Item`, `Book`, `Change`, `State` from Task 1.
- Produces:
  - `fn load(data_dir: &Path) -> Result<Book, String>`: items that were `Open` load as `Withdrawn`, because their waiter belonged to the last run.
  - `fn save(data_dir: &Path, change: &Change) -> Result<(), String>`: one row per item; `Removed` deletes its row.

- [ ] **Step 1: Write the failing tests:**
  - `round_trips_items`: save Raised(a), Updated(b), Removed(c) into a temp dir, then `load` returns a and b.
  - `open_items_from_a_previous_run_load_as_withdrawn`.
  - `missing_db_loads_empty`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine inbox::tests_persist`. Expected: FAIL.
- [ ] **Step 3: Implement** with `store::open` and one IMMEDIATE transaction per save.
- [ ] **Step 4: Run** the tests again. Expected: PASS.
- [ ] **Step 5: Commit** `engine: inbox rows in notchling.db`.

### Task 3: `InboxOps` in the daemon

**Files:**
- Modify: `crates/local-api/src/wire.rs`:
  - ops `INBOX_LIST = "inbox.list"`, `INBOX_ANSWER = "inbox.answer"`, `INBOX_SEEN = "inbox.seen"`
  - topic `INBOX = "inbox"`
  - events `INBOX_CHANGED = "inbox.changed"`, `INBOX_REMOVED = "inbox.removed"`
- Create: `crates/notchlingd/src/inbox_ops.rs`, `crates/notchlingd/src/inbox_ops_tests.rs`
- Modify: `crates/notchlingd/src/daemon_wiring.rs` (build `InboxOps` before `vault`, `.with_group(inbox.clone())`; a 1 s tick calls `expire`), `crates/notchlingd/src/lib.rs`

**Interfaces:**
- Consumes: Tasks 1 and 2; `OpGroup`, `Room::publish`, `Caller`, `Tier`.
- Produces:
  - `struct InboxOps`, with `fn new(data_dir: PathBuf, room: Arc<Room>) -> Arc<InboxOps>`
  - `trait Responder: Send + Sync { fn answered(&self, item: &Item, action: ActionId) }`
  - `fn raise(&self, item: Item, responder: Option<Arc<dyn Responder>>) -> String` returns the item's id
  - `fn resolve(&self, id: &str, state: State, answer: Option<ActionId>)`
  - `fn tick(&self, now_ms: i64)`
  - ops:
    - `inbox.list {}` → `{items: [Item], unread: n}`, app tier
    - `inbox.answer {id, action}` → `{answered: bool, state: State, answer: Option<ActionId>}`, app tier
    - `inbox.seen {ids: [..]}` → `{unread: n}`, app tier
  - topic `inbox`, app tier; every `Change` publishes `inbox.changed {item}` or `inbox.removed {id}`.

- [ ] **Step 1: Write the failing tests** in `inbox_ops_tests.rs`, using the existing fake `Room` / test callers from `vault_ops_*_tests.rs`:
  - `list_requires_app_tier`: an agent-tier caller gets `forbidden`.
  - `answer_calls_responder_once`.
  - `second_answer_is_refused_and_reports_state`: two answers give `answered: true` then `answered: false` with `state: "answered"` and the first action; the responder is called once.
  - `raise_publishes_changed`.
  - `tick_expires_and_publishes`.
- [ ] **Step 2: Run** `cargo test -p notchlingd inbox_ops`. Expected: FAIL.
- [ ] **Step 3: Implement.** A `Mutex<Book>`. Persist each `Change` (Task 2) before publishing it. Call the responder outside the lock.
- [ ] **Step 4: Run** the tests again. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: inbox op group and topic`.

### Task 4: Producers: vault approvals, vault requests, limit notices

**Files:**
- Modify: `crates/notchlingd/src/vault_asks.rs`, `crates/notchlingd/src/vault_ops.rs` (hold an `Arc<InboxOps>`), `crates/notchlingd/src/limits_ops.rs` (around line 262), `crates/notchlingd/src/daemon_wiring.rs`
- Test: `crates/notchlingd/src/vault_ops_inbox_tests.rs`, plus a test in `limits_ops_tests.rs`

**Interfaces:**
- Consumes: `InboxOps::raise/resolve`, `Responder` (Task 3); the existing `approvals.answer(id, Decision)` and request cancel/save paths.
- Produces: the item shapes below. The app's Task 5 opens `open` payloads by `kind`.
  - **SecretApproval**
    - title `Allow <names> for <command>` (names joined with ", ")
    - detail `<folder>`
    - actions `[AllowOnce "Allow Once" primary, AllowAlways "Always Allow", Deny "Deny"]`
    - `expires_ms = now + approval_wait`
    - `open = {"vault_approval": id}`
  - **SecretRequest**
    - title `Add secret <name>`
    - detail = the agent's reason or note
    - actions `[Open "Open Form" primary, Cancel "Cancel"]`
    - `open = {"vault_request": id}`
  - **LimitNotice**
    - title = the notice text, cut to 120
    - actions `[Done "Done"]`
  - Answer mapping: AllowOnce → `Decision::Once`, AllowAlways → `Decision::Always`, Deny → `Decision::Deny`, Cancel → the request's cancel, Done → nothing.
  - When a producer's own wait ends (answered in the floating window, timed out, cancelled), it calls `resolve` with `Answered`, `Expired` or `Withdrawn`.

- [ ] **Step 1: Write the failing tests:**
  - `approval_raises_item_and_inbox_answer_decides`: start an approval wait on a thread; `inbox.answer(AllowOnce)` makes the wait return `Decision::Once`.
  - `window_answer_resolves_item`: answering with `vault.approval.answer` turns the item `Answered` with `AllowOnce`.
  - `approval_timeout_expires_the_item`.
  - `no_follower_still_refuses_at_once`: with nobody following `vault`, the approval refuses at once and raises no item.
  - `request_cancel_from_inbox_cancels_request`.
  - `limit_notice_raises_done_item`.
- [ ] **Step 2: Run** `cargo test -p notchlingd vault_ops_inbox limits_ops`. Expected: FAIL.
- [ ] **Step 3: Implement.** Raise the item next to the existing `publish` of `VAULT_APPROVAL_PENDING` and `VAULT_REQUEST_PENDING`, and resolve it next to the matching `*_RESOLVED`. The responder maps actions as above.
- [ ] **Step 4: Run** the tests, then `cargo test -p notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: vault and limits raise inbox items`.

### Task 5: Chat bell and inbox panel

**Files:**
- Create: `crates/app/src/inbox/mod.rs`, `crates/app/src/inbox/feed.rs` (follows topic `inbox` on the daemon socket, like `switches/feed.rs`), `crates/app/src/inbox/model.rs` (pure view model), `crates/app/src/inbox/view.rs`, `crates/app/src/inbox/renders.rs`
- Test: `crates/app/src/inbox/model_tests.rs`
- Modify: `crates/app/src/chat/view/sidebar.rs` (a `footer_button("inbox", Icon::Bell, "Inbox", "Notifications that need you", …)` before Vault, with an unread badge), `crates/app/src/lib.rs`, `crates/kit/src/icon.rs` (add `Icon::Bell` if missing)

**Interfaces:**
- Consumes: wire constants (Task 3); item shapes (Task 4); the existing `crate::vault::approval` window opener and `open_vault_sheet`.
- Produces:
  - `struct InboxModel` with `fn apply(&mut self, event: &Value)`, `fn sections(&self) -> (Vec<Row>, Vec<Row>)` (Needs You, Done), `fn unread(&self) -> usize`
  - `struct Row { id, title, detail, ago: String, actions: Vec<(ActionId, String, bool)>, enabled: bool, open: Option<Value> }`
  - parts `"Inbox Button"`, `"Inbox Panel"`, `"Inbox Row"`

- [ ] **Step 1: Write the failing tests** in `model_tests.rs`:
  - `changed_event_upserts_and_orders_newest_first`
  - `answered_rows_move_to_done_with_buttons_disabled`
  - `expired_row_says_expired`, with the detail suffix `· Expired`
  - `unread_ignores_done`
- [ ] **Step 2: Run** `cargo test -p notchling-app inbox::`. Expected: FAIL.
- [ ] **Step 3: Implement** the model, then the view:
  - The panel is a popover from the bell, 360 wide, with a "Needs You" list and then a collapsed "Done" list.
  - Each row shows its actions as kit buttons; the primary one uses the primary style.
  - Clicking the row body opens `open`: `vault_approval` → the existing approval window; `vault_request` → the existing request form; otherwise the session or nothing.
  - Opening the panel sends `inbox.seen` for the visible ids.
- [ ] **Step 4: Run** the tests, then render with `notchling-app --render inbox-panel --out /tmp/inbox.png` (add the view to `renders.rs` with fixture items) and look at it.
- [ ] **Step 5: Commit** `app: inbox bell and panel in Chat`.

### Task 6: Notch badge and macOS notifications

**Files:**
- Modify: the notch's row/header view under `crates/app/src/notch/` (badge with the unread count; clicking it opens Chat with the inbox panel), `crates/app/src/inbox/feed.rs`
- Create: `crates/app/src/inbox/system.rs`
- Test: `crates/app/src/inbox/system_tests.rs`

**Interfaces:**
- Consumes: `InboxModel` (Task 5); `notchling_platform::notify::{Notifier, Notification, RecordingNotifier, system_notifier}`.
- Produces: `fn notify_new(model_before: &InboxModel, model_after: &InboxModel, notifier: &dyn Notifier)`. It sends one system notification per newly raised item that has a non-`Done` action (title = item title, body = detail); clicking it opens Chat's inbox panel. Notice-only items don't notify.

- [ ] **Step 1: Write the failing tests** with `RecordingNotifier`:
  - `actionable_item_notifies_once`, which also checks that a re-applied event doesn't notify again
  - `notice_only_item_does_not_notify`
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement**, and add the notch badge, tagged `.part("Inbox Badge")`.
- [ ] **Step 4: Run** the tests, and render the notch with the `render-notch-view` skill.
- [ ] **Step 5: Commit** `app: inbox badge in the notch and system notifications`.

### Task 7: Phone inbox (bridge, additive)

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- `protocol/agentconsole.v1.schema.json`
- new fixtures under `protocol/fixtures/`
- regenerate with `tools/protocol/gen.py`
- `crates/engine/src/bridge/router_ops.rs` (ops `inbox.list`, `inbox.answer`; push `inbox` events to subscribed phones)
- `apps/flutter/lib/features/inbox/` (a tab with the list and the action buttons)
- Test: `crates/engine/src/bridge/router_tests.rs`, `apps/flutter/test/inbox_test.dart`

**Interfaces:**
- Consumes: `InboxOps` through the bridge backend (`crates/notchlingd/src/bridge_backend.rs`).
- Produces:
  - wire `inbox.list {} → {items, unread}` and `inbox.answer {id, action} → {answered, state, answer}`
  - push `{type: "inbox", item}`
  - Older phones ignore all of it.

- [ ] **Step 1: Write the failing fixtures and router tests** `phone_lists_inbox`, `phone_answer_first_wins` (the Mac answered first, so the phone gets `answered: false`), and `old_phone_without_inbox_is_unaffected`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine bridge::` and the protocol fixture tests. Expected: FAIL.
- [ ] **Step 3: Implement** the schema, regenerate, add the router ops and the backend calls, then the Flutter tab (list, buttons, badge on the tab).
- [ ] **Step 4: Run** `cargo test -p notchling-engine bridge::`, `flutter test test/inbox_test.dart` and `dart analyze`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: inbox over agentconsole.v1 (additive)`.

### Task 8: Docs, checks, install

**Files:**
- Modify: `crates/AGENTS.md` (an inbox entry: where items come from, first answer wins, Withdrawn on restart), `docs/feature-map.md`, `CHANGELOG.md` (`[Unreleased]` → Added), `docs/decisions/` only if the reviewer asks

- [ ] **Step 1: Run** `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: clean.
- [ ] **Step 2: Run** `cargo test --workspace --locked`. Expected: PASS, except the known-flaky tests listed in earlier reports.
- [ ] **Step 3: Run** `PYTHONPATH=tools/guard python3 -m guard lint` and `featuremap`. Expected: 0 new.
- [ ] **Step 4: Install** with `tools/bundle-next.sh --install`, relaunch the app, then trigger a vault Ask First use with `notchling secret <an ask-first entry> -- true`. Expected: an inbox item, a bell badge and a system notification; Allow Once in the panel lets the command run.
- [ ] **Step 5: Commit** `docs: inbox`.
