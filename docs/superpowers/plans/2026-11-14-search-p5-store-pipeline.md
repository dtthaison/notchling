# Search P5: Vector Store and Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything search by meaning needs except a real model: a `vectors.db` with docs, facets, chunks and vectors per model; an in-memory int8 set searched by brute force under a document mask; corpora behind one trait; a chunker; and a pipeline that embeds only what changed, batches for the helper, refills after a model change and stays within its CPU budget. All on fakes.

**Architecture:** New folders under `crates/engine/src/search/`: `embed/` (the `Embedder` trait, `HashEmbedder`, `TableEmbedder`, int8), `store/` (`vectors.db`, `VectorSet`), `corpus/` (sessions, pages, settings), `pipeline/` (passes, the batch queue, the budget). Also `chunk.rs`, and `query::fuse` and `query::shape`, which the eval tool (P4a) and hybrid ranking (P6) both use. Nothing here is wired into the daemon yet; P6 does that.

**Tech Stack:** Rust; rusqlite (bundled); `sha2`, `base64` (already engine dependencies). No new dependency.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 4 (table, schema, sizes), 5 (trait, chunking, ids and stamps, model changes, budget), 6 (RRF, text-only shapes), 11, 12 P5; call 6 (queue). Plan set: `2026-11-14-search-plans-index.md` (calls 5, 6).

**Branch:** `agent/search-p5-store`, from the integration branch (it needs only P1's `query::fold`; it can start as soon as P1's first pull request merged, in parallel with P2 and P3). Merge Tasks 1–3 early if P4a is waiting on them.

**Depends on:** P1 (`query::fold`, `query::words`). Not on P2, P3 or P4a.

## Global Constraints

- No comments in code; files under 1,000 lines (600 warns); tests in `*_tests.rs`.
- No test downloads or reads a model. Embedders in tests are `HashEmbedder` and `TableEmbedder`.
- `vectors.db` is its own file (`<data>/daemon/vectors.db`), 0600 in a 0700 folder, never opened through a symlink; a damaged file is set aside as `vectors.db.damaged` like `search.db`.
- No lock across I/O or an embed call: a `VectorSet` is an `Arc` swapped after each batch; a query clones the `Arc` and scans without a lock.
- Chunk id: `SHA-256(text)[..16]`; vectors keyed by `(hash, model)`.
- Times are `f64` seconds passed in; no clock read inside the store, the queue or the budget.
- Stage by path.

## Review Focus

1. A transcript that grows: its old turns keep their hashes and only new chunks reach the embedder. (Task 7 test `growth_embeds_only_new_chunks`.)
2. A session still being written: its last turn is not chunked until it has been quiet 60 s or a new prompt closes it. (Task 3 test `the_open_turn_waits`.)
3. A model switched while half its corpus is embedded: the old model's vectors stay usable until the new one covers every chunk, then are deleted. (Task 7 test `a_model_change_refills_from_stored_text`.)
4. A filter that masks every chunk: the scan returns nothing and costs no score. (Task 5 test `masks_are_exact`.)
5. A Mac on battery: the indexing budget halves to 25% of one core; Low Power Mode or a serious thermal state pauses it. (Task 8 test `battery_and_heat_lower_or_pause`.)

---

### Task 1: The `Embedder` trait, fakes and int8

