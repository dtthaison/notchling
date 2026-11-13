# SDD ledger — plan: docs/superpowers/plans/2026-11-14-versions-p2-check.md

Spec: docs/superpowers/specs/2026-11-13-version-sync-design.md (agent/version-sync-design 344d8a5)
Code read on: origin/merge/settings-batch 9de4da1
Overview: docs/superpowers/plans/2026-11-14-versions-overview.md
Branch: agent/versions-p2-check, start: (set at dispatch)

## Preflight scan

| Rows | Produces / consumes | Found |
| --- | --- | --- |
| T1→T2, T4, T5 | Skips, strict_* | consistent; `transcript/**` is auto zone |
| T2 | `FLAGS_USED` from `tools/codex/plan.rs` | a new export; the test builds each turn kind |
| T2→T3→T4 | Outcome, CheckResult, Book | consistent; `CheckTier` is named apart from local-api's `Tier` |
| T4 | Launcher, FakeLauncher | `control/launcher.rs`: `run`, `run_capped`, `run_pty`; the test asserts no `run_pty` |
| T6 | inbox `Kind` | today: SecretApproval, SecretRequest, LimitNotice, ActionFailed, Notice, SettingChange; additive |
| T7 | curl through Launcher | the pattern of `app/src/welcome/updates.rs` `CurlFetcher` |
| T7 | `install` switch | `notchlingd/src/switches.rs`; `forbidden` with the off message |
| T9 | Phase 1 test kept | `unchecked_mac_version_is_labelled` still holds before a first check |
| Global | privacy.md | protected; changes in T7's PR |

## Tasks
