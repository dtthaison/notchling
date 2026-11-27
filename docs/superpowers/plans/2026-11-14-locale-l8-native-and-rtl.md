# Localization L8: Native Phone Outputs, and Right to Left Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a native phone app (Swift or Kotlin) exists: turn on the iOS `Localizable.xcstrings` and Android `strings.xml` outputs L1 built, bind them to the source in CI, and prove the plural forms in Xcode and on Android. Right to left is out of this plan: it needs its own design first.

**Architecture:** `languages.json`'s `outputs` gains `"ios"` and `"android"` with the native app's folders; `gen.py` writes and `--check`s them as it does Rust and ARB. Nothing in the Mac app changes.

**Tech Stack:** Python (`tools/locale/`), Xcode string catalogs, Android resources.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "Outputs" (iOS, Android), "Longer text, and right to left", Steps L8. Calls P2 in `2026-11-14-locale-l0-decision.md`.

**Trigger:** a native phone app's folder lands in the repository. Not before.

**Branch:** `agent/locale-l8-native-outputs` from the latest merged locale step.

## Global Constraints

- Generated files are bound to the source by `GUARD-LOCALE-GEN`, like Rust and ARB.
- The native apps take `shared.ftl`, `consent.ftl` and `phone/**`, as ARB does.
- Python standard library only; no comments in code.

## Review Focus

1. A plural message with two variables in iOS (`%#@count@` substitution): checked by building in Xcode and rendering both forms, since no Python test can prove Xcode reads it (Task 2).
2. Android `values-b+zh+Hans` and `values-pt-rBR` folders resolving on a device set to those languages (Task 2).
3. A missing message on Android falls back to `values/` by itself; on iOS the catalog's `en` entry: both checked with a draft language missing a message (Task 2).
4. An apostrophe or `%` reaching Android unescaped crashes `getString` with arguments: `test_android_apostrophe_and_percent` from L1 already pins it.
5. Removing a language from `ship` leaves stale native files: `--check` fails on a file the tree no longer writes (Task 1, `stale_native_file_is_reported`).

---

### Task 1: Outputs on, bound by `--check`

**Zone:** maintainer (`tools/`, `languages.json`). Expected hits: `ZONE`.

**Files:**
- Modify: `tools/locale/check.py` (`Config.outputs` accepts `"ios"`, `"android"`; `languages.json` gains `"paths": {"ios": "<catalog path>", "android": "<res folder>"}`), `tools/locale/gen.py` (write and check them when listed), `locales/languages.json`, `tools/guard/zones.json` (the generated native files in `auto`), `.github/CODEOWNERS`
- Create: `tools/locale/tests/test_native_outputs.py`

- [ ] **Step 1: Write the failing tests**: `outputs_written_when_listed`, `check_fails_on_stale_native_file`, `stale_native_file_is_reported` (a `values-de/` folder left after `de` leaves `ship`).
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_native_outputs`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `tools/test-python.sh` and `python3 tools/locale/gen.py --check`. Expected: PASS.
- [ ] **Step 5: Commit** `tools: the iOS and Android outputs are on`.

### Task 2: Proven on the platforms

**Zone:** the native app's own zone. No Python change.

- [ ] **Step 1:** Build the iOS app in Xcode with the generated catalog; show `session-subagents-running` (or the first plural message with two variables) for 1 and 2 in English and one shipped language; screenshot.
- [ ] **Step 2:** Same on Android, including a `pt-BR` or `zh-Hans` device and a draft language missing a message.
- [ ] **Step 3:** Record the results and any change to the xcstrings substitution form in `tools/AGENTS.md` (`tools/locale/emit_xcstrings.py::plural`), and fix the writer if Xcode disagrees (a golden update in `tools/locale/tests/golden/`, maintainer).
- [ ] **Step 4: Commit** `tools: native outputs checked on device`.

## Right to left

Not planned here. It needs a design of its own first (decision 0022 lists it as a non-goal): mirrored layout (GPUI 0.2.2 has no layout direction; the kit's `Corner` already says leading and trailing), bidirectional caret and selection in the kit's text input and Chat's selection, mirrored icons, isolation marks around variables at run time (a new `Active` option), and Flutter's `EdgeInsetsDirectional` where the phone uses `left`/`right`. The format and the generator already carry right-to-left text unchanged; when that design is approved, its plan follows this one's shape.
