# Search P7: Model Download and the Semantic Search UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Settings > Search gets the Semantic Search switch and, once it is on, a list of models by purpose (what each is good for, its size and speed) with Download, progress and Cancel, Remove Model, and Pause; downloads are pinned, hashed and limited to known hosts; the helper ships in the app bundle; the privacy page says what is fetched.

**Architecture:** A compiled-in manifest `crates/engine/data/models.json` (like `catalog.json`). A streaming fetcher in `crates/engine/src/search/models/` behind a `Fetch` trait with a fake: HTTPS over the engine's rustls, redirects only to the manifest's hosts, `.part` files written without following links, SHA-256 and size checked before the rename, Range resume. The daemon's `embed.fetch`, `embed.fetch.cancel` and `embed.remove` (app tier, only while `semanticSearch` is on) run it and report progress on the `embed` topic; after the static model's files check, the daemon runs `notchling-embed convert-static`. `platform::power` gives the budget its battery, Low Power Mode and thermal state, and puts the indexing thread at background priority.

**Tech Stack:** Rust; rustls 0.23 with ring and `rustls-native-certs` (already engine dependencies); `sha2`; GPUI 0.2.2 and `crates/kit`; `crates/locale`; `objc2-foundation` for `NSProcessInfo` (already in `Cargo.lock`).

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 3 ("The download", "The static table"), 5 ("Budget, pause and progress"), 8 (the Search section's row 2), 9 (`embed.fetch`, `.cancel`, `embed.remove`, the topic), 10, 12 P7; answer 4. Plan set: `2026-11-14-search-plans-index.md` (call 14 there: a streaming `Fetch` fake stands in for the spec's `FakeHttp`, which holds whole bodies in memory).

**Branch:** `agent/search-p7-download`, from the integration branch with P6 merged.

**Depends on:** P6 (keys, `embed_ops.rs`, model folders); P4b for whether the Large row is listed and for the pinned revisions and hashes; L1 for words.

## Global Constraints

- No comments in code; files under 1,000 lines (600 warns); tests in `*_tests.rs`.
- No test touches the network or downloads a model: every fetch test uses the fake. A real download happens only from the app's Download button, or the development commands of P4a.
- A stored setting never starts a download; only `embed.fetch` does, from the app tier, while `semanticSearch` is on.
- Files: `<data>/models/<model folder>/` 0700; files 0600, written as `<file>.part` with no-follow, renamed only after size and SHA-256 both match; a mismatch deletes the `.part`.
- Requests: `GET https://huggingface.co/<repo>/resolve/<revision>/<path>`, `User-Agent: Notchling`, no other header that identifies the user; redirects followed only to hosts the manifest lists, at most 5.
- Every model and file license in the manifest is on `deny.toml`'s allow list.
- Words in `locales/en/mac/settings.ftl`, the spec's English verbatim; Title Case for buttons ("Download", "Cancel", "Remove Model", "Pause", "Resume").
- Stage by path.

## Review Focus

1. A download interrupted by sleep and resumed after the server changed the file: the resumed hash fails, the `.part` is deleted and the row offers Download again. (Task 2 test `a_changed_file_fails_its_hash_after_resume`.)
2. A redirect to a host the manifest doesn't list (or to `http://`): refused, nothing written. (Task 2 test `an_unlisted_redirect_is_refused`.)
3. A symlink planted at the `.part` path: refused before a byte is written. (Task 2 test `no_part_through_a_symlink`.)
4. Semantic Search turned off during a download: the download is cancelled and its `.part` kept for a later resume. (Task 4 test `turning_off_cancels_and_keeps_the_part`.)
5. A full disk mid-download: the fetch fails with a plain message, the `.part` is deleted, and full text still works. (Task 2 test `a_write_error_cleans_up`.)

---

### Task 1: The manifest

**Zone:** maintainer (`crates/engine/data/**`); auto for `crates/engine/src/search/models/manifest.rs`. CAP/WEAK: none. Existing tests edited: none.

