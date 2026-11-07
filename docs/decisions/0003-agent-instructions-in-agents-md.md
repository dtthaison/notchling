# 0003: Agent instructions in AGENTS.md

## Context

Agents from three tools work in this repository. Codex and Antigravity read `AGENTS.md`. Claude Code reads `CLAUDE.md`. Two sets of instructions would drift apart.

The first draft of the design said that any `CLAUDE.md` turns `AGENTS.md` off. The review found that wrong.

From the design: [The operating manual](https://github.com/dtthaison/notchling/blob/d285219/docs/superpowers/specs/2026-11-07-agent-run-open-source-design.md#the-operating-manual), "Agent instructions".

## Decision

- Agent instructions live in `AGENTS.md` files.
- The root `AGENTS.md` is short and points to the manual's layers. Each folder's `AGENTS.md` adds only that folder's rules.
- For Claude Code, the root keeps a one-line `CLAUDE.md` containing `@AGENTS.md`. That is the documented way to share them, and it works on every version and platform.
- CI fails on any other `CLAUDE*.md` in the repository.
- The worker's preflight checks that the instructions loaded, and pins the Claude Code version.

## Consequences

- Every tool reads the same instructions.
- Today's notes about the Swift app move from `CLAUDE.md` to [`Sources/AGENTS.md`](../../Sources/AGENTS.md). The root [`AGENTS.md`](../../AGENTS.md) imports it with `@Sources/AGENTS.md`, so Claude Code still loads them.
- A stray `CLAUDE.md`, such as `Sources/CLAUDE.md`, would be a second instruction source nobody reviewed. Rule `GUARD-CLAUDE-MD` in [`docs/rules.md`](../rules.md) fails it.
- Every `AGENTS.md` and `CLAUDE.md` is protected wherever it sits (`tools/guard/zones.json`). Agents can't change their own instructions without the maintainer.
- The worker's preflight comes with Phase 5, when agents are turned on.

## Status

Accepted. The maintainer decided that `AGENTS.md` is the source *(decided)*; the review corrected how Claude Code loads it.

## Date

2026-11-07
