# Hybrid search: the plan set

The spec is `docs/superpowers/specs/2026-11-13-hybrid-search-design.md` (revision 4). One plan per phase; this page says the order, the lanes, what only the maintainer may do, and the calls the plans make where the spec and the newer localization design disagree or leave a gap. Every plan repeats what its own executor needs.

| Plan | Branch | Tasks | Zone of the pull request |
| --- | --- | --- | --- |
| [P1 Tier 0 and Settings search](2026-11-14-search-p1-tier0-settings-search.md) | `agent/search-p1-tier0-settings` (Tasks 1–7), `agent/search-p1-effect-texts` (Task 8) | 8 | auto, then maintainer |
| [P2 Range, categories, Actions](2026-11-14-search-p2-range-categories-actions.md) | `agent/search-p2-range-categories` | 8 | maintainer |
| [P3 Facets](2026-11-14-search-p3-facets.md) | `agent/search-p3-facets` | 7 | maintainer |
| [P4a The embedding helper and the eval tool](2026-11-14-search-p4a-embed-helper-eval.md) | `agent/search-p4a-embed` | 7 | maintainer |
| [P4b The real-data gate](2026-11-14-search-p4b-real-data-gate.md) | `agent/search-p4b-gate` (the maintainer's) | 12 steps | maintainer only |
| [P5 Store and pipeline](2026-11-14-search-p5-store-pipeline.md) | `agent/search-p5-store` | 9 | auto by path; CAP-FILEWRITE |
| [P6 Hybrid](2026-11-14-search-p6-hybrid.md) | `agent/search-p6-hybrid` | 9 | maintainer |
| [P7 Download and the Semantic Search UI](2026-11-14-search-p7-download-ui.md) | `agent/search-p7-download` | 8 | maintainer |
| [P8 More corpora](2026-11-14-search-p8-more-corpora.md) | `agent/search-p8-corpora` | 6 | maintainer |

## Before any of it

- **Base.** Every branch starts from the integration branch that holds its dependencies: `merge/settings-batch` until it merges into `agent-run-oss`, then `agent-run-oss`. Never `main`. Nothing merges into `agent-run-oss` but by the maintainer.
- **Locale.** L0 (decision 0022) and L1 (`locales/en/`, `tools/locale/gen.py`, `crates/locale`, `t::lookup`) land before P1, and L3 (Settings' words) preferably too. P1 has no fallback: it waits for L1 (coordinator's call, 2026-11-14). P2, P3 and P7 add their words to `locales/en/mac/*.ftl` once L1 is in; before that they write English literals and list each in the pull request for L3 and L6 to move.
- **Decision 0020.** `merge/settings-batch` holds it (`KeySpec` with `access`, `title`, `about`, `why`; `settings_ops.rs` raises an Ask item). P1 Task 8 adds `effect` to `KeySpec`; every Ask key added later (P2, P6) writes one, and a test fails until it does.
- **`agent/account-log-fix`** (`cost::account_log::account_at`) merges before P3 Task 4.

## Order and lanes

```
lane A (text, the app)     P1 ──▶ P2 ──▶ P3 ─────────────────┐
lane B (models)            P4a T1–T4 ──▶ T5–T7 ──▶ P4b ─────┐ │
lane C (store, on fakes)   P5 T1–T3 ──▶ T4–T9 ─────────────┐│ │
                                 └──▶ (P4a T5–T7 need P5 T1–T3)
                                                     P6 ◀───┴┴─┘ ──▶ P7 ──▶ P8
```

- Lanes A, B and C start together. Lane A and lane C both add modules under `crates/engine/src/search/`; the second to merge rebases (only `mod.rs` lines collide).
- P4a Tasks 5–7 (the static reader, the eval tool) need P5 Tasks 1–3 (the `Embedder` trait, `query::fuse` and `query::shape`, the chunker). Merge P5's first three tasks early, or rebase P4a onto P5's branch for those tasks.
- P6 needs P3 (`query::filter`), P4a (the helper crate) and P5 (store, pipeline). Its floors are 0 until P4b writes them (P4b step 9).
- P7 needs P6. Its Large row stays hidden unless P4b listed it.
- P8 needs P6 and P3.
- P4b needs P4a and the maintainer's queries; it gates the decision record and the floors, not P5 or P6's merge (with no model, every query falls back to text).

## Only the maintainer

- Merges every pull request, and approves every `maintainer-gate` hit (`ZONE`, `CAP-*`, `WEAK-*`).
- Writes the ~40 real queries and runs P4b; decides the Large tier, the floors, and granite-r2 against granite-107m; writes decision 0021 and `docs/perf/embedding-spike.md`.
- Installs (`tools/bundle-next.sh --install`); agents never pass `--install`.
- Reviews new dependencies (`candle-*`, `kitoken`, `safetensors`, `tokenizers` if kitoken's WordPiece parity fails) and `cargo deny check`'s output.
- Approves the protected files every phase touches: `Cargo.toml`, `Cargo.lock`, `deny.toml`, `crates/notchlingd/**`, `crates/local-api/**`, `crates/platform/**`, `crates/engine/src/settings/**`, `crates/engine/data/**`, the new `crates/embed/**`, `tools/**`, every `AGENTS.md`, `docs/feature-map.md`, `docs/decisions/**`, `docs/policies/privacy.md`, `locales/*/consent.ftl` and `locales/languages.json`.

## Downloads

No test and no CI job downloads a model. In development, models are fetched only by the explicit commands in P4a ("Models in development") and P4b step 2, into the git-ignored `tools/embed-eval/models/`; at run time only the app's Download button fetches (P7).

## Calls these plans make

1. **Fluent layout follows the localization design (coordinator's call, 2026-11-14).** Not `shared.ftl` with attributes: `locales/en/mac/settings.ftl`, keys `settings-<kebab key>` (title), `settings-<kebab key>-about`, `settings-<kebab key>-words` (keywords), `settings-<kebab key>-<value>` for choices (`.` becomes `-`, camelCase becomes kebab: `searchShortcut` → `settings-search-shortcut`, `daemon.accounts` → `settings-daemon-accounts`). Settings search writes `-about` and `-words`; L3 moves only the words the sheet shows today (titles, choices), and P1 writes those too only if L3 hasn't merged. ⌘⇧P's own words go in `locales/en/mac/search.ftl` (`search-…`).
2. **Effect texts live on the key line, not in Fluent.** The daemon raises the Ask item and never links `crates/locale` (localization design), so the English effect is `KeySpec::effect` in `keys.rs`, as the spec allows "until the locale work lands". L6 later gives the item a `code` and `facts`.
3. **The registry lives in the app (coordinator's call, 2026-11-14).** `crates/app/src/sheets/settings_entries.rs` holds the generated `Text` constants, aliases and places; nothing of it is in the engine, which never depends on `notchling-locale`. A test fails if a registry title differs from the row title the sheet shows today, so the two can't drift before L3 moves the rows.
4. **The app gives the daemon the Settings text.** The daemon needs English text to embed Settings for the vector lists, and can't read Fluent. P6 adds one app-tier op, `embed.settings {docs: [{id, text}]}`, sent when Semantic Search is on and at app start. The spec's op table doesn't list it; it is additive.
5. **`query::fuse`, `query::shape` and the chunker are built in P5, not P6 and P4a.** The eval tool (P4a) needs all three and must measure what ships; P5 is auto zone and runs on fakes. `query::shape::text_only` is the spec's "exact-looking" test.
6. **The `Embedder` trait is P5 Task 1.** P4a's static reader implements it.
7. **The helper's serve loop is P6, not P4a.** P4a builds the model, the bench and the eval; P6 builds both ends of the line protocol and the lifetime.
8. **`semanticSearch`, `semanticPaused` and `semanticModel` are added in P6**, which reads them; P7 builds their UI.
9. **The FTS fold is done by us before insert, in a contentless FTS5 table.** `search.db` schema 2 keeps the original text in `docs.body` and gives FTS folded text (`content=''`, `contentless_delete=1`), so snippets keep "đăng" while "dang" matches.
10. **P2 lists six categories in the sheet.** Activity and Machines have no corpus until P8, so their rows appear in P8; all eight ids are defined in P2 so the stored keys never change. Vault Names stays the app's own `vault.list` match until P8 moves it into the daemon.
11. **`SessionIndex` keeps a 14-day default.** The daemon sets the range from `searchIndexRange` (default `all`); the engine's default stays 14 days, so the existing index tests keep their meaning unedited.
12. **Ignored tests are honest.** Model-reading tests use `#[ignore]` and trip `WEAK-SKIP`; P1 adds no ignored test so it stays auto (its render is checked by eye with `--render`).
13. **P1 is two pull requests.** The auto part (fold, matcher, registry, field) can merge alone once Phase 5 allows; the effect texts, the Ask item and the notes in protected files are a second, maintainer pull request.
14. **A streaming fetch fake replaces `FakeHttp` for downloads.** The engine's `FakeHttp` holds whole bodies in memory; a 434 MB file can't. P7 adds a `Fetch` trait (streaming, Range) with `FakeFetch`; the spec's fetch tests run against it.
15. **Choice values that start with a digit get a word in their message id** (`searchIndexRange`: `-recent`, `-month`, `-all`), since a Fluent key word must start with a letter.
