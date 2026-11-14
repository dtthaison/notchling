# Search P1: Tier 0 and Settings Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Full-text search folds `đ` like every other accent, and the Settings sheet gets a search field that finds any setting or place by title, description, search words or Vietnamese alias, jumps to it and flashes it.

**Architecture:** A pure shared module `crates/engine/src/search/query/` starts with `fold.rs` (lowercase, strip marks exactly as SQLite's `remove_diacritics 2`, plus `đ`→`d`) and `matcher.rs` (all words as prefixes, field weights). `search.db` moves to schema 2: FTS5 gets folded text in a contentless table, the original stays in `docs.body`. A settings registry in the app holds the generated Fluent `Text` constants, aliases and places (coordinator's call: the engine never depends on `notchling-locale`), and the app matches it on the keystroke, with no daemon. A second pull request gives every Ask key an effect text and shows it in the agent's Ask item.

**Tech Stack:** Rust; rusqlite FTS5 (bundled); GPUI 0.2.2 and `crates/kit`; `crates/locale` and `tools/locale/gen.py` (localization step L1).

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` (sections 1, 3 "Full text", 6 "Snippets", 8, 10, 12 P1; answers 2; call 1). Words follow `docs/superpowers/specs/2026-11-13-localization-design.md` on `origin/agent/locale-design` ("Files and keys", "The registries that are coming"). Plan set and calls: `2026-11-14-search-plans-index.md`.

**Branches:** Tasks 1–7 on `agent/search-p1-tier0-settings`, Task 8 on `agent/search-p1-effect-texts`, both from `merge/settings-batch` after L1 (and, preferably, L3) merged into it. The two pull requests are independent.

**Depends on:** L1 merged (no fallback, coordinator's call of 2026-11-14), L3 preferred; decision 0020 (`KeySpec` classes, on `merge/settings-batch`).

## Global Constraints

- No comments in code, doc comments included; `python3 tools/strip_comments.py --check` is clean.
- Files under 1,000 lines, 600 with a warning (`LINT-FILE-SIZE`); tests go in `<module>_tests.rs` or `tests_<area>.rs`.
- No file I/O, process or daemon call in a model compute or a draw. Settings matching is pure over compiled-in tables.
- User-facing words through Fluent in `locales/en/mac/settings.ftl`, keys `settings-<kebab id>`, `-about`, `-words`, `-<value>` for choices (no attributes); run `python3 tools/locale/gen.py`. Titles in Title Case as the sheet shows them today. Vietnamese only as aliases (data), never UI text.
- Views use kit tokens (`Ink`, `TypeRole`, `metric`, `Motion`), never raw colors or sizes; every new view gets a `.part("…")` tag; GPUI code only under `#[cfg(target_os = "macos")]`.
- No `#[ignore]` added (it trips `WEAK-SKIP`); the render is checked by eye.
- Stage files by path; never `git add -A` (`.claude/worktrees/` is untracked).

## Review Focus

1. A Vietnamese query with `Đ` capitalized ("Đăng nhập") and one with stacked marks typed as combining characters ("đăng") find the same setting as "dang nhap". (Task 1 test `stacked_and_precomposed_fold_alike`.)
2. A snippet whose text has `đ` gets its highlight on the original characters, not shifted by the fold. (Task 2 test `highlights_mark_the_accented_original`.)
3. An old `search.db` (schema 1) opens: it is rebuilt once and the sessions come back on the next refresh, with their original text. (Task 2 test `a_schema_1_file_is_rebuilt_and_keeps_originals`.)
4. A conditional row (Switch at, shown only while a limit mode is Notify and Switch) is searchable even while hidden; Return turns nothing on, it jumps to the section and says nothing is there to flash. (Task 6 test `a_hidden_row_jumps_to_its_section`.)
5. Esc with text clears the field; Esc with an empty field closes the sheet; ⌘F while the field has focus selects its text. (Task 6 test `esc_clears_then_closes`.)

---

### Task 1: `query::fold`

