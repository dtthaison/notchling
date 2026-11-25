# Localization L1: The Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `locales/en/` as the one source of words, `tools/locale/gen.py` that checks it and writes every platform's file, the `notchling-locale` crate that serves the Mac its words, the guard rules that keep it so, and every Mac entry point choosing a language at start. The app looks the same.

**Architecture:** A Python standard-library generator parses a Fluent subset (`ftl.py`), checks it (`check.py`), and writes Rust tables (`emit_rust.py`), ARB (`emit_arb.py`, off for the real tree until L2a), and the iOS and Android files (off). The Rust crate holds static tables and typed functions generated from the source, with ICU4X for plural categories and number formats; `choose` picks the language once per process. Guard rules bind the generated files to the source, keep the subset, and arm a literal lint per moved area.

**Tech Stack:** Python 3.9+ standard library; Rust 1.99.1 (`icu_plurals`, `icu_decimal` 2.4.0; `fluent-bundle` 0.16.0 dev only); `objc2-foundation` `NSLocale`; GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md` (sections Source format, Files and keys, The generator, Runtime in the Mac app, Checks and rules, Zones, Translating). Calls P1 to P29 in `2026-11-14-locale-l0-decision.md` apply.

**Branch:** `agent/locale-l1-foundation`, from `agent/locale-l0-decision`. **Critical path:** L2a, L3 to L6, the Settings search registry and the log event-kind registry all build on this.

## Global Constraints

- Python: standard library only, 3.9 or later; modules imported by bare name, tests run with `python3 -m unittest discover -s tools/locale/tests -t tools/locale`.
- No comments in code, doc comments included, in Rust, Dart, Python and shell (`python3 tools/strip_comments.py --check`). Translator notes are `#` lines in `.ftl` files only. Generated Rust has one `@generated` header line and no notes.
- Every error names a rule id, a file and a line: `GUARD-LOCALE-SUBSET: locales/de/mac/chat.ftl:12: …`.
- Language tags in one form: `en`, `de`, `pt-BR`, `zh-Hans` (lowercase language, title-case script, uppercase region).
- Key: lowercase ASCII words joined by `-`, each word `[a-z][a-z0-9]*`, at least two words, at most 64 characters; starts with its file's name (`settings-` in `mac/settings.ftl`, `consent-` in `consent.ftl`); `shared.ftl` uses only `language-`, `state-`, `session-`, `item-`; unique across a language's files.
- Variable: `[a-z][a-z0-9]*` then `_[a-z0-9]+` groups.
- Outputs by file location: `shared.ftl` and `consent.ftl` to every output; `mac/**` to Rust only; `phone/**` to ARB, xcstrings and Android only.
- `notchling-locale` has no GPUI and no platform code; it builds and tests on macOS, Linux and Windows. `app` depends on it in this step; the engine, daemon, hook and command line never do.
- A plain message is `&'static str` with no allocation; the active language is read with one atomic load.
- Dependencies pinned exactly (`"=2.4.0"`), all on `deny.toml`'s list; `fixed_decimal`'s `ryu` off.
- `crates/AGENTS.md` rule to add: a draw may call a plain message; a message with variables is formatted in a model compute.

## Review Focus

1. A translation that breaks the phone's ICU only (a brace, an apostrophe in a plural, a `#`): the subset refuses braces outside `mac/`, and the ARB test round-trips `'` and `#` (Task 5, `test_apostrophe_and_hash_pass_through`).
2. A translation selecting on a variable English uses only as a string, or dropping a variable from `*[other]`: refused (Task 3, `test_select_on_string_variable_is_refused`, `test_variable_missing_from_other_is_refused`).
3. Hidden characters: a raw U+200C in a `.ftl` is refused, the escaped form is accepted and written as `\u{200c}` in Rust and `‌` in ARB, so no generated file trips `WEAK-UNICODE` (Tasks 1, 4, 5).
4. A process that never called `start` (a test, a forgotten entry point) shows English and never panics (Task 10, `get_before_start_is_english`).
5. `pt-BR` missing a message shows `pt` if `pt` ships, else English; a region-only system tag (`en-DE`) picks `en` and German number grouping (Task 10, `fallback_chain_parent_then_english`; Task 9, `en_with_region_de_groups_with_dots`).

---

### Task 1: The Fluent subset parser

**Zone:** maintainer (`tools/`). Expected hits: `ZONE`. New tests only.

**Files:**
- Create: `tools/locale/ftl.py`, `tools/locale/tests/__init__.py`, `tools/locale/tests/test_ftl.py`

**Interfaces:**
- Produces, in `ftl.py`:
  - `class FtlError(ValueError)` with `rule: str`, `path: str`, `line: int`, `message: str`; `str()` is `f"{rule}: {path}:{line}: {message}"`
  - `Lit(text: str)`, `Var(name: str)`, `Num(name: str)`, `Term(name: str)` (dataclasses, frozen); `Pattern = List[Union[Lit, Var, Num, Term]]`
  - `Variant(category: str, pattern: Pattern, default: bool)`; `Select(var: str, variants: List[Variant])`
  - `Message(key: str, value: Union[Pattern, Select], notes: List[str], group: Optional[str], line: int)`
  - `Resource(path: str, messages: List[Message], terms: Dict[str, str], file_notes: List[str])`
  - `def parse(text: str, path: str) -> Resource` (raises `FtlError` with rule `GUARD-LOCALE-SUBSET`)

