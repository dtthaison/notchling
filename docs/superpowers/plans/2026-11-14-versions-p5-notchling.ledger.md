# SDD ledger — plan: docs/superpowers/plans/2026-11-14-versions-p5-notchling.md

Spec: docs/superpowers/specs/2026-11-13-version-sync-design.md (agent/version-sync-design 344d8a5)
Code read on: origin/merge/settings-batch 9de4da1
Overview: docs/superpowers/plans/2026-11-14-versions-overview.md
Branch: agent/versions-p5-notchling, start: (set at dispatch)

## Preflight scan

| Rows | Produces / consumes | Found |
| --- | --- | --- |
| T1 | Ed25519 verification | `ring 0.17` is an engine dependency; no new crate |
| T2→T3 | manifest writer → workflows | `tools/release` has `appcast.py`, `version.py`, `tests/`; `.github/workflows` has `beta.yml`, `promote.yml`, `release.yml` |
| T3 | signing key | maintainer only (M3) |
| T4 | `ToolAdapter` | from Phase 4 T5 |
| T5 | node link, node tier | cluster B T10 to T13 |
| T7 | Developer ID | maintainer only (M2) |

## Tasks
