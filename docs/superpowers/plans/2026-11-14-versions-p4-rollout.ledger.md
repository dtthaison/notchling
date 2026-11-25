# SDD ledger — plan: docs/superpowers/plans/2026-11-14-versions-p4-rollout.md

Spec: docs/superpowers/specs/2026-11-13-version-sync-design.md (agent/version-sync-design 344d8a5)
Code read on: origin/merge/settings-batch 9de4da1
Overview: docs/superpowers/plans/2026-11-14-versions-overview.md
Branch: agent/versions-p4-rollout, start: (set at dispatch)

## Preflight scan

| Rows | Produces / consumes | Found |
| --- | --- | --- |
| T5→T3 | `ToolAdapter` | T3 takes the trait: dispatch T5 first, or T3 with a test adapter |
| T1, T2, T3, T4, T6→T7 | `next`, approval, plans, journal and lock, `Busy` | consistent |
| T4 | IMMEDIATE transaction | `store::update_doc` already uses `TransactionBehavior::Immediate`; no schema change |
| T4 | `Alive` | the real one in T7 reads the process table (`accounts::procs` pattern) |
| T7→T8 | `RolloutHost` | the simulation implements it |
| T9 | `PlanKind` | `engine_plan.rs`: Fix, Replace, SetUpAll, Running, SendAgain, Update; additive |
| T10 | `keys.rs` classes | `.free`, `.ask`, `.never` exist (decision 0020 on merge/settings-batch) |
| T11 | inbox `Kind` | additive; shares `model.rs` with the cluster plans |
| T14 | `agy update` | unverified (spec §26): `AGY_UPDATE_VERIFIED` false until M5 |
| Global | `tests_engine_world.rs` `World` | reused for the host tests |

## Tasks
