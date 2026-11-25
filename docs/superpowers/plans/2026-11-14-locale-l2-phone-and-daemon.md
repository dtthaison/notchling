# Localization L2: The Phone to English, the Daemon's Words to English Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Part A: the phone's words come from `locales/` (English source, generated ARB), every visible Dart literal included, and the phone's lint area moves. Part B: every text the daemon, the bridge and ptyhost send is English.

**Architecture:** Part A converts today's `app_en.arb` into `locales/en/phone/*.ftl` once, turns on the ARB output, renames the Dart getters, maps `BridgeException` codes to messages in the UI, and gives code without a `BuildContext` one holder of the current `AppL10n`. Part B replaces Vietnamese strings in Rust with English and says so in the protocol's description; codes on the wire don't change.

**Tech Stack:** Flutter (`flutter gen-l10n`, ARB), Dart; Rust (engine bridge, notchlingd, ptyhost); `tools/protocol/gen.py`.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "The phone" (L2a, L2b) and "Daemon text and the wire". Calls P2, P3, P4, P16 to P19, P25, P28 and P29 in `2026-11-14-locale-l0-decision.md`.

**Branches:** Part A `agent/locale-l2a-phone` from `agent/locale-l1-foundation` (critical path), then `agent/locale-l2a-plurals` from it. Part B `agent/locale-l2b-daemon-english` from `agent/locale-l0-decision`, in parallel with L1 (P25).

## Global Constraints

- The phone keeps its casing; a text goes to `shared.ftl` only in the shared namespaces (`state-`, `session-`, `item-`) and only when the Mac shows exactly the same text, case included (P4); consent words go to `consent.ftl` (P3).
- English renders stay as they are today, wrong plurals included; plural fixes go in `agent/locale-l2a-plurals`.
- `use-escaping` stays off in `l10n.yaml`; the subset keeps braces out of phone text.
- Dictation stays Vietnamese: Whisper's language `vi`, `features/voice/prompt_builder.dart`, `transcript_cleanup.dart` (the user's decision 3).
- `agentconsole.v1` changes additively only; only `error.message`'s description changes in Part B.
- Phone files: `lib/app/`, `lib/core/net/`, `lib/features/pairing/`, existing tests and goldens need the maintainer (`apps/flutter/AGENTS.md`).
- A new phone test that lays out text also runs at the harness's `textScale: 1.3` (spec "Longer text" item 7).
- No comments in Dart or Rust.

## Review Focus

1. A Mac on an older daemon sends Vietnamese `error.message`, and a newer one English: the phone shows its own words for every known code and the daemon's text only as detail (Task A3, `unknown_code_shows_detail_under_general_message`).
2. Send Now against an older Mac (no `session.sendNow`): the phone still hides it, whether the refusal says `lệnh` or `No such op` (Task A5, `send_now_unsupported_on_either_marker`).
3. A notification fired with no widget tree (background, app just started): its title comes from the holder, never crashes, never shows a key (Task A4, `holder_before_first_frame_is_english`).
4. An apostrophe or `#` in a plural branch reaching `flutter gen-l10n`: pinned by Task A2's `l10n_format_test.dart`.
5. Part B changes a string the Mac app or a phone matches on: the only matcher is `send_now.dart` (P17); the phone tests and `protocol_conformance_test` run in Part B's checkpoint.

---

## Part A: the phone (L2a)

### Task A1: The phone's words as Fluent

**Zone:** `locales/en/phone/**` and `shared.ftl` auto; `consent.ftl` maintainer. Expected hits: `ZONE` (consent). No tests edited.

