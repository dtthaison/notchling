# Search P4a: The Embedding Crate and the Eval Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `crates/embed` that runs granite-embedding-97m-multilingual-r2 in Rust (candle with Accelerate on macOS, our own `modernbert`, kitoken) with a bench, a static-model reader in the engine behind a cargo feature, and an eval tool that prints R@1 and R@5 for text-only against hybrid over the research sets or over a local data root with the maintainer's queries.

**Architecture:** `crates/embed` (package `notchling-embed`) is the only crate that links candle. Its library holds the granite model (`modernbert.rs` patched, f16 word table kept in the memory map), the kitoken tokenizer, the static-table converter and the eval's pure parts; its binary `notchling-embed` has `bench` and `convert-static` now and `serve` in P6. The static model's reader lives in the engine (`search/embed/static_model.rs`, feature `static-embed`) so the daemon can rank with it in-process. The eval tool, `crates/embed/examples/eval/main.rs`, builds text-only and hybrid rankings through the engine's own `SearchDb`, chunker and `query::fuse`, so it measures what ships.

**Tech Stack:** Rust 1.99.1; candle-core, candle-nn, candle-transformers 0.11.1 (`accelerate` on macOS only); kitoken 0.11.1 (BSD-2-Clause); safetensors; memmap2 0.9 and half 2 (already in `Cargo.lock`); `tokenizers` 0.22.2 as a dev-dependency only, for parity.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 3 (table, candle patch, memory, "Where each model runs"), 11, 12 P4a; answer 3; calls 3 and 4. Research: `docs/local/embedding-research.md` (maintainer's, git-ignored) and `tools/embed-eval/` on this branch. Plan set: `2026-11-14-search-plans-index.md` (calls 5, 6, 7).

**Branch:** `agent/search-p4a-embed`, from the integration branch. Tasks 1–4 start at once; Tasks 5–7 need P5 Tasks 1–3 (`Embedder`, `query::fuse`, `query::shape`, `chunk.rs`) merged or rebased in.

**Depends on:** P1 (`query::fold`, schema 2); P5 Tasks 1–3 for Tasks 5–7.

## Global Constraints

- No comments in code. The candle copy's license goes in `crates/embed/THIRD-PARTY.md` and `crates/embed/src/LICENSE-candle`, not in the file.
- No test downloads a model. Tests that need model files read them from `NOTCHLING_EMBED_MODELS` and are `#[ignore]` (each trips `WEAK-SKIP`; the maintainer approves). Every other test builds tiny weights in memory.
- Models are fetched only by the explicit development commands in "Models in development" below, into the git-ignored `tools/embed-eval/models/`; never in CI, never from a test.
- `cargo deny check` clean; every new crate's license is on `deny.toml`'s list; any exception is the maintainer's.
- The crate builds and its pure tests run on macOS, Linux and Windows; Accelerate is `[target.'cfg(target_os = "macos")'.dependencies]` only.
- No transcript text is committed: the maintainer's query file and any output over real data stay in `docs/local/`.
- Stage by path.

## Models in development

Run once on the Mac, by hand, when a task's ignored tests or the bench need them:

```sh
python3 -m venv tools/embed-eval/py/.venv
tools/embed-eval/py/.venv/bin/pip install 'huggingface_hub>=0.26'
tools/embed-eval/py/.venv/bin/python -c "from huggingface_hub import HfApi; a=HfApi(); [print(r, next(c.commit_id for c in a.list_repo_commits(r) if c.commit_id.startswith(p))) for r,p in [('ibm-granite/granite-embedding-97m-multilingual-r2','835ad14'),('sentence-transformers/static-similarity-mrl-multilingual-v1','b68f412')]]"
tools/embed-eval/py/.venv/bin/huggingface-cli download ibm-granite/granite-embedding-97m-multilingual-r2 --revision <full hash printed above> --local-dir tools/embed-eval/models/ibm-granite__granite-embedding-97m-multilingual-r2
tools/embed-eval/py/.venv/bin/huggingface-cli download sentence-transformers/static-similarity-mrl-multilingual-v1 --revision <full hash printed above> --local-dir tools/embed-eval/models/sentence-transformers__static-similarity-mrl-multilingual-v1
export NOTCHLING_EMBED_MODELS="$PWD/tools/embed-eval/models"
```

F2LLM-v2-330M (P4b only): the same `download` for `codefuse-ai/F2LLM-v2-330M` at the revision P4b records.

## Review Focus

1. Weights in f16 or bf16: the attention mask takes the weights' dtype, so a half-precision load gives the same vector as f32 within 1e-2 cosine. (Task 2 test `mask_follows_the_weights_dtype`.)
2. A batch mixing a 3-token query and a 1,200-character chunk: padding never changes the short text's vector. (Task 4 test `padding_does_not_change_a_vector`.)
3. A tokenizer edge: kitoken and `tokenizers` give the same ids for Vietnamese with combining marks, emoji, CRLF, tabs and 32k-character input. (Task 3 ignored test `kitoken_matches_tokenizers_on_the_check_strings`.)
4. A query file line that is malformed or names a doc the data root lacks: the eval says which line and skips it, never counts it as a miss. (Task 6 test `bad_lines_are_reported_not_scored`.)
5. A static table file truncated or with a wrong header: the reader refuses it and the daemon ranks without S. (Task 5 test `a_bad_table_is_refused`.)

---

### Task 1: The crate and its dependencies

**Zone:** maintainer (new crate `crates/embed/**`, `Cargo.lock`, `docs/feature-map.md`, `crates/AGENTS.md`; `deny.toml` only if an exception is needed). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/embed/Cargo.toml`, `crates/embed/src/lib.rs`, `crates/embed/src/main.rs`, `crates/embed/THIRD-PARTY.md`, `crates/embed/src/LICENSE-candle` (from `tools/embed-eval/rs/src/LICENSE-candle`), `crates/embed/AGENTS.md`
- Modify: `Cargo.lock`, `docs/feature-map.md` (a row for `crates/embed/`), `crates/AGENTS.md` (the crate table)

**Interfaces:**
- Produces: package `notchling-embed`, binary `notchling-embed` (prints usage and exits 2 for now), `[features] default = []`; `[target.'cfg(target_os = "macos")'.dependencies]` turns on candle's `accelerate`.

- [ ] **Step 1: Run** `cargo build -p notchling-embed --locked` after writing the manifest. Expected: builds.
- [ ] **Step 2: Run** `cargo deny check`. Expected: clean. If a crate's license is not allowed, stop and write it in the pull request for the maintainer; do not edit `deny.toml` yourself.
- [ ] **Step 3: Run** `cargo tree -p notchling-embed -e normal --target x86_64-unknown-linux-gnu | grep -c accelerate`. Expected: `0`.
- [ ] **Step 4: Run** `PYTHONPATH=tools/guard python3 -m guard featuremap` and `python3 -m guard lint`. Expected: clean.
- [ ] **Step 5: Commit** `embed: notchling-embed crate with candle 0.11.1 and kitoken`.

### Task 2: Our `modernbert` with the mask fix

**Zone:** maintainer (`crates/embed/**`). CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/embed/src/modernbert.rs` (from `tools/embed-eval/rs/src/modernbert.rs`: SiLU gate, no `model.` key prefix, comments stripped; plus the mask in the weights' dtype)
- Test: `crates/embed/src/modernbert_tests.rs`

**Interfaces:**
- Produces: `pub struct ModernBert`, `pub struct Config` (serde from `config.json`, with `hidden_activation`), `ModernBert::load(vb: VarBuilder, cfg: &Config) -> candle_core::Result<ModernBert>`, `fn forward(&self, ids: &Tensor, mask: &Tensor) -> candle_core::Result<Tensor>`.

- [ ] **Step 1: Write the failing tests** with a 2-layer, 32-dim random `VarMap` (fixed seed): `silu_config_is_honored` (a config with `hidden_activation: "silu"` differs from `"gelu"` on the same weights); `mask_follows_the_weights_dtype` (the same weights in f32 and f16 give CLS vectors with cosine ≥ 0.99); `keys_have_no_model_prefix`.
- [ ] **Step 2: Run** `cargo test -p notchling-embed modernbert`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `embed: modernbert with SiLU, plain keys and a dtype-following mask`.

### Task 3: kitoken, with parity

**Zone:** maintainer. CAP/WEAK: **WEAK-SKIP** (the ignored parity test). Existing tests edited: none.

**Files:**
- Create: `crates/embed/src/tokens.rs`, `crates/embed/src/check_strings.rs` (200 fixed strings: Vietnamese precomposed and combining, unaccented Vietnamese, English, Rust and shell code, emoji, CRLF, tabs, and one of 32,000 characters)
- Test: `crates/embed/src/tokens_tests.rs`

**Interfaces:**
- Produces: `pub struct Tokens`, `Tokens::from_tokenizer_json(path: &Path) -> Result<Tokens, String>` (`Kitoken::from_tokenizers_file`), `fn encode(&self, text: &str, max: usize) -> Vec<u32>` (special tokens on, cut to `max`), `fn cls_id(&self) -> u32`, `fn pad_id(&self) -> u32`.

- [ ] **Step 1: Write the tests:** `cut_to_max_keeps_the_specials` (a tiny BPE `tokenizer.json` written in the test); `kitoken_matches_tokenizers_on_the_check_strings` (`#[ignore]`; reads granite-r2's and static-mrl's `tokenizer.json` from `NOTCHLING_EMBED_MODELS`; `tokenizers` is the dev-dependency; asserts identical ids for all 200 strings and every text in `tools/embed-eval/data/*.json`).
- [ ] **Step 2: Run** `cargo test -p notchling-embed tokens`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-embed tokens` (PASS), then with models present `cargo test -p notchling-embed tokens -- --ignored`. Expected: PASS for granite-r2. If static-mrl's WordPiece differs, record the first differing string in the pull request: call 4 then moves the static reader (Task 5) to `tokenizers` behind the engine feature, after the maintainer's `cargo deny` review.
- [ ] **Step 5: Commit** `embed: kitoken tokenizer with a parity test against tokenizers`.

### Task 4: The granite model and the bench

**Zone:** maintainer. CAP/WEAK: **WEAK-SKIP** (ignored tests); the bench's RSS read uses `libc::getrusage` (no process start). Existing tests edited: none.

**Files:**
- Create: `crates/embed/src/granite.rs`, `crates/embed/src/bench.rs`
- Test: `crates/embed/src/granite_tests.rs`
- Modify: `crates/embed/src/main.rs` (`notchling-embed bench <model dir> [--threads 1] [--budget 0.5]`)

**Interfaces:**
- Consumes: `ModernBert` (Task 2), `Tokens` (Task 3).
- Produces: `pub struct Granite`; `Granite::load(dir: &Path) -> Result<Granite, String>` (memory-mapped safetensors; the word table stays f16 in the map and rows are gathered per batch); `fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>` (CLS pooling, L2-normalized, 384 dims, max 512 tokens a text); `pub const MODEL: &str = "granite-97m-r2@835ad14/p1"`. The bench prints one `bench-json:` line: `{load_ms, query_p50_ms, query_p95_ms, chunks_per_s, chunks_per_s_at_budget, rss_mb, peak_rss_mb}` over 240 queries from `tools/embed-eval/data/*.json` and 64 chunks of 1,200 characters in batches of 8.

- [ ] **Step 1: Write the tests:** `padding_does_not_change_a_vector` (tiny random model: a text alone and in a padded batch give cosine ≥ 0.9999); `granite_gates` (`#[ignore]`, real model: query p50 ≤ 5 ms; chunks/s at the 50% budget ≥ 12; peak RSS ≤ 350 MB; printed and asserted).
- [ ] **Step 2: Run** `cargo test -p notchling-embed granite`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-embed granite` (PASS), then on the Mac `cargo test --release -p notchling-embed granite_gates -- --ignored --nocapture` and `cargo run --release -p notchling-embed -- bench "$NOTCHLING_EMBED_MODELS/ibm-granite__granite-embedding-97m-multilingual-r2"`. Expected: the three gates hold. The research measured 23.8 chunks/s on one thread, so the budget figure sits near 12: report the number either way, and if it is under 12, say so in the pull request for the maintainer.
- [ ] **Step 5: Commit** `embed: granite-97m-r2 in candle with Accelerate, and its bench`.

### Task 5: The static model, in the engine

**Zone:** auto path (`crates/engine/src/search/**`) plus maintainer (`crates/engine/Cargo.toml` feature, `crates/embed/**` for the converter). CAP/WEAK: **CAP-FILEWRITE** (the converter writes the table), **WEAK-SKIP** (ignored tests). Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/embed/static_model.rs` (`#[cfg(feature = "static-embed")]`), `crates/embed/src/convert.rs`
- Test: `crates/engine/src/search/embed/static_model_tests.rs`, `crates/embed/src/convert_tests.rs`
- Modify: `crates/engine/Cargo.toml` (`[features] static-embed = ["dep:kitoken", "dep:memmap2"]`), `crates/engine/src/search/embed/mod.rs`, `crates/embed/src/main.rs` (`notchling-embed convert-static <model dir> <out file>`)

**Interfaces:**
- Consumes: `Embedder`, `ModelId`, `quantize` (P5 Task 1).
- Produces:
  - The table file: magic `NLST1\0\0\0`, `u32` rows, `u32` dims (512), then `rows` f32 scales, then `rows × dims` int8, little-endian.
  - `pub fn convert(model_dir: &Path, out: &Path) -> Result<String, String>`: reads `0_StaticEmbedding/model.safetensors`, keeps the first 512 dims, quantizes per row, writes through a `.part` file with no-follow and renames; returns the new file's SHA-256 hex.
  - `pub struct StaticEmbedder`; `StaticEmbedder::open(table: &Path, tokenizer: &Path) -> Result<StaticEmbedder, String>`; `impl Embedder` with model `"static-mrl@b68f412/512i8/p1"`, 512 dims: mean of the token rows (specials off), L2-normalized.

- [ ] **Step 1: Write the tests:** `round_trips_a_tiny_table` (convert a 10-row, 1,024-dim safetensors written in the test; open it; a text of known ids gives the expected mean); `a_bad_table_is_refused` (wrong magic, truncated rows); `static_matches_the_research` (`#[ignore]`, real model: "dang nhap" and "đăng nhập" have cosine ≥ 0.8; R@1 on `tools/embed-eval/data/chat.json`'s unaccented Vietnamese queries is within 0.05 of `res_static-mrl-multilingual.json`).
- [ ] **Step 2: Run** `cargo test -p notchling-engine --features static-embed search::embed::static_model` and `cargo test -p notchling-embed convert`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** both (PASS); with models, `cargo run --release -p notchling-embed -- convert-static "$NOTCHLING_EMBED_MODELS/sentence-transformers__static-similarity-mrl-multilingual-v1" /tmp/static-512i8.bin` (about 54 MB) and the ignored test with `NOTCHLING_EMBED_STATIC_TABLE=/tmp/static-512i8.bin`.
- [ ] **Step 5: Commit** `search: static model reader behind static-embed; embed: convert-static`.

### Task 6: The eval tool

**Zone:** maintainer (`crates/embed/**`). CAP/WEAK: **WEAK-SKIP** (the ignored reproduction test). Existing tests edited: none.

**Files:**
- Create: `crates/embed/src/eval/mod.rs` (pure: query file, metrics, groups), `crates/embed/examples/eval/main.rs`
- Test: `crates/embed/src/eval/eval_tests.rs`
- Modify: `crates/embed/Cargo.toml` (`[dev-dependencies] notchling-engine = { path = "../engine", features = ["static-embed"] }`)

**Interfaces:**
- Consumes: `SearchDb::memory`, `query::{fold, fuse::rrf, shape::text_only}`, `chunk::{turns, session_chunks}`, `corpus::SessionsCorpus` (P5), `Granite` (Task 4), `StaticEmbedder` (Task 5).
- Produces:
  - Query file, JSON lines: `{"q": string, "want": [doc id], "lang": string?}`; doc ids are session keys (`claude:<id>`, `codex:<id>`).
  - `pub fn recall(rankings: &[Vec<String>], wants: &[Vec<String>], at: usize) -> f64`
  - `pub struct Report { pub rows: Vec<(String, Scores)> }` with groups `all`, each `lang`, and `exact` (queries where `text_only` is true); `Scores { r1: f64, r5: f64, n: usize }` for `text`, `text+granite`, `text+granite+static`, and `text+<extra>` when `--extra-model` is given.
  - Commands:
    ```sh
    cargo run --release -p notchling-embed --example eval -- --set tools/embed-eval/data/chat.json --models "$NOTCHLING_EMBED_MODELS"
    cargo run --release -p notchling-embed --example eval -- --queries docs/local/search-queries.jsonl --data-root <root with claude/ and codex/> --models "$NOTCHLING_EMBED_MODELS" [--json]
    ```
  - `--data-root` reads sessions with no time window, chunks them as the pipeline does, and never writes outside a temp folder.
  - The hybrid columns apply the production rule: a query `text_only` calls exact-looking is ranked by text alone.
  - `--find "<words>"` prints the text-only top 10 as `key  title  folder`, so the maintainer can label `want` without opening files.
  - `--sweep-floors` adds, per model, R@5 at each floor from 0.00 to 0.90 in steps of 0.05 and the share of meaning-only hits below each floor.
  - `--verbose` prints, per query, each ranking's top 5 keys; without it the output holds counts and scores only, never transcript text.

- [ ] **Step 1: Write the tests:** `recall_at_one_and_five`; `bad_lines_are_reported_not_scored`; `exact_group_uses_text_only` (a query `"foo/bar.rs"` lands in `exact` and its hybrid ranking equals its text ranking); `a_floor_of_zero_changes_nothing`; `eval_reproduces_research_numbers` (`#[ignore]`, real models: over `tools/embed-eval/data/chat.json` and `settings.json`, `text+granite+static` R@1 is within 0.05 of 0.97 (chat) and 0.81 (settings), and within 0.05 of 1.00 on chat's `vi-nodiac` group).
- [ ] **Step 2: Run** `cargo test -p notchling-embed eval`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-embed eval` (PASS); then `cargo test --release -p notchling-embed eval_reproduces -- --ignored --nocapture`. Expected: PASS.
- [ ] **Step 5: Commit** `embed: eval tool for text-only against hybrid`.

### Task 7: F2LLM-v2-330M in the eval, and the license check

**Zone:** maintainer. CAP/WEAK: none beyond the crate. Existing tests edited: none.

**Files:**
- Create: `crates/embed/examples/eval/qwen3.rs` (from `tools/embed-eval/rs/src/qwen3.rs`, comments stripped; last-token pooling with the instruct prefix of `tools/embed-eval/py/evalq.py`'s `f2llm-v2-330m`), used by `eval/main.rs`'s `--extra-model f2llm <dir>`
- Modify: `crates/embed/THIRD-PARTY.md` (the qwen3 copy), `crates/embed/AGENTS.md` (a Models section: the cards checked, below)

- [ ] **Step 1: Run** the extra model over the research set: `cargo run --release -p notchling-embed --example eval -- --set tools/embed-eval/data/chat.json --models "$NOTCHLING_EMBED_MODELS" --extra-model f2llm "$NOTCHLING_EMBED_MODELS/codefuse-ai__F2LLM-v2-330M"`. Expected: its dense R@1 within 0.05 of `res_f2llm-v2-330m.json`.
- [ ] **Step 2: Check the licenses (answer 3):** read the granite-97m-r2 model card at its pinned revision and the card of the base encoder it names, and note whether either names terms other than Apache-2.0 for the tokenizer (the special tokens are `<|startoftext|>`-style; the 311m-r2 sibling's card names Gemma terms, the 97m-r2 card did not at `835ad14`). Write what each card says, with its URL and revision, in `crates/embed/AGENTS.md` and in the pull request. Do not decide; P4b confirms.
- [ ] **Step 3: Run** `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `cargo deny check`, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`. Expected: clean.
- [ ] **Step 4: Commit** `embed: F2LLM-v2-330M in the eval; model card notes`, and open the pull request with the bench line, the gates and the reproduced numbers.