- [ ] **Step 1: Write the failing tests** in `test_ftl.py`:
  - `test_plain_message`: `"settings-act-on-sessions = Act on Sessions\n"` gives one `Message` whose value is `[Lit("Act on Sessions")]`.
  - `test_variables_and_number`: `"chat-moved-to = Moved to { $folder_name } at { NUMBER($n) }\n"` gives `[Lit("Moved to "), Var("folder_name"), Lit(" at "), Num("n")]`.
  - `test_plural_select`: the spec's `session-subagents-running` block gives `Select("count", [Variant("one", …, False), Variant("other", …, True)])`.
  - `test_notes_attach_to_next_message`: `#` lines attach to the next message, `##` sets `group`, `###` goes to `file_notes`.
  - `test_brand_term`: `"-brand-notchling = Notchling\n"` fills `terms`; `{ -brand-notchling }` in a value is `Term("notchling")`.
  - `test_string_literals`: `{"{"}`, `{"}"}`, `{" "}` and `{"‌"}` become `Lit` text `{`, `}`, ` `, `‌`.
  - `test_refusals`, one `subTest` per row, each asserting `rule == "GUARD-LOCALE-SUBSET"`, the line number, and a message containing the word in the last column:

    | Input | Line | Message contains |
    | --- | --- | --- |
    | `a-b = { $n -> \n  [0] none\n *[other] x\n}` | 2 | `exact-number` |
    | `a-b = { $g -> \n  [male] x\n *[other] y\n}` | 2 | `category` |
    | two selects in one value | 1 | `one select` |
    | nested select | 2 | `nested` |
    | `a-b = { other-key }` | 1 | `message reference` |
    | `a-b = x\n    .title = y` | 2 | `attribute` |
    | `a-b = { DATETIME($d) }` | 1 | `function` |
    | `a-b = { NUMBER($n, minimumFractionDigits: 1) }` | 1 | `options` |
    | `-term-x = y` | 1 | `-brand-` |
    | `-brand-x = a { $v }` | 1 | `variables` |
    | `a-b = one\n    two` | 2 | `one line` |
    | `a-b = x ` (trailing space) | 1 | `space` |
    | a tab | its line | `tab` |
    | `\r\n` | 1 | `CRLF` |
    | a BOM | 1 | `byte order mark` |
    | a raw U+200C in text | 1 | `hidden character` |
    | a select without `*[other]` | its line | `*[other]` |
    | `a-b = { $count ->` with no closing `}` | last line | `unclosed` |
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_ftl`. Expected: FAIL, `ModuleNotFoundError: No module named 'ftl'`.
- [ ] **Step 3: Implement `parse`** as a line-oriented reader over the grammar in the spec's "The source format"; values are tokenized left to right into `Lit` and placeables. Brace handling (`{` `}` as text) is checked in Task 2, by file location, not here.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `tools: the Fluent subset parser`.

### Task 2: Tree rules (GUARD-LOCALE-SUBSET)

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Create: `tools/locale/check.py`, `tools/locale/tests/test_check.py`, fixture tree `tools/locale/tests/fixtures/tree/` (`languages.json`, `en/shared.ftl`, `en/consent.ftl`, `en/mac/chat.ftl`, `en/mac/units.ftl`, `en/phone/pairing.ftl`, `de/shared.ftl`, `de/mac/chat.ftl`, `pt/shared.ftl`, `pt-BR/shared.ftl`)

**Interfaces:**
- Consumes: `parse`, `Resource`, `Message`, `FtlError` (Task 1).
- Produces, in `check.py`:
  - `Config(source: str, ship: List[str], draft: List[str], outputs: List[str])` read from `languages.json` (`outputs` ⊆ `{"rust", "arb"}`; P2)
  - `Lang(tag: str, files: Dict[str, Resource])` (keys are paths relative to the language folder: `shared.ftl`, `mac/chat.ftl`)
  - `Tree(root: Path, config: Config, langs: Dict[str, Lang])`
  - `Problem(rule: str, path: str, line: int, message: str)`
  - `def load(root: Path) -> Tuple[Tree, List[Problem]]`
  - `def check_structure(tree: Tree) -> List[Problem]`
  - `def targets(rel: str) -> FrozenSet[str]`: `{"rust", "phone"}` for `shared.ftl`/`consent.ftl`, `{"rust"}` for `mac/**`, `{"phone"}` for `phone/**`
  - `SHARED_NAMESPACES = ("language", "state", "session", "item")`

- [ ] **Step 1: Write the failing tests** in `test_check.py`, each building a small tree in a `tempfile.TemporaryDirectory` and asserting one `Problem` with rule `GUARD-LOCALE-SUBSET`, the path and line:
  - `test_fixture_tree_is_clean`: `load(fixtures/tree)` then `check_structure` gives `[]`.
  - `test_key_must_start_with_file_name`: `chat-x` in `mac/notch.ftl`.
  - `test_shared_namespaces_only`: `chat-x` in `shared.ftl`; and an area file using `state-x`.
  - `test_key_shape`: `Chat-x`, `chat_x`, `chat-1x`, `chat` (one word), a 65-character key.
  - `test_duplicate_key_across_files`: the same key in `mac/chat.ftl` and `shared.ftl` (both lines reported).
  - `test_unlisted_language_folder`: a `fr/` folder not in `ship` or `draft`.
  - `test_tag_form`: folders `pt-br`, `zh-hans`, `EN` refused.
  - `test_braces_only_in_mac`: `{"{"}` allowed in `mac/chat.ftl`, refused in `shared.ftl`, `consent.ftl` and `phone/pairing.ftl`.
  - `test_brand_terms_only_in_english_shared`: a `-brand-x` in `de/shared.ftl` or `en/mac/chat.ftl` is refused; an unknown `{ -brand-y }` is refused.
  - `test_language_name_required`: every language's `shared.ftl` defines `language-name`.
  - `test_unknown_output`: `"outputs": ["rust", "ios"]` refused.
  - `test_unknown_file`: `en/other.ftl` (not `shared.ftl`, `consent.ftl`, `mac/*.ftl`, `phone/*.ftl`) refused.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_check`. Expected: FAIL (no module `check`).
- [ ] **Step 3: Implement** `load` (parse errors become `Problem`s; reading continues) and `check_structure`.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `tools: locale tree rules`.

### Task 3: A translation against English (GUARD-LOCALE-MATCH)

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Modify: `tools/locale/check.py`
- Create: `tools/locale/tests/test_match.py`

**Interfaces:**
- Produces:
  - `def var_types(tree: Tree) -> Dict[str, Dict[str, str]]`: per English key, each variable as `"num"` (used in `NUMBER()` or as a selector) or `"str"`
  - `def check_match(tree: Tree) -> List[Problem]` (rule `GUARD-LOCALE-MATCH`)
  - `def english(tree: Tree) -> Dict[str, Tuple[str, Message]]` (key to its file and message)

- [ ] **Step 1: Write the failing tests**:
  - `test_unknown_key_is_refused_and_names_prune`: `de` has a key English lacks; the message contains `gen.py --prune` (P13).
  - `test_same_variables_required`: a `de` value missing `{ $folder_name }`, and one adding `{ $other }`.
  - `test_select_on_string_variable_is_refused`.
  - `test_select_on_number_english_does_not_select_is_allowed`: English `x-open = { NUMBER($count) } open`, Polish-style `de` select over `$count` with `one`, `few`, `*[other]`: clean.
  - `test_variable_missing_from_other_is_refused`; and `test_other_variants_may_drop_a_number`: `[one] one file` without `$count` is clean.
  - `test_no_links_or_markup`: values with `https://x`, `www.x.org`, `a@b.co`, `<b>` refused; English with them is not checked by this rule.
  - `test_length_cap`: English `abc` (3), `de` of 30 characters (`3 * 3 + 20 = 29`) refused, 29 accepted.
  - `test_missing_message_is_not_an_error`.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_match`. Expected: FAIL.
- [ ] **Step 3: Implement.** Length counts characters of the text with each placeable as one character; brand terms count as their English text.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `tools: a translation must match English`.

### Task 4: The Rust writer

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Create: `tools/locale/emit_rust.py`, `tools/locale/tests/test_emit_rust.py`, `tools/locale/tests/golden/generated.rs`

**Interfaces:**
- Consumes: `Tree`, `english`, `var_types`, `targets` (Tasks 2, 3).
- Produces: `def render(tree: Tree) -> str`. The text it writes (consumed by Task 8's crate):
  - first line `// @generated by tools/locale/gen.py from locales/; do not edit.`; nothing else that is a comment
  - `use crate::{Arg, Body, Cat, Num, Part, Table, Text};`
  - per namespace (first word of the key) `pub mod <ns> { use crate::{…}; … }` with, per message, sorted by key:
    - plain: `pub const <NAME>: Text = Text(<id>);` and `pub fn <name>() -> &'static str { <NAME>.get() }`
    - with variables: `pub fn <name>(<p>: impl Into<Num>, <q>: &str) -> String { crate::format(Text(<id>), &[Arg::Num(<p>.into()), Arg::Str(<q>)]) }`; parameters in first-appearance order, selector first (P14); a Rust keyword gets `r#`
  - `pub(crate) const COUNT: usize = <n>;`
  - `pub(crate) static KEYS: [(&str, Text); COUNT]` sorted by key (for `lookup`)
  - `pub(crate) static EN: [Body; COUNT]` (complete) and, per other shipped tag, `pub(crate) static <TAG>: [Option<Body>; COUNT]` (`pt-BR` is `PT_BR`, `zh-Hans` is `ZH_HANS`)
  - `pub(crate) static TABLES: [(&str, Table); <k>]`, English first
  - bodies: `Body::Plain("…")`, `Body::Parts(&[Part::Lit("…"), Part::Str(0), Part::Int(1)])`, `Body::Plural(0, &[(Cat::One, &[…]), (Cat::Other, &[…])])`; brand terms inlined; escapes `\u{200c}` for hidden characters, `\\`, `\"`; ids assigned by sorted key
  - only messages whose file's `targets` include `"rust"`; only `ship` languages (drafts are checked, never built)

- [ ] **Step 1: Write the failing tests**:
  - `test_golden`: `render(load(fixtures/tree))` equals `golden/generated.rs` (`NOTCHLING_UPDATE_GOLDEN=1` rewrites it; read the diff).
  - `test_phone_messages_are_left_out`: no `pairing` module.
  - `test_keyword_function_name`: a key `chat-move` makes `pub fn r#move()`.
  - `test_hidden_character_is_escaped`: `{"‌"}` comes out as `\u{200c}` and the output has no character in Unicode category `Cf`.
  - `test_draft_language_not_built`: a `draft` tag has no table.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_emit_rust`. Expected: FAIL.
- [ ] **Step 3: Implement `render`.** Write it so `rustfmt --edition 2021` leaves it unchanged (four-space indents, one item per line, trailing commas in multi-line arrays).
- [ ] **Step 4: Run** the tests, then `rustfmt --edition 2021 --check tools/locale/tests/golden/generated.rs`. Expected: PASS and no diff.
- [ ] **Step 5: Commit** `tools: the locale Rust writer`.

### Task 5: The ARB writer

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Create: `tools/locale/emit_arb.py`, `tools/locale/tests/test_emit_arb.py`, `tools/locale/tests/golden/app_en.arb`, `tools/locale/tests/golden/app_pt_BR.arb`

**Interfaces:**
- Produces: `def render(tree: Tree, tag: str) -> str` and `def arb_name(key: str) -> str` (`pairing-title` to `pairingTitle`), `def arb_var(var: str) -> str` (`folder_name` to `folderName`), `def file_name(tag: str) -> str` (`app_pt_BR.arb`).
- Output: `"@@locale"` first, then keys sorted; each message as `"<name>": "<ICU>"` and `"@<name>": {"description": "<notes joined by a space>", "placeholders": {…}}` (`description` only when there are notes; placeholders `{"type": "int", "format": "decimalPattern"}` for numbers, `{"type": "String"}` for strings); a plural is `{count, plural, one{…} other{…}}`; JSON with two-space indents, `ensure_ascii=False`, then hidden characters (Unicode `Cf`, `Cc` but newline, `Zl`, `Zp`, variation selectors) written as `\uXXXX`.
- Complete per language: a missing message is filled from the parent (`pt` for `pt-BR`) when the parent ships, then English.

- [ ] **Step 1: Write the failing tests**: `test_golden_en`, `test_golden_pt_br_filled_from_pt_then_english`, `test_mac_messages_are_left_out`, `test_apostrophe_and_hash_pass_through` (`don't` and `# of` arrive unchanged in the ICU string), `test_brand_inlined`, `test_hidden_character_escaped`.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_emit_arb`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `tools: the locale ARB writer`.

### Task 6: The iOS and Android writers (off)

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Create: `tools/locale/emit_xcstrings.py`, `tools/locale/emit_android.py`, `tools/locale/tests/test_emit_native.py`, `tools/locale/tests/golden/Localizable.xcstrings`, `tools/locale/tests/golden/android/values/strings.xml`, `tools/locale/tests/golden/android/values-pt-rBR/strings.xml`

**Interfaces:**
- Produces: `emit_xcstrings.render(tree: Tree) -> str`; `emit_android.render(tree: Tree) -> Dict[str, str]` (relative path to file text); `emit_android.qualifier(tag: str) -> str` (`de` to `values-de`, `pt-BR` to `values-pt-rBR`, `zh-Hans` to `values-b+zh+Hans`).
- Rules from the spec's output table: `%N$@`/`%N$lld` and `variations.plural` (one variable) or `%#@count@` substitutions (several); Android `%N$s`/`%N$d`, `<plurals>`, `\'`, `%%` in a formatted string, notes as `<!-- -->`; Android files only for messages a language has.

- [ ] **Step 1: Write the failing tests**: `test_xcstrings_golden`, `test_android_golden`, `test_qualifiers`, `test_android_apostrophe_and_percent`, `test_xcstrings_plural_with_two_variables_uses_substitution`.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale -k test_emit_native`. Expected: FAIL.
- [ ] **Step 3: Implement.** Only the phone's messages (`targets` has `"phone"`).
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `tools: the iOS and Android locale writers, off`.

### Task 7: The command and the report

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Create: `tools/locale/gen.py`, `tools/locale/report.py`, `tools/locale/tests/test_gen.py`, `tools/locale/tests/test_report.py`

**Interfaces:**
- Produces:
  - `report.Row(tag: str, by_part: Dict[str, float], all: float, same_as_english: int)`; parts `shared`, `consent`, `mac`, `phone`
  - `report.rows(tree: Tree, lang: Optional[str] = None) -> List[Row]`, `report.text(rows) -> str` (the spec's table), `report.markdown(rows) -> str`, `report.missing(tree, tag) -> List[str]` (keys)
  - `gen.main(argv: Optional[List[str]] = None) -> int` with flags: none (check, write outputs in `config.outputs`, print the report), `--check` (write nothing; exit 1 on any problem or stale output, rule `GUARD-LOCALE-GEN` naming the file and `run python3 tools/locale/gen.py`), `--report [--lang TAG] [--missing] [--markdown]`, `--new TAG` (copy `en/` to `TAG/`, add `TAG` to `draft`, refuse an existing folder or a malformed tag), `--prune` (remove from every translation the keys English lacks), `--ios DIR`, `--android DIR`; path flags for tests and probes: `--locales DIR` (default `locales/`), `--rust FILE` (default `crates/locale/src/generated.rs`), `--arb-dir DIR` (default `apps/flutter/lib/l10n/`)
  - `RULE_GEN = "GUARD-LOCALE-GEN"`, `RULE_SUBSET = "GUARD-LOCALE-SUBSET"`, `RULE_MATCH = "GUARD-LOCALE-MATCH"`

- [ ] **Step 1: Write the failing tests** (each over a copy of `fixtures/tree` in a temporary folder):
  - `test_writes_then_check_passes`; `test_check_fails_on_stale_rust` (edit an English value; `--check` exits 1 and stderr has `GUARD-LOCALE-GEN` and the Rust path); `test_check_fails_on_hand_edit` (append a space to the written file).
  - `test_arb_written_only_when_output_on`: with `"outputs": ["rust"]` no ARB file is written or checked; with `["rust", "arb"]` both are.
  - `test_check_reports_every_problem_then_exits_1`: two broken files give two lines.
  - `test_new_copies_and_lists_draft`; `test_new_refuses_existing_and_bad_tag`.
  - `test_prune_removes_unknown_keys_only`.
  - `test_ios_and_android_flags_write_files`.
  - `test_report_numbers` (`report.rows`: `de` with 2 of 4 `mac` messages is `50%`; `same_as_english` counts values equal to English) and `test_markdown_table_header`.
  - `test_missing_messages_never_fail_check`.
- [ ] **Step 2: Run** `python3 -m unittest discover -s tools/locale/tests -t tools/locale`. Expected: FAIL in `test_gen` and `test_report`.
- [ ] **Step 3: Implement.** `gen.py` sets `ROOT = Path(__file__).resolve().parents[2]` as `tools/protocol/gen.py` does.
- [ ] **Step 4: Run** the same command. Expected: PASS (every suite in `tools/locale/tests`).
- [ ] **Step 5: Commit** `tools: gen.py, the locale command and its report`.

### Task 8: The crate, the English source and its tables

**Zone:** maintainer (new crate, `Cargo.toml`, `Cargo.lock`, `locales/languages.json`, `locales/en/consent.ftl`); `locales/en/shared.ftl`, `locales/en/mac/*.ftl` and `generated.rs` are auto once Task 16 lands. Expected hits: `ZONE`.

**Files:**
- Create: `locales/languages.json` (`{"source": "en", "ship": ["en"], "draft": [], "outputs": ["rust"]}`), `locales/en/shared.ftl`, `locales/en/consent.ftl`, `locales/en/mac/units.ftl`
- Create: `crates/locale/Cargo.toml` (package `notchling-locale`, lib `notchling_locale`, workspace version, edition, license, publish), `crates/locale/src/lib.rs`, `crates/locale/src/format.rs`, `crates/locale/src/generated.rs` (written by `gen.py`), `crates/locale/src/format_tests.rs`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: Task 4's output shape.
- Produces, in `notchling_locale`:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub struct Text(pub(crate) u16)` with `pub fn get(self) -> &'static str`, `pub fn english(self) -> &'static str`, `pub fn key(self) -> &'static str`
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Num(pub(crate) i64)` with `From` for `i8 i16 i32 i64 isize u8 u16 u32 u64 usize` (`u64` and `usize` saturate at `i64::MAX`)
  - `pub enum Arg<'a> { Str(&'a str), Num(Num) }`
  - `pub(crate) enum Body`, `pub(crate) enum Part`, `pub(crate) enum Cat { Zero, One, Two, Few, Many, Other }`, `pub(crate) enum Table { Full(&'static [Body]), Part(&'static [Option<Body>]) }`
  - `pub fn format(text: Text, args: &[Arg<'_>]) -> String`
  - `pub fn lookup(key: &str) -> Option<Text>`: binary search in `KEYS`; `None` for a message with variables (P10)
  - `pub fn shipped() -> Vec<&'static str>`
  - `pub use generated::*` (the namespace modules: `t::units`, `t::language`, `t::consent`)
- English seed:
  - `shared.ftl`: brand terms `-brand-notchling = Notchling`, `-brand-claude = Claude`, `-brand-claude-code = Claude Code`, `-brand-codex = Codex`, `-brand-antigravity = Antigravity`; `language-name = English` with a note.
  - `consent.ftl`: a `###` file note saying what belongs here (every word that grants or refuses something; maintainer reviews every change) and no message yet.
  - `mac/units.ftl`: `units-list-separator = ,{" "}` with a note (P21).

- [ ] **Step 1: Write the seed files and generate**: `python3 tools/locale/gen.py`. Expected: `wrote crates/locale/src/generated.rs` and a report line `en 100% …`.
- [ ] **Step 2: Write the failing tests** in `format_tests.rs` (`#[cfg(test)] #[path = "format_tests.rs"] mod format_tests;` in `lib.rs`):
  - `plain_message_is_static`: `units::list_separator()` is `", "` and `units::LIST_SEPARATOR.get()` points to the same address twice (`std::ptr::eq`).
  - `lookup_finds_plain_and_refuses_unknown`: `lookup("units-list-separator") == Some(units::LIST_SEPARATOR)`; `lookup("units-nope")` is `None`.
  - `language_name_is_english`.
  - `num_saturates`: `Num::from(u64::MAX).0 == i64::MAX`.
  - `format_fills_parts`: a test-only table built in the test (`Body::Parts(&[Part::Lit("a "), Part::Str(0), Part::Lit(" b "), Part::Int(1)])`) formats `["x", 7]` as `"a x b 7"` through `format_body(&body, args, &English)` (Task 9 adds grouping; here `7` prints as `7`).
- [ ] **Step 3: Run** `cargo test -p notchling-locale`. Expected: FAIL to compile (no crate code).
- [ ] **Step 4: Implement** `lib.rs` and `format.rs`; `pub(crate) fn format_body(body: &Body, args: &[Arg<'_>], numbers: &dyn Numbers) -> String` where `trait Numbers { fn int(&self, n: i64) -> String; fn category(&self, n: i64) -> Cat; }` (English stand-in until Task 9).
- [ ] **Step 5: Run** `cargo test -p notchling-locale` and `cargo build --workspace --locked`. Expected: PASS (update `Cargo.lock` with `cargo build` once, then `--locked`).
- [ ] **Step 6: Commit** `locale: the notchling-locale crate and the English source`.

### Task 9: Plurals and numbers (ICU4X)

**Zone:** maintainer (`Cargo.toml`, `Cargo.lock`). Expected hits: `ZONE`.

**Files:**
- Modify: `crates/locale/Cargo.toml` (`icu_plurals = "=2.4.0"`, `icu_decimal = "=2.4.0"`, `icu_locale_core = "=2.4.0"`; default features, which include `compiled_data`), `Cargo.lock`
- Create: `crates/locale/src/fmt.rs`, `crates/locale/src/numbers.rs`, `crates/locale/src/fmt_tests.rs`

**Interfaces:**
- Produces:
  - `pub(crate) struct Icu { plurals: icu_plurals::PluralRules, decimal: icu_decimal::DecimalFormatter }` implementing `Numbers`; `pub(crate) fn Icu::new(lang: &str, region: Option<&str>) -> Icu` (locale `lang-REGION` when it parses, else `lang`, else `en`)
  - `pub mod fmt` with `pub fn count(n: impl Into<Num>) -> String`, `pub fn decimal(value: f64, digits: u8) -> String` (P12: `(value * 10^digits).round()` as `i64`, then `Decimal::from(i).multiplied_pow10(-(digits as i16))`; non-finite values print as `"0"`), `pub fn list<S: AsRef<str>>(items: &[S]) -> String` (joined with `units::list_separator()`)

- [ ] **Step 1: Write the failing tests** in `fmt_tests.rs`, over `Icu::new` directly (no global state):
  - `en_groups_with_commas`: `int(1234) == "1,234"`, `int(-5) == "-5"`.
  - `en_with_region_de_groups_with_dots`: `Icu::new("en", Some("DE")).int(1234) == "1.234"`.
  - `english_plural_categories`: `category(1) == Cat::One`, `0`, `2`, `21` are `Other`.
  - `decimal_rounds_to_digits`: `12.34` with 1 digit is `"12.3"`; `0.05` with 1 is `"0.1"`; `f64::NAN` is `"0"`.
  - `list_joins_with_separator`: `["a", "b"]` is `"a, b"`; one item is itself; none is `""`.
- [ ] **Step 2: Run** `cargo test -p notchling-locale fmt_tests`. Expected: FAIL.
- [ ] **Step 3: Implement.** Check the ICU4X 2.4 signatures on docs.rs (`PluralRules::try_new_cardinal`, `DecimalFormatter::try_new`, `Decimal::multiplied_pow10`); a constructor error falls back to English data, never panics.
- [ ] **Step 4: Run** `cargo test -p notchling-locale` and `cargo deny check`. Expected: PASS; no license or advisory error.
- [ ] **Step 5: Measure** `cargo build --release -p notchling-app` before (base commit) and after; note both sizes of `notchling-app` in the commit message. Over 1 MB added: stop and ask the maintainer.
- [ ] **Step 6: Commit** `locale: plural rules and number formats from ICU4X`.

### Task 10: Choosing the language, fallback, the pseudo-language

**Zone:** maintainer. Expected hits: `ZONE`.

**Files:**
- Create: `crates/locale/src/choose.rs`, `crates/locale/src/active.rs`, `crates/locale/src/pseudo.rs`, `crates/locale/src/choose_tests.rs`, `crates/locale/src/pseudo_tests.rs`, `crates/locale/tests/before_start.rs`

**Interfaces:**
- Produces:
  - `pub const LOCALE_ENV: &str = "NOTCHLING_LOCALE"`, `pub const MARK_FALLBACK_ENV: &str = "NOTCHLING_LOCALE_MARK_FALLBACK"`, `pub const PSEUDO: &str = "en-XA"`
  - `pub struct Inputs<'a> { pub env: Option<&'a str>, pub setting: Option<&'a str>, pub system: &'a [String], pub region: Option<&'a str>, pub mark_fallback: bool }`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Choice { pub tag: &'static str, pub pseudo: bool, pub region: Option<String>, pub mark_fallback: bool }`
  - `pub fn choose(inputs: &Inputs<'_>) -> Choice` (pure): the first of `env`, `setting` (unless `"system"`), then each `system` tag that names a shipped language exactly, then by language and script, then by language alone; else `"en"`. `env == "en-XA"` gives `tag: "en", pseudo: true`. Tags are compared after normalizing case and `_` to `-`.
  - `pub(crate) struct Active { table: usize, parent: Option<usize>, pseudo: bool, mark_fallback: bool, numbers: Icu, cache: Box<[OnceLock<&'static str>]> }` with `pub(crate) fn new(choice: &Choice) -> Active`, `pub(crate) fn get(&self, text: Text) -> &'static str`, `pub(crate) fn format(&self, text: Text, args: &[Arg<'_>]) -> String`
  - `static ACTIVE: OnceLock<Active>` (P11); `pub fn apply(choice: &Choice) -> bool` (false when already set); `pub fn start(system: &[String], region: Option<&str>, setting: Option<&str>) -> Choice` (reads both env vars, `choose`, `apply`); `pub fn active() -> &'static str` (the chosen tag, `"en"` before start)
  - `Text::get`, `format` and `fmt::*` read `ACTIVE`, or an English `Active` built once when it was never set
  - `pseudo::transform(text: &str) -> String`: accented letters (`a` to `à`, `c` to `ç`, `e` to `é`, `o` to `ô`, `u` to `û`, `n` to `ñ`, `t` to `ţ`, `s` to `š`, capitals likewise), padded with `~` to 140% of the character count (rounded up), wrapped in `⟦ ⟧`; applied to literal parts only, never to variables
  - fallback marks `‹ ›` around a message that fell back, when `mark_fallback`

- [ ] **Step 1: Write the failing tests**:
  - `choose_tests.rs`, a table test `chooses_over_preferred_lists` with `shipped = ["en", "de", "pt", "pt-BR", "zh-Hans"]` (the test builds `Choice` through a pure `choose_among(inputs, shipped)` that `choose` calls with `shipped()`): `env "de"` wins over everything; `setting "pt-BR"` wins over system; `system ["fr-FR", "de-AT"]` gives `de`; `["zh-Hans-CN"]` gives `zh-Hans`; `["pt_BR"]` gives `pt-BR`; `["fr"]` gives `en`; `[]` gives `en`; `setting "system"` is ignored; `env "en-XA"` gives `en` with `pseudo`.
  - `get_before_start_is_english`: in a fresh process (`crates/locale/tests/before_start.rs`, an integration test, so no other test has called `apply`) `units::list_separator() == ", "` and `active() == "en"`.
  - `fallback_chain_parent_then_english`: an `Active` over a test `Table::Part` where `pt-BR` lacks a message and `pt` has it returns `pt`'s; lacking both returns English; with `mark_fallback` the English one is `‹…›`.
  - `pseudo_tests.rs`: `transform("Send")` starts with `⟦Šéñ`, ends with `~⟧`, and its inside is `ceil(4 * 1.4) == 6` characters; a formatted message keeps its variables unaccented (`"Moved to ⟦…⟧"` style: the folder name `docs` stays `docs`); `get` returns the same `&'static str` twice (cached).
- [ ] **Step 2: Run** `cargo test -p notchling-locale`. Expected: FAIL.
- [ ] **Step 3: Implement.** Leaked strings (`Box::leak`) only in the pseudo and fallback-mark caches, once per message per `Active`.
- [ ] **Step 4: Run** `cargo test -p notchling-locale`. Expected: PASS.
- [ ] **Step 5: Commit** `locale: choose the language once, fall back per message, en-XA`.

### Task 11: Conformance and sample numbers

**Zone:** maintainer (dev-dependencies). Expected hits: `ZONE`.

**Files:**
- Modify: `crates/locale/Cargo.toml` (`[dev-dependencies]` `fluent-bundle = "=0.16.0"`, `fluent-syntax = "=0.12.0"`, `unic-langid = "=0.9.6"`), `Cargo.lock`
- Create: `crates/locale/tests/conformance.rs`, `crates/locale/tests/samples.rs`

**Interfaces:**
- Consumes: `notchling_locale::testing` (a `#[doc(hidden)] pub mod testing` with `pub fn messages(tag: &str) -> Vec<(&'static str, Vec<(&'static str, bool)>)>` (key and its variables, `true` for a number) and `pub fn format_plain_numbers(tag: &str, key: &str, args: &[Arg<'_>]) -> Option<String>` (numbers printed without grouping, for the comparison)).

- [ ] **Step 1: Write the failing tests**:
  - `conformance.rs::every_message_matches_fluent`: for each shipped tag, parse every `.ftl` under `locales/<tag>/` (path from `env!("CARGO_MANIFEST_DIR")/../../locales`) with `fluent_bundle::FluentResource`, add to a `FluentBundle` with `set_use_isolating(false)`, add English `shared.ftl` for the brand terms, and compare `format_plain_numbers` with `format_pattern` for each message over sample arguments (`"x"` for strings; `0, 1, 2, 3, 5, 11, 21, 101, 1000` for numbers). Every `.ftl` must parse with no Fluent error.
  - `samples.rs::every_message_formats_with_sample_numbers` (no panic, non-empty output) and `samples.rs::every_plural_category_used_exists`: for each shipped tag, each variant category a message uses is one `icu_plurals` can return for that language (probe `0..=200` and `1000, 1_000_000`).
- [ ] **Step 2: Run** `cargo test -p notchling-locale --tests`. Expected: FAIL (no `testing` module).
- [ ] **Step 3: Implement** `testing`.
- [ ] **Step 4: Run** `cargo test -p notchling-locale` and `cargo deny check`. Expected: PASS.
- [ ] **Step 5: Commit** `locale: the tables agree with Fluent`.

### Task 12: `platform::locale`

**Zone:** maintainer (`crates/platform/`, its `Cargo.toml`). Expected hits: `ZONE`.

**Files:**
- Create: `crates/platform/src/locale.rs`, `crates/platform/src/locale_tests.rs`
- Modify: `crates/platform/src/lib.rs` (`pub mod locale;`), `crates/platform/Cargo.toml` (`objc2-foundation` features gain `"NSLocale"`), `Cargo.lock` if it changes

**Interfaces:**
- Produces:
  - `pub fn preferred() -> crate::Result<Vec<String>>`: macOS `NSLocale::preferredLanguages()`; Linux and other Unix `from_env(&|k| std::env::var(k).ok())`; Windows `Err(Error::Unsupported("reading the preferred languages"))`
  - `pub fn region() -> Option<String>`: macOS `NSLocale::currentLocale().countryCode()`; elsewhere the region of the first `preferred()` tag
  - pure, every system: `pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Vec<String>` (`LANGUAGE` split on `:`, then `LC_ALL`, `LC_MESSAGES`, `LANG`, each through `posix_tag`; duplicates dropped); `pub fn posix_tag(value: &str) -> Option<String>` (`de_DE.UTF-8` to `de-DE`, `pt_BR@euro` to `pt-BR`, `C`, `POSIX` and empty to `None`); `pub fn region_of(tag: &str) -> Option<String>` (`de-AT` to `AT`, `zh-Hans-CN` to `CN`, `de` to `None`)

- [ ] **Step 1: Write the failing tests** in `locale_tests.rs`: `posix_tags`, `language_list_order_and_duplicates` (`LANGUAGE=de:fr`, `LANG=de_DE.UTF-8` gives `["de", "fr", "de-DE"]`), `region_of_tags`, and on macOS only `#[cfg(target_os = "macos")] preferred_is_not_empty_on_a_mac`.
- [ ] **Step 2: Run** `cargo test -p notchling-platform locale`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-platform` and `cargo clippy -p notchling-platform --all-targets -- -D warnings`. Expected: PASS.
- [ ] **Step 5: Commit** `platform: the preferred languages and the region`.

### Task 13: Every entry point chooses at start

**Zone:** maintainer for `crates/app/Cargo.toml` (`notchling-locale = { path = "../locale" }`); the Rust files are auto. Expected hits: `ZONE` (Cargo.toml).

**Files:**
- Create: `crates/app/src/language.rs`, `crates/app/src/language_tests.rs`
- Modify: `crates/app/Cargo.toml`, `crates/app/src/lib.rs` (`pub mod language;`), `crates/app/src/mac/shell.rs::run` (first line, before `history::apply`), `crates/app/src/mac/render.rs::run` (first line), `crates/app/src/machines/window.rs::run_command` (both arms, first line)

**Interfaces:**
- Produces:
  - `pub fn start_for_run() -> notchling_locale::Choice`: `notchling_locale::start(&preferred, region.as_deref(), None)` with `preferred = notchling_platform::locale::preferred().unwrap_or_default()` (the Settings Language row's value comes in L7)
  - `pub fn start_for_render() -> notchling_locale::Choice`: `start(&[], None, None)`, so a render is English unless `NOTCHLING_LOCALE` says otherwise
  - pure: `pub fn system_languages(render: bool, preferred: notchling_platform::Result<Vec<String>>) -> Vec<String>` (empty for a render or an error)

- [ ] **Step 1: Write the failing tests** in `language_tests.rs`: `render_ignores_system_languages`, `error_reads_as_none`, `run_keeps_order`.
- [ ] **Step 2: Run** `cargo test -p notchling-app language`. Expected: FAIL.
- [ ] **Step 3: Implement** and add the three calls. The benches run through `--render`, so they are covered.
- [ ] **Step 4: Run** `cargo test -p notchling-app`, `cargo clippy --workspace --all-targets -- -D warnings` (on the Mac), then `NOTCHLING_LOCALE=en-XA target/release/notchling-app --render settings --out /tmp/l1-xa.png` after `cargo build --release -p notchling-app`. Expected: PASS; the picture looks like today's (nothing has moved yet).
- [ ] **Step 5: Commit** `app: choose the language at every entry point`.

### Task 14: Guard rules for the generator

**Zone:** maintainer (`tools/guard/`, `docs/rules.md`). Expected hits: `ZONE`, `CAP-PROCESS` (the probes run `gen.py` through `subprocess`, as `_guard_protocol_gen` does).

**Files:**
- Modify: `tools/guard/guard/rules.py` (probes `_guard_locale_gen`, `_guard_locale_subset`, `_guard_locale_match`, registered under their ids), `docs/rules.md` (three rows in "The guard itself", the spec's text)
- Create: `tools/guard/tests/test_locale_probes.py`

**Interfaces:**
- Consumes: `gen.main` flags `--locales`, `--rust`, `--arb-dir`, `--check` (Task 7).
- Produces: each probe copies `tools/locale/tests/fixtures/tree` to a temporary folder, runs `gen.py` into temporary outputs, then: GEN edits an English value and expects `--check` to exit 1 with `GUARD-LOCALE-GEN`; SUBSET adds `x-y = { other-key }` and expects `GUARD-LOCALE-SUBSET`; MATCH adds `{ $nope }` to a `de` value and expects `GUARD-LOCALE-MATCH`.

- [ ] **Step 1: Write the failing tests**: `test_each_locale_probe_fires` (each of the three returns `True`) and `test_rules_md_lists_them` (`rules.check(Path("docs/rules.md"))` has no problem naming a `GUARD-LOCALE` id).
- [ ] **Step 2: Run** `PYTHONPATH=tools/guard python3 -m unittest discover -s tools/guard/tests -t tools/guard -k test_locale_probes`. Expected: FAIL.
- [ ] **Step 3: Implement** the probes and the rows.
- [ ] **Step 4: Run** the test, then `PYTHONPATH=tools/guard python3 -m guard rules`. Expected: PASS, no `GUARD-RULES` problem.
- [ ] **Step 5: Commit** `guard: GUARD-LOCALE-GEN, -SUBSET and -MATCH`.

### Task 15: The literal lint, the consent warning, generated-file sizes

**Zone:** maintainer (`tools/`, `docs/rules.md`). Expected hits: `ZONE`.

**Files:**
- Create: `tools/locale/areas.json`, `tools/guard/guard/locale_lint.py`, `tools/guard/tests/test_locale_lint.py`
- Modify: `tools/guard/guard/lint.py` (register the two rules from `locale_lint`; `Rule.probe_files: Tuple[Tuple[str, str], ...] = ()`; `GENERATED_FILES` gains `crates/locale/src/generated.rs`; `GENERATED_DIRS = ("apps/flutter/lib/l10n/",)` exempt from both size rules), `tools/guard/guard/rules.py` (`_lint_probe` writes `probe_files` too, P15), `docs/rules.md` (two lint rows; `LINT-FILE-SIZE` and `-WARN` rows name the new exemptions)

**Interfaces:**
- `areas.json`: `{"areas": [{"name": "<area>", "paths": ["<glob>", …], "moved": false}, …]}` with every area from the start, so each step flips one line (P20):
  - `phone`: `apps/flutter/lib/**`
  - `settings`: `crates/app/src/sheets/settings*.rs`, `crates/app/src/sheets/settings_view/**`, `crates/app/src/sheets/settings_hooks_view.rs`, `crates/app/src/sheets/hook_rows.rs`, `crates/app/src/sheets/agents*.rs`, `crates/app/src/activity/**`
  - `chat-sidebar`, `chat-conversation`, `chat-reply-box`, `chat-details`, `chat-window`: the file lists in `2026-11-14-locale-l4-chat.md`, Task 0
  - `notch`: `crates/app/src/notch/**`
  - `accounts`, `vault`, `pages`, `search`, `welcome`, `machines`, `tools`, `switches`, `phone-pairing` (`crates/app/src/phone/**`), `inbox` (`crates/app/src/inbox/**`), `shell` (`crates/app/src/mac/**`, `crates/app/src/menu.rs`, `crates/app/src/dock.rs`, `crates/app/src/acting.rs`), `kit` (`crates/kit/src/**`): `crates/app/src/<name>/**` unless named
- `locale_lint.py`:
  - `LINT-LOCALE-LITERAL`, paths `crates/app/src/**/*.rs`, `crates/kit/src/**/*.rs`, `apps/flutter/lib/**/*.dart`; excludes `**/*_tests.rs`, `**/tests_*.rs`, `**/tests/**`, `**/renders.rs` and `crates/app/src/chat/bench_fixture.rs` (fixture data for `--render` and the benches), `crates/app/src/testutil.rs`, `apps/flutter/lib/l10n/**`, `apps/flutter/lib/core/protocol/generated.dart`; active only for a path in a moved area of `repo/tools/locale/areas.json`; in Rust, stops at a line that is exactly `#[cfg(test)]`. A finding is a string literal (read with `guard.source`, strings kept) that has a word of two or more letters and a space, or starts with a capital letter and has a lowercase letter. Exempt lines: they hold `.part(`, `.part_at(`, `.part_inside(`, `.window_region(`, `.id(`, `ElementId`, `eprintln!`, `println!`, `debugPrint(`, `log::`, `tracing::`, `env::var`, `_ENV`, `#[`, `import `, `Key(`. Bad: `("crates/app/src/notch/a.rs", '…div().child("Reply to Claude")\n')` with `probe_files` holding an `areas.json` where `notch` is moved. Good: the same line in an area not moved, and `div().child(t::notch::reply())`.
  - `LINT-LOCALE-CONSENT` (`warn=True`), paths `locales/en/**/*.ftl`, excluding `locales/en/consent.ftl`: a message value that starts with `Allow`, `Deny`, `Always Allow`, `Approve`, `Pair`, `Trust`, `Install` or `Sign In`. Bad `("locales/en/mac/chat.ftl", "chat-allow = Allow\n")`.

- [ ] **Step 1: Write the failing tests** in `test_locale_lint.py`: `test_literal_in_moved_area`, `test_area_not_moved_is_quiet`, `test_each_exemption` (one `subTest` per exempt token), `test_rust_test_module_is_skipped`, `test_dart_literal`, `test_consent_warning`, `test_generated_locale_files_have_no_size_limit` (`lint.run` on a 5,000-line `crates/locale/src/generated.rs` and `apps/flutter/lib/l10n/app_localizations.dart` finds nothing).
- [ ] **Step 2: Run** `PYTHONPATH=tools/guard python3 -m unittest discover -s tools/guard/tests -t tools/guard -k test_locale_lint`. Expected: FAIL.
- [ ] **Step 3: Implement**, then remove `LINT-FILE-SIZE|apps/flutter/lib/l10n/app_localizations.dart|over 1000 lines` from `tools/guard/lint-baseline.json` (it shrinks).
- [ ] **Step 4: Run** the tests, `python3 -m guard lint` (0 new: no area is moved), `python3 -m guard lint --warnings`, `python3 -m guard rules`. Expected: PASS.
- [ ] **Step 5: Commit** `guard: the locale literal lint and the consent warning`.

### Task 16: Zones, CODEOWNERS, CI

**Zone:** maintainer (`tools/guard/zones.json`, `.github/`, `tools/test-python.sh`). Expected hits: `ZONE`.

**Files:**
- Modify: `tools/guard/zones.json` (in `auto`, after `crates/engine/src/search/**` and before `!crates/**/Cargo.toml`: `"locales/**"`, `"!locales/languages.json"`, `"!locales/*/consent.ftl"`, `"crates/locale/src/generated.rs"`), `.github/CODEOWNERS` (regenerated), `.github/workflows/guard.yml` (a step named `Locale` after `Protocol types`: `python3 tools/locale/gen.py --check`, `python3 -m unittest discover -s tools/locale/tests -t tools/locale`, `python3 tools/locale/gen.py --report --markdown >> "$GITHUB_STEP_SUMMARY"`), `tools/test-python.sh` (`for suite in guard agents release protocol locale`)

- [ ] **Step 1: Check the zone** for five paths: `PYTHONPATH=tools/guard python3 -c "from guard.zones import load; z=load('tools/guard/zones.json'); print([z.in_zone(p) for p in ['locales/en/mac/chat.ftl','locales/de/shared.ftl','locales/en/consent.ftl','locales/languages.json','crates/locale/src/generated.rs','crates/locale/src/lib.rs']])"`. Expected: `[True, True, False, False, True, False]`.
- [ ] **Step 2: Regenerate** `python3 -m guard codeowners --write`, then `--check`. Expected: no error.
- [ ] **Step 3: Run** `tools/test-python.sh`, `python3 -m guard rulesets`, `python3 -m guard pins`, `python3 -m unittest discover -s tools/guard/tests -t tools/guard -k test_workflows`. Expected: PASS.
- [ ] **Step 4: Commit** `guard: locales in the zone, the Locale CI step`.

### Task 17: The notes

**Zone:** maintainer (every `AGENTS.md`, `CONTRIBUTING.md`, `docs/feature-map.md`); `docs/translating.md` is auto. Expected hits: `ZONE`.

**Files:**
- Create: `crates/locale/AGENTS.md` (the crate's files and what a change could break: ids are positions in sorted keys, so never hand-edit `generated.rs`; `ACTIVE` is set once; `lookup` finds plain messages only; the pseudo and fallback caches leak one string per message; the conformance test is the referee for the parser), `docs/translating.md` (the spec's list for it)
- Modify: `crates/AGENTS.md` (crate table row `locale` (`notchling-locale`) | words from `locales/`, `generated.rs` by `tools/locale/gen.py` | library | app; the draw rule; `choose` at every entry point; `NOTCHLING_LOCALE`, `NOTCHLING_LOCALE_MARK_FALLBACK`), `AGENTS.md` (style: one sentence, "Translator notes go in `.ftl` files under `locales/`, never in code."; build list: `python3 tools/locale/gen.py`), `tools/AGENTS.md` (`tools/locale/gen.py::subset` and `::outputs` notes), `CONTRIBUTING.md` (the spec's "Translating Notchling" section), `docs/feature-map.md` (row "Localization": `locales/`, `tools/locale/`, `crates/locale/`, `crates/platform/src/locale.rs`, `crates/app/src/language.rs`; tests by path; zone mixed)
- Contract for designs that build on L1 (in `crates/locale/AGENTS.md`, a section "Registries"): Settings search entries hold `Text` constants from `t::settings` and live in the app (P5 to P7); log kinds are `logs-event-<kind>`, `-about`, `-why` in `mac/logs.ftl`, read through `t::lookup` (P8); neither copies text.

- [ ] **Step 1: Write them.**
- [ ] **Step 2: Run** `python3 -m guard featuremap`, `python3 -m guard claudemd`, `python3 tools/strip_comments.py --check`. Expected: no problem.
- [ ] **Step 3: Commit** `docs: localization notes, the translators' guide`.

### Checkpoint L1

- [ ] `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` (Mac); `cargo test --workspace --locked`; `cargo deny check`.
- [ ] `tools/test-python.sh`; `python3 tools/locale/gen.py --check`; `PYTHONPATH=tools/guard python3 -m guard lint` (0 new), `rules`, `codeowners --check`, `featuremap`, `claudemd`; `python3 tools/strip_comments.py --check`.
- [ ] `cd apps/flutter && flutter analyze && flutter test` (unchanged phone; `outputs` has no `arb`).
- [ ] `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1`: counts as before.
- [ ] The release binary sizes from Task 9 in the PR description. Push; PR against `merge/settings-batch`.