**Files:**
- Create: `locales/en/phone/app.ftl`, `bridge.ftl`, `sessions.ftl`, `chat.ftl`, `inbox.ftl`, `pairing.ftl`, `macs.ftl`, `visualize.ftl`, `voice.ftl`, `settings.ftl`, `notifications.ftl` (one per feature folder of `lib/features/`, plus `app` for `lib/app/` and `bridge` for `lib/core/net/`)
- Modify: `locales/en/consent.ftl` (the phone's `allow`, `deny` and other consent words), `locales/en/shared.ftl` (only shared-namespace texts the Mac shows exactly alike)

**Interfaces:**
- Produces: the rename table, old ARB key to new Fluent key, used by Task A2. Rules: the key gets its file's prefix and keeps the rest in kebab case (`pairTitle` to `pairing-title`, `tabSessions` to `app-tab-sessions`, `stateRunning` to `state-running` if the Mac shows "Running" for that state, else `sessions-state-running`); the ARB `description` becomes `#` notes; placeholders keep their names in snake case (`folderName` to `folder_name`); `{count}` placeholders of type `int` become `{ NUMBER($count) }`.

- [ ] **Step 1: Convert** with a one-off script run from `/tmp` (not committed): read `apps/flutter/lib/l10n/app_en.arb`, assign each key a file by where `lib/` uses it (`grep -rl "l10n.<key>\b" apps/flutter/lib`; a key used in several features goes to `app.ftl`), write the files, print the rename table to `/tmp/l2a-renames.tsv`. For each English value, check the Mac with `grep -rn --include='*.rs' '"<value>"' crates/app/src crates/kit/src`; only an exact match in a shared namespace goes to `shared.ftl`.
- [ ] **Step 2: Run** `python3 tools/locale/gen.py --check` and the converter's own check: every English value of the old `app_en.arb` appears once among the new messages, with its placeholders. Expected: clean; 199 messages accounted for.
- [ ] **Step 3: Commit** `locales: the phone's words as Fluent`.

### Task A2: Generated ARB, the getters renamed, Vietnamese files gone

**Zone:** maintainer (`languages.json`, `lib/app/`, `l10n.yaml`, deleting `app_vi.arb`); `lib/features/**` auto. Expected hits: `ZONE`.

**Files:**
- Modify: `locales/languages.json` (`"outputs": ["rust", "arb"]`, P2), `apps/flutter/l10n.yaml` (`template-arb-file: app_en.arb`), `apps/flutter/lib/app/app.dart` (drop `localeResolutionCallback`), every Dart file using a renamed getter (from `/tmp/l2a-renames.tsv`)
- Delete: `apps/flutter/lib/l10n/app_vi.arb`, `apps/flutter/lib/l10n/app_localizations_vi.dart`
- Regenerate: `apps/flutter/lib/l10n/app_en.arb` (`gen.py`), `app_localizations.dart`, `app_localizations_en.dart` (`flutter gen-l10n`)
- Create: `apps/flutter/test/l10n_format_test.dart`

- [ ] **Step 1: Write the failing test** `l10n_format_test.dart` (new test, auto): `apostrophe_survives_gen_l10n` (a message holding `'` renders it), `plural_one_and_other` (a plural message gives its one and other forms for 1 and 2), `hash_in_plural_branch` (pin whatever `flutter gen-l10n` does with `#`; if it substitutes the number, the subset's checker gains a refusal of `#` inside a phone variant in this task, with a `tools/locale` test).
- [ ] **Step 2: Run** `cd apps/flutter && flutter test test/l10n_format_test.dart`. Expected: FAIL (getters missing).
- [ ] **Step 3: Generate and rename:** `python3 tools/locale/gen.py`, `cd apps/flutter && flutter gen-l10n`, then rename call sites with `sed` from the table (whole-word `l10n.<old>` to `l10n.<new>`).
- [ ] **Step 4: Run** `flutter analyze` and `flutter test test/l10n_format_test.dart`. Expected: no issue; PASS.
- [ ] **Step 5: Commit** `phone: English source, ARB generated from locales/`.

### Task A3: Bridge errors by code

**Zone:** maintainer for `lib/core/net/bridge_client.dart`; `lib/features/**` auto. Expected hits: `ZONE`.

**Files:**
- Modify: `apps/flutter/lib/core/net/bridge_client.dart` (the client's own `BridgeException(code, '')`: empty message, P19), `locales/en/phone/bridge.ftl` (`bridge-error-disconnected`, `-view-only`, `-unauthorized`, `-not-found`, `-invalid`, `-busy`, `-not-reachable`, `-internal`, `-unknown`, and every client code `bridge_client.dart` raises)
- Create: `apps/flutter/lib/features/connection/bridge_errors.dart`, `apps/flutter/test/bridge_errors_test.dart`

**Interfaces:**
- Produces: `String bridgeErrorText(AppL10n l10n, BridgeException error)` and `String? bridgeErrorDetail(BridgeException error)` (the daemon's message, only for a code with no message of its own or an unknown code; `null` when empty).

- [ ] **Step 1: Write the failing tests** `bridge_errors_test.dart`: `every_client_code_has_words` (each code `bridge_client.dart` raises maps to a non-empty English text that is not the code), `known_daemon_code_ignores_message`, `unknown_code_shows_detail_under_general_message`.
- [ ] **Step 2: Run** `flutter test test/bridge_errors_test.dart`. Expected: FAIL.
- [ ] **Step 3: Implement**, and replace every UI use of `error.message` with the two functions.
- [ ] **Step 4: Run** `flutter analyze && flutter test test/bridge_errors_test.dart`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: bridge errors shown by code`.

### Task A4: Words where there is no BuildContext

**Zone:** maintainer (`lib/app/`). Expected hits: `ZONE`.

**Files:**
- Create: `apps/flutter/lib/app/l10n_holder.dart`, `apps/flutter/test/l10n_holder_test.dart`
- Modify: `apps/flutter/lib/main.dart` or `lib/app/app.dart` (set at start, and from a `WidgetsBindingObserver.didChangeLocales`), `apps/flutter/lib/app/notifications.dart` (titles, bodies, channel names), `apps/flutter/lib/app/alerts.dart` (the foreground service text, a plural over the session count), `locales/en/phone/notifications.ftl`

**Interfaces:**
- Produces: `abstract final class L10nHolder { static AppL10n get current; static void update(List<Locale>? locales); }`; `update` resolves through `AppL10n.supportedLocales` (English today) and `lookupAppL10n`.

- [ ] **Step 1: Write the failing tests**: `holder_before_first_frame_is_english`, `update_with_unknown_locale_stays_english`.
- [ ] **Step 2: Run** `flutter test test/l10n_holder_test.dart`. Expected: FAIL.
- [ ] **Step 3: Implement**; notification and service texts read `L10nHolder.current`.
- [ ] **Step 4: Run** `flutter analyze && flutter test test/l10n_holder_test.dart`. Expected: PASS.
- [ ] **Step 5: Commit** `phone: one holder of the words for code without a context`.

### Task A5: The other literals

**Zone:** `lib/features/**` auto; `lib/core/net/` maintainer if touched. Expected hits: none beyond `ZONE` for `core/net`.

**Files:**
- Modify: `apps/flutter/lib/features/chat/photo_send.dart`, `chat_pictures.dart`, `chat_notifier.dart` (the takeover and answer errors), `features/sessions/new_session_sheet.dart`, `features/visualize/viz_providers.dart` (each `BridgeException('disconnected', 'Chưa kết nối tới Mac.')` becomes `BridgeException('disconnected', '')`), `features/voice/speech_engine.dart` (`SpeechEngineException` carries a code; the UI shows `voice-model-missing`), `features/chat/send_now.dart` (P17)
- Create: `apps/flutter/lib/features/chat/send_now_markers.dart`, `apps/flutter/test/send_now_markers_test.dart`

**Interfaces:**
- Produces: `bool isNoSuchOp(BridgeException error)`: `error.isInvalid` and the message contains `lệnh` (an older Mac) or `No such op` (Part B's English, P18).

- [ ] **Step 1: Write the failing test** `send_now_unsupported_on_either_marker` (both markers true; `busy` with either false; `invalid` with other text false).
- [ ] **Step 2: Run** `flutter test test/send_now_markers_test.dart`. Expected: FAIL.
- [ ] **Step 3: Implement**; `send_now.dart` calls `isNoSuchOp`. `chat_notifier.dart`'s state carries codes, not texts, where it held `'Chưa kết nối tới Mac.'`.
- [ ] **Step 4: Run** `flutter analyze`, then `grep -rlP '[àáảãạăằắẳẵặâầấẩẫậđèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵĐ]' apps/flutter/lib | grep -v l10n/`. Expected: only `prompt_builder.dart`, `transcript_cleanup.dart` and `send_now_markers.dart`.
- [ ] **Step 5: Commit** `phone: no Vietnamese left in the UI`.

### Task A6: The tests assert English

**Zone:** maintainer: `WEAK-TEST-EDIT` on each file. Why: the phone's default language becomes English, and these tests assert Vietnamese UI text; nothing they prove changes.

**Files:**
- Modify: `apps/flutter/test/support/harness.dart` (`host(…, {Locale locale = const Locale('en'), …})`) and the files that assert Vietnamese UI text: `background_alerts_test`, `bridge_client_test`, `chat_kit_test`, `chat_sync_test`, `computers_view_test`, `inbox_test`, `keyboard_focus_test`, `markdown_table_test`, `message_alerts_test`, `motion_test`, `pairing_test`, `phase2_test`, `send_now_test`, `settings_test`, `state_test`, `takeover_computers_test`, `takeover_row_test`, `takeover_test`, `tunnel_test`, `voice_controller_test`, `voice_logic_test`, `widgets_test` (`.dart`). `test/support/preview_data.dart` and `fake_ws.dart` keep Vietnamese conversation data unless a test asserts it as UI text (P16). Voice tests that assert the Vietnamese prompt stay as they are.

- [ ] **Step 1: Edit** each expectation from the Vietnamese text to the English text of the same message (read from `app_en.arb`); never loosen a matcher (`find.text` stays `find.text`).
- [ ] **Step 2: Run** `flutter test`. Expected: PASS, goldens unchanged (`git status apps/flutter/test/goldens` clean). A changed golden stops the task for the maintainer.
- [ ] **Step 3: Commit** `phone: tests assert English`, listing every edited file and why in the body.

### Task A7: The lint arms, the zone, CI, notes

**Zone:** maintainer (`tools/`, `.github/`, `apps/flutter/AGENTS.md`). Expected hits: `ZONE`, `GUARD-BASELINE` (baseline grows, P28).

**Files:**
- Modify: `tools/locale/areas.json` (`phone` moved), `tools/guard/lint-baseline.json` (the leftover `LINT-LOCALE-LITERAL` findings: `prompt_builder.dart`, `transcript_cleanup.dart`, `send_now_markers.dart`), `tools/guard/zones.json` (`"apps/flutter/lib/l10n/**"` in `auto` after `apps/flutter/lib/features/**`), `.github/CODEOWNERS`, `.github/workflows/phone.yml` (in `phone-analyze`, after `flutter pub get`: `flutter gen-l10n` then `git diff --exit-code -- lib/l10n`), `apps/flutter/AGENTS.md` (rule 3 as the spec's item 9; the `lib/app/l10n_holder.dart` and `features/connection/bridge_errors.dart` notes; dictation stays Vietnamese), `docs/feature-map.md` (the phone rows name `locales/en/phone/`)

- [ ] **Step 1: Run** `PYTHONPATH=tools/guard python3 -m guard lint` and copy only the expected leftovers into the baseline by hand. Expected after: 0 new.
- [ ] **Step 2: Run** `python3 -m guard codeowners --write`, `--check`, `python3 -m guard rulesets`, `python3 -m unittest discover -s tools/guard/tests -t tools/guard -k test_workflows`. Expected: PASS.
- [ ] **Step 3: Commit** `phone: the literal lint covers lib/, l10n in the zone`.

### Checkpoint L2a

- [ ] `python3 tools/locale/gen.py --check`; `cd apps/flutter && flutter gen-l10n && git diff --exit-code -- lib/l10n && flutter analyze && flutter test`.
- [ ] `cargo test -p notchling-locale` (the shared and consent messages the phone added reach the Rust tables).
- [ ] Screenshots of Sessions, Chat, Inbox and Pairing in English for the PR. Push; PR against `merge/settings-batch`.

### Task A8: Plural fixes (`agent/locale-l2a-plurals`)

**Zone:** `locales/en/phone/**` auto; the Dart call sites auto; existing tests asserting the wrong form are maintainer (`WEAK-TEST-EDIT`).

- [ ] **Step 1:** List English values with a number placeholder and no select: `grep -n 'NUMBER(\$' locales/en/phone/*.ftl locales/en/shared.ftl`. Each one read as "1 <plural noun>" becomes a select (`sessions-running-subagents`: `[one] { $count } subagent running` / `*[other] { $count } subagents running`).
- [ ] **Step 2:** New tests `plural_forms_test.dart` assert the one and other forms of each fixed message.
- [ ] **Step 3:** `python3 tools/locale/gen.py`, `flutter gen-l10n`, `flutter test`. Before and after screenshots in the PR.
- [ ] **Step 4: Commit** `phone: English plurals agree with their count`.

---

## Part B: the daemon's words (L2b)

All of Part B is maintainer zone (engine outside `transcript/` and `search/`, `notchlingd`, `ptyhost`, `protocol/`). Expected hits: `ZONE`. Existing unit tests in `src/*_tests.rs` change (no `WEAK-TEST-EDIT` trip, P29, still listed); `crates/*/tests/**` and `protocol/fixtures/**` edits are existing tests and fixtures for the maintainer.

Find the strings: `grep -rnP '[àáảãạăằắẳẵặâầấẩẫậđèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵĐ]' crates/engine/src/bridge crates/engine/src/viz crates/engine/src/control crates/notchlingd/src crates/ptyhost/src protocol`. Test data that is Vietnamese on purpose (`frame.rs`'s `xin chào`, search and transcript fixtures, Unicode-width tests) stays.

### Task B1: The bridge's errors

**Files:**
- Modify: `crates/engine/src/bridge/router.rs` (unknown op: exactly `No such op "{op}".`, P18), `router_ops.rs`, `router_inbox.rs`, `model.rs` (`TAKEOVER_HINT`, `ALREADY_CONTROLLABLE`, `not_found` texts), `backend.rs`, `stream.rs`, `extras.rs`, `snapshot.rs`, `live.rs`, `images.rs`, and the tests that assert their text: `wire_tests.rs`, `stream_tests.rs`, `snapshot_tests.rs`, `net/devices_tests.rs` (only lines asserting these messages)

- [ ] **Step 1: Write the failing test** `router_tests.rs::unknown_op_says_no_such_op` (new test in an existing file): `{"op":"nope"}` answers `invalid` with message `No such op "nope".`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine bridge::`. Expected: FAIL on that test.
- [ ] **Step 3: Translate** each string to short, plain English; keep codes. Update the assertions that named the old text to the new text (same matcher).
- [ ] **Step 4: Run** `cargo test -p notchling-engine bridge::`. Expected: PASS.
- [ ] **Step 5: Commit** `bridge: errors in English`.

### Task B2: Visualize, takeover, the daemon's bridge backend

**Files:**
- Modify: `crates/engine/src/viz/engine.rs` (empty states), `viz/tests.rs`, `viz/tests_git.rs` (assertions), `crates/engine/src/control/takeover.rs` (`TakeoverError` texts, the hint), `control/takeover_tests.rs`, `crates/notchlingd/src/bridge_backend.rs` (five messages) and its tests if they assert them

- [ ] **Step 1: Write the failing test** `takeover_tests.rs::reasons_are_english` (new): each `TakeoverError` message is ASCII and non-empty.
- [ ] **Step 2: Run** `cargo test -p notchling-engine control::takeover`. Expected: FAIL.
- [ ] **Step 3: Translate.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine` and `cargo test -p notchling-notchlingd bridge_backend`. Expected: PASS.
- [ ] **Step 5: Commit** `engine: Visualize and takeover text in English`.

### Task B3: ptyhost

**Files:**
- Modify: `crates/ptyhost/src/error.rs`, `ops.rs` (unknown op: `No such op "{op}".`), `pty_handle.rs`, `system.rs`, `client.rs`, `server.rs`, and `crates/ptyhost/tests/**` lines asserting them

- [ ] **Step 1: Write the failing test** `error.rs`'s test module: `every_host_error_is_english` (each variant's `Display` is ASCII).
- [ ] **Step 2: Run** `cargo test -p notchling-ptyhost`. Expected: FAIL.
- [ ] **Step 3: Translate.** Keep `hostd.sock`'s contract: no message is parsed by the Swift app or `ReplySender` (check `grep -rn "Refused\|message" legacy/macos/Sources/Notchling/Agents/ReplySender*.swift` for a match on Vietnamese; none expected).
- [ ] **Step 4: Run** `cargo test -p notchling-ptyhost`. Expected: PASS.
- [ ] **Step 5: Commit** `ptyhost: errors in English`.

### Task B4: The protocol says English

**Files:**
- Modify: `protocol/README.md` (line 77: "`message` is text for the person, in English; do not parse it."), `protocol/agentconsole.v1.schema.json` (`error.message` description: "Text for the person, in English. Never parse it."), regenerated `crates/protocol/src/generated.rs` and `apps/flutter/lib/core/protocol/generated.dart`, `protocol/fixtures/ErrorResponse*.json`, `SessionsListResponse*.json`, `SessionsEvent.json` (messages to the English the daemon now sends)

- [ ] **Step 1: Run** `python3 tools/protocol/gen.py`, then `python3 tools/protocol/gen.py --check`. Expected: clean.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/protocol/tests -t tools/protocol`, `cargo test -p notchling-protocol`, `cargo test -p notchling-engine bridge::fixtures`, `cd apps/flutter && flutter test test/protocol_conformance_test.dart`. Expected: PASS.
- [ ] **Step 3: Commit** `protocol: error messages are English`.

### Checkpoint L2b

- [ ] `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check`.
- [ ] The grep above lists only test data that is Vietnamese on purpose.
- [ ] `cd apps/flutter && flutter test` (the phone still passes against English daemon text). Push; PR against `merge/settings-batch`, after or beside L1.
