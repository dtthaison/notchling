# Localization L5: The Notch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every word the notch shows comes from `locales/en/mac/notch.ftl`, and the `notch` area's literal lint is armed. English renders are pixel-identical.

**Architecture:** The notch's pure models (`crates/app/src/notch/`) keep their types; titles become `t::notch::NAME`, built sentences become messages with variables formatted where the snapshot becomes a model. The notch's shapes have fixed sizes, so every text in it elides.

**Tech Stack:** Rust, GPUI, `notchling-locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "Longer text, and right to left" item 4, Steps L5. Calls P4, P27, P28 in `2026-11-14-locale-l0-decision.md`.

**Branch:** `agent/locale-l5-notch` from `agent/locale-l1-foundation` (or the latest merged locale step), after open branches touching `crates/app/src/notch/` have merged. Parallel with L3, L4 and L6. Follow-up `agent/locale-l5-plurals` if needed.

## Global Constraints

- English pixel-identical, wrong plurals included.
- Session states the phone shows with exactly the same text go to `shared.ftl` as `state-*` (P4), and the phone's key is renamed to it in the same PR (its Dart call site, auto zone); otherwise `notch-*`.
- Text matched against Claude Code's screen in `notch/answer.rs` and the permission readers stays literal.
- Text in the notch elides (`elide()`); no new fixed width around words.
- A draw may call a plain message; a message with variables is formatted in the model.
- No comments in code.

## Review Focus

1. A state word moved to `shared.ftl` whose phone text differs in case ("Needs input" vs "Needs Input"): two keys, not one (Task 1, `shared_states_match_the_phone_exactly`).
2. The pet's speech or peek text built per animation frame: formatted once per snapshot, not per tick (`bench_chat.rs` is Chat's; the notch's tick is checked by rendering `notch-pet` with `NOTCHLING_RENDER_BENCH=5` before and after: same draw count).
3. An en-XA title that overflows the notch shape: it must elide, never wrap or clip mid-glyph (Task 2's en-XA renders).
4. The question and permission cards' button labels are consent words: they go to `consent.ftl` (Task 1).
5. A count in the collapsed notch (the inbox badge) is a number, never a message.

---

### Task 1: The notch's words

**Zone:** auto (`crates/app/**`, `locales/en/mac/notch.ftl`, `shared.ftl`); maintainer for `consent.ftl` lines and any phone file outside `lib/features/`. Expected hits: `ZONE` for `consent.ftl`.

**Files:**
- Create: `locales/en/mac/notch.ftl`, `crates/app/src/notch/words_tests.rs`
- Modify: `crates/app/src/notch/*.rs` (non-test files), `locales/en/shared.ftl` (`state-*` only where exact), `locales/en/consent.ftl` (Allow, Deny and the other answers on the permission card), the phone's Dart call sites for any key renamed to a shared one

- [ ] **Step 1: Write the failing tests** in `words_tests.rs`: one per moved table or sentence (the model's text equals its message), `shared_states_match_the_phone_exactly` (each `state-*` key the notch uses has the same English as the phone's ARB value it replaced), `permission_buttons_use_consent`.
- [ ] **Step 2: Run** `cargo test -p notchling-app notch::words`. Expected: FAIL.
- [ ] **Step 3: Move the words**, run `python3 tools/locale/gen.py` (and `flutter gen-l10n` if a phone key was renamed), replace the literals.
- [ ] **Step 4: Run** `cargo test -p notchling-app notch::` and, if the phone changed, `cd apps/flutter && flutter analyze && flutter test`. Expected: PASS; existing notch tests unchanged.
- [ ] **Step 5: Commit** `notch: words from locales/`.

### Task 2: Arm `notch`

**Zone:** maintainer (`areas.json`, baseline). Expected hits: `ZONE`, `GUARD-BASELINE` if matched text is baselined.

- [ ] **Step 1: Render before and after** (release, `NOTCHLING_RENDER_FORCE_FRAMES=1`): `notch-collapsed`, `notch-peek`, `notch-list`, `notch-detail`, `notch-question`, `notch-permission`, `notch-question-tabs`, `notch-permission-actions`, `notch-pet`, `notch-perch`, `notch-inbox`; `cmp` each pair. Expected: equal.
- [ ] **Step 2: Render en-XA** the same views; every title elides inside its shape; nothing outside `⟦ ⟧`.
- [ ] **Step 3: Arm**: `notch` moved in `tools/locale/areas.json`; `PYTHONPATH=tools/guard python3 -m guard lint` (0 new, matched text baselined by hand).
- [ ] **Step 4: Commit** `notch: the area has moved`.

### Checkpoint L5

- [ ] `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets -- -D warnings`; `python3 tools/locale/gen.py --check`; `python3 -m guard lint`.
- [ ] `NOTCHLING_RENDER_BENCH=5 notchling-app --render notch-pet --out /tmp/p.png`: the `bench:` line's draws equal the base commit's.
- [ ] en-XA renders in the PR. Push; PR against `merge/settings-batch`.