**Zone:** auto (`crates/engine/src/search/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/embed/mod.rs`, `crates/engine/src/search/embed/fake.rs`, `crates/engine/src/search/embed/quant.rs`
- Test: `crates/engine/src/search/embed/embed_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ModelId(pub String);                  // "granite-97m-r2@835ad14/p1", "static-mrl@b68f412/512i8/p1"
  pub const PIPELINE: &str = "p1";
  pub enum EmbedError { NotReady, Failed(String) }
  pub trait Embedder: Send + Sync {
      fn model(&self) -> &ModelId;
      fn dims(&self) -> usize;
      fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
  }
  pub struct HashEmbedder { … }   // HashEmbedder::new(model: &str): folded character trigrams hashed into 64 dims, L2-normalized
  pub struct TableEmbedder { … }  // TableEmbedder::new(model: &str, dims: usize, rows: Vec<(String, Vec<f32>)>); unknown text → Failed
  pub struct Q8 { pub vec: Vec<i8>, pub scale: f32 }
  pub fn quantize(v: &[f32]) -> Q8;
  pub fn dot(a: &Q8, b: &Q8) -> f32;
  pub fn dot_f32(q: &[f32], b: &Q8) -> f32;
  ```

- [ ] **Step 1: Write the failing tests:** `hash_embedder_is_deterministic_and_normalized`; `similar_text_scores_higher_under_hash` ("đăng nhập lỗi" closer to "dang nhap loi" than to "keep the mac awake"); `table_embedder_refuses_unknown_text`; `int8_dot_is_within_1_percent_of_f32` (random unit vectors, 384 dims, fixed seed).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::embed`. Expected: FAIL.
- [ ] **Step 3: Implement.** `dot` sums in `i32` lanes of 16 so it vectorizes.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: Embedder trait, HashEmbedder, TableEmbedder and int8 vectors`.

### Task 2: `query::fuse` and `query::shape`

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/query/fuse.rs`, `crates/engine/src/search/query/shape.rs`
- Test: `crates/engine/src/search/query/fuse_tests.rs`, `crates/engine/src/search/query/shape_tests.rs`

**Interfaces:**
- Produces:
  - `pub const K: f64 = 60.0; pub const RECENT_BONUS: f64 = 0.5;`
  - `pub struct List<'a> { pub ids: &'a [String], pub weight: f64 }`
  - `pub fn rrf(lists: &[List], k: f64) -> Vec<(String, f64)>`: `Σ w / (k + rank)` with rank starting at 1; sorted by score, ties by first appearance across lists in order.
  - `pub fn recency(fused: Vec<(String, f64)>, age_secs: &dyn Fn(&str) -> Option<f64>) -> Vec<(String, f64)>`: × `(1 + 0.5 × max(0, 1 − age / RECENT_SECS))`, re-sorted.
  - `pub fn text_only(query: &str) -> bool`: true for a URL or `claude.ai/…artifact` link, a bare artifact id (22 base62 characters), a path (contains `/` and no space, or starts with `~`), any quoted phrase, a hex token of 7+ characters or a UUID, or when every word is 2 characters or fewer.

- [ ] **Step 1: Write the failing tests:** `rrf_golden` (lists `[a, b, c]`, `[b, a]`, `[c]` with k = 60 give exact scores to 1e-12 and the order b, a, c); `a_missing_rank_adds_nothing`; `weights_scale_lists`; `recency_lifts_a_fresh_tie`; `text_only_shapes` (each shape above true; "đăng nhập lỗi", "how do I switch accounts" false).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::query::fuse` and `…::shape`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** them again. Expected: PASS.
- [ ] **Step 5: Commit** `search: query::fuse (RRF k=60, recency) and query::shape`.

### Task 3: The chunker

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/chunk.rs`
- Test: `crates/engine/src/search/chunk_tests.rs`

**Interfaces:**
- Consumes: `search::{TranscriptLine, SessionRole}`.
- Produces:
  - `pub struct Turn { pub prompt: String, pub reply: String, pub closed: bool }`
  - `pub fn turns(lines: impl IntoIterator<Item = TranscriptLine>, quiet: bool) -> Vec<Turn>`: a user message opens a turn; assistant text after it is its reply; the last turn is `closed` only when `quiet`.
  - `pub enum ChunkKind { Head, Turn, Page, Entry }`
  - `pub struct ChunkText { pub kind: ChunkKind, pub text: String }`
  - `pub fn session_chunks(title: &str, folder: &str, turns: &[Turn]) -> Vec<ChunkText>`: one head chunk (title, the folder's last part, the first prompt; ≤ 600 chars); each closed turn → windows ≤ 1,200 chars split at blank lines then sentence ends, 150 chars of overlap, each starting with the turn's prompt cut to 200 chars; a fenced code block keeps its first 20 lines; ≤ 6 windows a turn; ≤ 200 chunks a session, newest turns first.
  - `pub fn page_chunks(html: &str) -> Vec<ChunkText>`: tags and scripts stripped, sections at `h1`–`h3`, the same window rule.
  - `pub fn hash(text: &str) -> [u8; 16]`
  - `pub const QUIET_SECS: f64 = 60.0;`

- [ ] **Step 1: Write the failing tests:** `the_open_turn_waits`; `windows_respect_size_and_overlap`; `code_blocks_keep_twenty_lines`; `caps_six_windows_and_two_hundred_chunks`; `a_grown_transcript_keeps_old_hashes`; `page_sections_split_at_headings`; `cuts_never_split_a_character` (Vietnamese text at every boundary).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::chunk`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: chunker for sessions and pages`.

**Checkpoint A:** Tasks 1–3 are what P4a Tasks 5–7 need. Push the branch; if P4a is waiting, open a pull request for these three commits alone.

### Task 4: `vectors.db`

**Zone:** auto by path; trips **CAP-FILEWRITE** (it creates a folder, sets permissions, renames a damaged file), so the maintainer approves. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/store/mod.rs`, `crates/engine/src/search/store/vectors.rs`
- Test: `crates/engine/src/search/store/vectors_tests.rs`