**Files:**
- Create: `crates/engine/data/models.json`, `crates/engine/src/search/models/mod.rs`, `crates/engine/src/search/models/manifest.rs`
- Test: `crates/engine/src/search/models/manifest_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Manifest { pub hosts: Vec<String>, pub models: Vec<ModelEntry> }
  pub struct ModelEntry { pub id: String, pub folder: String, pub tier: String, pub listed: bool, pub recommended: bool,
                          pub message: String, pub download_bytes: u64, pub kept_bytes: u64, pub files: Vec<ModelFile> }
  pub struct ModelFile { pub repo: String, pub revision: String, pub path: String, pub save_as: String, pub size: u64, pub sha256: String, pub license: String }
  pub fn manifest() -> &'static Manifest;   // include_str!("../../../data/models.json"), parsed once
  ```
  Entries: `granite-97m-r2@835ad14/p1` (folder `granite-97m-r2`, tier `standard`, recommended, message `settings-model-standard`, files `config.json`, `model.safetensors`, `tokenizer.json`), `static-mrl@b68f412/512i8/p1` (folder `static-mrl`, tier `helper`, recommended, message `settings-model-accent-helper`, files `0_StaticEmbedding/model.safetensors` saved as `table-f32.safetensors`, `tokenizer.json`), and the Large entry only with `listed: false` unless P4b listed it. Revisions are full commit hashes; sizes and SHA-256s come from the files P4a's development download fetched: `shasum -a 256 tools/embed-eval/models/<folder>/<path>` and `stat -f %z`.

- [ ] **Step 1: Write the failing tests:** `licenses_are_within_deny_toml` (the test reads `deny.toml` from the workspace root and parses `[licenses] allow`); `every_file_has_a_sha256_and_a_size`; `ids_match_the_daemons_model_ids` (granite's id is `"granite-97m-r2@835ad14/p1"`, P4a's `granite::MODEL`; static's equals `StaticEmbedder`'s model id); `hosts_are_https_names_only`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::models::manifest`. Expected: FAIL.
- [ ] **Step 3: Write** `models.json` and `manifest.rs`.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: model manifest with pinned revisions and hashes`.

### Task 2: The fetcher

**Zone:** auto path (`crates/engine/src/search/**`); trips **CAP-NETWORK** (`TcpStream` in `net.rs`) and **CAP-FILEWRITE** (the `.part` file, rename, delete). Existing tests edited: none.

**Files:**
- Create: `crates/engine/src/search/models/fetch.rs`, `crates/engine/src/search/models/net.rs` (the real `Fetch` over rustls with native roots), `crates/engine/src/search/models/fake.rs`
- Test: `crates/engine/src/search/models/fetch_tests.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Head { pub status: u16, pub location: Option<String>, pub length: Option<u64>, pub range_start: Option<u64> }
  pub trait Fetch: Send + Sync {
      fn get(&self, url: &str, from: u64, body: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<Head, String>;
  }
  pub struct TlsFetch;            // net.rs
  pub struct FakeFetch { … }      // fake.rs: scripted heads and bodies per URL, records requests and their Range
  pub struct Progress { pub bytes: u64, pub total: u64 }
  pub fn fetch_file(net: &dyn Fetch, hosts: &[String], file: &ModelFile, dir: &Path,
                    cancel: &AtomicBool, progress: &dyn Fn(Progress)) -> Result<PathBuf, String>;
  ```
  An existing `.part` is hashed first, then resumed with `Range: bytes=<len>-`; a 200 answer to a Range request starts over; more bytes than `size` aborts; the rename happens only when size and hash match.

