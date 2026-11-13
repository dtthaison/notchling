# Localization L6: The Other Windows and the Inbox Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every word in Accounts, the Vault, Pages, Search, Welcome, Machines, the tool windows, the switches, phone pairing, the menus, the Dock and tray, and the kit comes from `locales/`; inbox items carry a `code` and `facts`, so both apps show their titles and action labels in their own words. Each area's literal lint is armed.

**Architecture:** Parts A to F are the same recipe as L4 (words, then arm), one pull request each, in parallel lanes. Part G adds two optional fields to the inbox item (engine, daemon, local API, protocol), sets them in every producer, and makes the Mac and the phone build titles and labels from them, falling back to the stored English title when a daemon is older.

**Tech Stack:** Rust (app, kit, engine, notchlingd), `agentconsole.v1` (additive), Flutter.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "Daemon text and the wire" (inbox), "The registries that are coming" (log kinds), Steps L6. Calls P3, P5, P8, P9, P21, P22, P27, P28 in `2026-11-14-locale-l0-decision.md`, and P30 below.

**Branches and lanes** (each after open branches touching its folders have merged):

| Part | Branch | From | Area(s) in `areas.json` | Words file(s) |
| --- | --- | --- | --- | --- |
| A | `agent/locale-l6a-accounts` | L1 | `accounts` | `mac/accounts.ftl` |
| B | `agent/locale-l6b-vault` | L1 | `vault` | `mac/vault.ftl`, `consent.ftl` |
| C | `agent/locale-l6c-pages-search-welcome` | `agent/locale-l4a-chat-sidebar` (dates use `units.ftl`, P21) | `pages`, `search`, `welcome` | `mac/pages.ftl`, `mac/search.ftl`, `mac/welcome.ftl`, `units.ftl` |
| D | `agent/locale-l6d-machines` | L1 | `machines` | `mac/machines.ftl` |
| E | `agent/locale-l6e-tools-switches-pairing` | L1 | `tools`, `switches`, `phone-pairing` | `mac/tools.ftl`, `mac/switches.ftl`, `mac/pairing.ftl`, `consent.ftl` |
| F | `agent/locale-l6f-shell-kit` | L1 | `shell`, `kit` | `mac/shell.ftl`, `mac/kit.ftl` |
| G | `agent/locale-l6g-inbox-codes` | `merge/settings-batch` once L2a, L3 and L6b have merged | `inbox` | `shared.ftl` (`item-*`), `consent.ftl`, `mac/inbox.ftl`, `phone/inbox.ftl` |

Each part may be followed by `agent/locale-l6<x>-plurals`.

## Global Constraints

- English pixel-identical in every Mac render and phone screen, wrong plurals included.
- `consent.ftl` and `shared.ftl`: each PR inserts its `##` group in alphabetical order of the heading, so parallel PRs touch different places of the file.
- Every word that grants or refuses something goes to `consent.ftl` (P3): the vault's Allow Once, Always Allow, Deny; pairing's Pair and Unpair; Trust Folder; Install Hook; Sign In; turning a switch back on.
- Never translated: part names, element ids, account emails, provider and model names from data, folder names, commands, the `notchling` command line.
- `crates/kit` gains `notchling-locale` as a dependency in Part F (P22).
- Inbox: the local API and `agentconsole.v1` change additively only; the stored row keeps its English `title` and `label`s for logs and older clients; an item with no `code` (older daemon) shows its stored `title`.
- A draw may call a plain message; a message with variables is formatted in a model compute.
- No comments in code.

**P30 (settled here):** of the inbox's six kinds, only four have a producer in the tree (`secret_approval`, `secret_request`, `limit_notice`, `setting_change`). `action_failed` and `notice` get their `code` and `facts` with their first producer; until then the generic path (stored title, labels by action id) covers them. `crates/AGENTS.md` says so.

## Review Focus