**Interfaces:**
- Consumes: `Q8`, `ModelId` (Task 1); `ChunkText`, `hash` (Task 3).
- Produces: the spec's schema, verbatim (`docs`, `facets`, `chunks`, `vectors`, `meta`), `PRAGMA user_version = 1`.
  ```rust
  pub struct VectorDb;
  impl VectorDb {
      pub fn open(path: &Path) -> Result<VectorDb, String>;
      pub fn memory() -> Result<VectorDb, String>;
      pub fn stamps(&self, corpus: &str) -> HashMap<String, String>;
      pub fn put_doc(&self, doc: &Doc, chunks: &[ChunkText]) -> Result<(), String>;
      pub fn remove_docs(&self, corpus: &str, keys: &[String]) -> Result<(), String>;
      pub fn missing(&self, model: &ModelId, limit: usize) -> Vec<([u8; 16], String)>;
      pub fn put_vectors(&self, model: &ModelId, rows: &[([u8; 16], Vec<u8>)]) -> Result<(), String>;
      pub fn sweep(&self) -> Result<usize, String>;
      pub fn drop_model(&self, model: &ModelId) -> Result<(), String>;
      pub fn coverage(&self, model: &ModelId) -> Vec<Coverage>;
  }
  pub struct Coverage { pub corpus: String, pub docs: usize, pub chunks: usize, pub embedded: usize }
  ```
  Granite stored as f16 bytes, static as int8 with its scale (`vec` = 4-byte scale then the bytes). `Doc` is Task 6's type; define it here in `corpus/mod.rs` first if Task 6 isn't written yet.

- [ ] **Step 1: Write the failing tests:** `put_then_stamps_round_trip`; `remove_drops_chunks_and_facets`; `missing_lists_chunks_without_this_models_vector`; `sweep_removes_vectors_nothing_references`; `the_file_is_private_and_a_link_is_refused`; `a_damaged_file_is_set_aside`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::store::vectors`. Expected: FAIL.
- [ ] **Step 3: Implement** with `notchling_platform::files::ensure_private_dir`, the same open, symlink and damage handling as `fts.rs`.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: vectors.db`.

