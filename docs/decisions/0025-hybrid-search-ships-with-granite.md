# 0025: Hybrid search ships with Granite, floor 0.80

## Context

Semantic Search fuses the text index with meaning search ([the design](../superpowers/specs/2026-11-13-hybrid-search-design.md)). Its floors stayed 0.0 and the Large tier unlisted until a gate on real data ran ([the P4b checklist](../superpowers/plans/2026-11-14-search-p4b-real-data-gate.md)): hybrid must beat text-only by at least 0.15 R@5 overall, no language may get worse, and exact-looking queries may not get worse.

On 2026-11-25 the gate ran over the maintainer's own Claude Code and Codex sessions on his Mac (668 sessions, 12,654 chunks, 45 labelled queries), with his approval, read only. Only `granite-97m-r2` was on disk; the static model and F2LLM were not, and nothing was downloaded. Numbers: [embedding-spike.md](../perf/embedding-spike.md).

## Decision

- Hybrid search ships with text + Granite (`granite-97m-r2@835ad14/p1`, the revision and SHA-256s already in `crates/engine/data/models.json`), fused by reciprocal rank with k 60. It passed all three criteria: R@5 0.889 against 0.222 overall, English and Vietnamese both better, exact-looking queries equal (0.750).
- The Granite floor is 0.80 (`hybrid.rs::FLOOR_GRANITE`): the highest floor of the sweep whose R@5 stays within 0.01 of floor 0.
- The floor belongs to the loaded embedder, not the mode at query time: `embed_ops.rs::open` gives the Granite helper 0.80 and the fixture's hash embedder 0.0, since its cosines are not Granite's, and `Lists.floors` carries them to the scan and to fusion.
- The static floor stays 0.0 until the static model is scored.
- The Large tier stays unlisted: F2LLM was not scored.

## Consequences

- A Granite hit whose best chunk scores below 0.80 no longer reaches results. On this set that dropped no wanted session; 3 of 30 meaning-only hits scored between 0.80 and 0.85, so the floor is near the edge and a larger set may lower it.
- `text+granite+static` is unmeasured. The static model is still asked whenever Semantic Search is on with a model, with no floor; a run with it on disk settles its floor and whether it helps.
- Granite alone scored above the fusion (R@5 0.911 against 0.889); the text list's weight for meaning-style queries may be tuned later.
- The eval scores Granite alone when the static model is absent instead of stopping.

## Status

Proposed. The maintainer approved running the gate; he settles the record by merging `agent/p4b-gate`. Open: the static model's run and the Large tier.

## Date

2026-11-25
