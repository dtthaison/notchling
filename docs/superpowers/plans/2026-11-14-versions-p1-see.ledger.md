# SDD ledger — plan: docs/superpowers/plans/2026-11-14-versions-p1-see.md

Spec: docs/superpowers/specs/2026-11-13-version-sync-design.md (agent/version-sync-design 344d8a5)
Code read on: origin/merge/settings-batch 9de4da1
Overview: docs/superpowers/plans/2026-11-14-versions-overview.md
Branch: agent/versions-p1-see, start: (set at dispatch)

## Preflight scan

| Rows | Produces / consumes | Found |
| --- | --- | --- |
| T1→T3, T4 | Tool, Version, Method, cli_output | consistent |
| T2→T3 | IdentityCache | consistent; `sha2 = "0.10"` is already in `crates/engine/Cargo.toml` |
| T3→T5 | detect, Roots, watch_paths | consistent |
| T4→T6, T7, T8 | parse_probe, WorkerTools, Mark, mismatch | consistent |
| T5→T7, T9 | `versions.get` reply shape (final now; Phase 2 fills the reference fields) | consistent |
| T6 | `Detected` gains `tools: Option<WorkerTools>` | `step.rs` `Detected::merged` must take the new field |
| T6 | catalog order | `catalog.rs` inserts `claude_login` after `CLAUDE_CODE`; Tool Versions goes after it |
| T7 | Machines → daemon client | `crates/app` already depends on `notchling-local-api`; Machines has no daemon client today |
| T9 | Settings > Agents | merge/settings-batch has `Page::Agents` with `Tool::ClaudeCode` and `Section::Hooks` only (0019's Client part is on another branch) |
| T10 | cluster B T3, T13 | not merged; parked until they are (conflict C1) |
| Global | package names | `notchling-engine`, `notchling-notchlingd`, `notchling-local-api`, `notchling-app`; one filter per run |

## Tasks
