# Logging, Phase 7: Later Parts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four parts the spec leaves for later, each shippable alone: a paired phone keeps the Audit's checkpoints as a second anchor; the phone shows Activity; Search finds Activity rows; rows from other Macs are told apart by their `host`.

**Architecture:** The phone parts are additive `agentconsole.v1` fields and ops (schema first, generated types, `GUARD-PROTOCOL-GEN`), served by the daemon's bridge from the same `events.db` and checkpoint the Mac uses. Search indexes Activity rows' titles and names (no free text exists to index) in `daemon/search.db` as a corpus of its own. Host ids are already on every event; this phase filters and shows them.

**Tech Stack:** Rust (engine `bridge/`, `search/`; notchlingd), JSON Schema and `tools/protocol/gen.py`, Flutter (`apps/flutter/lib/features/`).

**Spec:** `docs/superpowers/specs/2026-11-13-logging-design.md`, sections 5 ("Later features"), 12 (the phone as a second anchor), 17 (Phase 7), 19 (decision 3).

**Branches:** one per part, each from `agent/logging-p6-bundle-cli` (or the integration branch holding Phase 6): `agent/logging-p7-phone-anchor` (Task 7.1), `agent/logging-p7-phone-activity` (7.2), `agent/logging-p7-search` (7.3), `agent/logging-p7-hosts` (7.4). They touch disjoint files except `protocol/agentconsole.v1.schema.json` and the generated files (7.1 and 7.2): land 7.1 first, then regenerate on 7.2's rebase.

**Calls:** C-numbers refer to "Calls these plans make" in `2026-11-13-logging-p1-registry.md`.

## Global Constraints

- Every rule of the earlier phases' Global Constraints.
- `agentconsole.v1` changes are additive only, schema first; `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart` are written by `python3 tools/protocol/gen.py`, never by hand (`.claude/skills/add-protocol-field`).
- The phone stores checkpoints only (`n`, `seal`, `month`, `at`, `host`), never Audit lines or the key.
- A phone sees Activity with names resolved on the Mac; it reads, it never changes log settings (log settings ops are not bridge ops).
- Search indexes no field the registry doesn't declare; Activity has no text to index beyond titles, kind names and `Ref` names.
- Flutter checks: `cd apps/flutter && flutter analyze && flutter test`.

## Review Focus

1. A phone that was away for a month, then connects to a Mac whose Audit was cut back past the phone's last checkpoint: the Mac records `audit.checked{tail_missing}` with the phone as the anchor, raises the Inbox item, and the phone shows the warning on that Mac's page. (Task 7.1 test `phone_checkpoint_beyond_tail_is_tail_missing`.)
2. Two phones paired, one with an older checkpoint: the older one is still accepted when its `n` and `seal` match the Audit at that `n`. (Task 7.1 test `older_phone_checkpoint_that_matches_is_intact`.)
3. A phone checkpoint from a different Mac (a phone paired with two Macs): ignored by `host`. (Task 7.1 test `checkpoint_from_another_host_is_ignored`.)
4. A phone on an old app version that doesn't know the fields: the Mac sends them, the phone ignores them, nothing breaks. (Task 7.1 conformance fixture without the field.)
5. An Activity search while `events.db` is being pruned: results never point at a row that no longer exists; a stale hit opens Activity at the nearest row. (Task 7.3 test `pruned_rows_leave_the_index`.)

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited (why) |
| --- | --- | --- | --- |
| 7.1 | Maintainer (`protocol/`, `crates/protocol/`, `apps/flutter/lib/core/protocol/`, engine `bridge/`, `notchlingd`); auto for `apps/flutter/lib/features/macs/**`; new Flutter tests are `new_files_only` | `GUARD-PROTOCOL-GEN` checked; none of `CAP-*` | `apps/flutter/test/protocol_conformance_test.dart` only if it lists fixtures by name (`WEAK-TEST-EDIT`); prefer a new test file |
| 7.2 | As 7.1, plus a new `apps/flutter/lib/features/activity/` (auto) | as 7.1 | none |
| 7.3 | auto (`crates/engine/src/search/**`, `crates/app/src/search/**`); maintainer for `notchlingd` wiring | none | `crates/engine/src/search/tests.rs` only gains tests |
| 7.4 | Maintainer (`notchlingd`, engine `activity/`); auto for `crates/app/src/activity/**` | none | none |

