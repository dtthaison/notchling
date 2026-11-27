# Localization L3: Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every word the Settings sheet shows (General, Agents and Activity pages) comes from `locales/en/mac/settings.ftl`, named so the Settings search registry and the inbox's setting-change items find a setting's title from its key; the `settings` area's literal lint is armed. English renders are pixel-identical.

**Architecture:** Model tables that held `&'static str` (`DAEMON_ROWS`, the choice lists, section and row titles, `Page::title`, `hook_rows::TITLE`) hold `Text` constants or call plain messages; sentences built with `format!` become messages with variables, formatted where the model is computed. No type a view reads changes.

**Tech Stack:** Rust, GPUI, `notchling-locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "How views call it", "The registries that are coming", Steps L3. Calls P5, P6, P26, P27, P28 in `2026-11-14-locale-l0-decision.md`.

**Branch:** `agent/locale-l3-settings` from `agent/locale-l1-foundation` (rebased onto `merge/settings-batch` once L1 merges), after every open branch touching `crates/app/src/sheets/` or `activity/` has merged (check `git branch -r --contains` is empty of such work, and ask the coordinator). Follow-up `agent/locale-l3-plurals` only if a wrong plural turns up.

## Global Constraints

- English pixel-identical, wrong plurals included (call 8).
- Key names (P5): a setting's title is `settings-<kebab key>` (`hideNotch` to `settings-hide-notch`); a choice is `settings-<kebab key>-<kebab value>` (`mode`/`whileWorking` to `settings-mode-while-working`); rows with no setting key and section and page titles are named by meaning: `settings-act-on-sessions`, `settings-section-keep-mac-awake`, `settings-page-general`.
- `-about` and `-words` are not written here; the search registry's plan adds them (P6).
- A draw may call a plain message; a message with variables is formatted in a model compute.
- Part names (`.part("…")`) and setting keys stay English literals.
- `crates/engine/src/settings/keys.rs` titles and descriptions are for agents and stay English in the engine; Settings rows get their words from `settings.ftl` only.
- No comments in code.

## Review Focus

1. A setting whose row title differs from its `keys.rs` title: the row's own text wins and goes in `settings-<key>` (the search registry shows what the row shows). (Task 1, `every_keyed_row_has_its_message`.)
2. A choice label shared by two settings ("Off", "On"): two messages, one per setting, so a language can word them differently. (Task 1, `choice_labels_are_per_setting`.)
3. The Activity page's filter and empty texts with counts: formatted in the model, not per row per frame (Task 3; `bench_chat.rs` counts unchanged).
4. The `LINT-LOCALE-CONSENT` warning on "Allow network in Codex turns": expected, not moved to `consent.ftl` (P26).
5. The pseudo-language making a choice row wider than the sheet: the en-XA render shows it; a choice row that doesn't fit is noted for a popup (spec "Longer text" item 2), not fixed here.

---

### Task 1: The General page's model

**Zone:** auto (`crates/app/**`, `locales/en/mac/settings.ftl`). Expected hits: none. Existing tests in `crates/app/src/sheets/settings.rs` keep asserting the same English (unchanged).

**Files:**
- Create: `locales/en/mac/settings.ftl`, `crates/app/src/sheets/settings_words_tests.rs`
- Modify: `crates/app/src/sheets/settings.rs` (`DAEMON_ROWS: [(Area, Text); 5]`, `AWAKE_MODES`, `WIDTHS`, `LIMIT_MODES`, the `toggle`/`choice` titles and section titles), `crates/app/src/sheets/settings_view/shortcut.rs`

**Interfaces:**
- Consumes: `notchling_locale::{Text, lookup}`; `t::settings::*` generated from the new file.
- Produces: `pub(crate) fn title_key(setting: &str) -> String` (`"hideNotch"` to `"settings-hide-notch"`) and `pub(crate) fn choice_key(setting: &str, value: &str) -> String`, used by L6g and the search registry; kebab rule: a capital letter starts a new word, lowercased.

- [ ] **Step 1: Write the failing tests** in `settings_words_tests.rs`:
  - `kebab_keys`: `title_key("hideNotch") == "settings-hide-notch"`, `title_key("codexLimitMode") == "settings-codex-limit-mode"`, `choice_key("mode", "whileWorking") == "settings-mode-while-working"`.
  - `every_keyed_row_has_its_message`: for each row with a setting key in the General page's model, `lookup(&title_key(key))` is `Some` and its `english()` equals the row's title.
  - `choice_labels_are_per_setting`: each choice's label is `lookup(&choice_key(key, value))`.
- [ ] **Step 2: Run** `cargo test -p notchling-app settings_words`. Expected: FAIL.
- [ ] **Step 3: Move the words**: write each English text into `settings.ftl` under `##` groups per section, with a `#` note saying where it shows; run `python3 tools/locale/gen.py`; replace each literal with `t::settings::NAME` or `t::settings::name()`.
- [ ] **Step 4: Run** `cargo test -p notchling-app sheets::`. Expected: PASS, including the existing tests asserting `"General"`, `"Daemon"`, `"Conversation history to load"`.
- [ ] **Step 5: Commit** `settings: the General page's words from locales/`.

### Task 2: The sheet's views, Agents and Hooks

**Zone:** auto. Expected hits: none.

**Files:**
- Modify: `crates/app/src/sheets/settings_view.rs`, `settings_hooks_view.rs`, `hook_rows.rs` (`TITLE`), `agents.rs` (`Page::title` returns `&'static str` from `t::settings::page_*()`), `agents_view.rs`, `locales/en/mac/settings.ftl`

- [ ] **Step 1: Write the failing test** in `settings_words_tests.rs`: `pages_and_hooks_use_messages` (`Page::Agents.title() == t::settings::page_agents()`, `hook_rows::TITLE == t::settings::hooks()`; adjust if `TITLE` becomes a function).
- [ ] **Step 2: Run** `cargo test -p notchling-app settings_words`. Expected: FAIL.
- [ ] **Step 3: Move the words.** Sentences built with `format!` become one message with variables each; a sentence with a clickable or styled part may split (call 7), each part's note naming the others.
- [ ] **Step 4: Run** `cargo test -p notchling-app sheets::`. Expected: PASS (`agents_tests.rs` still asserts `"Agents"` and `"Activity"`).
- [ ] **Step 5: Commit** `settings: the sheet's views and the Agents page from locales/`.

### Task 3: The Activity page

**Zone:** auto. Expected hits: none.

**Files:**
- Create: `crates/app/src/activity/words_tests.rs`
- Modify: `crates/app/src/activity/model.rs`, `view.rs`, `renders.rs` (fixture text stays as data), `locales/en/mac/settings.ftl` (prefix `settings-activity-`)

- [ ] **Step 1: Write the failing test** in a new `crates/app/src/activity/words_tests.rs` (not the existing `model_tests.rs`): `filter_and_empty_texts_use_messages` (the model's empty text and filter labels equal their messages).
- [ ] **Step 2: Run** `cargo test -p notchling-app activity::`. Expected: FAIL.
- [ ] **Step 3: Move the words**; counts through `t::settings::activity_*(n)` in the model.
- [ ] **Step 4: Run** `cargo test -p notchling-app activity::`. Expected: PASS, `model_tests.rs` unchanged.
- [ ] **Step 5: Commit** `settings: the Activity page from locales/`.

### Task 4: Arm the area, prove nothing moved

**Zone:** maintainer for `tools/locale/areas.json` and `tools/guard/lint-baseline.json`. Expected hits: `ZONE`, `GUARD-BASELINE` only if leftovers exist (P28).

**Files:**
- Modify: `tools/locale/areas.json` (`settings` moved), `tools/guard/lint-baseline.json` (only text matched against another program, if any)

- [ ] **Step 1: Render before**, on the base commit, release build: for each of `settings`, `settings-agents`, `settings-agents-partial`, `settings-agents-empty`, `settings-agents-detached`, `activity`, `activity-filtered`, run `NOTCHLING_RENDER_FORCE_FRAMES=1 NOTCHLING_RENDER_APPEARANCE=light target/release/notchling-app --render <view> --out /tmp/l3-before/<view>.png`, and again with `dark`.
- [ ] **Step 2: Render after** the same way into `/tmp/l3-after/`, then `for f in /tmp/l3-before/*.png; do cmp "$f" "/tmp/l3-after/$(basename $f)" || echo "DIFF $f"; done`. Expected: no `DIFF` line (P27).
- [ ] **Step 3: Render en-XA**: `NOTCHLING_LOCALE=en-XA` for the same views into `/tmp/l3-xa/`; look at each; anything not in `⟦ ⟧` is still hard-coded and goes back to Tasks 1 to 3.
- [ ] **Step 4: Arm**: flip `settings` to `moved: true`; run `PYTHONPATH=tools/guard python3 -m guard lint`. Expected: 0 new (or only matched text, baselined by hand).
- [ ] **Step 5: Commit** `settings: the area has moved; the literal lint is armed`.

### Checkpoint L3

- [ ] `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets -- -D warnings`; `python3 tools/locale/gen.py --check`; `python3 -m guard lint --warnings` (the expected `settings-codex-network` warning, P26).
- [ ] `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1`: counts as before.
- [ ] The en-XA renders attached to the PR. Push; PR against `merge/settings-batch`.
- [ ] Wrong plurals found? Open `agent/locale-l3-plurals` with the select messages, new tests for one and other forms, and before and after renders.