### Task 5: `VectorSet`, the masked scan

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/store/set.rs`
- Test: `crates/engine/src/search/store/set_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct VectorSet { … }   // one model: rows of (doc index, chunk hash, Q8); docs with key, corpus, at, interned facets
  impl VectorSet {
      pub fn load(db: &VectorDb, model: &ModelId) -> VectorSet;
      pub fn mask(&self, keep: &dyn Fn(&str, &[(&str, &str)], f64) -> bool) -> Vec<bool>;   // corpus, facets, at
      pub fn scan(&self, query: &[f32], top_chunks: usize, mask: Option<&[bool]>, floor: f32) -> Vec<DocHit>;
      pub fn len(&self) -> usize;
  }
  pub struct DocHit { pub corpus: String, pub key: String, pub chunk: [u8; 16], pub score: f32 }
  pub struct Sets { … }       // Mutex<HashMap<ModelId, Arc<VectorSet>>>: get() clones the Arc; swap() replaces it
  ```
  `scan` keeps the top `top_chunks` (300 in P6) above `floor`, then the best chunk per doc, sorted by score.

- [ ] **Step 1: Write the failing tests:** `masks_are_exact` (a mask off for doc A never yields A; all off yields nothing); `best_chunk_per_doc`; `floor_drops_low_scores`; `a_query_reads_a_snapshot_while_a_swap_happens` (scan on one thread, swap on another, no panic, old or new result whole); `scan_50k_is_fast` (50,000 × 384 random rows; assert under 20 ms in debug, a loose bound; the real number is P6's bench).
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::store::set`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: VectorSet with masked brute-force scan`.

### Task 6: Corpora

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none; `read` keeps its signature and calls the new function.

**Files:**
- Create: `crates/engine/src/search/corpus/mod.rs`, `crates/engine/src/search/corpus/sessions.rs`, `crates/engine/src/search/corpus/pages.rs`, `crates/engine/src/search/corpus/settings.rs`
- Test: `crates/engine/src/search/corpus/corpus_tests.rs`
- Modify: `crates/engine/src/search/index/read.rs` (new `pub(crate) fn read_with_lines(meta: &SessionMeta, tool: SessionTool) -> Option<(IndexedSession, Vec<TranscriptLine>)>`, which `read` calls, so the full-text index and the sessions corpus share one parser)

**Interfaces:**
- Consumes: `chunk::{turns, session_chunks, page_chunks}` (Task 3); `index::scan` candidates; `pages::PageLibrary`.
- Produces:
  ```rust
  pub enum Tier { Agent, App }
  pub struct DocStamp { pub key: String, pub stamp: String, pub at: f64 }
  pub struct Doc { pub key: String, pub title: String, pub at: f64, pub facets: Vec<(String, String)>, pub chunks: Vec<ChunkText> }
  pub trait Corpus: Send + Sync {
      fn id(&self) -> &'static str;
      fn tier(&self) -> Tier;
      fn scan(&self, window: Option<f64>, now: f64) -> Vec<DocStamp>;
      fn read(&self, doc: &DocStamp, now: f64) -> Option<Doc>;
  }
  pub struct SessionsCorpus;  // SessionsCorpus::new(roots: IndexRoots); stamp "<mtime>:<size>"; facets tool, project
  pub struct PagesCorpus;     // PagesCorpus::new(library: PageLibrary); stamp "<version>:<mtime>"; facets owner, project
  pub struct SettingsCorpus;  // SettingsCorpus::new(); set(docs: Vec<(String, String)>) from the app (P6's embed.settings); stamp = hash of the text
  ```

- [ ] **Step 1: Write the failing tests** over fixtures under a `TempDir` (copies of `crates/engine/tests/fixtures/` transcripts): `sessions_scan_is_stat_only` (a counting reader: `scan` reads no file); `sessions_read_gives_head_and_turns`; `pages_read_local_html`; `settings_stamp_changes_with_text`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::corpus`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine search::`. Expected: PASS.
- [ ] **Step 5: Commit** `search: Corpus trait with sessions, pages and settings`.

### Task 7: The pipeline pass

**Zone:** auto. CAP/WEAK: none beyond Task 4's store. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/pipeline/mod.rs`, `crates/engine/src/search/pipeline/pass.rs`
- Test: `crates/engine/src/search/pipeline/pass_tests.rs`

