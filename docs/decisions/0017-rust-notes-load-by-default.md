# 0017: The Rust notes load by default; the Swift notes are read on demand

## Context

[0003](0003-agent-instructions-in-agents-md.md) has the root `AGENTS.md` import the Swift app's notes, `@legacy/macos/Sources/AGENTS.md`, so Claude Code loads them in every session. That was right while the Swift app was the app.

By 2026-11-12 the Rust workspace (`crates/`, about 236,000 lines against the Swift app's 49,000) is the app on the maintainer's Mac, and the Swift app is retired there. Yet the only working notes ("architecture in one pass", "things that bit before") were the Swift app's: every session loaded about 50 KB of Swift detail, and an agent working on Rust got none for its own code. The root also carried two long paragraphs about the daemon and the hook, which pushed it past its 80-line limit and failed `test_root_is_short`.

## Decision

- The Rust app gets working notes of its own, [`crates/AGENTS.md`](../../crates/AGENTS.md), in the Swift notes' form. The daemon and hook paragraphs move there from the root.
- The root `AGENTS.md` ends with `@crates/AGENTS.md`, so Claude Code loads the Rust notes in every session. Codex and Antigravity read it themselves when they work under `crates/`.
- The Swift notes stay where they are and stay linked from the root's layers and folder notes. Nothing imports them; an agent reads the whole file before changing `legacy/macos/` or porting a part, as it already had to for the phone's notes.
- `tools/guard/tests/test_manual_move.py` checks the new chain: the root ends with `@crates/AGENTS.md`, links both other notes, and the facts of the old `CLAUDE.md` are still found across the root, the Swift notes and the Rust notes.

The rest of 0003 holds: `AGENTS.md` is the source, `CLAUDE.md` is `@AGENTS.md`, no other `CLAUDE*.md`.

## Consequences

- A Claude Code session loads the notes for the code that runs, and about 30 KB less in all.
- An agent changing Swift code must open its notes itself. The root says so, in the same words it uses for the phone.
- `crates/AGENTS.md` is protected like every `AGENTS.md` (`tools/guard/zones.json`).

## Status

Proposed. Amends [0003](0003-agent-instructions-in-agents-md.md)'s note on importing the Swift notes. The maintainer settles it by merging the change that brings it.

## Date

2026-11-12
