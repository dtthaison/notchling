# Localization L7: When a Second Language Arrives Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Once a second language is ready to ship: a Settings Language row (System or a shipped language, applied at the next launch), the app bundle declaring its localizations so macOS offers a per-app language, a pass over the language's script (fonts, line heights, grapheme-safe cuts, casing), and the language moved from `draft` to `ship`.

**Architecture:** A new `language` setting the store holds and `start_for_run` passes to `notchling_locale::start`; the row appears only when `shipped()` has two or more tags. `tools/bundle-next.sh` writes `CFBundleLocalizations` from `locales/languages.json`. Cuts in display code go through grapheme clusters; the kit's uppercase role goes through `t::upper`.

**Tech Stack:** Rust, GPUI, `notchling-locale`, shell, `unicode-segmentation` (already in `Cargo.lock`).

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "Choosing the language", "GPUI text", Steps L7. Calls P23, P24, P27 in `2026-11-14-locale-l0-decision.md`.

**Trigger:** a translation in `locales/<tag>/` (listed under `draft`) whose report shows it complete enough for the maintainer to ship. Not before.

**Branches:** `agent/locale-l7a-language-row` and `agent/locale-l7b-script-pass` from the latest merged locale step (parallel); then `agent/locale-l7c-ship-<tag>` from both.

## Global Constraints

- A language change applies at the next launch, with a Relaunch button; no live switching.
- The row exists only while two or more languages ship.
- The `language` key is class Free (P24): it gives an agent nothing.
- Renders and tests stay English unless `NOTCHLING_LOCALE` says otherwise.
- A new language's first PR and every move from `draft` to `ship` wait for the maintainer.
- No comments in code.

## Review Focus

1. A stored `language` naming a tag that no longer ships (a downgrade): read as System, never a panic (Task 1, `unshipped_setting_reads_as_system`).
2. macOS's per-app language and the Settings row disagree: the row wins when it isn't System (spec order: env, setting, system) (Task 1, `setting_beats_system`).
3. Cutting a session title with combining marks (Vietnamese, Devanagari) mid-cluster: never splits a mark from its letter (Task 3, `cut_keeps_clusters`).
4. Turkish `i` uppercased by the kit's uppercase role: `İ` under `tr`, `I` under `en` (Task 3, `upper_follows_language`).
5. A script whose glyphs are taller than Latin clipped by `TypeRole` line heights: checked in the language's renders (Task 3, Step 4).

---

### Task 1: The `language` setting and the row

**Zone:** maintainer for `crates/engine/src/settings/keys.rs` and its count test (existing test edited: 42 to 43 keys, because a key is added); auto for `crates/app/**` and `locales/en/mac/settings.ftl`. Expected hits: `ZONE`.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`language`: string, default `"system"`, allowed `"system"` plus every shipped tag, `.free`, title "Language", description), `crates/engine/src/settings/migrate.rs` (count 43), `crates/app/src/language.rs` (`start_for_run` reads the store once, like `history::apply`), `crates/app/src/sheets/settings.rs` (a General row: System, then each shipped language by its own `language-name`), `crates/app/src/sheets/settings_view.rs` (Relaunch button after a change), `locales/en/mac/settings.ftl` (`settings-language`, `settings-language-system`, `settings-language-relaunch`)
- Create: `crates/app/src/language_setting_tests.rs`

**Interfaces:**
- Produces: `pub fn read_setting(data_dir: &Path) -> Option<String>` in `language.rs`; `pub fn language_choices() -> Vec<(String, String)>` (tag, label) in `sheets/settings.rs`; the row shows only when `notchling_locale::shipped().len() >= 2`.

- [ ] **Step 1: Write the failing tests**: `row_hidden_with_one_language`, `row_lists_system_then_each_language_by_its_own_name`, `setting_beats_system` (through `notchling_locale::choose`), `unshipped_setting_reads_as_system`, `store_unreadable_reads_as_none`.
- [ ] **Step 2: Run** `cargo test -p notchling-app language_setting` and `cargo test -p notchling-engine settings::`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `settings: a Language row once two languages ship`.

### Task 2: The bundle declares its languages

**Zone:** maintainer (`tools/`). Expected hits: `ZONE`.

**Files:**
- Modify: `tools/bundle-next.sh` (`CFBundleLocalizations` array from `python3 -c` reading `locales/languages.json`'s `ship`; `CFBundleDevelopmentRegion` `en`), `tools/AGENTS.md` (why: macOS offers the per-app language and localizes its own panels only for declared localizations)

- [ ] **Step 1: Run** `tools/bundle-next.sh` (never `--install`), then `plutil -extract CFBundleLocalizations json -o - "build/next/Notchling.app/Contents/Info.plist"`. Expected: the `ship` list.
- [ ] **Step 2: Commit** `bundle: declare the shipped localizations`.

### Task 3: The script pass

**Zone:** maintainer for `crates/locale/` (`upper`); auto for `crates/app/**` and `crates/kit/**`. Expected hits: `ZONE`.

**Files:**
- Modify: `crates/locale/src/lib.rs` (`pub fn upper(text: &str) -> String`: Turkish and Azerbaijani dotted `i`, Greek accents dropped in capitals, else `to_uppercase`), `crates/kit/src/typescale.rs` (uppercase role through `t::upper`), the three `chars().take(n)` cuts in display code (find with `grep -rn "chars().take(" crates/app/src crates/kit/src | grep -v test`) through `unicode_segmentation::UnicodeSegmentation::graphemes`
- Create: `crates/locale/src/upper_tests.rs`, `crates/app/src/cut_tests.rs` (or beside each cut's module)

- [ ] **Step 1: Write the failing tests**: `upper_follows_language` (an `Active` for `tr` and `en`), `greek_capitals_drop_tonos`, `cut_keeps_clusters` (`"Việt"` cut to 3 clusters is `"Việ"`, never a bare mark).
- [ ] **Step 2: Run** `cargo test -p notchling-locale upper` and `cargo test -p notchling-app cut`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Render** every view listed in L3 to L6 with `NOTCHLING_LOCALE=<tag>` and look at fonts, fallback glyphs and line heights; a clipped line gets room in the kit's `TypeRole` (a kit change with before and after English renders, which must stay pixel-identical).
- [ ] **Step 5: Commit** `locale: script pass for <tag>`.

### Task 4: Ship the language

**Zone:** maintainer (`languages.json`; the translation's first PR if not merged already). Expected hits: `ZONE`.

- [ ] **Step 1:** `python3 tools/locale/gen.py --report --lang <tag> --missing`; the maintainer reads it.
- [ ] **Step 2:** Move `<tag>` from `draft` to `ship`; `python3 tools/locale/gen.py` (writes its Rust table and its ARB); `flutter gen-l10n`; `cargo test -p notchling-locale` (conformance and categories for the new language).
- [ ] **Step 3:** Renders with `NOTCHLING_LOCALE=<tag>` and `NOTCHLING_LOCALE_MARK_FALLBACK=1` in the PR; phone screenshots with the device in that language.
- [ ] **Step 4: Commit** `locales: ship <tag>`.

### Checkpoint L7

- [ ] `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets -- -D warnings`; `python3 tools/locale/gen.py --check`; `flutter analyze && flutter test`; English renders `cmp` equal to the base commit's.