**Zone:** auto (`crates/engine/src/search/**`, new files). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/query/mod.rs`, `crates/engine/src/search/query/fold.rs`, `crates/engine/src/search/query/fold_table.rs`
- Test: `crates/engine/src/search/query/fold_tests.rs`
- Modify: `crates/engine/src/search/mod.rs` (`pub mod query;`)

**Interfaces:**
- Produces:
  - `pub fn fold_char(c: char) -> Option<char>`: `None` for a mark SQLite drops; `'đ' | 'Đ'` → `'d'`.
  - `pub fn fold(text: &str) -> String`
  - `pub fn words(text: &str) -> Vec<String>`: folded, split on every char that is not alphanumeric after folding, empty pieces dropped.
  - `fold_table.rs`: `pub(crate) const TABLE: &[(char, char)]`, sorted by the first char, holding every code point below U+2500 whose SQLite fold differs from Rust's `to_lowercase`; marks SQLite drops are in `pub(crate) const DROPPED: &[(char, char)]` as inclusive ranges.

- [ ] **Step 1: Write the failing tests** in `fold_tests.rs`:
  - `fold_matches_sqlite_below_0x2500`: for each code point `c` from U+0080 to U+24FF that SQLite's `unicode61 remove_diacritics 2` keeps inside a token, insert the row `q{c}q` into an in-memory `fts5(x, tokenize='unicode61 remove_diacritics 2')`, read its term from `fts5vocab(…, 'row')`, and assert it equals `format!("q{}q", fold_char(c).map(String::from).unwrap_or_default())`, with SQLite's own `đ` (kept) replaced by `d`. On failure, print the whole expected `TABLE` and `DROPPED` as Rust source so the implementer pastes it.
  - `d_stroke_folds_to_d`: `fold("Đăng nhập") == "dang nhap"`, `fold("ĐỒNG") == "dong"`.
  - `stacked_and_precomposed_fold_alike`: `fold("đa\u{0306}ng") == fold("đăng")`.
  - `words_split_on_punctuation`: `words("phím-tắt, Login!") == ["phim", "tat", "login"]`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::fold`. Expected: FAIL, module missing.
- [ ] **Step 3: Implement** `fold.rs`: binary search in `TABLE`, then `DROPPED`, else `to_lowercase` (first char), then the `đ` rule. Fill `fold_table.rs` from the test's printed output.
- [ ] **Step 4: Run** the same command. Expected: PASS.
- [ ] **Step 5: Commit** `search: query::fold, SQLite's diacritic fold plus đ`.

### Task 2: `search.db` schema 2, folded

**Zone:** auto. CAP/WEAK: none expected (no new `fs::` line; schema SQL only). Existing tests edited: none; the tests in `fts.rs` stay as they are and must still pass.

