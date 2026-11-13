# Localization L0: Decision Record 0022 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record the approved localization design as decision 0022, add the note on decision 0008 that names Unicode-3.0 as permissive, and bring the approved spec onto the implementation line.

**Architecture:** Documents only. 0022 states the decision and points to the spec for detail; 0008 gets a dated note; the decisions index gets a row. This file also holds what every L-step shares: the order and lanes, the maintainer-only list, and the calls settled while planning.

**Tech Stack:** Markdown.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md` (branch `agent/locale-design`, commit 2ec9764 or later).

**Branch:** `agent/locale-l0-decision`, from `origin/merge/settings-batch`. Critical path.

## Global Constraints

- Every L-step targets `agent-run-oss` through the integration branch `merge/settings-batch`; none targets `main`.
- Decision number is 0022 (0019 to 0021 are taken or reserved).
- `docs/decisions/` is protected: every change here waits for the maintainer.
- User-facing text short and plain; no comments in code (no code here).

## Review Focus

1. 0022 says something the spec doesn't (a shortened rule that changes meaning): the record must quote the spec's decisions, not paraphrase their limits. (Task 1 Step 2 reads them side by side.)
2. The 0008 note widening the license list beyond Unicode-3.0: it names only Unicode-3.0, and only for ICU4X's runtime crates. (Task 2 Step 1.)
3. The decisions index row out of order or with the wrong status. (Task 1 Step 3.)
4. A broken relative link from 0022 to the spec (the spec must exist on this branch). (Task 1 Step 4.)
5. The spec's status line still saying "waiting for approval" after it is accepted. (Task 1 Step 1.)

---

### Task 1: Decision record 0022 and the spec on the branch

**Zone:** maintainer (`docs/decisions/**`, `docs/superpowers/**`). Expected hits: `ZONE`. No code, no tests.

**Files:**
- Create: `docs/decisions/0022-localization-fluent-source.md`
- Create: `docs/superpowers/specs/2026-11-13-localization-design.md` (checked out from `origin/agent/locale-design`)
- Modify: `docs/decisions/README.md` (index row)

- [ ] **Step 1: Bring the spec onto the branch.** `git checkout origin/agent/locale-design -- docs/superpowers/specs/2026-11-13-localization-design.md`. Edit its status line (line 3) to: `Status: accepted by the maintainer on 2026-11-14 as [decision 0022](../../decisions/0022-localization-fluent-source.md).`
- [ ] **Step 2: Write 0022** with the sections every record has (`Context`, `Decision`, `Consequences`, `Status`, `Date`), following `0020-agents-change-settings-by-class.md`:
  - Context: about 2,000 English literals in `crates/app` and `crates/kit`; the phone's hand-written ARB with Vietnamese as template; the daemon's Vietnamese; going open source.
  - Decision: the spec's "Decided by the maintainer" list, the nine "Calls settled after review" and the three "Decided by the user after review", each one line, in the spec's words. Link the spec for the subset table, the generator's outputs and the steps.
  - Consequences: new top folder `locales/` (zone: auto except `languages.json` and `consent.ftl`); `tools/locale/`, `crates/locale/`; three `GUARD-LOCALE-*` rules and two `LINT-LOCALE-*` rules; the phone becomes English; the daemon's text becomes English; first direct Unicode-3.0 dependencies (0008's note).
  - Status: `Accepted. The maintainer approved the design on 2026-11-14 *(decided)*.`
  - Date: `2026-11-14`.
- [ ] **Step 3: Add the index row** after 0020: `| [0022](0022-localization-fluent-source.md) | One English source in Fluent, every platform's words generated from it | Accepted |`.
- [ ] **Step 4: Check the links.** Run `grep -o '](\.\./[^)]*)' docs/decisions/0022-localization-fluent-source.md` and confirm each target exists with `ls`. Expected: every path exists.
- [ ] **Step 5: Commit** `docs: decision 0022, localization from one Fluent source`.

### Task 2: The note on 0008

**Zone:** maintainer (`docs/decisions/**`). Expected hits: `ZONE`.

**Files:**
- Modify: `docs/decisions/0008-mit-permissive-crates.md` (append a note before `## Date`), `docs/decisions/README.md` (0008's status cell: `Accepted, noted by 0022`)

- [ ] **Step 1: Write the note** `## Note, 2026-11-14: Unicode-3.0 is permissive`: ICU4X's `icu_plurals`, `icu_decimal` and their data crates (`icu_plurals_data`, `icu_decimal_data`, `icu_locale_fallback`, `fixed_decimal`) become direct runtime dependencies of `notchling-locale` (decision 0022). Their license, Unicode-3.0, is a permissive license, already on `deny.toml`'s allow list, and ICU4X is already linked through `url` and `idna`. The user took them on 2026-11-14 over `intl_pluralrules` (older CLDR, last released 2022). Nothing else widens: no new exception, no change to `deny.toml`.
- [ ] **Step 2: Run** `cargo deny check licenses` on the branch. Expected: unchanged (no crate added yet).
- [ ] **Step 3: Commit** `docs: 0008 names Unicode-3.0 as permissive`.

### Checkpoint L0

- [ ] `PYTHONPATH=tools/guard python3 -m guard lint` (0 new) and `python3 -m guard claudemd`.
- [ ] Push `agent/locale-l0-decision`; open the PR against `merge/settings-batch`. The maintainer merges it before L1's PR.

---

## All steps: order and lanes

| Step | Plan | Branch(es) | Starts from | Depends on | Lane |
| --- | --- | --- | --- | --- | --- |
| L0 | this file | `agent/locale-l0-decision` | `merge/settings-batch` | the design's approval | critical path |
| L1 | `2026-11-14-locale-l1-foundation.md` | `agent/locale-l1-foundation` | `agent/locale-l0-decision` | L0 | critical path |
| L2a | `2026-11-14-locale-l2-phone-and-daemon.md` part A | `agent/locale-l2a-phone`, then `agent/locale-l2a-plurals` | `agent/locale-l1-foundation` | L1 | critical path (phone lane) |
| L2b | same file, part B | `agent/locale-l2b-daemon-english` | `agent/locale-l0-decision` | L0 only | daemon lane, parallel with L1 |
| L3 | `2026-11-14-locale-l3-settings.md` | `agent/locale-l3-settings` (+ `-plurals` if needed) | `agent/locale-l1-foundation` | L1; Settings branches merged | Mac lane 1 |
| L4 | `2026-11-14-locale-l4-chat.md` | `agent/locale-l4a-chat-sidebar` … `l4e-chat-window`, each from the previous | `agent/locale-l1-foundation` | L1; Chat branches merged | Mac lane 2, sequential inside |
| L5 | `2026-11-14-locale-l5-notch.md` | `agent/locale-l5-notch` | `agent/locale-l1-foundation` | L1 | Mac lane 3 |
| L6 | `2026-11-14-locale-l6-other-windows.md` | `agent/locale-l6a-accounts` … `l6g-inbox-codes` | see that plan | L1; L6d after L4a; L6g after L2a and L3 | Mac lanes 4 to 7 |
| L7 | `2026-11-14-locale-l7-second-language.md` | `agent/locale-l7a-language-row`, `l7b-script-pass`, `l7c-<tag>` | latest merged step | a second language arrives | later |
| L8 | `2026-11-14-locale-l8-native-and-rtl.md` | `agent/locale-l8-native-outputs` | latest merged step | a native phone app | later |

- **Critical path:** L0, then L1, then L2a. The Settings search registry and the log event-kind registry (other designs) can start once L1 merges; the Settings search registry also wants L3, so its titles are the sheet's own messages.
- **Parallel lanes after L1:** L2a (phone), L3, the L4 chain, L5, and the L6 parts each touch their own folders, their own `.ftl` files and their own line in `tools/locale/areas.json` (L1 lists every area, so a step flips one line). L2b runs beside L1.
- **Merge-order rules:** a migration (L3 to L6) starts only after open branches touching its area have merged into `merge/settings-batch`. A step that moves an area changes no English; its wrong plurals go in a follow-up branch `agent/locale-l<N>-plurals` right after, with before and after renders.

## Maintainer-only list

What waits for the maintainer whatever Phase 5 allows:

1. L0: decision 0022, the 0008 note, the index, the spec's status line.
2. L1: all of it: `tools/locale/`, `crates/locale/` (runtime), every `Cargo.toml` and `Cargo.lock` change (ICU4X direct, `fluent-bundle` dev only, `objc2-foundation`'s `NSLocale` feature), `locales/languages.json`, `locales/en/consent.ftl`, `tools/locale/areas.json`, `tools/guard/` (rules, probes, lint, baseline), `docs/rules.md`, `tools/guard/zones.json` and `.github/CODEOWNERS`, `.github/workflows/guard.yml`, `tools/test-python.sh`, every `AGENTS.md`, `CONTRIBUTING.md`, `docs/feature-map.md`. Also the binary-size measure: over 1 MB added needs his word.
3. L2a: approving the phone's Vietnamese-to-English test edits (`apps/flutter/test/support/harness.dart` and the test files that assert Vietnamese, `WEAK-TEST-EDIT`), `lib/app/`, `lib/core/net/`, deleting `app_vi.arb`, `l10n.yaml`, `languages.json` (`arb` output on), `zones.json` (`apps/flutter/lib/l10n/**` into the zone), `areas.json` (phone moved), the lint baseline, `.github/workflows/phone.yml`, `apps/flutter/AGENTS.md`.
4. L2b: the engine, daemon and ptyhost text, their existing tests, `protocol/README.md`, the schema, regenerated types, `protocol/fixtures/`.
5. Each Mac migration (L3 to L6) once: its `areas.json` line and lint-baseline entries; L6g's engine, local API, protocol and phone-protocol parts; any existing test it edits.
6. L7: the `language` key in `crates/engine/src/settings/keys.rs` and the key-count test, `tools/bundle-next.sh` (`CFBundleLocalizations`), moving a language from `draft` to `ship`, and the new language's first pull request (it adds the tag to `languages.json`).
7. Every `consent.ftl` change in any language, English included, forever.
8. L8: turning on the iOS and Android outputs; any right-to-left work.

## Calls settled in planning

Gaps the spec left, or facts the tree has changed since it was written. Each is applied in the plan named.

- **P1. Plan and branch shape.** One plan per step; L2, L4 and L6 hold several pull requests, each its own branch `agent/locale-l<N><x>-<slug>`.
- **P2. The ARB output is off until L2a.** `languages.json` gets `"outputs": ["rust"]`; L2a adds `"arb"`. Otherwise L1's `gen.py` would overwrite the hand-written `app_en.arb` with only shared and consent words and break the phone. iOS and Android stay behind `--ios`/`--android`. (L1, L2a)
- **P3. Consent words always go to `consent.ftl`.** A consent text only one app shows is named `consent-mac-…` or `consent-phone-…`; it reaches both outputs and is unused in one, which costs a table entry. The zone outranks the sharing rule. (L2a, L6)
- **P4. `shared.ftl` holds brand terms and its four namespaces only** (`language-`, `state-`, `session-`, `item-`). A generic word both apps show alike (Cancel, Close) stays one key per app; the spec's key rule 2 leaves no namespace for it. (L2a, L4 to L6)
- **P5. Settings keys are named from the setting's key:** title `settings-<kebab key>` (the row's text today), `settings-<kebab key>-about`, `settings-<kebab key>-words`, choice labels `settings-<kebab key>-<kebab value>`. Rows with no setting key are named by meaning (`settings-act-on-sessions`). The spec's example `settings-keep-awake-while-working` becomes `settings-mode-while-working`. Why: L6g's setting-change inbox item finds a setting's title from its key by `t::lookup`. (L3, L6g)
- **P6. L3 moves only words the sheet shows today.** `-about` and `-words` are written by the Settings search registry's own plan, which owns the search words; L3 reserves the names. (L3)
- **P7. Conflict with the hybrid search design** (`agent/hybrid-search-design`): it puts the settings registry in `crates/engine/src/search/settings_entries.rs` and expects `shared.ftl` messages `setting-<key>` with `.about`, `.keywords`, `.label` attributes. Under this design attributes are outside the subset, settings words are Mac only (`mac/settings.ftl`, prefix `settings-`), and the engine never depends on `notchling-locale`. The contract L1 publishes: the registry lives in the app and holds `Text` constants; if it must stay in the engine, it names message ids as `&'static str` and an app test proves each resolves through `t::lookup`. What text the daemon embeds for vector search is that design's call. Flagged to the coordinator. (L1)
- **P8. Log kinds:** message key `logs-event-<kind>` with `.` and `_` turned into `-`, plus `-about` (what it records) and `-why` (why it matters). The logging design's registry writes them from its first line; L6 adds nothing unless a viewer exists that isn't on messages yet. (L1 contract, L6)
- **P9. The inbox has six kinds,** not five: decision 0020 added `setting_change`. L6g gives all six a `code` and `facts`; a setting change's title is consent text (`consent-setting-change = Set { $title } to { $value }`). (L6g)
- **P10. `t::lookup` finds plain messages only;** `Text` constants exist only for plain messages. A message with variables is reached through its typed function. (L1)
- **P11. Runtime state is one `OnceLock<Active>`,** set once by `notchling_locale::start`; before it, everything reads as English. Reading it is one atomic load, as the spec's `AtomicU8`. Tests build `Active` values directly. (L1)
- **P12. `t::fmt::decimal`** scales the value to an integer and shifts the decimal point (`multiplied_pow10`), since `fixed_decimal`'s `ryu` feature stays off. (L1)
- **P13. An unknown key in a translation fails `--check`** (goal 4). A pull request that renames an English key runs `gen.py --prune` in the same pull request; the error says so. (L1)
- **P14. Generated Rust parameters** come in the order the variables first appear in English, the selector first. Every key has at least two words, so each maps to `t::<first word>::<rest>`. (L1)
- **P15. `LINT-LOCALE-LITERAL`'s probe needs `areas.json`:** `lint.Rule` gains `probe_files`, which `_lint_probe` writes beside the bad example. Inside a Rust file, a line `#[cfg(test)]` at column 0 ends the scanned part. (L1)
- **P16. The phone has grown since the spec counted:** 25 test files hold Vietnamese (about 216 lines), not 14 (about 95). Only UI-text assertions and the harness default change; Vietnamese conversation data (`test/support/preview_data.dart`, `fake_ws.dart`) stays unless a test asserts it as UI text. (L2a)
- **P17. More Vietnamese UI in `lib/` than the spec listed:** `app/alerts.dart` (the foreground service text), `features/chat/chat_pictures.dart`, `features/voice/speech_engine.dart` (missing model), and `features/chat/send_now.dart`, which matches the daemon's Vietnamese `lệnh`. All become messages except that marker: `send_now.dart` matches the old Vietnamese marker and L2b's English `No such op` (older Macs still send Vietnamese); both sit in the lint baseline. (L2a)
- **P18. L2b's English for an unknown op is exactly `No such op "<op>".`** (`engine/src/bridge/router.rs`, `ptyhost/src/ops.rs`). (L2b)
- **P19. `BridgeException`s the client makes carry an empty message;** the UI maps every code to its own words, and an unknown daemon code shows the daemon's message as detail. (L2a)
- **P20. Chat is five areas in `areas.json`** (sidebar, conversation, reply box, details, window), moved by L4a to L4e in one sequential lane, since all add to `mac/chat.ftl`. (L4)
- **P21. `mac/units.ftl` starts in L1** with the list separator (`t::fmt::list` needs it); ages, durations and token counts move in L4a, dates in L6d. (L1, L4a, L6d)
- **P22. The kit depends on `notchling-locale` from L6f,** when its words move, not from L1. (L6)
- **P23. Grapheme-safe cuts and `t::upper` come with L7's script pass** (three `chars().take(n)` in display code today). (L7)
- **P24. The `language` setting is class Free;** adding it moves `keys.rs`'s count test from 42 to 43, an existing-test edit for the maintainer. (L7)
- **P25. L2b doesn't need L1** and runs beside it. (L2b)
- **P26. `LINT-LOCALE-CONSENT` will warn on Settings rows** such as "Allow network in Codex turns". Warnings don't fail; Settings rows stay out of `consent.ftl`, as the spec says. (L3)
- **P27. Pixel-identical English** is checked with `cmp` on PNGs rendered from the base commit and from the branch, same environment, `NOTCHLING_RENDER_FORCE_FRAMES=1` on a locked screen. (L3 to L6)
- **P28. Lint baseline growth:** arming an area adds its leftovers (matched text) to `tools/guard/lint-baseline.json`, which `GUARD-BASELINE` sends to the maintainer; the step waits for him once anyway. (L2a, L3 to L6)
- **P29. `WEAK-TEST-EDIT` fires only on `apps/flutter/test/`, `crates/*/tests/`, `tools/guard/tests/` and goldens.** Existing unit tests in `src/*_tests.rs` don't trip it; each plan still lists them for the maintainer. (all)
- **P30. Only four inbox kinds have a producer today** (`secret_approval`, `secret_request`, `limit_notice`, `setting_change`). `action_failed` and `notice` get their `code` and `facts` with their first producer; until then the stored English title and labels-by-action-id cover them. (L6g)
- **P31. Render fixtures are not UI words:** `LINT-LOCALE-LITERAL` skips `**/renders.rs`, `crates/app/src/chat/bench_fixture.rs` and `crates/app/src/testutil.rs`, and the kit's developer gallery (`crates/kit/src/ui/gallery.rs`) is left out of the `kit` area. (L1, L6f)
- **P32. Shared files edited by parallel pull requests** (`consent.ftl`, `shared.ftl`): each pull request inserts its `##` group in alphabetical order of the heading, so parallel branches touch different places. (L5, L6)