1. An older daemon (no `code`) talking to a new app or phone: the stored English title shows, actions still work (Task G4, `item_without_code_shows_stored_title`; Task G5, `inbox_item_without_code`).
2. A setting-change item for a key the app has no words for (a newer daemon's key): the title falls back to the stored English title, never a key name (Task G4, `unknown_setting_key_falls_back`).
3. An action id a client doesn't know: labeled with the stored English label, still clickable (Task G4, `unknown_action_uses_stored_label`).
4. Vault approval text for many names (`Allow a, b, c for cmd`): names join through `t::fmt::list`, one message, no word order glued in code (Task G2, `secret_approval_facts_round_trip`; Task G4, `approval_title_lists_names`).
5. The Machines window and Accounts window in en-XA: rows with fixed widths around words must elide with a tooltip (spec "Longer text" item 1), seen in each part's en-XA renders.

---

## Parts A to F: the recipe

As in `2026-11-14-locale-l4-chat.md`, "The recipe every part follows": a words task with a new `<area>/words_tests.rs` (zone auto; for a `consent.ftl` line, maintainer), then an arm task (zone maintainer; `ZONE`, `GUARD-BASELINE` if matched text is baselined) with before and after renders compared by `cmp`, en-XA renders, the area flipped in `tools/locale/areas.json`, and `python3 -m guard lint` at 0 new. Part-specific notes:

### Part A: Accounts

- Files: `crates/app/src/accounts/**`; words `mac/accounts.ftl`. Sign In and Sign Out go to `consent.ftl` (`consent-mac-sign-in`, P3).
- Notes: the limit bars' "5-hour" and "Weekly" are words; percentages are numbers through `t::fmt::count`; costs use `t::fmt::decimal` with today's digits.
- Renders: `accounts-window`, `accounts-window-off`, `accounts-window-codex`, `limit-bars`, `cost`; `NOTCHLING_RENDER_ACCOUNT_MENU=1` with `chat`.
- Tasks: A1 words (model then views), A2 arm. Commits `accounts: words from locales/`, `accounts: the area has moved`.

### Part B: the Vault

- Files: `crates/app/src/vault/**`, `crates/app/src/mac/vault_prompts.rs` (its words move here; the file's area is `shell`, armed in Part F); words `mac/vault.ftl` and the approval window's buttons and title in `consent.ftl` (`consent-allow-once`, `consent-always-allow`, `consent-deny`, `consent-secret-approval = Allow { $names } for { $command }`; Part G reuses them).
- Notes: `vault/approval.rs`'s "line"/"lines" becomes one plural message.
- Renders: `vault-sheet`, `vault-form`, `vault-approval`, `vault-request`.
- Tasks: B1 words, B2 arm. Commits `vault: words from locales/`, `vault: the area has moved`.

### Part C: Pages, Search, Welcome, dates

- Files: `crates/app/src/pages/**` (`model.rs::count_text` becomes `pages-count` and `pages-count-partial`, as in the spec's example), `search/**`, `welcome/**`; dates: `units-date-short`, `units-date-long`, `units-month-1` to `units-month-12` in `units.ftl`, and `pages/dates.rs` formats through them, keeping its arithmetic.
- Install Hook and its confirmation go to `consent.ftl`.
- A dates test (new `pages/dates_words_tests.rs`): every month and both forms equal today's output for twelve fixed timestamps.
- Renders: `search-panel`, `search-panel-results`, `first-run`, `first-run-swift-hook`, `about`; the Pages window is a `wry` page: its HTML strings in Rust are words too, checked by opening the window once (`--render` has no Pages view).
- Tasks: C1 dates, C2 words, C3 arm (three areas at once). Commits `pages: dates from units.ftl`, `pages, search, welcome: words from locales/`, `pages, search, welcome: the areas have moved`.

### Part D: Machines

- Files: `crates/app/src/machines/**`, `crates/app/src/bin/machines.rs` has none; words `mac/machines.ftl`.
- Notes: ssh and `gh` output and host names are data; the checks' titles and fix buttons are words.
- Renders: the Machines window's own `--render` views (`notchling-machines --help` lists them).
- Tasks: D1 words, D2 arm.

### Part E: Tools, switches, phone pairing

- Files: `crates/app/src/tools/**` (Codex, Antigravity windows), `switches/**` (`restart.rs`'s "is off"/"are off" becomes one plural message over the count), `phone/**` (the pairing panel; Pair, Unpair and Trust go to `consent.ftl`); words `mac/tools.ftl`, `mac/switches.ftl`, `mac/pairing.ftl`.
- Renders: `phone-panel`, `phone-panel-off`, `phone-panel-unpaired`, `phone-panel-tunnel`, `phone-panel-detached`, and the tools' views from `--help`.
- Tasks: E1 words, E2 arm (three areas).

### Part F: the shell (menus, Dock, tray) and the kit

- Files: `crates/app/src/mac/**` (tray menu, Dock menu, main menu, restart notice, pair prompt, vault prompts), `crates/app/src/menu.rs`, `dock.rs`, `acting.rs`; `crates/kit/src/**` except `ui/gallery.rs` (a developer's view, left English); words `mac/shell.ftl` (menu titles in Title Case), `mac/kit.ftl` (the ? button's tooltip and the kit's few labels).
- Zone: maintainer for `crates/kit/Cargo.toml` (`notchling-locale = { path = "../locale" }`) and `Cargo.lock`. Expected hits: `ZONE`.
- Note: the kit's uppercase `TypeRole` keeps `to_uppercase()` until L7 (P23).
- Renders: `kit-gallery`, `kit-gallery-light`, `kit-gallery-dark`, `chat` (menus via the row and account menu flags).
- Tasks: F1 kit dependency and words, F2 shell words, F3 arm (`shell`, `kit`).

---

## Part G: inbox codes and facts

Zone: maintainer for the engine, `notchlingd`, the protocol, `protocol/fixtures/`, `consent.ftl`; auto for `crates/app/src/inbox/**`, `apps/flutter/lib/features/inbox/**`, `shared.ftl`, `mac/inbox.ftl`, `phone/inbox.ftl`. Expected hits: `ZONE`. Existing tests: none edited; new tests only, in new files.

### Task G1: The item carries a code and facts

**Files:**
- Modify: `crates/engine/src/inbox/model.rs` (`Item` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub code: Option<String>` and the same for `pub facts: Option<serde_json::Value>`), every `Item { … }` literal in the tree (`code: None, facts: None` where no producer sets them yet)
- Create: `crates/engine/src/inbox/tests_codes.rs`

- [ ] **Step 1: Write the failing tests**: `item_round_trips_code_and_facts` (serde JSON both ways), `item_without_code_reads_as_none` (a stored row from before has neither field), `persisted_row_keeps_code_and_facts` (through `persist::save` and `load` in a temp dir).
- [ ] **Step 2: Run** `cargo test -p notchling-engine inbox::tests_codes`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `inbox: items carry an optional code and facts`.

### Task G2: Producers set them

**Files:**
- Modify: `crates/notchlingd/src/vault_inbox.rs`, `crates/notchlingd/src/limits_ops.rs` (`limit_item`), `crates/notchlingd/src/settings_ops.rs` (`item`)
- Create: `crates/notchlingd/src/inbox_codes_tests.rs`

**Interfaces:** codes equal the kind's wire name; facts:
- `secret_approval`: `{"names": [String], "command": String}`
- `secret_request`: `{"name": String}`
- `limit_notice`: the fields its text is built from today (`engine::limits::logic::Fired`: `{"level": u32, "name": String}` plus the provider's display name if the text holds it); the builder reads `announcement`'s callers and names the exact fields in the commit
- `setting_change`: `{"key": String, "value": Value, "before": Value}`

- [ ] **Step 1: Write the failing tests**: `secret_approval_facts_round_trip`, `secret_request_facts`, `limit_notice_facts`, `setting_change_facts`; each raises the item through its producer and reads it back from `InboxOps` (`flush()` first).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd inbox_codes`. Expected: FAIL.
- [ ] **Step 3: Implement**; titles and labels keep today's English text.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: inbox producers say what they raised`.

### Task G3: The phone protocol

**Files:** follow `.claude/skills/add-protocol-field/SKILL.md`:
- Modify: `protocol/agentconsole.v1.schema.json` (`InboxItem`: optional `code` (string) and `facts` (object, `additionalProperties: true`), described), regenerated `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart`
- Create: `protocol/fixtures/InboxListResponse.codes.json`, `protocol/fixtures/InboxEvent.codes.json`
- Modify: the bridge's inbox mapping in `crates/engine/src/bridge/inbox.rs` and `crates/notchlingd/src/bridge_backend.rs` if they copy fields one by one

- [ ] **Step 1: Write the fixtures** (an approval and a setting change with codes and facts).
- [ ] **Step 2: Run** `python3 tools/protocol/gen.py --check`, `python3 -m unittest discover -s tools/protocol/tests -t tools/protocol`, `cargo test -p notchling-engine bridge::`. Expected: FAIL until the schema and mapping change.
- [ ] **Step 3: Implement**, regenerate.
- [ ] **Step 4: Run** the same, plus `cargo test -p notchling-protocol` and `cd apps/flutter && flutter test test/protocol_conformance_test.dart`. Expected: PASS.
- [ ] **Step 5: Commit** `protocol: inbox items may carry a code and facts`.

### Task G4: The Mac's inbox in its own words

**Files:**
- Create: `locales/en/mac/inbox.ftl` (the panel's headings, `Row::result` texts such as "Allowed Once · Phone"), `crates/app/src/inbox/words.rs`, `crates/app/src/inbox/words_tests.rs`
- Modify: `locales/en/shared.ftl` (`item-secret-request = Add secret { $name }`, `item-action-open-form`, `item-action-cancel`, `item-action-done`, `item-action-continue`, and `item-limit-*` where the phone shows the same), `locales/en/consent.ftl` (`consent-allow`, `consent-setting-change = Set { $title } to { $value }`), `crates/app/src/inbox/model.rs`, `view.rs`, `system.rs` (notification titles)

**Interfaces:**
- Produces: `pub fn title(item: &serde_json::Value) -> String` and `pub fn action_label(action: &serde_json::Value) -> String` in `inbox/words.rs`; a setting change's `title` variable is `lookup(&sheets::settings::title_key(key))` (P5) and its `value` the choice label `lookup(&choice_key(key, value))` or the value as shown today.

- [ ] **Step 1: Write the failing tests**: `approval_title_lists_names` (`["A", "B"]`, `ls` gives `Allow A, B for ls`), `secret_request_title`, `setting_change_title_uses_settings_words`, `unknown_setting_key_falls_back`, `item_without_code_shows_stored_title`, `unknown_action_uses_stored_label`, `known_action_ids_use_messages`, `result_texts_unchanged`.
- [ ] **Step 2: Run** `cargo test -p notchling-app inbox::words`. Expected: FAIL.
- [ ] **Step 3: Implement**; the model computes titles and labels once per event.
- [ ] **Step 4: Run** `cargo test -p notchling-app inbox::`. Expected: PASS; `model_tests.rs`, `feed_tests.rs`, `system_tests.rs` unchanged.
- [ ] **Step 5: Commit** `inbox: titles and labels in the app's words`.

### Task G5: The phone's inbox in its own words

**Files:**
- Modify: `apps/flutter/lib/features/inbox/inbox_screen.dart`, `inbox_notifier.dart`, `locales/en/phone/inbox.ftl`; regenerate ARB and `app_localizations*.dart`
- Create: `apps/flutter/test/inbox_codes_test.dart`

- [ ] **Step 1: Write the failing tests**: `inbox_item_with_code_uses_messages`, `inbox_item_without_code` (stored title), `unknown_action_label`.
- [ ] **Step 2: Run** `cd apps/flutter && flutter test test/inbox_codes_test.dart`. Expected: FAIL.
- [ ] **Step 3: Implement**; the phone keeps its own casing for words that aren't in `shared.ftl` or `consent.ftl`.
- [ ] **Step 4: Run** `flutter analyze && flutter test`. Expected: PASS; `inbox_test.dart` unchanged.
- [ ] **Step 5: Commit** `phone: inbox titles and labels from codes`.

### Task G6: Arm `inbox`, notes

- [ ] **Step 1:** Renders `inbox-panel`, `inbox-panel-done`, `notch-inbox` before and after, `cmp` equal; en-XA renders.
- [ ] **Step 2:** `inbox` moved in `areas.json`; `python3 -m guard lint` 0 new.
- [ ] **Step 3:** `crates/AGENTS.md` Inbox notes: items carry `code` and `facts`; the stored English title is for logs and older clients; a producer adds its code and facts (P30); `docs/feature-map.md` row names the new tests.
- [ ] **Step 4: Commit** `inbox: the area has moved`.

## Log kinds (conditional)

The logging design's registry writes `mac/logs.ftl` from its first line (P8). If, when L6 runs, a log viewer exists that still holds English literals, it gets a Part H on this recipe (`agent/locale-l6h-logs`): `logs-event-<kind>`, `-about`, `-why` through `t::lookup`, and a test that walks every kind in the registry and fails on one with no words; a kind from a newer daemon shows its id. Otherwise nothing.

### Checkpoint, after each part

- [ ] `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check`; `cargo deny check` (Part F); `python3 tools/locale/gen.py --check`; `PYTHONPATH=tools/guard python3 -m guard lint` (0 new); for G also `flutter analyze && flutter test` and `python3 tools/protocol/gen.py --check`.
- [ ] Renders `cmp` equal, en-XA renders in the PR. Push; PR against `merge/settings-batch`.
