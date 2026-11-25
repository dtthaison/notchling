# Search P8: More Corpora Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Search finds pages by their content, and, when the user checks them, Activity rows, Vault Names and Machines, by words and by meaning, for the app only.

**Architecture:** `search.db` (schema 3 from P3) gains a generic document path so any corpus can put text and facets beside sessions. Three new `Corpus` implementations in the engine (activity, vault names, machines) read names and facts only, never secrets, never the network. The daemon registers them in the pipeline and in full text, serves them to the app tier only, and empties Vault Names when the `vault` switch goes off. The app adds the two missing category rows and their Return actions, and stops matching vault names itself.

**Tech Stack:** Rust; rusqlite FTS5; GPUI 0.2.2 and `crates/kit`; `crates/locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 4 (table), 5 ("Categories and range": who sees what, Vault Names; "Chunking"), 7 (Return per hit), 9 (`dropped`), 12 P8. Plan set: `2026-11-14-search-plans-index.md` (call 10).

**Branch:** `agent/search-p8-corpora`, from the integration branch with P6 merged.

**Depends on:** P3 (schema 3, facets, `Filter`), P5 (`Corpus`, pipeline), P6 (`embed_ops.rs`, hybrid `search.query`), P2 (categories).

## Global Constraints

- No comments in code; files under 1,000 lines (600 warns); tests in `*_tests.rs`.
- Activity, Vault Names and Machines are app tier: an agent-tier `search.query` asking for them gets them in `dropped`; `search.facets` refuses them for agents.
- Vault Names: names and kinds only; never a value; never through the Keychain (tests use `PanicKeychain`); only while the `vault` switch is on; its rows deleted from `search.db` and `vectors.db` when it goes off.
- Machines: names, hosts, roles and kinds from the registry file; no ssh, no `gh`, no network.
- No file I/O or daemon call in a model compute or draw.
- Stage by path.

## Review Focus

1. The vault switch goes off while a pass is indexing vault names: after the pass, no vault row is left in either database, and a query in that run finds none. (Task 3 test `switch_off_mid_pass_leaves_nothing`.)
2. A page whose HTML holds a script or a huge inline image: text is stripped, the image's base64 never becomes a chunk. (Task 1 test `scripts_and_data_urls_are_not_text`.)
3. An agent asks `corpora: ["activity"]` with a filter on `kind`: dropped, and the reply leaks no count. (Task 5 test `app_tier_corpora_are_dropped_for_agents`.)
4. `audit.jsonl` rotates mid-pass: rows from both files are indexed once, none twice. (Task 2 test `rotation_does_not_duplicate_rows`.)
5. A machine removed from the registry: its doc leaves both indexes on the next pass. (Task 4 test `a_removed_machine_leaves_the_index`.)

---

### Task 1: Generic documents in `search.db`, and pages by content

**Zone:** auto (`crates/engine/src/search/**`). CAP/WEAK: none expected (SQL only). Existing tests edited: none.

**Files:**
- Modify: `crates/engine/src/search/fts.rs` (`pub fn put_docs(&self, corpus: &str, docs: &[FtsDoc])`, `pub fn remove_corpus_docs(&self, corpus: &str, keys: &[String])`, `pub fn rank_corpus(&self, corpus: &str, words, filter: &Filter, now, limit) -> Vec<(String, f64)>`), `crates/engine/src/search/corpus/pages.rs` (body from `chunk::page_chunks`)
- Test: `crates/engine/src/search/tests_docs.rs`

**Interfaces:**
- Produces: `pub struct FtsDoc { pub key: String, pub title: String, pub folder: String, pub body: String, pub at: f64, pub facets: Vec<(String, String)> }`. Sessions keep their own `put`; both write `docs.corpus`.

- [ ] **Step 1: Write the failing tests:** `a_page_is_found_by_its_content`; `corpora_do_not_mix` (a page and a session with the same word: `rank_corpus("pages", …)` returns only the page); `scripts_and_data_urls_are_not_text`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::tests_docs`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine search::`. Expected: PASS.
- [ ] **Step 5: Commit** `search: generic documents in search.db; pages by content`.

### Task 2: The Activity corpus

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/corpus/activity.rs`
- Test: `crates/engine/src/search/corpus/activity_tests.rs`

**Interfaces:**
- Consumes: `crate::activity::{rows, Files, Query, Row}`.
- Produces: `pub struct ActivityCorpus`; `ActivityCorpus::new(files: Files)`; `id() == "activity"`, `tier() == Tier::App`; one doc per row keyed `<time_ms>:<source>:<op>`; title the row's op, body its detail; facets `kind`, `source`; `at` the row's time; stamp the files' sizes and mtimes.

- [ ] **Step 1: Write the failing tests:** `rows_become_docs_with_kind_and_source`; `rotation_does_not_duplicate_rows`; `an_unchanged_log_reads_nothing`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::corpus::activity`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: Activity corpus`.

### Task 3: The Vault Names corpus

**Zone:** auto path; **CAP-PROTECTED-TYPE** does not apply (that list names Swift types), but the vault is a protected area by `AGENTS.md`, so the maintainer reviews it. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/corpus/vault.rs`
- Test: `crates/engine/src/search/corpus/vault_tests.rs`

**Interfaces:**
- Consumes: `crate::vault::store` (entries' names and kinds only), `PanicKeychain`.
- Produces: `pub struct VaultCorpus`; `VaultCorpus::new(store: Arc<VaultStore>, on: Arc<dyn Fn() -> bool + Send + Sync>)`; `id() == "vault"`, `tier() == Tier::App`; one doc per entry keyed by name; body "name, kind"; facet `kind`; `scan` returns nothing while `on()` is false.

- [ ] **Step 1: Write the failing tests:** `names_and_kinds_only` (a store with values: no value string appears in any doc); `never_touches_the_keychain` (`PanicKeychain`); `switch_off_mid_pass_leaves_nothing` (with Task 5's pipeline and both databases in memory).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::corpus::vault`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: Vault Names corpus, names and kinds only`.

### Task 4: The Machines corpus

**Zone:** auto path; Machines is a protected area by `AGENTS.md`, so the maintainer reviews it. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/corpus/machines.rs`
- Test: `crates/engine/src/search/corpus/machines_tests.rs`

**Interfaces:**
- Consumes: `crate::machines::registry` (`Machine`, the registry's `load`).
- Produces: `pub struct MachinesCorpus`; `MachinesCorpus::new(registry_file: PathBuf)`; `id() == "machines"`, `tier() == Tier::App`; one doc per machine: name, host, role; facet `kind` where the registry has one.

- [ ] **Step 1: Write the failing tests:** `machines_become_docs`; `a_removed_machine_leaves_the_index`; `reads_only_the_registry_file` (a fake runner that panics if called).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::corpus::machines`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: Machines corpus`.

### Task 5: The daemon serves them

**Zone:** maintainer (`crates/notchlingd/**`). CAP/WEAK: none expected. Existing tests edited: none.

**Files:**
- Modify: `crates/notchlingd/src/embed_ops.rs` (register the three corpora when their category is on; the vault one reads the `vault` switch; a switch-off listener deletes its rows at once), `crates/notchlingd/src/search_query.rs` (`activity`, `vault`, `machines` answered for the app tier, `dropped` for agents; pages ranked by content too), `docs/local-api-ops.md`
- Test: `crates/notchlingd/src/search_corpora_tests.rs`

- [ ] **Step 1: Write the failing tests:** `app_tier_corpora_are_dropped_for_agents`; `the_app_gets_activity_and_machines`; `vault_off_empties_both_indexes`; `an_unchecked_category_is_not_indexed`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd search_corpora`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: Activity, Vault Names and Machines in search, app tier only`.

### Task 6: The app: rows, results and Return

**Zone:** auto (`crates/app/**`, `locales/en/mac/*.ftl`). CAP/WEAK: none. Existing tests edited: `crates/app/src/search/model_tests.rs` where a test pins the app's own vault-name matching (now the daemon's); the pull request lists each.

**Files:**
- Modify: `crates/app/src/sheets/search_rows.rs` (Activity and Machines rows appear), `crates/app/src/search/model.rs`, `crates/app/src/search/wire.rs`, `crates/app/src/search/rows.rs`, `crates/app/src/search/view.rs` (Return: an Activity row opens Settings > Activity at it; a Machine opens Machines; a vault name does what it does today), `locales/en/mac/search.ftl`
- Test: `crates/app/src/search/corpora_tests.rs`

- [ ] **Step 1: Write the failing tests:** `the_sheet_lists_all_eight_categories`; `activity_hits_open_settings_activity`; `machine_hits_open_machines`; `vault_names_come_from_the_daemon` (no `vault.list` call while searching).
- [ ] **Step 2: Run** `cargo test -p notchling-app search::corpora`. Expected: FAIL.
- [ ] **Step 3: Implement**; words; `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the tests; render `search-panel-groups` with the three categories checked and look.
- [ ] **Step 5: Commit** `search: Activity, Vault Names and Machines in ⌘⇧P`.

**Checkpoint:** `cargo test --workspace --locked`, clippy, fmt, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`, `python3 tools/locale/gen.py --check`; update the notes in `crates/AGENTS.md`, `crates/engine/AGENTS.md`, `crates/notchlingd/AGENTS.md` and `docs/feature-map.md` (maintainer zone) in a last commit `docs: more search corpora`; open the pull request.