## Maintainer only (Phase 7)

- Approve the protocol changes and regenerated files; build and install the phone app on a real phone (Flutter builds and TestFlight are the maintainer's).
- Pair a phone with a dev profile's Mac, cut that Mac's Audit file back by hand, and check the warning appears on the Mac and on the phone.

---

### Task 7.1: The phone as a second anchor

**Files:**
- Modify: `protocol/agentconsole.v1.schema.json` (a definition `AuditCheckpoint { n, seal, month, at, host }`; `Subscription` gains optional `audit: boolean`; a push event `audit.checkpoint` with an `AuditCheckpoint`; the `hello` request gains optional `lastCheckpoint: AuditCheckpoint`), `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart` (generated), `crates/engine/src/bridge/hub.rs` (push the checkpoint to subscribed connections when the daemon writes one), `crates/engine/src/bridge/backend.rs` (`BridgeBackend::audit_checkpoint() -> Option<AuditCheckpoint>` and `check_phone_checkpoint(cp) -> PhoneCheck`, default `None` and `Unknown`), `crates/notchlingd/src/bridge_backend.rs` (implements both from the Audit sink's checkpoint and the month files), `crates/notchlingd/src/log_checks.rs` (a phone result raises the same Inbox item), `crates/events/src/kinds/security.rs` (`audit.checked` gains optional `anchor: Option<Anchor>` with `keychain`, `phone`: an added optional field, `kv` unchanged, golden updated), `apps/flutter/lib/features/macs/` (store the newest 50 checkpoints per Mac `host` in the app's existing storage; send the newest in `hello`; show a warning row on the Mac's page when told), `protocol/fixtures/AuditCheckpoint.json`, `protocol/fixtures/HelloRequest.checkpoint.json`
- Test: `crates/engine/src/bridge/audit_anchor_tests.rs`, `crates/notchlingd/src/bridge_anchor_tests.rs`, `apps/flutter/test/audit_anchor_test.dart`

**Interfaces:**
- Produces: `pub enum PhoneCheck { Intact, TailMissing, Broken, OtherHost, Unknown }`; the daemon's check: the phone's `n` beyond the Audit's last `n` → `TailMissing`; the line at `n` exists but its seal differs → `Broken`; `host` differs → `OtherHost` (ignored); else `Intact`. A result other than `Intact`/`OtherHost` pushes `audit.checked{result, anchor: phone, first_bad_n}` and raises the Inbox item.
- [ ] **Step 1: Write the failing tests:** `phone_checkpoint_beyond_tail_is_tail_missing`, `older_phone_checkpoint_that_matches_is_intact`, `wrong_seal_at_n_is_broken`, `checkpoint_from_another_host_is_ignored`, `checkpoint_pushed_to_subscribed_phone_only`; Flutter: `stores_newest_fifty_per_host`, `sends_newest_in_hello`, `shows_warning_when_told`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine audit_anchor`, `cargo test -p notchling-notchlingd bridge_anchor`, `cd apps/flutter && flutter test test/audit_anchor_test.dart`; expected FAIL.
- [ ] **Step 3: Implement**, schema first, then `python3 tools/protocol/gen.py`, then the Rust and Dart code.
- [ ] **Step 4: Run** `python3 tools/protocol/gen.py --check`, `python3 -m unittest discover -s tools/protocol/tests -t tools/protocol`, `cargo test --workspace --locked`, `cd apps/flutter && flutter analyze && flutter test`. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "bridge: a paired phone keeps the Audit's checkpoints as a second anchor"`.

### Task 7.2: Activity on the phone

**Files:**
- Modify: `protocol/agentconsole.v1.schema.json` (`ActivityListRequest` with `op: "activity.list"`, optional `limit`, `beforeSeq`, `groups`; `ActivityListResponse` with `rows` of an `ActivityRow` definition: `seq`, `time`, `title`, `group`, `outcome`, `who`, `result`, `detail`), generated files, `crates/engine/src/bridge/router_ops.rs` (serves it through `BridgeBackend::activity_list(args) -> Value`), `crates/notchlingd/src/bridge_backend.rs` (reads `events.db` with Phase 2's `list`, names resolved on the Mac), `protocol/fixtures/ActivityListRequest.json`, `ActivityListResponse.json`
- Create: `apps/flutter/lib/features/activity/activity_screen.dart`, `activity_notifier.dart`
- Test: `crates/engine/src/bridge/activity_route_tests.rs`, `apps/flutter/test/activity_test.dart`
- [ ] **Step 1: Write the failing tests:** `phone_activity_list_returns_rows`, `phone_activity_list_pages_by_before_seq`, `phone_activity_has_no_log_settings_ops` (the router has no `log.settings.*`); Flutter `lists_rows_and_loads_more`, `old_mac_without_op_shows_not_available`.
- [ ] **Step 2: Run** the commands of Task 7.1 for these files; expected FAIL.
- [ ] **Step 3: Implement**, schema first.
- [ ] **Step 4: Run** Task 7.1's Step 4 commands. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "bridge, phone: Activity on the phone"`.

### Task 7.3: Activity as a Search corpus

**Files:**
- Create: `crates/engine/src/search/activity.rs`
- Modify: `crates/engine/src/search/fts.rs` (a second table `activity_fts(seq, title, kind, names)`), `crates/engine/src/search/mod.rs`, `crates/notchlingd/src/log_activity.rs` (each written row is offered to the index; each prune removes its rows), `crates/app/src/search/model.rs`, `rows.rs`, `view.rs` (an "Activity" section; opening a hit opens Settings > Activity at that row)
- Test: `crates/engine/src/search/tests_activity.rs`, `crates/app/src/search/model_tests.rs` (new tests only)

**Interfaces:**
- Produces: `pub fn index_rows(db: &mut SearchDb, rows: &[ActivityRow])`, `pub fn remove_rows(db, seqs: &[u64])`, `pub fn search_activity(db, query, limit) -> Vec<ActivityHit { seq, title, ts, score }>`; `names` holds the row's `Ref` values resolved to names at index time (session titles, folder names, secret names, device names).
- [ ] **Step 1: Write the failing tests:** `finds_rows_by_title_and_name`, `pruned_rows_leave_the_index`, `index_holds_no_field_outside_the_registry` (the indexed text of a row is only its title, kind and names), app `activity_section_shows_hits`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::` and `cargo test -p notchling-app search::`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` and clippy on the Mac. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "search: Activity rows as a corpus"`.

### Task 7.4: Rows from other Macs

**Files:**
- Modify: `crates/engine/src/activity/db.rs` (`DbQuery.host: Option<String>`), `crates/engine/src/activity/reply.rs` (rows gain `host` and `host_name` when it isn't this Mac), `crates/notchlingd/src/log_read_ops.rs` and `activity_ops.rs` (`host?` arg), `crates/notchlingd/src/log_bundle.rs` (`README.txt` names the hosts in the bundle), `crates/app/src/activity/model.rs` and `view.rs` (a row from another Mac shows its name)
- Test: `crates/engine/src/activity/host_tests.rs`, `crates/app/src/activity/model_tests.rs` (new tests only)
- [ ] **Step 1: Write the failing tests:** `rows_from_two_hosts_filter_by_host`, `other_host_row_shows_its_name` (names from the Machines list through the `names` closure: `names("host", id)`), `this_mac_rows_show_no_host`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine activity::host` and `cargo test -p notchling-app activity::`; expected FAIL.
- [ ] **Step 3: Implement.** When Machines moves into the daemon (decision 0012's follow-up), its `audit.jsonl` becomes kinds in the registry; that is its own plan.
- [ ] **Step 4: Run** `cargo test --workspace --locked`. Expected: PASS.
- [ ] **Step 5: Commit** by path, `git commit -m "activity: rows from other Macs show their host"`.

## Checkpoint P7 (each part)

- `cargo test --workspace --locked`, clippy, `cargo fmt --all --check`, `cargo deny check`, `tools/test-python.sh`, `python3 -m guard lint`, `rules`; for 7.1 and 7.2 also `python3 tools/protocol/gen.py --check` and `cd apps/flutter && flutter analyze && flutter test`.
