# Search P3: Facets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every document carries facets (tool, project, account and time for sessions; section for settings; owner, project and time for pages; kind and state for the app's own corpora), and one filter, typed as chips or sent by an agent, applies the same way in the app and the daemon.

**Architecture:** Two more pure files in `crates/engine/src/search/query/`: `when.rs` (bounds from a `now` and an offset passed in) and `filter.rs` (the wire type, the in-memory match, the SQL clause). `query::parse` learns facet and time tokens. `search.db` moves to schema 3 with `docs.corpus` and a `facets` table, so bm25's top 100 is already filtered. The daemon takes `filter` in `search.query` and serves `search.facets`; the app turns tokens into chips and completes values with Tab.

**Tech Stack:** Rust; rusqlite FTS5; GPUI 0.2.2 and `crates/kit`; `crates/locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 4 ("Facets", "How a filter applies"), 5 ("One shared module"), 7 ("Prefixes and chips"), 9 (`filter`, `search.facets`), 12 P3; call 6 (account facet). Plan set: `2026-11-14-search-plans-index.md`.

**Branch:** `agent/search-p3-facets`, from the integration branch after P2 merged.

**Depends on:** P2 (`query::parse`, `Category`, `search.query`'s `corpora`); `agent/account-log-fix` (`cost::account_log::{load, account_at}`) before Task 4.

## Global Constraints

- No comments in code; files under 1,000 lines (600 warns); tests in `*_tests.rs`.
- `query::*` is pure: no I/O, no clock, no SQLite type in its signatures (`SqlArg` instead).
- Local API additive only; `search.query` with no `filter` answers as before.
- No file I/O or daemon call in a model compute or draw; `search.facets` goes through the `Pacer`, off the UI thread.
- AND across facet names, OR within one name's list; a filter on a facet a corpus lacks matches none of that corpus's documents.
- Words in `locales/en/mac/search.ftl`; Title Case for chip labels ("Today", "Last 7 Days", "No Such Project").
- Stage by path.

## Review Focus

1. A session whose account changed during it: its `account` is the one active when it started, not now. (Task 4 test `account_is_the_one_active_at_start`.)
2. `when:today` at 00:30 local time on a Mac whose offset is not UTC: the bound is local midnight. (Task 1 test `today_starts_at_local_midnight`.)
3. A filter on `project` combined with `tool:codex`, where the only project match is a Claude session: no result, not the Claude one. (Task 2 test `and_across_names_or_within`.)
4. A typed `project:` value nothing has: the chip turns red, no filter is sent, and the words still search. (Task 6 test `an_unknown_value_sends_no_filter`.)
5. An agent asking `search.facets` for an app-tier corpus (`vault`): refused like `search.query` drops it. (Task 5 test `facets_follow_the_corpus_tier`.)

---

### Task 1: `query::when`

**Zone:** auto (`crates/engine/src/search/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/query/when.rs`
- Test: `crates/engine/src/search/query/when_tests.rs`

**Interfaces:**
- Produces: `pub enum When { Today, Days(u32), Month { year: i32, month: u32 } }`, `pub fn parse_when(token: &str) -> Option<When>` (`today`, `7d`, `30d`, any `<n>d` up to 3650, `2026-11`), `pub fn bounds(when: When, now: f64, offset_secs: i64) -> (f64, f64)` (after, before; `before` is `now` except for a past month). Callers pass `crate::pages::clock::local_offset_secs(now)`.

- [ ] **Step 1: Write the failing tests:** `today_starts_at_local_midnight` (offset +7 h and −5 h); `seven_days_is_seven_times_86400_back`; `a_month_is_its_first_to_its_last_local_second`; `junk_is_none` (`7x`, `2026-13`, `0d`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::when`. Expected: FAIL.
- [ ] **Step 3: Implement** with `pages::clock::days_from_civil`.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: query::when`.

### Task 2: `query::filter`

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/query/filter.rs`
- Test: `crates/engine/src/search/query/filter_tests.rs`, golden `crates/engine/src/search/query/filter_golden.json`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
  pub struct Filter { pub tool: Option<Vec<String>>, pub project: Option<Vec<String>>, pub account: Option<Vec<String>>,
                      pub section: Option<Vec<String>>, pub owner: Option<Vec<String>>, pub kind: Option<Vec<String>>,
                      pub state: Option<Vec<String>>, pub after: Option<f64>, pub before: Option<f64> }
  pub enum SqlArg { Text(String), Real(f64) }
  impl Filter {
      pub fn is_empty(&self) -> bool;
      pub fn names(&self) -> Vec<(&'static str, &[String])>;
      pub fn matches(&self, facets: &[(&str, &str)], at: Option<f64>) -> bool;
      pub fn sql(&self, doc: &str, at: &str) -> (String, Vec<SqlArg>);
  }
  ```
  `sql("d.id", "d.modified")` returns ` AND d.id IN (SELECT doc FROM facets WHERE name = ? AND value IN (?, …))` per name plus `AND d.modified >= ?`/`< ?`, or `""`.

- [ ] **Step 1: Write the failing tests:** `and_across_names_or_within`; `a_missing_facet_does_not_match`; `bounds_are_half_open`; `sql_goldens` (from `filter_golden.json`: filter JSON in, SQL text and arguments out); `serde_round_trips_and_ignores_unknown_fields`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::filter`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: query::filter, one filter for both sides`.

### Task 3: Facet and time chips in the parser

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none (new golden file; P2's `parse_golden.json` stays).

**Files:**
- Modify: `crates/engine/src/search/query/parse.rs`
- Test: `crates/engine/src/search/query/parse_facets_tests.rs`, golden `crates/engine/src/search/query/parse_facets_golden.json`

**Interfaces:**
- Consumes: `When`, `parse_when` (Task 1); `Filter` (Task 2).
- Produces: `Chip` gains `Facet { name: &'static str, value: String }` and `When(When)`; tokens anywhere in the text: `tool:`, `project:` and `in:` (→ `project`), `account:`, `section:`, `owner:`, `kind:`, `state:`, `when:`. `pub fn filter(parsed: &Parsed, now: f64, offset_secs: i64) -> Filter`.

- [ ] **Step 1: Write the failing tests:** `facet_goldens` (`tool:codex deploy`, `in:notchling when:7d`, `account:work s:`, `when:bogus` stays a word, `project:` with no value stays a word); `filter_from_chips` (two `tool:` chips → one list of two).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::parse_facets`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine search::query`. Expected: PASS.
- [ ] **Step 5: Commit** `search: facet and time chips`.

### Task 4: `search.db` schema 3: corpus and facets

**Zone:** auto (`crates/engine/src/search/**`). CAP/WEAK: none expected (SQL only). Existing tests edited: none; `SearchDb::rank`'s old signature stays as a wrapper with an empty filter.

**Files:**
- Modify: `crates/engine/src/search/fts.rs` (`SCHEMA = 3`: `docs` gains `corpus TEXT NOT NULL DEFAULT 'sessions'`; `CREATE TABLE facets (doc INTEGER, name TEXT, value TEXT, PRIMARY KEY (name, value, doc)) WITHOUT ROWID`; `put` writes facets; `pub fn rank_filtered(&self, words, filter: &Filter, now, limit)`; `pub fn facet_values(&self, corpus: &str, name: &str, prefix: &str, limit: usize) -> Vec<(String, usize)>`), `crates/engine/src/search/meta.rs` (`SessionMeta` gains `#[serde(default)] pub started: Option<f64>`), `crates/engine/src/search/index/read.rs` (`started` from the first line's `timestamp`), `crates/engine/src/search/index/mod.rs` (`SessionIndex::with_accounts(resolver)`; `search_filtered`)
- Test: `crates/engine/src/search/tests_facets.rs`

**Interfaces:**
- Consumes: `Filter` (Task 2); `cost::account_log::{load, account_at}`; `lanes` (a lane's `path` to its `email`).
- Produces: `pub type AccountOf = Arc<dyn Fn(Option<&str>, f64) -> String + Send + Sync>` (config dir, start time → account, `"unknown"` when none); session facets `tool` (`tool_key`), `project` (`folder`), `account`; time is `docs.modified`.

- [ ] **Step 1: Write the failing tests:** `a_filter_applies_inside_fts` (101 matching docs where only one is `tool=codex`: `rank_filtered` with `tool:codex` and limit 100 returns that one); `account_is_the_one_active_at_start` (a resolver over a fixture `account-log.json`); `facet_values_count_and_prefix`; `a_schema_2_file_is_rebuilt`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::tests_facets`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine search::`. Expected: PASS.
- [ ] **Step 5: Commit** `search: search.db schema 3 with corpus and facets`.

### Task 5: The daemon: `filter` and `search.facets`

**Zone:** maintainer (`crates/notchlingd/**`, `crates/local-api/**`). CAP/WEAK: none expected. Existing tests edited: none.

**Files:**
- Modify: `crates/local-api/src/wire.rs` (`ops::SEARCH_FACETS = "search.facets"`), `crates/notchlingd/src/pages_ops.rs` (parse `filter`; sessions through `search_filtered`; pages through `Filter::matches` with `owner` = the page's account, `project`, time = `updated`; every hit gains `facets`), `crates/notchlingd/src/daemon_wiring.rs` (the `AccountOf` from `lanes.json` and `account-log.json`), `docs/local-api-ops.md`
- Test: `crates/notchlingd/src/pages_ops_facets_tests.rs`

**Interfaces:**
- Produces: `search.facets {corpus, name, prefix?, limit?}` → `{values: [{value, label, count}]}`; `label` is a project's last path part, else the value; tier as its corpus (`sessions`, `pages`: agent); `limit` default 20, at most 100. A bad `filter` is `bad_request` naming the field.

- [ ] **Step 1: Write the failing tests:** `a_filtered_query_returns_only_matching_docs`; `hits_carry_their_facets`; `facets_follow_the_corpus_tier`; `facets_labels_are_last_path_parts`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd pages_ops_facets`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: search.query filter and search.facets`.

### Task 6: Chips and Tab completion in ⌘⇧P

**Zone:** auto (`crates/app/**`, `locales/en/mac/search.ftl`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/app/src/search/chips.rs` (pure: chip labels, red state, completion)
- Test: `crates/app/src/search/chips_tests.rs`
- Modify: `crates/app/src/search/wire.rs` (`Call::query` sends `filter`; `Call::facets`), `crates/app/src/search/local.rs` (the app's corpora carry facets: Settings `section`, Actions `kind`, Inbox `kind`, `state`, `time`; matched through `Filter::matches`), `crates/app/src/search/view.rs`, `crates/app/src/search/rows.rs`, `crates/app/src/search/renders.rs` (`search-panel-facets`), `locales/en/mac/search.ftl`

**Interfaces:**
- Produces: `pub struct ChipView { pub label: String, pub known: bool }`; `pub fn chips(parsed: &Parsed, known: &Known) -> Vec<ChipView>`; `pub struct Known { values: HashMap<&'static str, Vec<String>> }` filled from `search.facets` replies; `pub fn complete(token: &str, known: &Known) -> Option<String>`; `pub fn sent_filter(parsed: &Parsed, known: &Known, now: f64, offset: i64) -> Filter` (unknown values left out).

- [ ] **Step 1: Write the failing tests:** `an_unknown_value_sends_no_filter`; `tab_completes_the_longest_common_prefix`; `when_chips_are_labeled` ("Today", "Last 7 Days", "November 2026"); `the_apps_corpora_filter_locally` (`kind:` on Actions).
- [ ] **Step 2: Run** `cargo test -p notchling-app search::chips`. Expected: FAIL.
- [ ] **Step 3: Implement**; words; `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the tests; render `--render search-panel-facets` in light and dark and look.
- [ ] **Step 5: Commit** `search: facet chips and Tab completion`.

### Task 7: Notes and the checks

**Zone:** maintainer (`crates/AGENTS.md`, `crates/engine/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `docs/feature-map.md`).

- [ ] **Step 1:** Notes: facets per corpus, the filter's meaning (AND/OR, missing facet), schema 3, account at start; feature-map names `query/when.rs`, `query/filter.rs`, `search/chips.rs`.
- [ ] **Step 2: Run** the full checks: `cargo test --workspace --locked`, clippy, fmt, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`, `python3 tools/locale/gen.py --check`. Expected: clean.
- [ ] **Step 3: Commit** `docs: search facets`, and open the pull request.
