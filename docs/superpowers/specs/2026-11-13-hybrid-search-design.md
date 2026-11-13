# Hybrid search for Notchling

Design, revision 4, 2026-11-14: revision 3 with the user's answers after review. Revision 3 added the calls the coordinator settled after a builder's review. Read against `agent/move-auto-trust` (8e73a97) and the open branches named below. Labels: **[M]** measured on this Mac (Apple M5, 32 GB, macOS 26; see `embedding-research.md`), **[S]** from a source, **[E]** estimate.

## The user's answers after review

The user answered every question revision 3 left open. Each answer below is the user's decision and is worked into the sections named; no question is open any more.

1. **Eval pass criterion** (user's decision).
   - **Who.** The maintainer writes about 40 queries over his own sessions, kept local and never committed, and runs P4b.
   - **Pass.** Three conditions:
     - hybrid beats text-only by at least 0.15 R@5 overall;
     - no language is worse than with text-only;
     - exact-looking queries (the text-only shapes of section 6) are no worse.
   - *Sections 12, 13.*
2. **Agent classes follow one rule for all settings** (user's decision).
   - **The rule.** A cosmetic change (look, order, display) is Free: an agent makes it with no confirmation. A sensitive change is Ask. Its Inbox item says in plain words what the change does before the user allows it: the setting's title, the old and the new value, and a one-line "what this does" from the registry.
   - **Applied here:**
     - Ask: `searchIndexRange`, turning a category on or off, and `semanticSearch`.
     - Free: reordering categories.
     - Never: `semanticModel` (only the app's Download fetches).
   - **The split.** Categories become two keys, because 0020 classes a key, not a change: `searchCategories` (which are on; Ask) and `searchCategoryOrder` (their order; Free).
   - **Registry requirement.** Every Ask key has an effect text, and a test fails until it does.
   - **Feeds back into decision 0020's table.** Its Ask keys get effect texts, and its Ask item shows the title, the old value, the new value and the effect text.
   - *Sections 2, 8, 10, 12.*
3. **Granite-r2's tokenizer license** (user's decision). Go ahead with granite-97m-r2.
   - **Checking it.** P4a checks the cards of the model and its base encoder, and P4b confirms before the decision record.
   - **Fallback.** granite-embedding-107m-multilingual.
   - *Sections 3, 12.*
4. **Semantic Search stays Off by default, and nothing loads until it's on** (user's decision).
   - **Off.** Nothing is downloaded or loaded, the static model included, until the user turns it on.
   - **On.** The user picks models by purpose from a list, then downloads them. Each model has a short note saying what it is good for, its size and its speed. For example, "Standard: good for English and Vietnamese, 195 MB", and the static helper as "finds Vietnamese typed without accents".
   - **The static model** lives in the daemon only while Semantic Search is on.
   - *Sections 3, 8, 9, 10, 12.*
5. **Unchecking Sessions turns session indexing off entirely** (user's decision, option a). Neither ⌘⇧P nor agents get sessions, and `search.query` reports Sessions as disabled. *Sections 5, 9.*

## Calls settled after review

A builder read revision 2, and the coordinator settled these six points. Each is worked into the sections named after it.

1. **Base and strings** (coordinator's call).
   - **Base.** P1 builds on the integration branch `merge/settings-batch`. It merges `agent-settings-cli` (decision 0020), appearance, the search hotkey, logs-activity, panel sizes, the hooks list and branch changes.
   - **Strings.** User-facing strings go through Fluent. The locale design (`agent/locale-design`, being written) has its own phase 1, which sets up `locales/en` and moves Settings there. Search P1 lands after it.
   - **If that slips.** P1 takes each key's title and about from its 0020 key line. It writes the rest in English into `crates/engine/src/search/settings_text.rs`: keywords, and the text of places with no key. That is a temporary table keyed by the future Fluent message ids; the locale migration moves it into `locales/en/shared.ftl` and deletes the file. P1's pull request says which of the two it used.
   - *Sections 1, 2, 8, 12.*
2. **Where corpora live** (coordinator's call).
   - **In the app.** The app keeps a pure matcher for the small corpora it owns: Settings, Actions, and Inbox (from its inbox feed). ⌘⇧P answers those at once, with no daemon, and in fixture renders.
   - **In the daemon.** The daemon serves Sessions, Pages, Activity, Vault Names and Machines. When Semantic Search is on, it also returns vector lists for Settings.
   - **Shared code.** The filter, prefix parsing, `when:` bounds, the matcher and RRF live once, in a pure engine module, `crates/engine/src/search/query/`, which both sides use. There is no duplicate code. P3's facets cover both sides through it.
   - *Sections 4, 5, 6, 7, 8, 9, 11, 12.*
3. **Eval** (coordinator's call). P4 splits into a buildable part and a maintainer-run gate.
   - **P4a, the buildable part.** A committed eval tool, `crates/embed/examples/eval.rs`. It reads a local query file the maintainer writes, which is never committed, and prints R@1 and R@5 for text-only against hybrid. No transcript content is committed.
   - **P4b, the gate.** The maintainer runs it.
   - **The research harness is kept** in `tools/embed-eval/` on this branch:
     - `rs`: the candle bench (`--features accelerate`);
     - `kt`: the kitoken probe;
     - `py`: the Python eval, with sentence-transformers in a local venv;
     - `data/*.json`: the hand-written eval sets and the per-model results.
     - Paths are relative to the folder. The two candle files are used under Apache-2.0 (`THIRD-PARTY.md`). The models are not committed; `py/download.py` fetches them into the git-ignored `models/`.
   - **Downloads.** Fetching models from Hugging Face is fine during development, never in tests.
   - **Decided since.** The pass criterion and the query author are the user's answer 1.
   - *Sections 3, 12, 13.*
4. **Tokenizer for the static model** (coordinator's call). Try kitoken first, with a parity test. If parity fails, `tokenizers` in the daemon for that one model (about 36 MB) is acceptable, after a `cargo deny` review. *Sections 3, 12.*
5. **Agent classes for `searchIndexRange` and `searchCategories`** (coordinator's call, superseded). Revision 3 kept them Free pending the user. The user's answer 2 makes them Ask and splits the category order off as Free. *Section 10.*
6. **Helper lifetime and scope** (coordinator's call).
   - **Lifetime.** The helper stays resident while a backlog exists. New chunks wait in a batch: the daemon wakes the helper when 16 are pending or the oldest has waited 5 minutes. The helper exits 5 minutes after its queue is empty.
   - **Fixture roots.** Under a fixture root (`NOTCHLING_DATA_ROOT`), the daemon uses `HashEmbedder` and never starts the real helper, unless `NOTCHLING_EMBED_REAL=1`.
   - **Out of scope:** the phone's session search.
   - **The account facet** reads `account-log.json`, which `agent/account-log-fix` is fixing.
   - *Sections 1, 3, 4, 5, 9, 12.*

## What changed in revision 2

- **Model.** The main model is now **granite-embedding-97m-multilingual-r2** (Apache-2.0, 384 dims, 195 MB), replacing multilingual-e5-small. On our bilingual set e5-small found the right chat passage first for 27% of Vietnamese queries, and granite-r2 for 91% [M].
- **Third ranker.** **static-similarity-mrl-multilingual-v1** now runs inside the daemon. It is the only model that handles Vietnamese typed without diacritics.
- **Fusion.** RRF with k = 60 over three lists: FTS5 AND, granite and static.
- **Runtime and tokenizer.** candle 0.11.1 with `accelerate` on macOS and our own patched `modernbert`. `kitoken` replaces `tokenizers`, with a parity test. The index stays a flat int8 matrix. FTS gets our own `đ`→`d` fold.
- **Memory and speed, now measured.** The helper takes about 200–300 MB while it works, against the "under 150 MB" first estimated. Queries take 3.5 ms and indexing runs at 24 chunks/s [M].
- **User decisions:**
  - Settings > Search gets a Semantic Search switch with a model list, a download with progress, and Remove.
  - An Index Sessions range, defaulting to Everything on Disk.
  - Facets with `filter` and prefix chips.
  - A Spotlight-style ⌘⇧P with categories the user checks and orders, including a new Actions category.
  - Registry entries for the Appearance and Search Shortcut settings that now exist on branches.
  - English UI text through Fluent, with Vietnamese kept as search aliases.
  - An agent class (decision 0020) for every new key.
- **Settled.** Old open question 3 (Appearance and Search Shortcut rows) is moot. Questions 1 and 2 are decided. The phases are reordered, and three new questions take their place.

## In ten lines

1. **Tier 0, on by default:** today's FTS5 index with our own `đ`→`d` fold, over the sessions in the chosen range (default Everything on Disk), settings, pages and the other checked categories.
2. **Tier 1, opt-in:** Settings > Search > Semantic Search, off by default. Nothing is downloaded or loaded, the static model included, until the user turns it on. Then the user picks models by purpose from a list, each with what it is good for, its size and its speed, and downloads them once: about 630 MB fetched, about 250 MB kept, pinned and checked against SHA-256. Full text works until they're ready.
3. **The Tier 1 models.** Granite-r2 runs in a helper, `notchling-embed` (candle with Accelerate, kitoken). It stays up while there is a backlog; after that, the daemon wakes it when 16 chunks are waiting or the oldest has waited 5 minutes, and it exits 5 minutes after its queue empties. The static model runs in the daemon, at 0.01 ms per query, and only while Semantic Search is on.
4. **Ranking.** RRF (k = 60) over FTS5 AND, granite and static, then today's recency bonus. This was the best measured: chat R@1 0.97, settings 0.81, unaccented Vietnamese chat 1.00 [M]. Exact-looking queries use text only.
5. **Store.** A flat int8 matrix per model in a new `daemon/vectors.db`, searched by brute force: 0.3 ms for 50k vectors [M]. No sqlite-vec, no HNSW.
6. **Facets.** Each document carries facets (tool, project, account and time for sessions; section for settings; owner for pages). `search.query` takes a `filter` that applies inside the FTS query and masks the vector scan before it runs. Filters, prefixes, `when:` bounds and RRF are one pure engine module that the app and the daemon share.
7. **⌘⇧P works like Spotlight.** Results are grouped by category in the user's order. Sessions, Settings, Pages, Inbox and Actions are on by default; Activity, Vault Names and Machines are off. An unchecked category is neither shown nor indexed. Prefixes such as `s:` and `p:` become chips. The app answers Settings, Actions and Inbox itself, at once; the daemon answers the rest.
8. **Actions** (Toggle Keep Awake, New Session, Switch Account, Open Settings…) run straight from search through the same app code as the menu. The daemon only ranks their titles, and `search.query` can't run anything.
9. **Settings registry.** Each entry names a settings key, whose title and description decision 0020 puts on the key line and the locale design moves to `locales/en/shared.ftl`, plus a place to jump to and Vietnamese search aliases. Every Ask key also has a plain-words effect text, which an agent's Ask item shows. A search field in the sheet jumps to the row and flashes it.
10. **Phases.** P1, Tier 0 and Settings search (auto zone, on `merge/settings-batch`, after the locale design's phase 1); then range, categories and Actions; facets; the helper and an eval tool (P4a), with a maintainer-run gate (P4b); store and pipeline; hybrid; the download UI; and more corpora.

## 1. What exists today

- **Sessions.** `search.query` (agent tier, `crates/notchlingd/src/pages_ops.rs`) reads `SessionIndex` over `daemon/search.db`. That is FTS5 with `unicode61 remove_diacritics 2`, every word a required prefix, and bm25 weights of title 10, folder 4, body 1, times up to 1.5 for a change in the last 14 days. A substring match runs when FTS finds nothing.
  - **What is indexed.** A session's text is user and assistant text only. Each message is cut to 6,000 characters, and the newest 40,000 are kept. Only files changed in the last 14 days are indexed.
  - **Refresh.** It is lazy, every 8 s at most. An unchanged (mtime, size) is never read again, and a schema change rebuilds `search.db`.
- **Pages** match on their title, project, account or link, never their content. The app adds vault names itself, through `vault.list`, which is app tier.
- **Settings.** `settings/keys.rs` has 38 keys. The sheet shows 21 rows in 7 sections, with titles only.
- **Open branches this builds on:**
  - `merge/settings-batch`: the integration branch P1 builds on (call 1). It merges the next four branches, panel sizes, the hooks list and branch changes.
  - `agent/appearance-setting`: key `appearance` (system, light or dark) in a new first section, General.
  - `agent/search-hotkey-setting`: key `searchShortcut` (default `ctrl+alt+p`, shown as ⌃⌥P; ⌘⇧P always works inside Notchling) in a new last section, Search.
  - `origin/agent/agent-settings-cli`: decision 0020. Every key gets an agent class (Free, Ask or Never) with a `title`, an `about` and a `why` on its line in `keys.rs`. A key with no class is Never, and a test fails until it gets one.
  - `agent/logs-activity`: Settings > Activity, `activity.list`, app tier.
  - `agent/locale-design`: Fluent, `locales/en/shared.ftl`. Not written yet, so this design states only what it needs from it. Its phase 1 sets up `locales/en` and moves Settings; search P1 lands after it.
  - `agent/account-log-fix`: fixes `account-log.json`, which the `account` facet reads.
- **Out of scope:** the phone's session search (`chat::read_window` and the bridge).
- **On this Mac** (counts only): `search.db` holds 650 sessions and 3.78M characters. 317 Claude Code and 146 Codex transcripts changed in the last 14 days, and 677 Claude Code transcripts are on disk in all. 42 saved pages. The app is 45 MB.

## 2. What drives the choices

- **Privacy.** privacy.md: *"Transcripts, conversations and search are read from files on this Mac and stay there."* There is no server and no telemetry. Embeddings are computed on the Mac.
- **Licenses.** 0008 and `deny.toml` allow only permissive licenses, and so does every model we ship; a test checks the model manifest against `deny.toml`'s allow list.
- **Rules.** A download trips CAP-NETWORK. Model files and `vectors.db` trip CAP-FILEWRITE, and the helper CAP-PROCESS. The auto zone covers `crates/app/**` and `crates/engine/src/search/**`, but not `Cargo.toml`, `notchlingd`, `local-api`, `platform` or `engine/src/settings`.
- **crates/AGENTS.md.** No file I/O in a model compute or draw, daemon calls off the UI thread, 1 MiB per local API line, no lock across I/O, slow work answers at once and reports by event.
- **0020.** Agents read every setting and change one by its class. Every new key here gets a class by the user's rule: cosmetic Free, sensitive Ask with an effect text (answer 2, section 10).
- **Locale.** All UI text is English. Every user-facing string goes through Fluent, so search P1 lands after the locale design's phase 1; the fallback if that slips is in call 1. Search aliases in other languages are data, not UI text.
- **Platforms.** The engine builds on macOS, Linux and Windows. Accelerate is macOS-only; other systems use candle's plain CPU path.

## 3. Where embeddings come from

**Rejected:**

- **Apple NaturalLanguage** [M]. There is no Vietnamese sentence model. The contextual model, mean-pooled, scored "đăng nhập" 0.585 against Logins and Lanes and 0.594 against Keep Mac Awake. It is also macOS-only.
- **Provider APIs** (OpenAI, Voyage, Gemini). They would send every transcript out, against privacy.md. None is built. The `Embedder` trait leaves room for one, behind a decision record and a privacy.md change; sessions would stay local unless turned on by name.

**Chosen, from the research:**

| Part | Choice | Why [M] unless marked |
| --- | --- | --- |
| Main model | **granite-embedding-97m-multilingual-r2**: IBM, Apache-2.0, 97M params, 384 dims, 32k tokens, revision `835ad14`, CLS pooling, no prefixes, code-trained | Chat R@1 0.89 (Vietnamese 0.91), settings 0.77. Query 3.5 ms; indexing 23.8 chunks/s on one thread with Accelerate. MMTEB retrieval 60.3 against e5-small's 49.3 [S]. A 195 MB download |
| Helper ranker | **static-similarity-mrl-multilingual-v1**: Apache-2.0, revision `b68f412`, cut to 512 dims, int8 | 0.01 ms per query, 1,707 chunks/s. Its tokenizer strips accents, so it is the only model good on unaccented Vietnamese (chat R@1 0.67; transformers 0 to 0.33). Weak alone on public benchmarks [S], so it is never the main model |
| Fusion | RRF, k = 60, over FTS5 AND + granite + static | Chat R@1 0.97, settings 0.81, unaccented chat 1.00, against 0.92 / 0.77 for FTS + granite alone. An OR full-text list lowered every result |
| Runtime | **candle 0.11.1**, `accelerate` on macOS, plain CPU elsewhere | Accelerate is about 3x plain CPU, and it is a framework link, not a `dlopen`. ort is still a release candidate and fetches ONNX Runtime from pyke's CDN at build time; rejected again |
| Tokenizer | **kitoken 0.11.1** (BSD-2-Clause) for both models, with a parity test against `tokenizers`, which is a dev-dependency only. If the static model's WordPiece parity fails, `tokenizers` runs in the daemon for that one model (about 36 MB), after a `cargo deny` review (call 4) | 53 MB and 22 ms to load, against 298 MB and 269 ms; identical ids on the strings checked |
| Index | Flat int8 matrix, brute force | 0.3 ms for 50k × 384 |
| Full text | Today's AND query, plus our own `đ`→`d` fold | `unicode61` folds stacked marks but never `đ` (U+0111) |

- **Why not e5-small.** Its Vietnamese chat queries landed on other Vietnamese turns, by language rather than meaning: R@1 0.27 against granite's 0.91. It is also 471 MB against 195 MB, and slower.
- **Caveat.** The research sets are small: 80 hand-written queries over 62 documents. Models within about 0.05 are a tie; e5-small's gap is not. P4b repeats the eval on real local data before the decision record.
- **Tokenizer license** (answer 3).
  - **The doubt.** Granite-r2 uses OpenAI-style special tokens (`<|startoftext|>`). Its bigger sibling's card names Gemma terms for its tokenizer; the 97m-r2 card doesn't.
  - **Decided.** We go ahead. P4a checks the cards of the model and its base encoder, and P4b confirms before the decision record.
  - **Fallback.** granite-embedding-107m-multilingual: Apache-2.0, XLM-R tokenizer, chat R@1 0.81, 2.0 ms per query [M].
- **Upgrade path, only if that eval shows a gap.** **F2LLM-v2-330M** (Apache-2.0, 669 MB, a Qwen3 architecture candle already runs). Qwen3-Embedding-0.6B is too slow on CPU: 56 ms per query and 2.2 chunks/s [M]. bge-m3 is the quality ceiling, but it is 2.27 GB and ships only as a pickle `.bin`.

**candle patch.** candle's `modernbert` hardcodes GeGLU, while granite-r2 uses SiLU. Its keys expect a `model.` prefix the files lack, and its attention mask is f32, which breaks f16 and bf16 weights [M]. We carry our own copy of the file, about 500 lines, MIT/Apache from candle, with candle's license beside it in `crates/embed/THIRD-PARTY.md` (the code takes no comments), and offer the fix upstream.

The research harness's copy, `tools/embed-eval/rs/src/modernbert.rs`, already has the SiLU and key fixes. It runs in f32, so the mask fix is P4a work.

**Where each model runs:**

- **The helper, `notchling-embed`.** Crate `crates/embed`, bundled in `Contents/Helpers`, it is the only binary that links candle; the bench binary was 7.7 to 8.9 MB [M]. The daemon starts it when there is work. It speaks one JSON line each way over stdin/stdout: texts in, base64 vectors out, batches of 8.
  - Two threads share one `Arc<Model>`: queries at user-initiated QoS, indexing at background QoS. A query never waits behind a batch.
  - **Lifetime** (call 6). It stays resident while a backlog exists (a first index, a range raised, a model change). After that, new chunks wait in the daemon's queue. The daemon wakes the helper when 16 are pending or the oldest has waited 5 minutes, and the helper exits 5 minutes after its queue is empty.
  - **Queries** start it too. The first one after a cold start answers with T and S (`warming`) while it loads, in about 0.8 s.
  - **Fixture roots.** Under a fixture root (`NOTCHLING_DATA_ROOT`), the daemon embeds with `HashEmbedder` and never starts the real helper, unless `NOTCHLING_EMBED_REAL=1`. Renders, e2e tests and dev runs over fixtures then need no model.
- **The static model, in the daemon itself.** It sits behind an engine cargo feature that only `notchlingd` turns on, so the app and ptyhost don't link it, using kitoken over a memory-mapped int8 table. If kitoken's WordPiece doesn't match `tokenizers`, it uses `tokenizers` instead (call 4).
  - **Only while Semantic Search is on** (answer 4). With the switch off, the daemon maps no model file and starts no helper. Turning it off unmaps the table and frees both vector sets in memory; their rows in `vectors.db` stay for next time, and Remove Model deletes them.
  - It ranks even while the helper is cold, so unaccented Vietnamese works from the first keystroke.
  - It indexes everything in seconds.

**Memory, corrected:**

| Process | State | Resident |
| --- | --- | --- |
| Helper | Measured with `tokenizers` and an f32 word table | 685 MB after load, 1,047 MB peak [M] |
| Helper | With kitoken and the f16 word table kept in the memory map, gathering rows per batch | 200–300 MB while working [E]; P4a gate ≤ 350 MB; **0 when idle** |
| Daemon | Static model, kitoken and the 512-d int8 table (54 MB file, touched pages only) | +40–60 MB while Semantic Search is on, 0 while it's off [E]; 36 MB measured for the 1024-d f32 table [M] |
| Daemon | Vectors in memory, granite and static | about 0.9 KB per chunk: 13.5 MB at 15k chunks, 45 MB at 50k |

**Cost.** Indexing is about 12 chunks/s at the half-core budget [E from M]. 15k chunks (everything on disk here) take about 20 minutes, once; after that a busy session adds a chunk or two a minute. The static model indexes the same 15k in about 10 s.

**The download.**

- **When.** Only after the user turns Semantic Search on and presses Download on a model (answer 4). A stored setting never starts one.
- **The manifest.** `crates/engine/data/models.json` is compiled in, like `catalog.json`. Each model lists its purpose note, size, speed and files, each file with a pinned Hugging Face revision, size, SHA-256 and license.
- **The fetch.** `GET https://huggingface.co/<repo>/resolve/<revision>/<file>` with `User-Agent: Notchling`, following redirects only to the CDN hosts the manifest names. It streams through the engine's rustls client into `<data>/models/<tier>/<file>.part` (folder 0700, files 0600, no-follow writes), hashing as it goes. The size and hash must both match before the rename. Range resume. A mismatch deletes the file.
- **The static table.** After its hash checks, the helper cuts the 434 MB f32 table to 512 dims, quantizes it to int8 (54 MB), records the new file's hash locally and deletes the f32 file.
- **Tier 1:** about **630 MB fetched once** (granite 195 MB and static 434 MB), about **250 MB kept**.
- **Privacy page.** privacy.md gets a "Semantic search models (Hugging Face)" section in the same pull request.

## 4. The vector store and facets

| Corpus | Docs here | Chunks [E] | In memory (two int8 sets) | On disk |
| --- | --- | --- | --- | --- |
| Sessions, everything on disk (the default) | about 1,000 | 12,000–15,000 | 11–14 MB | 34–42 MB |
| Sessions, last 14 days | 650 | 6,000–7,000 | 6 MB | 18 MB |
| Pages | 42 | about 400 | 0.4 MB | 1 MB |
| Settings (vector lists only; the app matches the text) | about 45 | about 45 | — | — |
| Activity, Vault Names, Machines (when on) | hundreds | ≤ 2,000 | ≤ 2 MB | ≤ 6 MB |
| Actions, Inbox | in the app only, never in `vectors.db` | | | |
| **Planned ceiling** | | **50,000** | **45 MB** | **about 140 MB** |

- **Sizes.** A chunk on disk is granite as f16 (768 B), static as int8 (516 B) and its text (about 1.5 KB). In memory it is two int8 vectors with their scales (388 B and 516 B).
- **Why no index structure.** The scan is 0.3 ms at 50k × 384 [M]. HNSW pays off near 100k to 300k vectors [S], and sqlite-vec (pre-1.0, C, brute force anyway), usearch (C++) and LanceDB (57 dependencies) buy nothing here.
- **Its own file.** Vectors go in their own `daemon/vectors.db`, because `search.db` drops its tables on a schema bump and vectors take minutes to rebuild.

```sql
CREATE TABLE docs    (id INTEGER PRIMARY KEY, corpus TEXT, key TEXT, stamp TEXT, at REAL, title TEXT,
                      UNIQUE (corpus, key));
CREATE TABLE facets  (doc INTEGER, name TEXT, value TEXT, PRIMARY KEY (name, value, doc)) WITHOUT ROWID;
CREATE TABLE chunks  (id INTEGER PRIMARY KEY, doc INTEGER, ord INTEGER, kind TEXT, hash BLOB NOT NULL,
                      text TEXT NOT NULL);
CREATE TABLE vectors (hash BLOB, model TEXT, vec BLOB, PRIMARY KEY (hash, model)) WITHOUT ROWID;
CREATE TABLE meta    (k TEXT PRIMARY KEY, v TEXT);
```

**Facets** (decision 3). `search.db` gets the same `facets` table, and `docs` gains `corpus`, so one filter works in both stores. The app's corpora carry the same facet names in memory, and one engine function, `query::filter`, applies a filter on both sides (call 2).

| Corpus | Where | Facets |
| --- | --- | --- |
| Sessions | daemon | `tool` (claude, codex, agy); `project` (the folder; label is its last part); `account` (a lane's account; for `~/.claude`, the account active when the session started per `account-log.json`, being fixed on `agent/account-log-fix`, else `unknown`); `time` |
| Settings | app (text), daemon (vectors) | `section` |
| Pages | daemon | `owner` (the saved account); `project`; `time` |
| Inbox | app | `kind`, `state`, `time` |
| Activity | daemon | `kind`, `source`, `time` |
| Vault Names, Machines | daemon | `kind` where they have one |
| Actions | app | `kind` |

**How a filter applies.**

- **In FTS,** it becomes `AND d.id IN (SELECT doc FROM facets WHERE name = ? AND value IN (…))` per facet, plus `d.at` bounds, so bm25's top 100 is already filtered.
- **For vectors,** the daemon keeps each document's facets interned in memory and turns a filter into a document mask once per query. The scan skips masked chunks.
- **The result:** a filtered search is exact, with no recall lost the way an ANN index loses it to post-filtering.

## 5. One pipeline, many corpora

```
categories (checked, ordered)   pipeline (daemon, background)                                    store
sessions · pages ────────────┐
settings (vectors only)      ├─ scan stamps ─ read changed ─ chunk ─ hash ─ vector missing? ─▶ daemon/vectors.db
activity · vault · machines ─┘   (range: 14d/30d/all)                        granite: helper     docs · facets · chunks
   (unchecked: skipped)                                                      static: daemon      vectors (hash, model)

query ─ query::parse ─ filter ─┬─ FTS5 AND, filtered (search.db) ─────────── doc ranks ─┐
                               ├─ granite: embed query (helper) ─ masked scan ─ doc ranks ─┼─ query::fuse (RRF k=60, × recency) ─ hits by category
                               └─ static: embed query (daemon) ─ masked scan ── doc ranks ─┘
app: settings · actions · inbox ─ query::matcher (text, at once) ─ query::fuse with the daemon's settings vector lists
```

**Where each corpus lives** (call 2):

| Side | Corpora | How |
| --- | --- | --- |
| App | Settings, Actions, Inbox | A pure matcher over compiled-in tables (the settings registry, the actions list) and the inbox feed's copy of its items. It answers on the keystroke, with no daemon, and in fixture renders. |
| Daemon | Sessions, Pages, Activity, Vault Names, Machines | FTS5 and the two vector lists, through `search.query` |
| Daemon, for the app | Settings vector lists, when Semantic Search is on | `search.query` with `mode: "semantic"`; the app fuses them with its own text list |

**One shared module, `crates/engine/src/search/query/`** (pure, no I/O, tested once):

| File | What |
| --- | --- |
| `parse.rs` | Prefix and chip grammar: the typed text becomes `{categories, facets, when, words}` |
| `when.rs` | `today`, `7d`, `30d` and `2026-11` become bounds, from a `now` passed in |
| `filter.rs` | `Filter`: the wire type, the in-memory match for the app's corpora, the SQL clause for FTS and the doc mask for vectors |
| `fold.rs` | Lowercase, strip marks, `đ`→`d`; the same fold as the FTS index |
| `matcher.rs` | Words as prefixes, all required, and field weights for small in-memory corpora |
| `fuse.rs` | RRF with k = 60 over any number of ranked lists, then the recency bonus |

The app and the daemon call the same functions, so a filter, a prefix or a fusion never means two things. P3's facets reach both sides through `filter.rs`.

**The trait** (engine, `crates/engine/src/search/corpus/`):

```rust
pub trait Corpus: Send + Sync {
    fn id(&self) -> &'static str;                        // "sessions", "pages", "settings", "activity", …
    fn tier(&self) -> Tier;                               // who may see its hits: Agent or App
    fn scan(&self, range: &Range, now: f64) -> Vec<DocStamp>;  // keys and stamps; stat only
    fn read(&self, doc: &DocStamp) -> Option<Doc>;        // title, time, facets, chunk texts
}
pub trait Embedder: Send + Sync {
    fn model(&self) -> &ModelId;                          // "granite-97m-r2@835ad14/p1", "static-mrl@b68f412/512i8/p1"
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
}
```

- **Corpora.** A new corpus is one `Corpus` implementation and one registry line.
- **Embedders:** `HelperEmbedder` (granite, through the helper) and `StaticEmbedder` (in the daemon).
- **Test fakes:** `HashEmbedder` (folded trigrams hashed into 64 dimensions, so CI needs no model) and `TableEmbedder` (scripted vectors). Under a fixture root the daemon itself uses `HashEmbedder` for granite and static alike, unless `NOTCHLING_EMBED_REAL=1` (call 6).

**Categories and range** (decisions 2 and 4):

- **Categories.** The pipeline reads `searchCategories` and `searchIndexRange` at each pass and on `settings.changed`.
  - An **unchecked category** is not scanned. Its `search.db` rows, chunks and vectors are deleted, and `search.query` answers `"disabled": ["activity"]` for it, to agents too. The app's matcher skips its own unchecked categories the same way.
  - **Sessions too** (answer 5). Unchecking Sessions turns session indexing off entirely. Neither ⌘⇧P nor agents get sessions, and `search.query` answers `"disabled": ["sessions"]` to every caller.
- **Range.** It applies to sessions in both indexes. Lowering it deletes the FTS rows and chunks past it, and a sweep removes their vectors. Raising it queues the older files, newest first. The full-text index follows the same range, so "Everything on Disk" also ends today's 14-day limit for full text. That first refresh reads about 1,000 transcripts once, in the background.
- **Who sees what.** Activity, Vault Names and Machines are app-tier corpora in the daemon. Their hits go only to the app, and an agent-tier `search.query` that asks for them gets `"dropped": [...]`. Actions and Inbox never go through `search.query` at all: they live in the app (call 2).
- **Vault Names** holds names and kinds, never values, never through the Keychain, and only while the `vault` switch is on. When the switch goes off its rows are deleted.

**Chunking:**

- **Sessions.** One parse serves both indexes: the FTS read also returns turns.
  - A **head chunk** per session: its title, the folder's name and the first prompt, up to 600 characters.
  - **Turns** become windows of up to 1,200 characters (about 290 granite tokens [M]), split at paragraphs and then sentences, with 150 characters of overlap. Each window starts with the turn's prompt, up to 200 characters.
  - Code blocks keep their first 20 lines (granite is code-trained). At most 6 windows per turn and 200 chunks per session, newest first.
- **Pages.** Local HTML stripped to text, sectioned at h1 to h3.
- **Settings.** One chunk per entry: the English title, the description, the keywords, and the aliases.
- **Activity.** One chunk per row: its op and its detail.
- **Actions and Inbox.** No chunks: the app matches their titles by text only.
- **Vault Names and Machines.** Name and kind; host and role for a machine.

**Ids, stamps and updates:**

- **Identity.** A chunk is `SHA-256(text)[..16]`, and vectors are keyed by `(hash, model)`. A grown transcript gives its old turns the same hashes, so only new chunks are embedded, and copies made by continued sessions dedupe for free.
- **Stamps.** A document is re-read only when its (size, mtime) stamp changes. Pages add their `version`; settings use the hash of their registry and message text.
- **The open turn.** A session's last turn waits until the session has been quiet for 60 s or a new prompt closes the turn.
- **Batching** (call 6). Outside a backlog, new chunks wait in the daemon's queue. It hands them to the helper when 16 are pending or the oldest has waited 5 minutes. The static model embeds them at once, in the daemon.
- **Deletes.** A missing document loses its chunks; a sweep removes vectors nothing references.
- **Refresh.** Every 60 s while Tier 0 indexing has work (stat only for unchanged files), at daemon start, and when a search setting changes.
- **Order.** Settings first (instant), then sessions newest first, then pages, then the rest.

**Model changes.** A model id includes our pipeline version (pooling, normalization, chunker, dims, quantization). After a change, coverage refills from stored chunk text without reading any transcript. A model's old vectors are deleted once its replacement covers every chunk. A list with no vectors under the active model sits out of the fusion.

**Budget, pause and progress:**

- **Budget.** The helper's indexing thread runs at background QoS: efficiency cores and I/O throttled on macOS, `nice 19` on Linux, background mode on Windows, all in a new `platform::power`.
  - As a second guard, it measures its own CPU time and sleeps to stay at or under **50% of one core**, or 25% on battery.
  - It pauses in Low Power Mode, at thermal state serious or worse, and when the user pauses it.
- **Progress.** `embed.status` returns per-corpus counts, and an `embed.changed` event goes out at most once a second.

## 6. Hybrid ranking

For each category asked, after the filter:

```
T = FTS5 AND, filtered, top 100 docs     (OR only when AND finds nothing, at half weight [E])
G = granite cosine, masked scan, top 300 chunks → best chunk per doc   (below floor τG dropped)
S = static cosine, masked scan, top 300 chunks → best chunk per doc    (below floor τS dropped)
fused(d) = Σ over lists L of  w_L / (60 + rank_L(d))      a missing rank adds 0
score(d) = fused(d) × (1 + 0.5 × fresh(d))               today's 14-day recency bonus
```

- **Weights and constant.** Weights start at 1, 1, 1 and k = 60 everywhere; those are the measured settings. A convex mix may beat RRF once P4b has a real labeled set (Bruch et al. [S]).
- **Floors.** The floors τG and τS are calibrated per model in P4b; e5's 0.80 doesn't carry over. When full text found 5 or more, hits found only by meaning fill at most half a category.
- **Text-only queries.** Exact-looking queries use text only, whatever was asked: a link or artifact id, a path, quotes, a hex or uuid token, or only words of 2 characters or fewer.
- **State.** With Semantic Search off, every list but T sits out. While the helper starts (`warming`), T and S still fuse.
- **One fusion.** The daemon and the app both fuse with `query::fuse`. For Settings, the app's text list and the daemon's two vector lists fuse in the app.
- **Snippets.** A text hit keeps today's snippet and highlights; the highlighter folds `đ` the same way. A hit found only by meaning shows its best chunk's opening and is tagged `match: "semantic"`; the panel shows it as **Related**.

**Mode per place:**

| Place | Mode |
| --- | --- |
| ⌘⇧P | The app's categories by text at once (Settings turns hybrid when its vector lists arrive); the daemon's categories hybrid when Semantic Search is ready, else text |
| Settings search | Text in the app at once; when ready, the daemon's vector lists for about 45 entries arrive and the app fuses them |
| `pages.find`, `notchling pages`, a pasted link or id | Text |

The user has no per-place mode switch: a mode is a call parameter.

## 7. ⌘⇧P: categories, prefixes, chips, actions

**Categories** (decision 4). Results are grouped by category in the user's order:

- **On by default:** Sessions, Settings, Pages, Inbox, Actions.
- **Off by default:** Activity, Vault Names, Machines.

Each group shows up to 8 sessions or 4 of anything else, then a "N more" row that narrows to that category. Return on a hit:

| Hit | Return does |
| --- | --- |
| Session | Opens it, as today |
| Setting | Opens Settings at that row |
| Page | Opens its window |
| Inbox item | Opens the inbox at it |
| Activity row | Opens Settings > Activity at it |
| Vault name | Today's behavior |
| Machine | Opens Machines |
| Action | Runs it |

**Prefixes and chips** (decision 3). A token the parser knows becomes a chip, a pill with ×. Backspace in an empty field removes the last chip. Tab completes a facet value from `search.facets`. A value nothing has turns its chip red ("No Such Project") and sends no filter. The parser is `engine::search::query::parse`: pure, tested once, and the same one the daemon uses for a query an agent types (call 2).

| Typed | Chip | Meaning |
| --- | --- | --- |
| `s:` or `settings:` | Settings | Only that category, no cap |
| `p:` / `pages:`, `c:` / `sessions:`, `i:` / `inbox:`, `a:` / `>` / `actions:`, `v:` / `vault:`, `m:` / `machines:`, `activity:` | its name | Same |
| `tool:codex` | Codex | Facet `tool` |
| `project:notchling` or `in:notchling` | notchling | Facet `project` |
| `account:work` | work | Facet `account` |
| `when:today`, `when:7d`, `when:30d`, `when:2026-11` | Today, Last 7 Days… | Time bounds |
| `section:notch`, `owner:me@…` | Notch, me@… | Facets `section`, `owner` |

**Actions** (decision 4). A registry in `crates/app/src/search/actions.rs` lists each action: an id, a Fluent message, aliases, the facet `kind` and the handler its menu item already uses. The app matches them with `query::matcher`, like Settings and Inbox (call 2).

- **First set:** Toggle Keep Awake, Keep Awake While Agents Work, New Session, Open Chat, Open Settings, Switch Account, Open Accounts, Open Pages, Open Secret Vault, Pair a Phone, Show or Hide the Notch.
- **Same rules as the menu.** Switch Account opens the account chooser, as the menu does; it never switches on its own. Anything the menu confirms is confirmed here. An action whose menu item is disabled (for example, its switch is off) shows disabled.
- **The daemon never sees them.** Actions are matched and run in the app. `search.query` has no actions corpus, so no agent can list or run one through it.
- **Tests:** every action has a message and a handler; `search.query` with `corpora: ["actions"]` returns it in `dropped` for every caller.

## 8. Settings: the Search section, the registry, the field

**Settings > Search** (decision 1). It is the section `agent/search-hotkey-setting` adds:

1. **Search Shortcut**: from that branch.
2. **Semantic Search**: Off or On, default Off. Off means nothing is downloaded or loaded, the static model included (answer 4). Turning it on shows the model list: one row per model in the manifest, each with what it is good for, its size and its speed. The wording is the English source for Fluent messages.
   - **Standard** (recommended): "Good for English and Vietnamese. 195 MB. About 4 ms a search; the first index takes about 20 minutes." Granite 97M multilingual r2.
   - **Accent Helper** (recommended): "Finds Vietnamese typed without accents. 434 MB download, 54 MB kept. Instant." The static model. It runs in the daemon only while Semantic Search is on.
   - **Large**, listed only after P4b shows Standard missing what it finds: "Better on hard questions. 669 MB. Slower to search and index [E]." F2LLM-v2-330M.
   - **Per row:** Download, then a progress bar with Cancel, then **Remove Model**.
   - **While it works:** once a model is in, "Indexing 1,240 of 15,000 passages" with **Pause**.
   - **Until then:** full text answers until vectors are ready, and a model not downloaded sits out of the fusion.
3. **Index Sessions**: Last 14 Days, Last 30 Days or Everything on Disk (default).
4. **Categories**: one row per category with a checkbox and a drag handle (⌥↑ and ⌥↓ move a focused row); the defaults are in section 7. The checkboxes write `searchCategories` and the order writes `searchCategoryOrder` (answer 2).

**The registry** (`crates/engine/src/search/settings_entries.rs`, auto zone):

```rust
pub struct Entry {
    pub id: &'static str,                    // stable, never reused: "appearance", "daemon.accounts", "window.accounts"
    pub key: Option<&'static str>,           // a settings::KEYS name: its title and about come from it
    pub message: Option<&'static str>,       // Fluent id for entries without a key (windows, switches, hooks)
    pub aliases: &'static [&'static str],    // search-only, any language: ["giao diện", "chế độ tối", "tối"]
    pub place: Place,                        // Section("General") | Daemon(Area) | Window(Accounts) | Hooks
}
```

- **One source of text.**
  - **Today:** decision 0020 puts `title` and `about` on every key line.
  - **When the locale work lands:** they become `locales/en/shared.ftl` messages, `setting-<key>` with `.about` and `.keywords` attributes, plus `.label` where the sheet's row label is shorter ("Width" under Notch).
  - **Neither way does the registry copy text.** It names the key, or its own message id for places with no key.
  - **If the locale phase 1 slips** (call 1): P1 takes keys' titles and about from their 0020 key lines and writes the rest (keywords, and the text of places with no key) in English into `crates/engine/src/search/settings_text.rs`. That is a temporary table keyed by the future message ids, which the locale migration moves into `shared.ftl` and deletes. P1's pull request says which it used.
- **Effect text for every Ask key** (answer 2).
  - **What it is.** Each key 0020 classes Ask has a one-line "what this does" in plain words. It is a Fluent `.effect` attribute (`setting-<key>.effect`), or an `effect` on the key line until the locale work lands.
  - **Where it shows.** An agent's Ask change raises an Inbox item showing the setting's title, the old and the new value, and that line, so the user knows what Allow does.
  - **Feeds back into decision 0020's table.** Its Ask keys (`continueAfterSleep`, `answerFromNotch`, `keepPages`, the limit modes, `limitSwitchAt`, `searchShortcut`) need effect texts, and its Ask item takes this shape.
- **Search text.** It is the title, about and keywords in the UI's locale, plus the aliases, which are searched under every locale. Vietnamese stays as aliases, since all UI text is English.
- **Tests** (new file):
  - Every row the sheet can show (all conditional rows forced on) names an entry.
  - Every entry placed in a section is shown there.
  - Every key that 0020 classes Free or Ask is either in the registry or listed in `NOT_SHOWN` with a reason, so a new user-facing key fails until it has an entry. 0020's own test already fails for a key with no class, title or about.
  - Every entry's messages exist and are non-empty in `locales/en/shared.ftl`, or in `settings_text.rs` while that table exists.
  - Every Ask key has a non-empty effect text.
  - Ids match a committed list.
  - "dark" finds Appearance, "phím tắt" and "phim tat" find Search Shortcut, and "login" and "đăng nhập" find Logins and Lanes and the Accounts window.
- **Places outside the sheet:** the Accounts, Secret Vault and Phone windows, and Machines.

**The field:**

- **Where.** Pinned under the sheet's header; ⌘F focuses it, and Esc clears it, then closes the sheet.
- **Results.** While it has text, a result list replaces the sections: title with highlights, description under it, section at the right. Return jumps: the field clears, the row scrolls to a third of the way down and flashes an Accent wash for 1.2 s. A `Window` place opens that window instead.
- **Text matching.** It runs in the app, pure and instant, over the compiled-in registry, through `query::matcher`. `query::fold` lowercases, strips marks and maps `đ`→`d`; it is the same fold the FTS index uses, and a test checks it against SQLite's `remove_diacritics 2`. Words are prefixes and all must match; title 3, keywords and aliases 2, description 1.
- **Hybrid matching.** When Semantic Search is ready, the same keystroke also asks the daemon for the Settings vector lists (`mode: "semantic"`, `corpora: ["settings"]`), through the `Pacer` and off the UI thread. The app fuses them with its text list through `query::fuse`, and the fused list replaces the shown one, keeping the selection by id.
- **Layout change.** Every row also shows its description under its title. Rows get taller, on purpose.

## 9. Local API (additive only)

| Op | Tier | Args → reply |
| --- | --- | --- |
| `search.query` | agent (app-tier corpora app only) | Args add `mode?: "text" \| "hybrid" \| "semantic"` (default `"text"`: old clients see no change) and `corpora?` (default `["pages","sessions"]`; new: `settings` (vector lists only; the text half is the app's), `activity`, `vault`, `machines`; Actions and Inbox are not daemon corpora and come back in `dropped`). Also `filter?: {tool?, project?, account?, section?, owner?, kind?, state?: [..], after?, before?}`: AND across names, OR within a list. The reply adds one array per corpus asked, every hit gaining `match: "text" \| "semantic" \| "both"` and `facets`, plus `semantic: {state: "off" \| "no_model" \| "warming" \| "indexing" \| "ready" \| "paused", coverage}`, `disabled: [..]` (unchecked, for every caller: with Sessions off, agents get no sessions either) and `dropped: [..]` (tier) |
| `search.facets` | as its corpus | `{corpus, name, prefix?, limit?}` → `{values: [{value, label, count}]}` |
| `embed.status` | agent | `{}` → state, tier and model ids, files present and verified, per-corpus `{docs, chunks, embedded}`, paused, budget, last error |
| `embed.fetch` | app | `{model}` → `{state: "downloading"}` at once, progress by event. Only the app's Download button calls it, and only while Semantic Search is on |
| `embed.fetch.cancel`, `embed.remove` | app | `{model}` |
| `embed.refresh` | app | `{}`: re-read the search settings now (the sheet writes the store itself, and those writes don't reach the `settings` topic) |
| topic `embed` | app | `embed.changed {state, coverage, fetch: {bytes, total}}`, at most once a second |

- **Agent tier.** Reading the index stays agent tier for sessions, pages and settings: ranking by meaning shows nothing full text doesn't. Agents find settings by text through `notchling settings` (decision 0020). Query vectors are cached (256 by text), and at most 2 query embeddings run at once.
- **Downloads.** Only the app tier downloads or removes models, and actions run only in the app. A stored setting never starts a download.
- **The phone.** Its session search is out of scope (call 6); the bridge gains nothing here.
- **Size.** Replies stay under 100 KB (at most 200 sessions, 20 of anything else, snippets of 200 characters or fewer), well under 1 MiB.

## 10. New setting keys and their agent class (decision 0020)

**The rule, for all settings** (answer 2):

- **Free.** A cosmetic change (look, order, display), with no confirmation.
- **Ask.** A sensitive change. The Inbox item shows the setting's title, the old and the new value, and the key's effect text, and the agent waits for Allow.
- **Never** for agents. A change that fetches or deletes models.

| Key | Type, default | Class | Effect text (Ask) or why |
| --- | --- | --- | --- |
| `semanticSearch` | bool, `false` | **Ask** | "Turns on search by meaning: indexes your sessions in the background (about 20 minutes the first time) and keeps the accent helper in memory (about 50 MB)." It never downloads by itself |
| `semanticModel` | list of manifest model ids, none | **Never** | Only the app's Download button fetches: 195 MB to about 1.1 GB |
| `semanticPaused` | bool, `false` | **Ask** | "Pauses or resumes indexing by meaning. While paused, new sessions are found by words only." Not cosmetic: it changes what search finds |
| `searchIndexRange` | `14d` \| `30d` \| `all`, `all` | **Ask** | "Changes how far back sessions are searched. A shorter range removes older sessions from search; a longer one indexes them in the background." |
| `searchCategories` | set of category ids that are on; the defaults in section 7 | **Ask** | "Turns a search category on or off. Off removes it from search and from the index, for agents too." App-tier categories stay app-tier, and Vault Names still needs the `vault` switch |
| `searchCategoryOrder` | ordered list of category ids; the defaults in section 7 | Free | Cosmetic: the order results are grouped in. An id it lacks takes its default place; an unknown id is ignored |

- **Why two keys.** Decision 0020 classes a key, not a change, so turning categories on and off and ordering them are separate keys rather than one key classed by the kind of change.
- **Registry entries** also cover `appearance` (Free: cosmetic) and `searchShortcut` (Ask in 0020). `searchShortcut` needs an effect text: "Changes the key that opens Search from any app. No other app can use that key."

## 11. Performance rules, checked

- **No I/O in a model compute or draw.** Settings, Actions and Inbox matching is pure, over compiled-in tables and the inbox feed's copy of its items. Everything else comes from the daemon off the UI thread through the `Pacer`. Parsing, filters and fusion are pure (`engine::search::query`).
- **1 MiB per line.** Replies are under 100 KB. The helper's lines carry 8 texts out and 8 × 384 floats back (about 16 KB).
- **No lock across I/O.** The helper call runs outside the vector set's lock. The set is an `Arc` swapped after each batch, so a query reads a snapshot.
- **Latency.** Hybrid p95 under 50 ms at 50k chunks with a warm helper: granite 3.5 ms, static 0.01 ms, two scans of 0.3 ms, FTS a few ms. An ignored bench, `bench_search.rs`, holds it.

## 12. Plan

| Phase | What | Tests | Zone |
| --- | --- | --- | --- |
| **P1** (ships first; on `merge/settings-batch`, after the locale design's phase 1) | **Tier 0:** the `đ`→`d` fold in the FTS index, query and highlights, with a `search.db` schema bump (one rebuild). **Settings search:** the registry (keys' titles and about from 0020, Fluent messages, aliases, places), the field, jump and flash. **Effect texts** for every Ask key, 0020's included (answer 2). **Shared module, first files:** `query::fold`, `query::matcher`. If the locale phase 1 slips, the temporary `settings_text.rs` (call 1) | Fold matches SQLite's; a stored "Sửa lỗi đăng nhập" is found by "dang nhap"; registry coverage and `NOT_SHOWN`; every Ask key has an effect text; id list; the three example queries; model tests for keys and jump; `--render settings-search` | **auto** (`crates/engine/src/search/**`, `crates/app/**`, new test files); the effect texts on 0020's keys wait for the maintainer |
| P2 | Index Sessions (`searchIndexRange`) and Categories (`searchCategories`, `searchCategoryOrder`) rows, with their classes and effect texts. The FTS index follows the range; unchecked categories are dropped and skipped. ⌘⇧P grouped by category order. `query::parse` with category chips. Actions (app registry and handlers); Inbox, Actions and Settings matched in the app | Range down deletes rows, up queues older files; an unchecked category is in no reply and no table; with Sessions unchecked an agent gets no sessions; every action has a handler; `search.query` returns `actions` and `inbox` in `dropped`; parser goldens | maintainer (`keys.rs`, `pages_ops.rs`, `local-api`) |
| P3 | Facets on both sides through `query::filter` and `query::when`: the `facets` table in `search.db`, `filter`, `search.facets`, facet chips; `section`, `kind` and `state` on the app's corpora. Account from lanes and `account-log.json`, after `agent/account-log-fix` | A doc outside the filter never appears, in the app or the daemon; filter goldens; `when:` bounds from a fixed `now`; Tab completion model | maintainer (`notchlingd`) |
| **P4a** (an agent can build it) | `crates/embed`: candle 0.11.1 with `accelerate` on macOS, our own `modernbert` with the mask fix, kitoken; `--bench`. The granite-r2 tokenizer's license is checked on the model and base encoder cards (answer 3). The static reader (engine feature): kitoken WordPiece, else `tokenizers` after a deny review (call 4). **The eval tool,** `crates/embed/examples/eval.rs`. It takes `--queries <file>`: JSON lines `{q, want: [doc ids], lang?}`, written by the maintainer and never committed, for example under the git-ignored `docs/local/`. It runs over `--data-root` and prints R@1 and R@5 for text-only against hybrid: overall, per language, and for exact-looking queries (the text-only shapes, through `engine::search::query`). The research harness stays in `tools/embed-eval/` | kitoken parity and the eval tool's own tests read model files from a local folder (`NOTCHLING_EMBED_MODELS`) and are ignored by default: tests never download (call 3). Automatic gates on a Mac: query p50 ≤ 5 ms; ≥ 12 chunks/s at the budget; helper RSS ≤ 350 MB; `cargo deny check` clean; the eval tool over `tools/embed-eval/data/` reproduces the research numbers within 0.05 | maintainer (new crate, `Cargo.toml`, `Cargo.lock`) |
| **P4b** (the maintainer runs it) | The real-data gate (answer 1). The maintainer writes about 40 queries over his own sessions (local, never committed) and runs the eval tool for FTS alone, FTS + granite + static, and F2LLM-v2-330M. It calibrates the floors τG and τS and decides whether a Large tier is listed. The granite-r2 tokenizer's license is confirmed (answer 3), else granite-107m replaces it. Writes `docs/perf/embedding-spike.md` and decision record 0021 (0019 and 0020 are taken) | **Pass:** hybrid beats text-only by at least 0.15 R@5 overall, no language is worse, and exact-looking queries are no worse | maintainer |
| P5 | Store and pipeline with fakes: `vectors.db` (two vector sets, facets), `Corpus`, chunkers, hashes, stamps, deletes, model-id switch, budget logic, the batch queue (16 chunks or 5 minutes), `HashEmbedder`, `TableEmbedder` | An unchanged stamp reads nothing; growth embeds only new chunks; deletes sweep; a model change refills from stored text; the CPU meter keeps ≤ 50%; masks are exact; the queue releases at 16 or at 5 minutes on a fake clock | auto by path; the new database likely trips CAP-FILEWRITE |
| P6 | Hybrid: three-list RRF through `query::fuse`, `embed.status`, the helper client (spawn, lines, resident while a backlog exists, exit 5 minutes after the queue empties), the static model in the daemon, Settings vector lists for the app; panel Related tag and progress | RRF golden; `search.query` without `mode` answers exactly as today; text-only shapes; floors; `warming` fuses T + S; helper lifetime on a fake clock with `FakeSpawner`; under a fixture root nothing spawns the helper unless `NOTCHLING_EMBED_REAL=1`; with Semantic Search off the daemon maps no model file and starts no helper; 1 MiB check; `bench_search.rs` | maintainer (`notchlingd`, CAP-PROCESS) |
| P7 | Semantic Search UI and download: the switch, the model list by purpose (each row's note, size and speed), per-model Download / Cancel / Remove, Pause, `models.json` with SHA-256, host allowlist, static conversion, `platform::power`, the helper in `tools/bundle-next.sh`, privacy.md | With `FakeHttp`: wrong hash deleted, too many bytes aborts, an unlisted redirect refused, Range resume, no `.part` through a symlink; `embed.fetch` refused at agent tier and while the switch is off; manifest licenses within `deny.toml`'s allow list; 0020 classes of the new keys | maintainer (CAP-NETWORK, CAP-FILEWRITE, policy) |
| P8 | More corpora by meaning in the daemon: Activity, Vault Names (names only, vault switch), Machines; pages' content in full text | One `Corpus` test file each; the vault corpus never touches the Keychain (`PanicKeychain`) and empties when the switch goes off | maintainer (`notchlingd`, vault and Machines) |

**Ordering.**

- P1 gives Tier 0 and Settings search before any model exists, and P2 and P3 make ⌘⇧P Spotlight-like with text alone.
- An agent can build P4a while P4b waits for the maintainer. P5 runs on fakes, so it doesn't wait for either.
- P6 needs P4a. The floors and the Large tier wait for P4b.
- P6 can merge before P7, because with no model every query falls back to text.

## 13. Open questions

None. The user answered the last ones after review:

- **Granite-r2's tokenizer license.** Go ahead, verify in P4a and P4b, with granite-107m as the fallback (answer 3).
- **The eval's pass criterion and who writes the queries** (answer 1).
- **Unchecking Sessions.** It stops session search for agents too (answer 5).
- **The classes of the new keys** (answer 2).
- **Where the static model runs:** in the daemon, only while Semantic Search is on (calls 4 and 6, answer 4).

## Sources

- `embedding-research.md` (2026-11-13, in the maintainer's git-ignored `docs/local/`), with its harness and eval sets committed in `tools/embed-eval/`: every [M] figure for models, fusion, speed, memory, tokenizers and scans, and its sources (Hugging Face cards for granite-embedding-97m-multilingual-r2, static-similarity-mrl-multilingual-v1, F2LLM-v2-330M and others; VN-MTEB, arXiv 2507.21500; Cormack et al. 2009; Bruch et al., arXiv 2210.11934; candle, ort, kitoken, sqlite-vec, usearch and LanceDB repositories).
- Decision 0020 on `origin/agent/agent-settings-cli` (`docs/decisions/0020-agents-change-settings-by-class.md`); keys `appearance` on `agent/appearance-setting` and `searchShortcut` on `agent/search-hotkey-setting`; `activity.list` on `agent/logs-activity`.
- Apple NaturalLanguage results: Swift probes run on this Mac in the job's scratch folder.