**Files:**
- Modify: `crates/engine/src/search/fts.rs` (`SCHEMA = 2`; `setup`, `put`, `sessions`, `rank`, `match_expr`), `crates/engine/src/search/snippet.rs` (`highlights`), `crates/engine/src/search/rank.rs` (`rank`'s substring fallback compares folded text)
- Test: `crates/engine/src/search/tests_fold.rs` (add `#[cfg(test)] mod tests_fold;` in `search/mod.rs`)

**Interfaces:**
- Consumes: `query::fold`, `query::words` (Task 1).
- Produces: schema 2:
  ```sql
  CREATE TABLE docs (id INTEGER PRIMARY KEY, key TEXT NOT NULL UNIQUE, modified REAL NOT NULL,
                     meta TEXT NOT NULL, body TEXT NOT NULL);
  CREATE VIRTUAL TABLE fts USING fts5(title, folder, body, content='', contentless_delete=1,
                     tokenize = 'unicode61 remove_diacritics 2');
  ```
  `put` writes `fold()` of title, folder and text into `fts` and the original text into `docs.body`; `sessions()` reads `docs.body`; `match_expr` folds each word. Public signatures unchanged.

- [ ] **Step 1: Write the failing tests** in `tests_fold.rs`:
  - `a_stored_dang_nhap_is_found_by_unaccented_words`: put "Sửa lỗi đăng nhập" (title) and a body "Lỗi đăng nhập khi mở app"; `rank(["dang", "nhap"])`, `rank(["đăng"])` and `rank(["DANG"])` each return it.
  - `sessions_keep_the_original_text`: after put and reopen, `sessions()[0].text` contains "đăng".
  - `highlights_mark_the_accented_original`: `highlights("Sửa lỗi đăng nhập", &["dang"])` is the byte range of "đăng".
  - `the_substring_fallback_folds`: `rank(&["dang"], …)` from `rank.rs` over an `IndexedSession` whose text is "đăng nhập" finds it.
  - `a_schema_1_file_is_rebuilt_and_keeps_originals`: a file with `user_version = 1` and schema 1's tables opens, is empty, then a put and a reopen keep "đăng".
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::tests_fold`. Expected: FAIL.
- [ ] **Step 3: Implement.** `IndexedSession`'s lowered fields become folded (`fold` instead of `lowercase`) so the fallback and snippets agree; `highlights` maps each char of the snippet through `fold_char` and keeps a byte offset per kept char.
- [ ] **Step 4: Run** `cargo test -p notchling-engine search::`. Expected: PASS, the old tests included.
- [ ] **Step 5: Commit** `search: search.db schema 2 folds đ in the index, the query and highlights`.

### Task 3: `query::matcher`

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/query/matcher.rs`
- Test: `crates/engine/src/search/query/matcher_tests.rs`

**Interfaces:**
- Consumes: `fold`, `words` (Task 1).
- Produces:
  - `pub struct MatchDoc { pub id: String, fields: Vec<(Vec<String>, f32)> }` with `pub fn new(id: &str, fields: &[(&str, f32)]) -> MatchDoc` (folds and splits once).
  - `pub struct Hit { pub index: usize, pub score: f32 }`
  - `pub fn rank(query: &str, docs: &[MatchDoc]) -> Vec<Hit>`: every query word must be a prefix of a word in some field; a word scores the highest weight among fields it matches; ties keep input order; an empty query gives nothing.
  - `pub const TITLE: f32 = 3.0; pub const WORDS: f32 = 2.0; pub const ABOUT: f32 = 1.0;`

- [ ] **Step 1: Write the failing tests:** `words_are_prefixes_and_all_required` ("app dar" matches "Appearance"/"dark", "ark" doesn't); `title_beats_description`; `aliases_fold_like_text` ("phim tat" and "phím tắt" both match a doc whose alias is "phím tắt"); `empty_query_matches_nothing`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::matcher`. Expected: FAIL.
- [ ] **Step 3: Implement** in `matcher.rs`.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: query::matcher for small in-memory corpora`.

### Task 4: The registry's words

**Zone:** auto once L1 put `locales/**` (but `languages.json` and `consent.ftl`) and `crates/locale/src/generated.rs` in the zone. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create or modify: `locales/en/mac/settings.ftl` (L3 creates it; if L3 has not merged, create it)
- Regenerate: `crates/locale/src/generated.rs` with `python3 tools/locale/gen.py`

**Interfaces:**
- Produces, per registry id (Task 5's list), in `locales/en/mac/settings.ftl` (coordinator's call, 2026-11-14; no attributes, no `shared.ftl`):
  - `settings-<kebab id>`: the title exactly as the sheet shows it today ("Show agents in the notch", "Logins and Lanes"; "Accounts" for `window-accounts`).
  - `settings-<kebab id>-about`: one plain sentence; for a key, 0020's `about` is its first draft.
  - `settings-<kebab id>-words`: English search words, comma separated, under the group note the localization design gives ("Search words, separated by commas…").
  - `settings-<kebab key>-<value>` for each choice the sheet shows (`settings-appearance-dark`, `settings-mode-while-working`).
  - Kebab case: `.` becomes `-`, an upper-case letter becomes `-` and its lower case (`searchShortcut` → `search-shortcut`, `daemon.accounts` → `daemon-accounts`).
  - L3 moves only the words the sheet shows today (titles and choices). If L3 has merged, those exist and this task writes only `-about` and `-words` (and the titles of places with no sheet row: `window-*`, `hooks` if missing). If L3 has not merged, this task writes the titles and choice labels too, with the ids L3 uses; the sheet keeps its literals until L3, and Task 5's test holds the two equal.

- [ ] **Step 1: Write** the messages; run `python3 tools/locale/gen.py`.
- [ ] **Step 2: Run** `python3 tools/locale/gen.py --check` and `cargo build -p notchling-app`. Expected: clean; `t::settings::APPEARANCE_ABOUT` and the rest exist.
- [ ] **Step 3: Commit** `settings: titles, lines and search words for Settings search`.

### Task 5: The settings registry, in the app

**Zone:** auto (`crates/app/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/app/src/sheets/settings_entries.rs`
- Test: `crates/app/src/sheets/settings_entries_tests.rs`

**Interfaces:**
- Consumes: the generated `notchling_locale::Text` constants (L1: `Text::get() -> &'static str`, `Text::english() -> &'static str`); `notchling_engine::settings::{KEYS, spec}`, `settings::agents::Access`.
- Produces:
  ```rust
  pub enum Window { Accounts, Vault, Phone, Machines }
  pub enum Place { Section(&'static str), Hooks, Window(Window) }
  pub struct Entry { pub id: &'static str, pub key: Option<&'static str>, pub title: Text, pub about: Text,
                     pub words: Text, pub choices: &'static [Text], pub aliases: &'static [&'static str], pub place: Place }
  pub const ENTRIES: &[Entry];
  pub const NOT_SHOWN: &[(&str, &str)];
  pub fn entry(id: &str) -> Option<&'static Entry>;
  pub fn words(entry: &Entry) -> Vec<&'static str>;          // `words.get()` split on `,`, trimmed
  pub fn english_words(entry: &Entry) -> Vec<&'static str>;  // `words.english()` split the same way
  ```
  Section ids: `general`, `search`, `daemon`, `awake`, `notch`, `claude-code`, `limits`, `codex`, `chat`. Entry ids: each shown key by its key name (`appearance`, `searchShortcut`, `mode`, `keepDisplayOn`, `continueAfterSleep`, `hideNotch`, `notchWidth`, `showPet`, `showLastReply`, `replyFromNotch`, `answerFromNotch`, `keepPages`, `limitMode`, `codexLimitMode`, `antigravityLimitMode`, `limitSwitchAt`, `codexNetwork`, `chatHistory`), `daemon.acting`, `daemon.bridge`, `daemon.accounts`, `daemon.vault`, `daemon.install`, `hooks`, `window.accounts`, `window.vault`, `window.phone`, `window.machines`. `NOT_SHOWN`: `("chatInspector", "set by the Details Panel button in Chat")`, `("chatPageWidth", "set by dragging the page panel's edge")`. Aliases include `appearance`: `["giao diện", "chế độ tối", "tối", "sáng"]`; `searchShortcut`: `["phím tắt", "tìm kiếm"]`; `daemon.accounts` and `window.accounts`: `["đăng nhập", "tài khoản"]`.

- [ ] **Step 1: Write the failing tests:**
  - `ids_match_the_committed_list`: the ids in order equal a `const IDS: &[&str]` written in the test.
  - `every_key_entry_names_a_real_key`; `ids_are_unique`.
  - `every_free_or_ask_key_is_shown_or_listed`: each `KEYS` entry with `Access::Free | Access::Ask` is an entry's `key` or in `NOT_SHOWN`, with a non-empty reason.
  - `every_entry_has_its_messages`: `title`, `about` and `words` are non-empty in English.
  - `registry_titles_are_the_row_titles`: for every row `sheets::settings::sections` returns with all conditional rows forced on, `entry.title.get() == row.title`; the same for daemon rows and the hooks row.
- [ ] **Step 2: Run** `cargo test -p notchling-app sheets::settings_entries`. Expected: FAIL.
- [ ] **Step 3: Implement** `settings_entries.rs`.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `settings: Settings search registry with places and aliases`.

### Task 6: The Settings search model

**Zone:** auto (`crates/app/**`). CAP/WEAK: none. Existing tests edited: none; `Section` and `Row` gain fields, the inline tests in `sheets/settings.rs` still compile and pass unchanged.

**Files:**
- Create: `crates/app/src/sheets/settings_search.rs`
- Test: `crates/app/src/sheets/settings_search_tests.rs`
- Modify: `crates/app/src/sheets/settings.rs` (`Section` gains `pub id: &'static str`; `Row` gains `pub entry: &'static str` and `pub about: &'static str`, from the registry)

**Interfaces:**
- Consumes: `query::matcher::{MatchDoc, rank, TITLE, WORDS, ABOUT}`, `snippet::highlights` (fold-aware), `settings_entries::{ENTRIES, Place, words, english_words}` (Task 5).
- Produces:
  - `pub struct Found { pub entry: &'static str, pub title: &'static str, pub about: &'static str, pub section: &'static str, pub marks: Vec<Range<usize>> }`
  - `pub struct SettingsSearch { pub query: String, pub found: Vec<Found>, pub selected: usize }` with `fn new() -> Self`, `fn set_query(&mut self, text: &str)`, `fn move_by(&mut self, by: i32)`, `fn press_return(&self) -> Jump`, `fn press_escape(&mut self) -> Escape`.
  - `pub enum Jump { None, Row { section: &'static str, entry: &'static str, shown: bool }, Hooks, Window(Window) }`
  - `pub enum Escape { Cleared, Close }`
  - The match fields: title at `TITLE`; words, English words, choice labels and aliases at `WORDS`; about at `ABOUT`.

- [ ] **Step 1: Write the failing tests:**
  - `dark_finds_appearance`; `phim_tat_and_phim_tat_accented_find_the_shortcut` ("phím tắt" and "phim tat" → `searchShortcut` first); `login_and_dang_nhap_find_logins_and_the_accounts_window` (both → `daemon.accounts` and `window.accounts` in the first two).
  - `return_jumps_to_the_row` (`Jump::Row { section: "general", entry: "appearance", shown: true }`); `a_window_place_opens_the_window`; `a_hidden_row_jumps_to_its_section` (`limitSwitchAt` with no switching mode → `shown: false`).
  - `esc_clears_then_closes`; `selection_keeps_its_entry_when_the_list_changes`.
- [ ] **Step 2: Run** `cargo test -p notchling-app sheets::settings_search`. Expected: FAIL.
- [ ] **Step 3: Implement.** Build the `MatchDoc`s once (`OnceLock`); `shown` comes from the `sections` the view passes in.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `settings: search model over the registry`.

### Task 7: The field, the results and the flash

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/app/src/sheets/settings_search_view.rs` (macOS)
- Modify: `crates/app/src/sheets/settings_view.rs` (pin the field under the header; ⌘F focuses it; while it has text the result list replaces the sections; every row shows its `about` under its title; scroll a jumped row to a third of the height and wash it with `Ink::Accent` for 1.2 s through a `Motion` token), `crates/app/src/sheets/renders.rs` (a `ViewSpec` `settings-search`, query from `NOTCHLING_RENDER_SETTINGS_QUERY`, default `dark`), `crates/app/src/mac/views.rs` (its arm)

**Interfaces:**
- Consumes: `SettingsSearch`, `Jump`, `Escape` (Task 6).
- Produces: parts `"Settings Search Field"`, `"Settings Search Results"`, `"Settings Row Flash"`.

- [ ] **Step 1: Build** `cargo build --release -p notchling-app`. Expected: clean.
- [ ] **Step 2: Render** `NOTCHLING_RENDER_FORCE_FRAMES=1 target/release/notchling-app --render settings-search --out /tmp/ss.png` and `NOTCHLING_RENDER_SETTINGS_QUERY="phim tat"` the same, then `--render settings --out /tmp/s.png`, in light and dark (`NOTCHLING_RENDER_APPEARANCE`). Look at each: field pinned, highlights on the accented title, descriptions under every row, no clipped text.
- [ ] **Step 3: Run** `cargo test -p notchling-app` and `cargo clippy --workspace --all-targets -- -D warnings` on the Mac. Expected: PASS, clean.
- [ ] **Step 4: Run** `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1`. Expected: the counts it holds still pass (the sheet draws inside Chat).
- [ ] **Step 5: Commit** `settings: search field, results, jump and flash`.

**Checkpoint A (end of the first pull request):** `cargo test --workspace --locked`, `cargo fmt --all --check`, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`, `python3 tools/locale/gen.py --check`. Open the pull request from `agent/search-p1-tier0-settings`; say whether L3 had merged (which messages Task 4 wrote), and attach the renders.

### Task 8: Effect texts and the Ask item (second pull request)

**Zone:** maintainer: `crates/engine/src/settings/**`, `crates/notchlingd/**`, `docs/decisions/**`, `crates/AGENTS.md`, `docs/feature-map.md`. CAP/WEAK: none expected. Existing tests edited: the agents tests file gains a test; if a test in `settings_ops_tests.rs` pins the old detail text, it changes, and the pull request names it.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs` (`KeySpec` gains `pub effect: &'static str`, `""` from `key()` and `choice()`; `pub const fn effect(self, text: &'static str) -> KeySpec`), `crates/engine/src/settings/agents_tests.rs`, `crates/notchlingd/src/settings_ops.rs` (`item`), `docs/decisions/0020-agents-change-settings-by-class.md` (a dated note), `crates/AGENTS.md` and `crates/engine/AGENTS.md` (the fold, schema 2, the registry and where words live), `docs/feature-map.md` (Search row: `search/query/`; the sheet's `settings_entries.rs` and search files)
- Test: `crates/notchlingd/src/settings_ops_effect_tests.rs`

**Interfaces:**
- Produces: effect texts, verbatim:
  - `continueAfterSleep`: "When the Mac wakes, agents whose reply sleep cut off are told to continue, so they run again without you."
  - `answerFromNotch`: "Permission prompts and questions can be answered from the notch, not only in the agent's terminal."
  - `keepPages`: "Claude Code's hook saves a local copy of every page published with the Artifact tool."
  - `limitMode`, `codexLimitMode`, `antigravityLimitMode`: "With Notify and Switch, Notchling moves to another saved login when this account nears its limit."
  - `limitSwitchAt`: "Changes the percent of a limit at which Notchling switches to another saved login."
  - `searchShortcut`: "Changes the key that opens Search from any app. No other app can use that key."
  - The Ask item: title `Set <title> to <new>`, detail `<effect> Now <old>.`

- [ ] **Step 1: Write the failing tests:** `every_ask_key_has_an_effect` in `agents_tests.rs` (every `Access::Ask` key has a non-empty `effect` ending in `.`); `the_ask_item_shows_title_values_and_effect` in the new file (an agent's `settings.set` of `keepPages` to false raises an item whose title is "Set Keep local copies of artifacts to false" (today's `shown` form) and whose detail starts with the effect and ends with "Now true.").
- [ ] **Step 2: Run** `cargo test -p notchling-engine settings::` and `cargo test -p notchling-notchlingd settings_ops_effect`. Expected: FAIL.
- [ ] **Step 3: Implement** the field, the eight `.effect(…)` calls and `item`'s detail; write the 0020 note (Ask items show the title, the old and the new value and the key's effect; every Ask key has one) and the doc lines.
- [ ] **Step 4: Run** `cargo test --workspace --locked` and `PYTHONPATH=tools/guard python3 -m guard rules`. Expected: PASS.
- [ ] **Step 5: Commit** `settings: an effect text for every Ask key, shown in the agent's Ask item`. Open the pull request from `agent/search-p1-effect-texts`.