- [ ] **Step 1: Write the failing tests:** `a_good_file_lands_and_matches`; `a_wrong_hash_is_deleted`; `too_many_bytes_aborts`; `an_unlisted_redirect_is_refused` (and `http://` to a listed host); `range_resume_continues_the_part`; `a_changed_file_fails_its_hash_after_resume`; `no_part_through_a_symlink`; `a_write_error_cleans_up`; `cancel_keeps_the_part`; `requests_carry_only_the_user_agent`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine search::models::fetch`. Expected: FAIL.
- [ ] **Step 3: Implement** with `notchling_platform::files::ensure_private_dir` and a no-follow open.
- [ ] **Step 4: Run** it again. Expected: PASS.
- [ ] **Step 5: Commit** `search: pinned model fetcher with resume and host allowlist`.

### Task 3: `platform::power`

**Zone:** maintainer (`crates/platform/**`). CAP/WEAK: none expected. Existing tests edited: none.

**Files:**
- Create: `crates/platform/src/power.rs`
- Test: `crates/platform/src/power_tests.rs`
- Modify: `crates/platform/src/lib.rs`, `crates/engine/src/search/pipeline/budget.rs` (nothing but a `From<platform::power::State> for Power`)

**Interfaces:**
- Produces: `pub struct State { pub on_battery: bool, pub low_power: bool, pub thermal_serious: bool }`, `pub fn state() -> State` (macOS: `NSProcessInfo` `isLowPowerModeEnabled` and `thermalState`, IOKit's power source; Linux: `/sys/class/power_supply/*/online` read only; Windows: all false), `pub fn background_thread(on: bool) -> Result<(), Error>` (macOS QoS background or default; Linux `nice` 19; Windows background mode; else `Unsupported`). `pub fn parse_power_supply(entries: &[(String, String)]) -> bool` is the pure Linux part.

- [ ] **Step 1: Write the failing tests:** `linux_parsing_finds_battery` (pure); `state_never_panics` (any system).
- [ ] **Step 2: Run** `cargo test -p notchling-platform power`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it again on the Mac; CI covers Linux and Windows. Expected: PASS.
- [ ] **Step 5: Commit** `platform: power state and a background thread priority`.

### Task 4: `embed.fetch`, `embed.fetch.cancel`, `embed.remove` and the static conversion

**Zone:** maintainer (`crates/notchlingd/**`, `crates/local-api/**`). CAP/WEAK: **CAP-PROCESS** (runs `notchling-embed convert-static`), **CAP-FILEWRITE** (deletes the f32 table and removed models). Existing tests edited: none.

**Files:**
- Create: `crates/notchlingd/src/embed_fetch.rs`
- Test: `crates/notchlingd/src/embed_fetch_tests.rs`
- Modify: `crates/local-api/src/wire.rs` (`EMBED_FETCH = "embed.fetch"`, `EMBED_FETCH_CANCEL = "embed.fetch.cancel"`, `EMBED_REMOVE = "embed.remove"`), `crates/notchlingd/src/embed_ops.rs` (route them; `embed.changed`'s `fetch: {model, bytes, total}`; the worker reads `platform::power::state()` for the budget and calls `background_thread(true)` on the indexing thread)

**Interfaces:**
- Consumes: `manifest()`, `fetch_file`, `Fetch` (Tasks 1–2); `embed_paths::helper_exe` (P6).
- Produces:
  - `embed.fetch {model}` → `{state: "downloading"}` at once (`APP` tier; `forbidden` with "Turn on Semantic Search first." while `semanticSearch` is off; `bad_request` for an id not listed). Progress by event at most once a second; at the end `{model, state: "ready" | "failed", error?}` in `embed.changed`.
  - After the static model's two files check: run `<helper> convert-static <folder> <folder>/table-512i8.bin`, record the hash it prints in `<folder>/table-512i8.sha256` (0600), delete `table-f32.safetensors`.
  - `embed.fetch.cancel {model}` → `{cancelled: bool}`; `.part` files stay.
  - `embed.remove {model}` → `{removed: bool}`: unload it, delete its folder and `VectorDb::drop_model`.
  - Turning `semanticSearch` off cancels every download.

- [ ] **Step 1: Write the failing tests** with `FakeFetch` and the fake spawner: `fetch_is_app_tier`; `fetch_is_refused_while_off`; `fetch_answers_at_once_and_reports_progress`; `the_static_model_is_converted_and_the_f32_file_deleted`; `remove_deletes_files_and_vectors`; `turning_off_cancels_and_keeps_the_part`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd embed_fetch`. Expected: FAIL.
- [ ] **Step 3: Implement.** The fetch runs on its own thread; no lock is held across it.
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `notchlingd: model download, cancel and remove`.

### Task 5: The Semantic Search rows

**Zone:** auto (`crates/app/**`, `locales/en/mac/settings.ftl`). CAP/WEAK: none. Existing tests edited: `crates/app/src/sheets/settings_entries_tests.rs`'s committed id list gains `semanticSearch` and `semanticPaused`, and `NOT_SHOWN` loses them (in-source; listed in the pull request).

**Files:**
- Create: `crates/app/src/sheets/semantic.rs` (pure), `crates/app/src/sheets/semantic_view.rs` (macOS)
- Test: `crates/app/src/sheets/semantic_tests.rs`
- Modify: `crates/app/src/sheets/settings.rs` (row 2 of the Search section), `crates/app/src/sheets/settings_view.rs`, `crates/app/src/sheets/renders.rs` (`settings-semantic-off`, `settings-semantic-models`, `settings-semantic-downloading`, `settings-semantic-indexing`), `crates/app/src/mac/views.rs`, `crates/app/src/sheets/settings_entries.rs`, `locales/en/mac/settings.ftl`

**Interfaces:**
- Consumes: `manifest()`; the embed feed (P6 Task 6).
- Produces:
  - `pub enum ModelRow { Absent, Downloading { bytes: u64, total: u64 }, Ready, Failed(String) }`
  - `pub struct SemanticModel { pub on: bool, pub rows: Vec<(&'static ModelEntry, ModelRow)>, pub indexing: Option<(u64, u64)>, pub paused: bool }` and `pub fn model(on: bool, status: &Value, changed: Option<&Value>) -> SemanticModel` (only `listed` entries; Standard and Accent Helper first, marked Recommended).
  - Words, verbatim from the spec: `settings-model-standard` "Standard", `settings-model-standard-about` "Good for English and Vietnamese. 195 MB. About 4 ms a search; the first index takes about 20 minutes.", `settings-model-accent-helper` "Accent Helper", `settings-model-accent-helper-about` "Finds Vietnamese typed without accents. 434 MB download, 54 MB kept. Instant.", `settings-model-large` "Large", `settings-model-large-about` "Better on hard questions. 669 MB. Slower to search and index.", `settings-semantic-indexing` "Indexing { NUMBER($done) } of { NUMBER($total) } passages".
  - Buttons: Download (sends `embed.fetch` and adds the id to `semanticModel` in the store), Cancel, Remove Model (sends `embed.remove`, removes the id), Pause and Resume (write `semanticPaused`, then `embed.refresh`). Every call off the UI thread.

- [ ] **Step 1: Write the failing tests:** `off_shows_only_the_switch`; `on_lists_models_by_purpose`; `large_is_hidden_unless_listed`; `progress_and_cancel_while_downloading`; `indexing_text_counts`.
- [ ] **Step 2: Run** `cargo test -p notchling-app sheets::semantic`. Expected: FAIL.
- [ ] **Step 3: Implement**; words; `python3 tools/locale/gen.py`.
- [ ] **Step 4: Run** the tests; render the four views in light and dark with `NOTCHLING_RENDER_FORCE_FRAMES=1` and look.
- [ ] **Step 5: Commit** `settings: Semantic Search switch, model list, download and pause`.

### Task 6: The helper in the bundle

**Zone:** maintainer (`tools/**`). CAP/WEAK: none expected. Existing tests edited: none.

**Files:**
- Modify: `tools/bundle-next.sh` (build `notchling-embed` in release with the app, copy it to `Contents/Helpers/notchling-embed`, sign it ad hoc like `notchling-ptyhost`)

- [ ] **Step 1: Run** `tools/bundle-next.sh` (never `--install`). Expected: `build/next/Notchling.app/Contents/Helpers/notchling-embed` exists.
- [ ] **Step 2: Run** `codesign -v build/next/Notchling.app/Contents/Helpers/notchling-embed` and `build/next/Notchling.app/Contents/Helpers/notchling-embed` (no arguments). Expected: valid; usage, exit 2.
- [ ] **Step 3: Run** `du -h build/next/Notchling.app/Contents/Helpers/notchling-embed`. Expected: about 8 to 10 MB; write the number in the pull request.
- [ ] **Step 4: Commit** `tools: bundle notchling-embed in Contents/Helpers`.

### Task 7: The privacy page

**Zone:** maintainer (`docs/policies/privacy.md`). Existing tests edited: none.

- [ ] **Step 1:** Add "Semantic search models (Hugging Face)": nothing is fetched until the user turns Semantic Search on and presses Download; which files from which repositories at which revisions; the request carries only `User-Agent: Notchling`; Hugging Face sees the Mac's IP address and the files asked for; files are checked by SHA-256; embeddings are computed on the Mac and never leave it; Remove Model deletes the files and their vectors.
- [ ] **Step 2: Commit** `docs: privacy page names the model download`.

### Task 8: Notes and the checks

**Zone:** maintainer (`crates/AGENTS.md`, `crates/engine/AGENTS.md`, `crates/notchlingd/AGENTS.md`, `crates/platform/AGENTS.md`, `docs/feature-map.md`, `docs/local-api-ops.md`).

- [ ] **Step 1:** Notes: the manifest and its license test; the fetcher's rules; what a stored setting may never start; `platform::power`; the ops' wire shapes.
- [ ] **Step 2: Run** `cargo test --workspace --locked`, clippy, fmt, `cargo deny check`, `python3 tools/strip_comments.py --check`, `PYTHONPATH=tools/guard python3 -m guard lint`, `python3 -m guard featuremap`, `python3 tools/locale/gen.py --check`. Expected: clean.
- [ ] **Step 3: Live check (the maintainer's call):** a fixture root never talks to a daemon, so this runs the bundle from Task 6 over the real folders: open it from `build/next/` without `--install`, turn Semantic Search on, Download Accent Helper; the row shows progress, then ready; `ls -l` shows `table-512i8.bin` 0600 and no `table-f32.safetensors`. This is a real download of 434 MB.
- [ ] **Step 4: Commit** `docs: semantic search download`, and open the pull request (CAP-NETWORK, CAP-FILEWRITE, CAP-PROCESS, policy).
