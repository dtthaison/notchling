# 0024: Search reach: the user everything on disk, an agent its organization's last 14 days, masked

## Context

Phase 9 indexes every transcript and page on disk. Agents search through `notchling search` and `search.facets`. The first build leaked: a Personal session saw every organization's pages, because a page had no organization. The maintainer chose the reach on 2026-11-24 (Phase 9 plan, decision 2).

## Decision

- The user's search reaches everything on disk; `searchIndexRange` can narrow it to 14 or 30 days.
- An agent's search needs a verified session (`no_session` otherwise, never Personal). It reaches its own organization only: a session whose folder the organization claims and that changed in the last 14 days, whatever `searchIndexRange` says; a page placed by its `cwd`, then its `source`, else Personal only when Personal is the only organization.
- The cut runs before `limit`, on text and meaning rows alike.
- What an agent reads is masked (titles, snippets, page accounts, facet values): emails as `c***@p***.com`; a known token prefix kept and the rest `***` (`sk-ant-`, `sk-`, `github_pat_`, `ghp_` and kin, `xox?-`, `AIza`, `AKIA`); a JWT as `eyJ***`; a run of 32 or more mixing letters and digits as `***`. A filter value matches only through a raw value in reach whose mask equals it, so a guessed email confirms nothing.
- Settings are never searched for an agent.

## Consequences

- An agent may get fewer rows than `limit`, and never sees an older session even when the user's range is `All`.
- Masking is for display, not a secret store: matching runs on the raw text, so a word of an email in a page title still matches.
- `embed.status` for an agent needs a verified session and gives no counts, which span every organization (`notchlingd/src/embed_scope.rs`).

## Status

Accepted: the maintainer decided the reach on 2026-11-24. The masking rules are the builders' call, not the maintainer's, and open to change.

## Date

2026-11-24