**Interfaces:**
- Consumes: Tasks 1, 4, 5, 6.
- Produces:
  ```rust
  pub struct Pipeline { … }
  impl Pipeline {
      pub fn new(db: Arc<VectorDb>, sets: Arc<Sets>) -> Pipeline;
      pub fn set_models(&self, now_embed: Vec<Arc<dyn Embedder>>, batched: Vec<ModelId>);   // static: now; granite: queued
      pub fn pass(&self, corpora: &[Arc<dyn Corpus>], window: Option<f64>, now: f64) -> PassStats;
      pub fn queue(&self) -> &Queue;
      pub fn accept(&self, model: &ModelId, rows: Vec<([u8; 16], Vec<f32>)>);   // results from the helper
      pub fn coverage(&self) -> Vec<(ModelId, Vec<Coverage>)>;
  }
  pub struct PassStats { pub read: usize, pub reused: usize, pub removed: usize, pub queued: usize, pub embedded_now: usize }
  ```
  Order inside a pass: settings, sessions newest first, pages, then any other corpus. A corpus not passed in loses its docs (an unchecked category). After `accept` or an immediate embed, the model's `VectorSet` is reloaded and swapped. When a model leaves `set_models`, its vectors stay until every chunk has a vector of its replacement, then `drop_model`.

- [ ] **Step 1: Write the failing tests:** `an_unchanged_stamp_reads_nothing`; `growth_embeds_only_new_chunks` (a `TableEmbedder` that records calls); `deletes_sweep_vectors`; `a_model_change_refills_from_stored_text` (no corpus `read` is called during the refill); `an_unpassed_corpus_is_emptied`; `static_embeds_at_once_granite_waits_in_the_queue`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::pipeline::pass`. Expected: FAIL.
- [ ] **Step 3: Implement.** No lock is held across a corpus read or an `embed` call.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: pipeline pass with stamps, deletes and model refill`.

### Task 8: The batch queue and the budget

**Zone:** auto. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/pipeline/queue.rs`, `crates/engine/src/search/pipeline/budget.rs`
- Test: `crates/engine/src/search/pipeline/queue_tests.rs`, `crates/engine/src/search/pipeline/budget_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const BATCH: usize = 8; pub const WAKE_AT: usize = 16; pub const WAKE_AFTER: f64 = 300.0; pub const IDLE_EXIT: f64 = 300.0;
  pub struct Queue { … }
  impl Queue {
      pub fn push(&self, model: &ModelId, rows: Vec<([u8; 16], String)>, now: f64);
      pub fn backlog(&self) -> bool;                 // set while a first index, a raised range or a model change is being worked
      pub fn set_backlog(&self, on: bool);
      pub fn due(&self, now: f64) -> bool;           // backlog and non-empty, or ≥ 16 pending, or the oldest ≥ 300 s
      pub fn take(&self, n: usize) -> Vec<([u8; 16], String)>;
      pub fn len(&self) -> usize;
  }
  pub struct Power { pub on_battery: bool, pub low_power: bool, pub thermal_serious: bool, pub user_paused: bool }
  pub enum Paused { No, User, LowPower, Thermal }
  pub struct Budget { … }
  impl Budget {
      pub fn new() -> Budget;
      pub fn paused(&self, power: &Power) -> Paused;
      pub fn rest_after(&mut self, cpu: Duration, wall: Duration, power: &Power) -> Duration;   // keeps cpu/wall ≤ 0.5, or 0.25 on battery
  }
  ```

- [ ] **Step 1: Write the failing tests:** `releases_at_sixteen`; `releases_after_five_minutes_on_a_fake_clock`; `a_backlog_releases_at_once`; `the_cpu_meter_keeps_half_a_core` (simulate 100 batches of 40 ms CPU in 40 ms wall: total wall ≥ 2 × total CPU − one batch); `battery_and_heat_lower_or_pause`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::pipeline`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: batch queue (16 chunks or 5 minutes) and CPU budget`.

### Task 9: Notes and the checks

**Zone:** maintainer (`crates/AGENTS.md`, `crates/engine/AGENTS.md`, `docs/feature-map.md`).

- [ ] **Step 1:** Notes: `vectors.db` and why it is its own file; ids and stamps; the open turn; the queue and the budget numbers; a `VectorSet` is swapped, never locked across a scan. Feature map: `search/embed/`, `search/store/`, `search/corpus/`, `search/pipeline/`, `chunk.rs`.
- [ ] **Step 2: Run** `cargo test --workspace --locked`, clippy, fmt, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`. Expected: clean.
- [ ] **Step 3: Commit** `docs: search store and pipeline`, and open the pull request (it trips CAP-FILEWRITE in Task 4).
