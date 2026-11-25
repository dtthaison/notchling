# Version sync: the plans, their order, and what needs the maintainer

These plans build `docs/superpowers/specs/2026-11-13-version-sync-design.md` (branch `agent/version-sync-design` at 344d8a5, with the user's four decisions and the coordinator's ten calls). One plan per phase of its section 23. Each is written for one subagent per task (superpowers:subagent-driven-development), on its own branch, merged by the maintainer only. Code was read on `origin/merge/settings-batch` (9de4da1); plan and build against it, or `agent-run-oss` once that batch is merged.

| Phase | Plan | Branch | Tasks | Checkpoints |
| --- | --- | --- | --- | --- |
| 0. Decisions | `2026-11-14-versions-p0-decisions.md` | `agent/versions-p0-decisions` | 3 | 0 (the maintainer accepts) |
| 1. See versions | `2026-11-14-versions-p1-see.md` | `agent/versions-p1-see` | 11 (Task 10 waits for cluster B) | 2 (the second on the Mac) |
| 2. Check on the Mac | `2026-11-14-versions-p2-check.md` | `agent/versions-p2-check` | 10 | 2 (the second on the Mac) |
| 3. Workers stay put | `2026-11-14-versions-p3-workers-stay.md` | `agent/versions-p3-workers-stay` | 8 | 2 (the second on a real worker) |
| 4. Roll out | `2026-11-14-versions-p4-rollout.md` | `agent/versions-p4-rollout` | 17 | 3 (the third on real workers) |
| 5. Notchling on workers | `2026-11-14-versions-p5-notchling.md` | `agent/versions-p5-notchling` | 8 (Tasks 3 and 7 the maintainer's) | 1 (maintainer) |
| 6. The daemon coordinates | `2026-11-14-versions-p6-daemon.md` | `agent/versions-p6-daemon` | 6 | 1 (maintainer) |
| 7. Known-good versions (B) | `2026-11-14-versions-p7-known-good.md` | `agent/versions-p7-known-good` | 4 | 0 |
| Logging hand-over | this file, below | `agent/versions-logging` | 2 | 0 |

69 tasks in all. Each plan has a `.ledger.md` beside it with its preflight scan, for the controller to continue.

## Order and dependencies

```
P0 ── any time.  P0 T3 (goals.md) accepted before P3 merges; P0 T2 (Sparkle record) before P5 T3.

P1  T1 > T2 > T3 ─┐
    T1 > T4 ──────┼─> T5 (daemon) ─┬─> T7 (feed) > T8 (Machines rows)
    T4 > T6 ──────┘                └─> T9 (Settings Client)
    T10 only after cluster B T3 and B T13 ; T11 last

P2 (after P1)   T1 (strict) ─┐
                T2 > T3 ─────┴─> T4 (check) > T5 (soak), T6 (notice) ; T4 > T7 (Latest, Update) > T8 ; T4 > T9 ; T10
P3 (after P1, beside P2)   T1 > T2 > T3 > T4 > T5 ; T6 (drift) after P2 T4 (it needs usable references) ; T7 ; T8

P4 (after P2 and P3)   T5 (adapters) > T3 (plans) ; T1, T2, T4, T6 any order ─> T7 (SshHost) > T8 (sim) > T9 (plan kinds)
                       > T11 (notices) ; T10 any time ; T9 > T12, T13 ; T5 > T14 > T15 ; T7 > T16 ; T17 last

P5   T1 (manifest) and T2 (writer) any time after P1 ; T3 (maintainer) after T2 and P0 T2 ; T4 after P4 T5 and T1
     T5 after cluster B T10 to T13 ; T6 after T4 and P4 T7 ; T7 after the Developer ID ; T8
P6 (after P4 and cluster B)   T1 > T2 > T3, T4 ; T5 after cluster B T13 ; T6
P7 (after P4 and P5 T1 to T3)   T1 > T2 ; T3 (maintainer) ; T4
Logging   L1 after logging Phase 1 ; L2 after logging Phase 2 and L1
```

## Dependencies on other work

**Cluster v1 step 1 (plan B, `2026-11-13-cluster-v1-b-nodes.md`, written but not yet committed: it sits in the `wt-cluster-plans` worktree, branch `agent/cluster-v1-plans`).**

| Here | Needs from B | Why |
| --- | --- | --- |
| P1 T1 | nothing; B T3 creates `engine::versions` if it lands first | Shared module. Whichever lands second keeps the other's names and adds what's missing (call V3) |
| P1 T10 | B T3 (`compat`), B T13 (`delegate --machine`) | The warning goes into B's code. Parked in the ledger until B merges; P1 ships without it |
| P5 T5, T6 | B T10 to T13, B T11's headless switch policy | A worker's daemon, the node tier, a restart the home may ask for |
| P6 all | B T6 (`NodeCore`), T10 (`ClusterNode::local`), T11, T13 | `NodeHost` runs steps through each worker's daemon; dispatch refusal is B's `compat` |
| P4 T8 | optional: B T7's simulator | A versions-only simulation now; merge later (call V8) |

**Conflicts with plan B to settle before either merges:**

- **C1. Refuse or warn.** B Task 3 has `compat(Dispatch)` *refuse* on a tool mismatch, with a test asserting it. The version-sync design (coordinator's call 10, 2026-11-14, after B was written) says warn from Phase 1 and refuse only from Phase 6, when rollouts keep workers in step. Recommended: B Task 3 adopts Warn for the tool rule now (one line and one test name in B's plan); if B lands as written, P1 T10 changes it and that is a `WEAK-TEST-EDIT` for the maintainer, and P6 T5 changes it back.
- **C2. Two Mac probes.** B Task 10's `cluster_versions::probe` reads the same files as P1 T3's `versions::mac::detect`. Recommended: B calls `detect` and keeps only the node-level fields (`notchling`, `proto`, `schema`, `os`, `arch`). Whichever lands second removes the duplicate.
- **C3. Two `Tool` enums.** B's `cluster::Tool` is `{Claude, Codex, Agy, Shell}`; `versions::Tool` is `{Claude, Codex, Agy, Notchling}`. Keep both; `cluster/versions.rs` gets `fn tool_of(t: cluster::Tool) -> Option<versions::Tool>` (Shell has none).

**The logging event stream (`agent/logging-design`, spec at 2a79196).** Until its Phase 2 (the bus) merges, every fact goes through `versions::record::Recorder` (P1 T5), whose daemon side appends `audit.jsonl` lines with fixed words only, as today's lines do; Machines' facts reach the daemon through `notchling.db` docs it reads (calls V4, V9). Two floating tasks then move them onto the stream:

### Task L1: The Versions kinds in the registry (after logging Phase 1)

**Zone:** maintainer (`crates/events`, `tests/golden/log-kinds.json` is a `WEAK-TEST-EDIT`, `docs/log-kinds.md`). No CAP.

**Files:**
- Create: `crates/events/src/kinds/versions.rs` (group `Versions`)
- Modify: `crates/events/src/kinds/mod.rs`, `tests/golden/log-kinds.json`, `docs/log-kinds.md` (regenerated with `notchling log kinds --markdown`)
- Test: the registry's own tests (`crates/events/src/registry_tests.rs`), unchanged

- [ ] **Step 1:** Declare, with `kinds!` and `log_choice!`, spec §19's kinds: `version.changed`, `version.checked`, `rollout.started`, `rollout.step`, `rollout.lock`, `rollout.finished`, `rollout.rolled_back`, `selfupdate.changed`, `version.drift` (`tool.version_seen` is already in the logging registry). Fields are typed only: `Version`, `Choice`, `Ref<Machine>`, `Count`, `Millis`, `Flag`. Locks and sinks exactly as §19's table; the rollout kinds `Always`.
- [ ] **Step 2: Run** `cargo test -p notchling-events`. Expected: FAIL on the golden and the docs list.
- [ ] **Step 3:** Regenerate the golden and `docs/log-kinds.md`; the PR names the golden as a `WEAK-TEST-EDIT`.
- [ ] **Step 4: Run** `cargo test -p notchling-events` and `PYTHONPATH=tools/guard python3 -m guard lint` (`LINT-LOG-NO-TEXT`). Expected: PASS.
- [ ] **Step 5: Commit** `events: the Versions kinds`.

### Task L2: The recorder pushes events (after logging Phase 2 and L1)

**Zone:** maintainer (`notchlingd`, `crates/engine/src/versions/record.rs`). No CAP.

**Files:**
- Modify: `crates/notchlingd/src/versions_ops.rs` (the daemon's `Recorder` maps each `Fact` to its kind and calls the bus's `push`; the audit-line path is removed), `crates/engine/src/versions/record.rs` (`Fact` to kind mapping, pure)
- Test: `crates/notchlingd/src/versions_events_tests.rs` (the logging design's test sink)

- [ ] **Step 1: Write the failing tests:** `each_fact_becomes_its_kind`, `no_fact_carries_text` (the canary marker planted in a worker's output never reaches an event), `rollout_kinds_reach_the_audit`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd --lib versions_events`. Expected: FAIL.
- [ ] **Step 3: Implement** with `push` (rollouts already save their journal before each effect; no version kind needs `commit`). **Step 4: Run.** Expected: PASS.
- [ ] **Step 5: Commit** `versions: facts go to the event stream`.

## Parallel lanes

One subagent per lane at a time, each with its own `CARGO_TARGET_DIR`.

1. **Engine pure lane:** P1 T1 to T4; P2 T2, T3, T5's engine half; P3 T1, T2; P4 T1 to T6, T8, T14; P5 T1, T4; P7 T1. (`crates/engine/src/versions/`, `install/toml_edit.rs`.)
2. **Transcript lane (auto zone):** P2 T1 (`crates/engine/src/transcript/`).
3. **Daemon lane, one task at a time:** P1 T5; P2 T4, T5's daemon half, T6, T7; P3 T6; P4 T10's daemon test, T11; P5 T5; P6 T1 to T5; L2.
4. **Machines engine lane:** P1 T6, T7's engine half; P3 T3, T4, T5; P4 T7, T9, T16; P5 T6's engine half.
5. **App lane (auto zone):** P1 T7's app half, T8, T9; P2 T8, T9; P3 T7; P4 T12, T13; P5 T6's app half.
6. **Release lane (maintainer):** P5 T2, T3, T7; P7 T3.
7. **Docs lane:** P0; each phase's last task; L1.

**Files one task at a time, across lanes:** `crates/local-api/src/wire.rs`, `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/lib.rs`, `crates/notchlingd/src/versions_ops.rs`, `crates/engine/src/versions/mod.rs`, `crates/engine/src/versions/record.rs` (`Fact`), `crates/engine/src/inbox/model.rs` (`Kind`), `crates/engine/src/machines/step.rs`, `crates/engine/src/machines/catalog.rs`, `crates/engine/src/machines/engine_plan.rs`, `crates/engine/src/settings/keys.rs`, `crates/app/src/sheets/client_model.rs`, `crates/app/src/machines/versions_model.rs`, `docs/feature-map.md`, `docs/local-api-ops.md`, `crates/AGENTS.md`, `CHANGELOG.md`. The cluster plans share `wire.rs`, `daemon_wiring.rs`, `inbox/model.rs` and `feature-map.md` too: rebase before each of those tasks.

## Maintainer only

No agent does any of these. Each gates only what is named.

| # | What | Gates |
| --- | --- | --- |
| M1 | Accept decisions 0026 (tool versions follow the Mac) and 0027 (Sparkle, one EdDSA key; amends 0010), renumbered at merge; accept the goals.md and privacy.md changes | P0; P3 merge (goals.md); P5 T3 (0027) |
| M2 | The **Developer ID**: signing and notarizing the app, then Sparkle | P5 T7 |
| M3 | The **Sparkle EdDSA key**: generate or reuse the appcast's, keep one offline backup, store the private half as the repository secret the `sign` job reads, put the public half in `versions::manifest::RELEASE_KEY` and `SUPublicEDKey` | P5 T3, T7 |
| M4 | The **release path**: `.github/workflows/beta.yml`, `promote.yml`, `release.yml`, `tools/release/*`, `tools/bundle-next.sh`; running a Beta with the four worker tarballs and the signed manifest | P5 T2, T3, T7; P7 T3 |
| M5 | **Real-worker checks** (record what printed, with no account details, in the phase's ledger): P2: `claude doctor`'s auto-update line, `codex exec --help`'s flags, Codex's standalone package layout, Claude Code's per-line `version` on a fresh transcript. P3: `AGY_CLI_DISABLE_AUTO_UPDATE` works, `agy --version` doesn't update, Codex's `daemon_auto_start` exists. P4: `ps` and `claude agents --json` outputs for the busy fixtures, a manual `claude install` and the supervisor, Codex keeping old versions, `agy update` exists. P5: one Linux worker from build A to B and back. P6: phone and Mac answering one item | Each `*_VERIFIED` const (P3 T4, P4 T14) and each phase's last checkpoint |
| M6 | Approve each **capability and test edit**: `CAP-PROCESS` (P1 T5 agy; P2 T4, T7; P4 T7, T16; P5 T5; P6 T1), `CAP-NETWORK` (P2 T7; P4 T15; P5 T6, T7), `CAP-FILEWRITE` (P3 T3, T4, T5; P4 T4, T15; P6 T2), `WEAK-TEST-EDIT` (P1 T6 and P3 T4 if the catalog count changes; P3 T5's service-file tests; P1 T10 and P6 T5's dispatch rule; L1's golden), protected `Cargo.toml` (P3 T1 if `toml` is added) | Each PR |
| M7 | **Merges** of every phase's PR; **installs** with `tools/bundle-next.sh --install` after merges; running `notchling-machines --live` for the checkpoints | Every live check |
| M8 | **Conflict C1** with cluster plan B (warn or refuse), and C2, C3 | P1 T10, cluster B T3 |

## Calls made where the design left room

- **V1. Phase 1's comparison.** Before Phase 2 there is no checked reference, so Phase 1's marks compare workers with the Mac's installed version, labelled "(not checked yet)". Nothing can roll out in Phase 1, so a mark against an unchecked version only informs. Phase 2 T9 drops the fallback.
- **V2. agy on workers.** No `agy --version` runs on a worker until its self-update knob is verified and set (spec §7's warning that the probe could be what updates it). agy rows show path and method with an unknown version until then; agy rollouts wait for `AGY_UPDATE_VERIFIED`.
- **V3. One `engine::versions`, strict pattern.** Shared with cluster B; the spec's strict pattern (§3) wins over B's looser one, since a version enters scripts.
- **V4. Machines speaks to the daemon only at the agent tier.** It reads `versions.get`; everything it knows about workers and rollouts it writes to `notchling.db` docs (`versions/workers`, `versions/rollouts`, `versions/lock`), which the daemon reads for `versions.workers` and its notices. Machines never calls an app-tier op.
- **V5. No keys for what isn't built.** `versionCheckScreens` and `versionCheckTurn` (spec §20) are added with their additions, not now; `versionGate` comes with Phase 7.
- **V6. Removing a worker turns self-update back on through a plan.** The spec says removal takes the key back; here the Remove confirm shows Self-Update On's plan and runs it on that click (plan, confirm, run kept).
- **V7. The lock is a doc row.** `store::update_doc` already runs in an IMMEDIATE transaction, so `versions/lock` needs no schema change.
- **V8. A versions-only fleet simulation** in P4 T8, standalone, so Phase 4 doesn't wait for cluster B; it can share B's simulator later (spec §25 left this open).
- **V9. Machines' own facts** (self-update writes, rollout steps) reach the recorder through the docs the daemon reads, not a second audit file.
- **V10. Unverified vendor facts are consts.** `AGY_KNOB_VERIFIED`, `CODEX_DAEMON_KNOB_VERIFIED`, `AGY_UPDATE_VERIFIED` start false; each flips in a one-line PR after the maintainer's check (M5).
- **V11. Decision numbers** are placeholders (0026, 0027); Phase 6 needs no record of its own, since 0026 already says when the coordinator moves.
- **V12. The strict pass's reach** (spec §25): the newest 20 transcripts or rollouts, each at most 4 MB; the check fails only on skips under the new version.
- **V13. Notchling's own check** is in-process (`hello`, `sessions.list`) plus fixtures compiled into the build, so it starts nothing.
- **V14. Settings > Agents** gets Codex and Antigravity pages in P1 T9 if decision 0019's branch hasn't added them; if it has, P1 uses its names.
- **V15. Opening Machines from an Inbox row** uses the app's existing Machines path; if there is none, the row says where to open it (P3 T6), never a new process launch path added silently.
