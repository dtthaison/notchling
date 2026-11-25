# Search P6: Hybrid Ranking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With Semantic Search on and its models present, `search.query` fuses three lists (FTS5 AND, granite through the helper, static in the daemon) with RRF k = 60 and the recency bonus; ⌘⇧P shows meaning-only hits as Related and the index's progress; Settings search fuses the daemon's vector lists with the app's text list. With it off, nothing loads and every reply is today's.

**Architecture:** The helper `notchling-embed serve` speaks one JSON line each way and exits after 5 idle minutes. The engine gets the helper client (`HelperEmbedder`, behind a spawner trait with a fake) and a pure `search/hybrid.rs` that turns lists into hits. The daemon gets `embed_ops.rs`: it owns P5's pipeline, store and sets, starts the helper only when the queue is due or a query needs it, maps the static table only while `semanticSearch` is on, and serves `embed.status`, `embed.settings`, `embed.refresh` and the `embed` topic. The app follows the topic and sends Settings' English text.

**Tech Stack:** Rust; `notchling-embed` (P4a); engine feature `static-embed`; GPUI 0.2.2 and `crates/kit`; `crates/locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 3 ("Where each model runs", memory), 5 (fixture roots), 6, 8 ("Hybrid matching"), 9, 10 (`semanticSearch`, `semanticModel`, `semanticPaused`), 11, 12 P6; answer 4; call 6. Plan set: `2026-11-14-search-plans-index.md` (calls 4, 7, 8).

**Branch:** `agent/search-p6-hybrid`, from the integration branch with P3, P4a and P5 merged.

**Depends on:** P3 (`query::filter`), P4a (`Granite`, `StaticEmbedder`, the crate), P5 (all). P4b sets the floors later (Task 3's constants start at 0.0).

## Global Constraints

- No comments in code; files under 1,000 lines (600 warns); tests in `*_tests.rs`.
- Local API additive only: `search.query` without `mode` answers as today (`mode` defaults to `"text"`); old keys keep their values.
- No test downloads or reads a model; the daemon's tests use `HashEmbedder`, `TableEmbedder` and a fake spawner. Under a fixture root (`NOTCHLING_DATA_ROOT`) the daemon uses `HashEmbedder` for both models and never spawns the helper, unless `NOTCHLING_EMBED_REAL=1`.
- Semantic Search off: no model file is mapped, no helper runs, no vector set is in memory. Turning it off unmaps and frees; rows in `vectors.db` stay.
- No lock across I/O: the helper call runs outside every lock; a query reads `Arc` snapshots.
- At most 2 query embeddings at once; query vectors cached, 256 by text.
- Replies under 100 KB (≤ 200 sessions, ≤ 20 of anything else, snippets ≤ 200 chars); helper lines ≤ 8 texts out, ≤ 8 × 384 floats back.
- App: daemon calls off the UI thread through the `Pacer`; words in `locales/en/mac/search.ftl`.
- Stage by path.

## Review Focus

1. The helper crashes mid-batch: the batch is re-queued, the state becomes `warming` on the next query, no chunk is marked embedded. (Task 2 test `a_dead_helper_requeues_its_batch`.)
2. Semantic Search turned off while a query is embedding: the query finishes with text only; nothing stays mapped. (Task 5 test `turning_off_frees_everything`.)
3. A query that is exact-looking (a path) with Semantic Search ready: text only, identical to `mode: "text"`. (Task 3 test `exact_looking_queries_use_text_only`.)
4. Full text finds 6 sessions and meaning finds 20 more: at most half of the shown sessions are meaning-only. (Task 3 test `meaning_only_fills_at_most_half`.)
5. An agent sends `embed.settings` to plant a fake setting: refused, app tier. (Task 5 test `embed_settings_is_app_tier`.)

---

### Task 1: The three semantic keys

**Zone:** maintainer (`crates/engine/src/settings/**`); auto for the app's registry line. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Modify: `crates/engine/src/settings/keys.rs`, `crates/app/src/sheets/settings_entries.rs` (`NOT_SHOWN` gains the two keys: "its row comes with the Semantic Search UI (P7)")
- Test: `crates/engine/src/settings/keys_semantic_tests.rs`

**Interfaces:**
- Produces, verbatim:
  - `semanticSearch`: Bool, `false`, Ask, title "Semantic search", about "Search by meaning as well as by words, with models kept on this Mac", why "it indexes every session in the background and keeps a model in memory", effect "Turns on search by meaning: indexes your sessions in the background (about 20 minutes the first time) and keeps the accent helper in memory (about 50 MB)."
  - `semanticPaused`: Bool, `false`, Ask, title "Pause indexing by meaning", about "Pause or resume indexing by meaning", why "while paused, new sessions are found by words only", effect "Pauses or resumes indexing by meaning. While paused, new sessions are found by words only."
  - `semanticModel`: StringList, empty, Never, title "Semantic search models", about "The models downloaded for search by meaning", why "only the app's Download button fetches models: 195 MB to about 1.1 GB".

- [ ] **Step 1: Write the failing tests:** `semantic_keys_have_their_classes`; `semantic_search_is_off_by_default`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine settings::keys_semantic`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine settings::` and `cargo test -p notchling-app sheets::settings_entries`. Expected: PASS.
- [ ] **Step 5: Commit** `settings: semanticSearch, semanticPaused, semanticModel`.

### Task 2: The helper's line protocol, both ends

**Zone:** maintainer (`crates/embed/**`); auto path for the engine client but **CAP-PROCESS** (`Command::new`). WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/embed/src/serve.rs`, `crates/engine/src/search/embed/helper.rs`, `crates/engine/src/search/embed/helper_fake.rs`
- Test: `crates/embed/src/serve_tests.rs`, `crates/engine/src/search/embed/helper_tests.rs`
- Modify: `crates/embed/src/main.rs` (`notchling-embed serve --model <dir>`)

**Interfaces:**
- Produces:
  - Lines (UTF-8 JSON, one per line). Helper → daemon first: `{"ready": true, "model": "granite-97m-r2@835ad14/p1", "dims": 384}`. Daemon → helper: `{"id": u64, "lane": "query" | "index", "texts": [string]}` (1 to 8 texts). Helper → daemon: `{"id": u64, "vecs": [base64 of little-endian f32]}` or `{"id": u64, "error": string}`.
  - Helper: two threads over one `Arc<Granite>`; `query` lines at user-initiated QoS, `index` at background QoS (`libc::pthread_set_qos_class_self_np` on macOS, `nice` on Linux, nothing elsewhere); a query never waits behind a batch; exits 300 s after both queues are empty, or at stdin EOF. `pub fn serve(model: Arc<dyn Encode>, input: impl BufRead, output: impl Write, clock: &dyn Fn() -> Instant) -> Result<(), String>` with `trait Encode: Send + Sync { fn encode(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>; }` so tests run without a model.
  - Engine client:
    ```rust
    pub trait HelperIo: Send { fn send(&mut self, line: &str) -> Result<(), String>; fn recv(&mut self, wait: Duration) -> Result<Option<String>, String>; }
    pub trait HelperSpawn: Send + Sync { fn spawn(&self) -> Result<Box<dyn HelperIo>, String>; }
    pub struct ProcessSpawn { pub exe: PathBuf, pub model_dir: PathBuf }   // the real one
    pub struct FakeSpawn;                                                  // helper_fake.rs: scripted replies, counts spawns, can die
    pub enum HelperState { Cold, Warming, Ready, Failed(String) }
    pub struct HelperEmbedder { … }   // impl Embedder; HelperEmbedder::new(spawn: Arc<dyn HelperSpawn>, model: ModelId)
    impl HelperEmbedder { pub fn state(&self) -> HelperState; pub fn warm(&self); pub fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError>; pub fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>; }
    ```
    `embed_query` returns `NotReady` and starts the helper while it is `Cold` or `Warming`; the query cache holds 256 vectors by text; a semaphore lets 2 queries run.

- [ ] **Step 1: Write the failing tests:** helper side `serve_answers_in_order_per_lane`, `a_query_overtakes_a_batch` (a slow fake `Encode`), `exits_after_five_idle_minutes_on_a_fake_clock`, `bad_lines_get_an_error_not_a_crash`; client side `cold_query_starts_the_helper_and_answers_not_ready`, `a_dead_helper_requeues_its_batch`, `the_cache_holds_256`, `at_most_two_queries_at_once`.
- [ ] **Step 2: Run** `cargo test -p notchling-embed serve` and `cargo test -p notchling-engine search::embed::helper`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both. Expected: PASS.
- [ ] **Step 5: Commit** `embed: serve over JSON lines; search: HelperEmbedder client`.

### Task 3: `search/hybrid.rs`

**Zone:** auto (`crates/engine/src/search/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/hybrid.rs`
- Test: `crates/engine/src/search/hybrid_tests.rs`, golden `crates/engine/src/search/hybrid_golden.json`

**Interfaces:**
- Consumes: `query::{fuse::{rrf, recency, List, K}, shape::text_only}`, `DocHit` (P5).
- Produces:
  ```rust
  pub const FLOOR_GRANITE: f32 = 0.0;   // P4b sets it
  pub const FLOOR_STATIC: f32 = 0.0;    // P4b sets it
  pub const TOP_TEXT: usize = 100; pub const TOP_CHUNKS: usize = 300;
  pub enum Semantic { Off, NoModel, Warming, Indexing, Ready, Paused }
  pub enum Match { Text, Semantic, Both }
  pub struct Lists { pub text: Vec<String>, pub text_or: bool, pub granite: Option<Vec<DocHit>>, pub statics: Option<Vec<DocHit>> }
  pub struct Ranked { pub key: String, pub score: f64, pub matched: Match, pub chunk: Option<[u8; 16]> }
  pub fn plan(query: &str, state: Semantic) -> Plan;      // which lists to compute: text always; static unless Off/NoModel; granite only when Ready/Indexing/Paused; none but text when text_only(query)
  pub fn fuse(lists: &Lists, cap: usize, age_secs: &dyn Fn(&str) -> Option<f64>) -> Vec<Ranked>;
  ```
  `text_or` gives the text list weight 0.5 (OR only when AND found nothing). When the text list has 5 or more, meaning-only hits fill at most `cap / 2` of the result.

- [ ] **Step 1: Write the failing tests:** `rrf_golden` (from `hybrid_golden.json`: three lists in, ranked keys with scores and `Match` out); `exact_looking_queries_use_text_only`; `warming_fuses_text_and_static`; `off_is_text_only`; `meaning_only_fills_at_most_half`; `floors_drop_low_scores` (with non-zero floors passed through a test-only constructor).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::hybrid`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: hybrid ranking over text, granite and static`.

### Task 4: `embed_ops.rs` in the daemon

**Zone:** maintainer (`crates/notchlingd/**`, `crates/local-api/**`, `crates/notchlingd/Cargo.toml` turns on `static-embed`). CAP/WEAK: **CAP-PROCESS** (the real spawner is built here), **CAP-FILEWRITE** (opens `vectors.db`). Existing tests edited: none.

**Files:**
- Create: `crates/notchlingd/src/embed_ops.rs`, `crates/notchlingd/src/embed_paths.rs`
- Test: `crates/notchlingd/src/embed_ops_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`ops::EMBED_STATUS = "embed.status"`, `ops::EMBED_SETTINGS = "embed.settings"`; `topics::EMBED = "embed"`; event `EMBED_CHANGED = "embed.changed"`), `crates/notchlingd/src/daemon_wiring.rs` (an `OpGroup`; `embed.refresh` moves here from P2's routing), `crates/notchlingd/Cargo.toml`

**Interfaces:**
- Consumes: P5's `Pipeline`, `VectorDb`, `Sets`, `Queue`, `Budget`, corpora; `HelperEmbedder`, `ProcessSpawn` (Task 2); `StaticEmbedder` (P4a); `SettingsCorpus::set`.
- Produces:
  - Model files: `<data>/models/granite-97m-r2/` (`config.json`, `model.safetensors`, `tokenizer.json`) and `<data>/models/static-mrl/` (`table-512i8.bin`, `tokenizer.json`); `embed_paths::helper_exe()` is `notchling-embed` in `../Helpers/` from the daemon's executable, else beside it (a cargo target folder).
  - `pub struct EmbedOps`; `EmbedOps::new(layout: &Layout, room: Arc<dyn Room>, settings: Arc<dyn Fn() -> EmbedSettings + Send + Sync>) -> Arc<EmbedOps>`; `pub struct EmbedSettings { pub on: bool, pub paused: bool, pub models: Vec<String>, pub range: IndexRange, pub categories: Vec<Category> }`; `fn query_lists(&self, query: &str, filter: &Filter, corpora: &[Category], limit: usize) -> (Semantic, HashMap<Category, Lists>)` for `search.query`; `fn start(&self)`, `fn close(&self)`.
  - A worker thread: a pass at start, every 60 s while there is work, on `embed.refresh`; feeds the queue to the helper when `Queue::due`; keeps the budget; publishes `embed.changed {state, coverage, fetch: null}` at most once a second.
  - `embed.status {}` → `{state, models: [{id, present, verified}], corpora: [{corpus, docs, chunks, embedded: {model: n}}], paused, budget, error}`, agent tier.
  - `embed.settings {docs: [{id, text}]}` → `{stored: n}`, app tier, at most 200 docs of 4,000 chars.
  - Fixture root: both embedders are `HashEmbedder` unless `NOTCHLING_EMBED_REAL=1`.

- [ ] **Step 1: Write the failing tests:** `off_maps_nothing_and_spawns_nothing` (a counting spawner and a counting file opener); `turning_off_frees_everything`; `a_fixture_root_never_spawns`; `status_reports_coverage`; `embed_settings_is_app_tier`; `the_queue_wakes_the_helper_at_sixteen` (fake clock, `FakeSpawn`); `changed_is_at_most_once_a_second`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd embed_ops`. Expected: FAIL.
- [ ] **Step 3: Implement.** The worker never holds a lock across a helper call or a pass.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: embed ops, the pipeline worker and the helper's lifetime`.

### Task 5: `search.query` hybrid

**Zone:** maintainer (`crates/notchlingd/**`, `docs/local-api-ops.md`). CAP/WEAK: none beyond Task 4. Existing tests edited: none; `pages_ops_tests.rs` and P2's and P3's tests pass unchanged.

**Files:**
- Create: `crates/notchlingd/src/search_query.rs` (the `search.query` handler moves here from `pages_ops.rs`, which calls it)
- Test: `crates/notchlingd/src/search_query_tests.rs`
- Modify: `crates/notchlingd/src/pages_ops.rs`, `docs/local-api-ops.md`

**Interfaces:**
- Consumes: `EmbedOps::query_lists` (Task 4), `hybrid::{plan, fuse}` (Task 3).
- Produces: args `mode?: "text" | "hybrid" | "semantic"` (default `"text"`); `"semantic"` returns only the vector lists' hits (used for Settings); every hit gains `match` (`"text"`, `"semantic"`, `"both"`); the reply gains `semantic: {state, coverage}`. A meaning-only hit's snippet is its best chunk's opening (≤ 200 chars) with no highlights. `corpora: ["settings"]` with `mode: "semantic"` returns `settings: [{id, score}]` (no longer in `dropped` for that mode).

- [ ] **Step 1: Write the failing tests:** `no_mode_answers_as_before` (old keys, values and order equal a `mode: "text"` call); `hybrid_tags_matches`; `semantic_settings_lists_for_the_app`; `a_hybrid_reply_stays_under_100_kb` (200 sessions with long chunks); `turning_off_frees_everything` (from Review Focus 2, over Task 4's ops).
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd search_query`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: search.query fuses text, granite and static`.

### Task 6: The app: Related, progress, Settings' vector lists

**Zone:** auto (`crates/app/**`, `locales/en/mac/search.ftl`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/app/src/search/embed_feed.rs` (follows topic `embed` like `switches/feed.rs`; holds the last `embed.changed`), `crates/app/src/sheets/settings_semantic.rs` (pure: the fused Settings list)
- Test: `crates/app/src/search/embed_feed_tests.rs`, `crates/app/src/sheets/settings_semantic_tests.rs`
- Modify: `crates/app/src/search/wire.rs` (`mode`, `match`, `semantic`), `crates/app/src/search/rows.rs` (a "Related" tag on `match: "semantic"`), `crates/app/src/search/view.rs` (footer: "Indexing 1,240 of 15,000 passages" while indexing), `crates/app/src/sheets/settings_view.rs` (when the feed says ready, each keystroke also asks `mode: "semantic"`, `corpora: ["settings"]` through the `Pacer`), `crates/app/src/mac/shell.rs` (at start and when `semanticSearch` turns on, send `embed.settings` with each registry entry's English title, about, English words and aliases, off the UI thread)

**Interfaces:**
- Consumes: `query::fuse::rrf`; P1's `SettingsSearch`; `sheets::settings_entries::{ENTRIES, english_words}` (English through `Text::english()`).
- Produces: `pub fn fuse_settings(text: &[Found], semantic: &[(String, f64)]) -> Vec<Found>` (RRF k = 60 over the two lists; keeps entries the text list lacks by looking them up in the registry); the model keeps the selected entry id across the swap.

- [ ] **Step 1: Write the failing tests:** `semantic_hits_show_related`; `progress_text_counts_passages`; `fused_settings_keep_the_selection`; `a_semantic_only_setting_is_added`; `nothing_is_asked_while_not_ready`.
- [ ] **Step 2: Run** `cargo test -p notchling-app search::embed_feed sheets::settings_semantic`. Expected: FAIL.
- [ ] **Step 3: Implement**; words; `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the tests; render `search-panel-results` with a fixture semantic hit (`NOTCHLING_RENDER_SEARCH_RELATED=1`) and look.
- [ ] **Step 5: Commit** `search: Related hits, indexing progress, Settings fused with meaning`.

### Task 7: The latency bench

**Zone:** maintainer (`crates/notchlingd/tests/` new file). CAP/WEAK: **WEAK-SKIP** (ignored). Existing tests edited: none.

**Files:**
- Create: `crates/notchlingd/tests/bench_search.rs`

- [ ] **Step 1: Write** `hybrid_p95_under_50_ms_at_50k_chunks` (`#[ignore]`): a temp root, 50,000 generated chunks with `HashEmbedder` vectors in both sets, a warm real helper from `NOTCHLING_EMBED_MODELS` when set (else `TableEmbedder` and a note), 200 queries; assert p95 < 50 ms and print `bench-json: {p50_ms, p95_ms, scan_ms}`.
- [ ] **Step 2: Run** `cargo test --release -p notchling-notchlingd --test bench_search -- --ignored --nocapture`. Expected: PASS.
- [ ] **Step 3: Commit** `notchlingd: search latency bench`.

### Task 8: A live check over a fixture, with the real models

**Zone:** none (no change). Development commands only.

- [ ] **Step 1:** With P4a's models present, lay them out as the daemon expects:
  ```sh
  cp -R crates/app/fixtures/notch /tmp/nl-root
  mkdir -p /tmp/nl-root/notchling/models/granite-97m-r2 /tmp/nl-root/notchling/models/static-mrl
  cp "$NOTCHLING_EMBED_MODELS"/ibm-granite__granite-embedding-97m-multilingual-r2/{config.json,model.safetensors,tokenizer.json} /tmp/nl-root/notchling/models/granite-97m-r2/
  cargo run --release -p notchling-embed -- convert-static "$NOTCHLING_EMBED_MODELS/sentence-transformers__static-similarity-mrl-multilingual-v1" /tmp/nl-root/notchling/models/static-mrl/table-512i8.bin
  cp "$NOTCHLING_EMBED_MODELS"/sentence-transformers__static-similarity-mrl-multilingual-v1/tokenizer.json /tmp/nl-root/notchling/models/static-mrl/
  ```
- [ ] **Step 2:** `cargo build --release -p notchling-notchlingd -p notchling-embed`; run `NOTCHLING_DATA_ROOT=/tmp/nl-root NOTCHLING_EMBED_REAL=1 target/release/notchling daemon`, after writing the key into the copy's store before the start (an Ask key from the command line needs an app to allow it): `sqlite3 /tmp/nl-root/notchling/notchling.db "INSERT OR REPLACE INTO settings (key, value) VALUES ('semanticSearch', 'true')"`. The first pass runs at start. Query `search.query {"text": "dang nhap", "mode": "hybrid"}` through `notchling_local_api::Client` from a scratch program kept outside the repository, and read `embed.status` the same way. Expected: `semantic.state` reaches `ready`; a Vietnamese query without accents finds the fixture session that has it with accents; `ps` shows `notchling-embed` exit about 5 minutes after the index finishes.
- [ ] **Step 3:** Stop the daemon; `rm -rf /tmp/nl-root`.

### Task 9: Notes and the checks

**Zone:** maintainer (`crates/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `crates/engine/AGENTS.md`, `crates/embed/AGENTS.md`, `docs/feature-map.md`, `docs/local-api-ops.md`).

- [ ] **Step 1:** Notes: the helper's protocol and lifetime; Semantic Search off loads nothing; fixture roots use `HashEmbedder`; floors live in `hybrid.rs` and come from P4b; `embed.settings` and why the app sends Settings' text (the daemon never links `crates/locale`); the ops' wire shapes.
- [ ] **Step 2: Run** `cargo test --workspace --locked`, clippy, fmt, `cargo deny check`, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`, `python3 tools/locale/gen.py --check`. Expected: clean.
- [ ] **Step 3: Commit** `docs: hybrid search`, and open the pull request (CAP-PROCESS, CAP-FILEWRITE, WEAK-SKIP).
