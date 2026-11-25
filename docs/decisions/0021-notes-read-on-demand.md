# 0021: The root AGENTS.md is one page; the working notes are read on demand

## Context

[0017](0017-rust-notes-load-by-default.md) has the root `AGENTS.md` end with `@crates/AGENTS.md`, so Claude Code loads the Rust notes in every session. They were about 20 KB then. By 2026-11-16 they were 113 KB, and with the root's 8 KB every session in this repository started with about 122 KB (some 30,000 tokens) of notes, most of them about areas the session never touches. The maintainer asked for the start to be about one page.

## Decision

- The root `AGENTS.md` imports nothing and stays under 6 KB (`test_root_is_small`, beside the 80-line `test_root_is_short`). It keeps what the app is, how to build and test, the hard rules that prevent damage, the style, and an index: one line per topic, saying when to read it.
- The Rust notes' "Architecture in one pass" and "Things that bit before" move, word for word, into one file per topic under [`docs/notes/`](../notes/). Each topic's "Things that bit before" sits in its own note; the few that fit no topic stay in `crates/AGENTS.md`.
- `crates/AGENTS.md` keeps the crate table, the cargo commands, Testing, and a list of the notes. Codex and Antigravity still read it when they work under `crates/`.
- The root's full check list, "The line", "How work flows" and "Folder notes" move to [`docs/notes/workflow.md`](../notes/workflow.md); the root keeps the short version.
- `docs/notes/` is protected like the `AGENTS.md` files (`!docs/notes/**` in `tools/guard/zones.json`, so `.github/CODEOWNERS`).
- `tools/guard/tests/test_manual_move.py` checks the new chain: the root imports nothing, links the Swift and phone notes and names every file in `docs/notes/`, and the facts of the old `CLAUDE.md` are still found across the root, the Swift notes, the Rust notes and `docs/notes/`.

The rest of 0017 holds: the Rust notes are the working notes, the Swift notes are read on demand. 0003 holds: `AGENTS.md` is the source, `CLAUDE.md` is `@AGENTS.md`, no other `CLAUDE*.md`.

## Consequences

- A Claude Code session starts with about 6 KB of instructions instead of about 122 KB.
- An agent must open the note for its change itself. The root's index and `crates/AGENTS.md` say which one.
- A note that nothing points to is never read, so the index test fails on a `docs/notes/` file the root doesn't name.

## Status

Proposed. Amends [0017](0017-rust-notes-load-by-default.md)'s import of the Rust notes. The maintainer settles it by merging the change that brings it.

## Date

2026-11-16
