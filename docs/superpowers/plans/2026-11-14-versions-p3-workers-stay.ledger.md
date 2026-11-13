# SDD ledger — plan: docs/superpowers/plans/2026-11-14-versions-p3-workers-stay.md

Spec: docs/superpowers/specs/2026-11-13-version-sync-design.md (agent/version-sync-design 344d8a5)
Code read on: origin/merge/settings-batch 9de4da1
Overview: docs/superpowers/plans/2026-11-14-versions-overview.md
Branch: agent/versions-p3-workers-stay, start: (set at dispatch)

## Preflight scan

| Rows | Produces / consumes | Found |
| --- | --- | --- |
| T1→T2 | `install::toml_edit` | a new module; the `toml` crate is in `Cargo.lock` but not an engine dependency (adding it is a protected `Cargo.toml` change) |
| T2 | `edit::set_member` | `edit.rs` has `append` and `remove` only; `set_member` is new |
| T2→T3→T4 | KnobPlan, read and write scripts | consistent |
| T4 | `StepId::self_update(tool)` | `step.rs` builds owned ids for `deploy_key`/`clone_repo`: same pattern |
| T4 | the `replace` slot for Self-Update On | `SetupStep.replace` exists (Replace Key precedent); `replace_title` only rewrites "Deploy key", so the step sets its own title |
| T5 | `service_files.rs` | the `Environment=PATH` line and the plist's `PATH` key exist; existing tests may change (`WEAK-TEST-EDIT`) |
| T6 | usable references | needs Phase 2 T3 and T4 merged |
| Global | goals.md | Phase 0 T3 accepted before merge |

## Tasks
