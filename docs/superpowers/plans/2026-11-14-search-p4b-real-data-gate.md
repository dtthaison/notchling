# Search P4b: The Real-Data Gate (Maintainer Checklist)

> **Not for agents.** The maintainer runs every step. It reads his own sessions, writes queries that are never committed, and decides. Agents may prepare tooling (P4a) and apply the one-line results afterwards (step 9), only when he asks.

**Goal:** Decide, on real local data, whether hybrid search is worth shipping and with which models and floors; confirm granite-r2's tokenizer license; write decision record 0021 and `docs/perf/embedding-spike.md`.

**Spec:** `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` sections 3 (caveat, tokenizer license, upgrade path), 6 (floors), 12 P4b, 13; answers 1 and 3. Plan set: `2026-11-14-search-plans-index.md`.

**Branch:** `agent/search-p4b-gate` (the maintainer's), from the integration branch with P4a merged.

**Depends on:** P4a (the eval tool with `--queries`, `--find`, `--sweep-floors`, `--extra-model`). P5 and P6 do not wait for this gate; P6's floors stay 0 until step 9, and P7's Large row stays hidden unless step 7 lists it.

**Pass criterion (answer 1), all three:**

1. Hybrid (text + granite + static) beats text-only by at least **0.15 R@5** over all queries.
2. **No language** is worse with hybrid than with text-only (R@1 and R@5, each `lang` group).
3. **Exact-looking queries** (the `exact` group) are no worse than text-only.

## Checklist

- [ ] **1. Build the tool.**
  ```sh
  cd <a checkout of the integration branch with P4a>
  cargo build --release -p notchling-embed --example eval
  ```

- [ ] **2. Fetch the three models** (development download, by hand; nothing else fetches them). Resolve each full revision hash first, then download into the git-ignored `tools/embed-eval/models/`:
  ```sh
  python3 -m venv tools/embed-eval/py/.venv
  tools/embed-eval/py/.venv/bin/pip install 'huggingface_hub>=0.26'
  tools/embed-eval/py/.venv/bin/python -c "from huggingface_hub import HfApi; a=HfApi(); [print(r, next(c.commit_id for c in a.list_repo_commits(r) if c.commit_id.startswith(p))) for r,p in [('ibm-granite/granite-embedding-97m-multilingual-r2','835ad14'),('sentence-transformers/static-similarity-mrl-multilingual-v1','b68f412'),('codefuse-ai/F2LLM-v2-330M','')]]"
  tools/embed-eval/py/.venv/bin/huggingface-cli download ibm-granite/granite-embedding-97m-multilingual-r2 --revision <hash starting 835ad14> --local-dir tools/embed-eval/models/ibm-granite__granite-embedding-97m-multilingual-r2
  tools/embed-eval/py/.venv/bin/huggingface-cli download sentence-transformers/static-similarity-mrl-multilingual-v1 --revision <hash starting b68f412> --local-dir tools/embed-eval/models/sentence-transformers__static-similarity-mrl-multilingual-v1
  tools/embed-eval/py/.venv/bin/huggingface-cli download codefuse-ai/F2LLM-v2-330M --revision <hash printed above> --local-dir tools/embed-eval/models/codefuse-ai__F2LLM-v2-330M
  export NOTCHLING_EMBED_MODELS="$PWD/tools/embed-eval/models"
  ```
  Write down the F2LLM hash: decision 0021 pins it if Large is listed.

- [ ] **3. Point a read-only root at the real folders.** The eval only reads.
  ```sh
  mkdir -p /tmp/nl-eval && ln -sfn ~/.claude /tmp/nl-eval/claude && ln -sfn ~/.codex /tmp/nl-eval/codex
  ```

- [ ] **4. Write about 40 queries** in `docs/local/search-queries.jsonl` (git-ignored; never committed), one JSON object a line: `{"q": "...", "want": ["claude:<session id>"], "lang": "en" | "vi" | "vi-nodiac" | "mixed"}`. Aim for about 12 English, 12 Vietnamese with accents, 6 Vietnamese without accents, 4 mixed, and 6 exact-looking (a path, an artifact id or link, a quoted phrase, a hex or uuid token). Ask the way you would ask Search, not with the session's own words. Find each `want` with:
  ```sh
  target/release/examples/eval --find "words you remember" --data-root /tmp/nl-eval
  ```
  A query whose session `--find` can't surface is still a good query; find its id in Chat's Details Panel instead.

- [ ] **5. Run the eval** and keep the output local:
  ```sh
  target/release/examples/eval --queries docs/local/search-queries.jsonl --data-root /tmp/nl-eval \
    --models "$NOTCHLING_EMBED_MODELS" --extra-model f2llm "$NOTCHLING_EMBED_MODELS/codefuse-ai__F2LLM-v2-330M" \
    --sweep-floors --json > docs/local/search-eval-2026-11.json
  target/release/examples/eval --queries docs/local/search-queries.jsonl --data-root /tmp/nl-eval \
    --models "$NOTCHLING_EMBED_MODELS" --extra-model f2llm "$NOTCHLING_EMBED_MODELS/codefuse-ai__F2LLM-v2-330M"
  ```
  The second prints the table: `text`, `text+granite`, `text+granite+static`, `text+f2llm` by `all`, each `lang`, and `exact`.

- [ ] **6. Apply the pass criterion.** Pass needs all three of the lines above, read from `all`, each `lang` group and `exact`. A language with fewer than 4 queries is reported but not judged alone. If it fails, stop here: hybrid does not ship; record the numbers and the reason in decision 0021 as "Rejected", and tell the coordinator so P6 and P7 wait.

- [ ] **7. Decide the Large tier.** List Large (F2LLM-v2-330M) only if `text+f2llm` (fused with static the same way) beats `text+granite+static` by more than **0.05 R@5** overall, the research's tie band. Otherwise it stays unlisted.

- [ ] **8. Calibrate the floors.** From `--sweep-floors`, take per model the highest floor whose R@5 stays within 0.01 of its R@5 at floor 0. Those are τG (granite) and τS (static).

- [ ] **9. Confirm the tokenizer license (answer 3).** Read the notes P4a wrote in `crates/embed/AGENTS.md` and the two cards they link. If nothing names terms other than Apache-2.0, keep granite-97m-r2. Else switch to granite-embedding-107m-multilingual (Apache-2.0, XLM-R tokenizer; chat R@1 0.81, 2.0 ms a query): that needs a follow-up plan adding XLM-RoBERTa to `crates/embed` before P6 merges, and a re-run of steps 5 to 8 with it.

- [ ] **10. Write the results.**
  - `docs/perf/embedding-spike.md`: the tables of step 5 (scores and counts only: no query text, no titles, no session ids), the floors, the gates of P4a's bench, the Mac and the date.
  - `docs/decisions/0021-local-embeddings-for-search.md`: Context (privacy.md, the research), Decision (models with pinned revisions and SHA-256s of the files, RRF k = 60 over FTS AND + granite + static, the floors, Large listed or not, the tokenizer license verdict), Consequences (download sizes, memory, the privacy page), Status "Proposed", Date.
  - The floors: set `FLOOR_GRANITE` and `FLOOR_STATIC` in `crates/engine/src/search/hybrid.rs` (P6 Task 3), or hand the two numbers to the coordinator if P6 hasn't merged.
  - If Large is listed: its manifest entry for P7 (`crates/engine/data/models.json`), with the file sizes and SHA-256s of the revision from step 2.

- [ ] **11. Commit and open the pull request** from `agent/search-p4b-gate`: `git add docs/perf/embedding-spike.md docs/decisions/0021-local-embeddings-for-search.md` (and `hybrid.rs` if changed), `git commit -m "docs: embedding spike results and decision 0021"`. Check `git status` shows nothing under `docs/local/` staged.

- [ ] **12. Clean up.** `rm /tmp/nl-eval/claude /tmp/nl-eval/codex`; keep or delete `tools/embed-eval/models/` (about 1.3 GB with F2LLM).
