# 0023: Semantic Search models: what is on disk decides

## Context

Semantic Search loads Granite through the `notchling-embed` helper and a static table in the daemon. The app wrote the chosen models to `semanticModel`, while the daemon loaded whatever it found complete in `<data>/models/`, so the two could disagree: Settings could show a model that was not there, or miss one that was. A model is fetched only on the user's yes.

## Decision

- The daemon loads the models it finds complete on disk (`notchlingd/src/embed_paths.rs`): Granite's three files in a real folder; the static table only when its SHA-256 matches `table-512i8.sha256`.
- The app shows what `embed.status` reports (`models[].present`, `fetch`) and no longer writes `semanticModel`.
- The key stays in the settings list and on the wire, unused, so nothing is removed from the protocol.
- A model arrives only by the app's Download after the user's click, or by an agent's `embed.request` card the user allows (the app then calls `embed.fetch`). Remove deletes the model's folder and its vectors.

## Consequences

- One source of truth; no migration. A model folder copied by hand loads if it is complete.
- `semanticModel` is dead weight until a later record removes it.

## Status

Accepted: the maintainer decided it on 2026-11-25.

## Date

2026-11-25
