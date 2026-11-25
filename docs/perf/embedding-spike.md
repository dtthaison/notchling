# Embedding spike: the P4b real-data gate

The gate of [the P4b checklist](../superpowers/plans/2026-11-14-search-p4b-real-data-gate.md), run on 2026-11-25 over the maintainer's own Claude Code and Codex sessions, read only, on this Mac. Scores and counts only: no query text, titles or session ids. The queries stay in `docs/local/search-queries.jsonl`, which is never committed.

**Result: pass** for text + granite. The static model and F2LLM were not on disk, so the parts that need them were not run (see Missing).

## Setup

- Mac: Apple M5, 32 GB, macOS 26.6.2.
- Tool: `crates/embed/examples/eval`, release build, `--queries`, `--data-root`, `--models`, `--sweep-floors`, `--verbose`.
- Data root: a temporary folder with `claude/projects` linked to `~/.claude/projects` and `codex/sessions` linked to `~/.codex/sessions`. 668 sessions, 12,654 chunks.
- Model: `granite-97m-r2` from Notchling's own models folder, linked under the folder name the eval expects. Its three files match the SHA-256s in `crates/engine/data/models.json` (revision `835ad14087e140460703cf0fae09f97d469d65c2`).
- Run: 473 s wall, 1,276 s CPU, peak RSS 621 MB (the eval embeds every chunk at once on every core; this is not the helper's budgeted indexing, whose gates are `granite_gates`).
- Fusion: reciprocal rank, k 60 (`query/fuse.rs::K`). Exact-looking queries (`query/shape.rs::text_only`) stay text only.

## Queries

45 queries, each with one or two wanted sessions.

| Kind | en | vi | All |
| --- | ---: | ---: | ---: |
| Meaning (paraphrase, not the session's words) | 27 | 10 | 37 |
| Exact-looking (3 URLs, 2 paths, 3 quoted phrases) | 8 | 0 | 8 |
| All | 35 | 10 | 45 |

The Vietnamese queries are all with diacritics. Across languages: one English query targets a session opened in Vietnamese, and four Vietnamese queries target sessions opened in English. There is no `vi-nodiac` or `mixed` group.

The queries were written by an agent the maintainer approved, from each session's opening request, and labelled only where the session was certain. The paraphrases avoid the session's own words on purpose, which is the case text search can't serve; the text-only numbers below are low for that reason.

## Scores

| Group | n | text R@1 | text R@5 | granite R@1 | granite R@5 | text+granite R@1 | text+granite R@5 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| all | 45 | 0.178 | 0.222 | 0.689 | 0.911 | 0.533 | 0.889 |
| en | 35 | 0.200 | 0.200 | 0.714 | 0.914 | 0.514 | 0.886 |
| vi | 10 | 0.100 | 0.300 | 0.600 | 0.900 | 0.600 | 0.900 |
| exact | 8 | 0.750 | 0.750 | 0.750 | 0.875 | 0.750 | 0.750 |

## Pass criterion

| Criterion | Read | Verdict |
| --- | --- | --- |
| Hybrid beats text-only by at least 0.15 R@5 overall | 0.889 against 0.222, +0.667 | Pass |
| No language worse (R@1 and R@5) | en +0.314 R@1, +0.686 R@5; vi +0.500 R@1, +0.600 R@5 | Pass |
| Exact-looking queries no worse | 0.750 = 0.750 (R@1 and R@5) | Pass |

## Floors

`--sweep-floors` for granite (R@5 of text+granite with granite hits below the floor dropped; 30 queries were found by meaning only):

| Floor | R@5 | Meaning-only hits lost |
| ---: | ---: | ---: |
| 0.00 to 0.80 | 0.889 | 0% |
| 0.85 | 0.822 | 10% |
| 0.90 | 0.533 | 53% |

The checklist's rule (the highest floor whose R@5 stays within 0.01 of floor 0) picks **0.80** for granite: `hybrid.rs::FLOOR_GRANITE`, pinned by `hybrid_tests.rs::floors_are_the_p4b_calibration`. It applies to the real Granite helper only; the fixture's hash embedder keeps 0.0 (`search_query_tests.rs::a_fixture_granite_hit_under_the_calibrated_floor_is_kept`). The floor sits at the edge: 3 of the 30 meaning-only hits scored between 0.80 and 0.85, so a re-run with more queries may move it down a step. `FLOOR_STATIC` stays 0.0.

## Missing

- **The static model** (`sentence-transformers__static-similarity-mrl-multilingual-v1`, or a converted `static-512i8.bin`) is not in the models folder. Not run: `text+granite+static`, the static floor sweep, and so criterion 1 as the checklist words it (text + granite + static). The eval was changed to score granite alone when the static model is absent instead of stopping.
- **F2LLM-v2-330M** is not on disk. Not run: `text+f2llm`, so the Large tier decision (step 7). Large stays unlisted.
- Nothing was downloaded for this run.

## Other findings

- Granite alone scored above the fusion (R@1 0.689 against 0.533, R@5 0.911 against 0.889): where text found nothing relevant, its wrong hits still take places in the fused list. Text's weight for meaning-style queries is worth a look later.
- Two exact-looking queries (a URL and a path) were missed by text but found first by granite. Exact queries stay text only by design, so hybrid did not help them; why text missed them is worth a look later.
