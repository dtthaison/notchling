# XXXX: One capability contract for every tool, and a matrix that cannot hide a gap

## Context

[0016](0016-provider-interface-in-the-engine.md) gave each tool one interface in the engine, but only for six things (the transcript, sessions, the catalog and a few more). The rest of what Notchling does for a tool (subagents, finished signals, holding a message, delegating, search text) grew tool by tool, so Claude Code does things Codex and Antigravity do not, and nothing says which.

Facts that bound the change:

- Three tools are supported: Claude Code, Codex and Antigravity. `catalog::Tool` already serializes them as `claude`, `codex` and `antigravity`; `agy` is only the command's name.
- Setup (decision 0019) and versions (the version-sync design) are not built yet, so those rows are honest gaps. Skills for every tool and Claude Code's hooks are built (`install::skills`, `install::hooks`), so they are Native.
- A gap nobody can see is a gap nobody closes.

## Decision

- The engine has one closed list of 39 capabilities (`contract::Capability`, in 10 groups) and one closed list of tools (`contract::Tool`, moved out of the catalog and re-exported there).
- Each tool declares every capability in `contract/<tool>/support.rs`, one exhaustive `match` under `#![deny(clippy::wildcard_enum_match_arm)]`, in one of four states with a one-line reason of at most 120 characters:
  - **Native**: Notchling does it with the tool's own files or API.
  - **Fallback**: Notchling does it itself, through a named flow (`held-queue`, `hide-row`, `delegate-jobs`, `process-tree`, `catalog-data`, `version-probe`, `version-cache`, `title-index`).
  - **Not applicable**: the tool has nothing to do here, and why.
  - **Gap**: not built, with the phase or design that closes it.
- `contract/gaps.json` (34 entries at the start) and `contract/unwired.json` (72) only shrink. `gaps.json` lists every Gap; `unwired.json` lists every Native or Fallback cell the registry does not route yet (a cell leaves it in the pull request that adds it to its provider's `WIRED` list). A new tool starts with no Gap and no unwired cell. The guard rule `GUARD-PROVIDER-GAPS` sends a pull request where either file grew to the maintainer.
- A conformance suite (`crates/engine/tests/conformance/`) has one test per tool and capability and runs a check only for wired cells; unwired cells are skipped and counted.
- `docs/provider-matrix.md` is generated from the declarations by the `provider_matrix` test and compared exactly. `notchling providers [--json]` prints the same table, and Settings > Agents shows each tool's Fallback, Not applicable and Gap rows under "What Notchling Can Do".
- The wire name is `antigravity`; `agy` is read, never written.
- This decision changes no behavior. Ports, one per phase, route surfaces through the registry in phases 1 to 6 (subagents, signals, delegate, the held queue, the other ports, clean-up).

## Consequences

- Adding a capability or a tool fails the build in every `support.rs` until each answers it.
- A gap cannot be added quietly, and one cannot be removed by relabelling it Not applicable without showing in the matrix diff and carrying a reason.
- The setup and version rows stay in `gaps.json` until their designs land; those pull requests remove them.
- Starting numbers (34 gaps: Claude Code 4, Codex 14, Antigravity 16; 72 unwired) are accepted by merging the pull request that brings them.

## Status

Proposed. Widens [0016](0016-provider-interface-in-the-engine.md). The maintainer settles it by merging the change that brings it, and assigns its number then.

## Date

2026-11-14
