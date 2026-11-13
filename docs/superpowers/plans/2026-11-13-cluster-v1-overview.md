# Cluster v1: the plans, their order, and what needs the maintainer

Plan A (the inbox, step 0) is merged. These plans build steps 1 to 5 of `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`, "Scope: v1 first", as the maintainer decided on 2026-11-14 (below). Every plan is written for one subagent per task (superpowers:subagent-driven-development), on its own branch off `agent-run-oss`, merged by the maintainer only.

| Plan | Step | File | Tasks | Checkpoints |
| --- | --- | --- | --- | --- |
| B | 1. Nodes: join over Tailscale with pinned mTLS, node tier, terms, Can Be Master, versions, cluster view, work on a picked Worker, seen in Chat and on the phone | `2026-11-13-cluster-v1-b-nodes.md` | 19 | 3 (the third is live) |
| C | 2. Per-job secret grants from the vault node, Worker logins | `2026-11-13-cluster-v1-c-secrets.md` | 11 | 2 |
| D | 3. Standby snapshots, the synced handover and phone continuity | `2026-11-13-cluster-v1-d-standby.md` | 14 | 2 |
| E | 4. Failover, promote and fencing | `2026-11-13-cluster-v1-e-promote.md` | 14 | 2 |
| F | 5. Phone push | `2026-11-13-cluster-v1-f-push.md` | 7 | 1 |

65 tasks in all (62 before the 2026-11-14 revision: D and E gained one and two). The spec's "Must exist first" row is covered by: the resume proof (M5 below, then `RESUME_PROOF_RECORDED`, D Task 9); `claude --bg` and attach on Linux (M4, then `LINUX_CLAUDE_PROVEN`, B Tasks 3 and 13); node tier and headless switch policy (B Tasks 4 and 11); the `notchling.db` split (B Task 8, D Task 1); the version handshake (B Tasks 3, 6, 10); the deterministic simulator (B Task 7, before any failover code in E).

## Words

- **Primary**: the machine whose term is the highest, which runs the main sessions, journals the cluster and is where the phone's commands go. In code it is `home` (`ClusterState.home`); the UI never says "home".
- **Can Be Master**: a per-machine flag, on by default for Macs and off for Workers, that lets a machine receive snapshots, relay for the phone and be promoted. It needs disk encryption (D Task 12).
- **Standby**: a machine with Can Be Master on that isn't the primary. **Worker**: any other member. Roles are derived, never stored (B Task 5's `role_of`).
- **Vault node**: the Mac the cluster was created on (m5), whose Keychain holds the vault. It never changes in v1.
- **Failover target**: with Automatic Failover on, the one Standby that may promote by itself (the first ready one by join order: m1).

## Decisions 2026-11-14

The maintainer confirmed proposals 1 to 9 of the discussion brief as written. What changed in the plans:

1. **Topology.** m5 is primary, m1 its Standby; any number of machines may have Can Be Master (on for both Macs, off for Workers; disk encryption required and checked, D Task 12). Snapshots go to every ready Can Be Master machine, sent at each turn end (D Tasks 3 and 6).
2. **No short lease.** The 15-second lease is gone. The setting `clusterFailover` (Off by default, 30 minutes, 1 hour; class Ask, decision 0020) decides whether the failover target takes over by itself. With it on, the primary pauses the tool calls of the sessions that would move 2 minutes before the deadline when it can't reach the target (directly or through another Can Be Master machine), and the target promotes at the deadline (E Tasks 1 to 3). Terms fence everything: Workers and Standbys refuse an older term, a machine that sees a newer one adopts it, a primary that sees one steps down (B Tasks 4 to 7).
3. **Manual promote.** Move to `<machine>` Now in the app's cluster view and on the phone. The candidate first checks whether it reaches the primary and relays instead if it does. With no primary reachable, the phone shows a Machines popup: online machines, their snapshot time, "can resume old sessions" or "controls jobs only" (E Tasks 5, 12, 13).
4. **Phone continuity.** The phone knows every Can Be Master machine and falls back to one it reaches; that machine relays to the primary or holds the commands until a promote or the primary's return. Paired phones are copied in the snapshots (pairing zone: maintainer) (D Tasks 3, 8, 10, 11).
5. **The old primary returns.** It steps down and shows "`<new>` is primary" with Take Back and Keep `<new>`. Take Back is the synced handover (catch up, wait for turn ends or Move Now, final delta, term + 1, resume) (D Task 9, E Tasks 6, 10). Sessions with unpushed code stay where they are as worker-hosted sessions. Work it did after its last snapshot is kept as a side copy, never deleted or merged.
6. **The old primary returns while the newer one is off.** It learns the newer term from Workers; sessions the newer primary never touched continue; touched ones offer Wait for `<machine>` (the default) or Continue From This Copy, and the other copy is kept when it returns (E Task 6).
7. **Uncommitted code, rule (a).** Only sessions whose working tree is clean and pushed move; code never travels in a snapshot, the new machine takes it from the remote (D Tasks 4 and 8). The shadow repository is gone.
8. **Secrets.** A per-machine allowlist, empty by default, and a per-machine Any Secret switch, off by default. The vault stays on m5; while another machine is primary no new secret is issued (a dispatch with one waits) (C Tasks 1, 4, 9).
9. **Push.** The primary sends; while it is silent, any reachable Can Be Master machine sends, with the item id as the shared collapse id (F Tasks 2 and 4).
10. **Version sync conflicts.** C1: dispatch on a tool-version mismatch warns, never refuses, until version sync Phase 6 (B Task 3, test `dispatch_warns_on_a_tool_mismatch_until_phase_6`). C2: B's Mac probe calls `versions::mac::detect` (B Task 10). C3: both `Tool` enums stay, with `tool_of` (B Task 3).
11. **Decision numbers** are placeholders (`XXXX-…`), assigned at merge in merge order: 0019 and 0020 are on unmerged branches, 0021 is search and 0022 localization.

## Cross-plan notes

- **Base.** `origin/agent-run-oss` (79f4b62 when written). One branch per plan, `agent/cluster-v1-<letter>-<name>`; a plan's later tasks may need a rebase once `merge/settings-batch` lands, since both touch `crates/local-api/src/wire.rs` and `crates/notchlingd/src/daemon_wiring.rs`.
- **Decision records.** `XXXX-cluster-v1-nodes` (B1), `XXXX-cluster-secrets-and-logins` (C11), `XXXX-cluster-standby-and-handover` (D14), `XXXX-cluster-failover-and-fencing` (E14), `XXXX-phone-push` (F1). The PR that merges one renames the file and every reference to the next free number.
- **Settings classes (decision 0020).** One key: `clusterFailover` (E Task 2), class Ask: an agent may only ask, the maintainer allows in the inbox. E Task 2 needs 0020's key classes (`agent/agent-settings-cli`). Every other piece of cluster configuration (members, Can Be Master, allowlists, Any Secret, main sessions) is journaled in `cluster.db` through app-tier ops, and no switch is ever reachable through `settings.set` (B's headless switches go through `node.switch.set`).
- **Version sync (`origin/agent/version-sync-plans`, `2026-11-14-versions-overview.md`).** B Task 3 uses its Phase 1 Tasks 1 and 4 (`versions::Version`, `cli_output`, `mismatch`), building them first under its names if they haven't merged; B Task 10 uses Phase 1 Tasks 2 and 3 (`versions::mac::detect`). Conflicts settled as that overview recommends: C1, warn until Phase 6 (B's test has the name version sync's P1 Task 10 planned, so P1 T10 shrinks to `delegate --machine`'s stderr line and no test is weakened; P6 Task 5 turns it into a refusal and adds Run Anyway); C2, one probe, `detect`, with B keeping only the node-level fields; C3, both enums and `tool_of` (B Task 3). Its Phase 5 (rollout to Workers) needs B's node link and node tier, its Phase 6 `NodeHost` is B's `ClusterNode::local(Forward)`, and its fleet simulation should reuse B Task 7's `World`.
- **Logging (`agent/logging-design`).** Its envelope `host` becomes B Task 9's node id once a node certificate exists (its `host.identity` event records the change; its Phase 7 "cluster host ids" consumes it). C Task 4's release is its first commit-before-effect event; until the event log lands, the audit line is written first and a failed write stops delivery. Events to add when it lands: `cluster.join`, `cluster.dispatch`, `cluster.refused`, `cluster.term`, `grant.release`, `snap.set`, `move.step`, `held.add`, `held.flush`, `promote.step`, `failover.set`, `claim`, `fence.deny`, `push.sent`.
- **Invariants by plan** (the spec's I1 to I7, checked in `engine::cluster::sim` after every step): B: I1 (one home per term, terms only grow, a stale term never accepted: `StaleTermAccepted`), I2 (one start per job), no dispatch to a removed node, one job per key. C: I3 (nothing renews; no refresh on a Worker), I5 across nodes, I6 with planted marker values, no release while the vault node isn't primary (`ReleasedOffPrimary`). D: I4 (a Standby never ahead, never torn), no restore of a session that may not move (`MovedUnpushed`). E: I1 with promotes, Take Back and returns; I2' (the fence passes on one node at a time per moving session, except windows the maintainer confirmed, which are reported); I7 (a cut step never rerun); no automatic promote before the primary's gates ran out; side copies never deleted; one owner per session after a heal.
- **The node tier** is one table, `engine::cluster::policy::RULES`, appended by each plan: B (sessions, replies, prompts, tails, dispatch, membership, Can Be Master, switches, workspace, blobs; the term check before every rule), C (grants only from the vault node while primary, vault relay to the vault node, inbox relay to the primary and Can Be Master machines, allowlist), D (snapshot transfer to every Can Be Master machine, move steps, main sessions, the phone's relayed ops and held commands), E (lease asks and grants with one forward, claims, promote, install from the primary), F (none: push goes to Google, not between nodes). A Worker never starts work on another node; a client relays its own app's tier only.
- **`notchling.db` today** (for B Task 8's class table): tables `meta`, `settings`, `drafts`, `docs`, `inbox`; docs `daemon/limits`, `daemon/cost-cache`, `accounts/codex`, `accounts/antigravity`, `accounts/ids`, `app/second-opinions`, `daemon/accounts`, `accounts/antigravity-usage`, `machines/pending-updates`, `machines/registry`, `daemon/vault`. All Node, except `inbox` (Cluster; it moves in D Task 1). `cluster.db` gains `held` (D10), `push_devices` (F2) and the claims and side copies in the journal (E6); every snapshot set carries it.

## Order and dependencies

```
A (merged)
 └─ B1 decision ── any time
 └─ B2 platform ──> B9 cluster-net ─────────────┐
 └─ B3 > B4 > B5 > B6 > B7 sim > B8 cluster.db ─┴─> B10 > B11 > B12 > B13 > B14 ─> B19
                  └─> B15 Machines (after B5)          │      │      │
                                                       │      │      └─> B17 Run on (app)
                                                       └─> B16 cluster view (app)
                                                              └─> B18 phone
 C1 > C2 > C3 (after B8) ; C4 > C5 > C6 > C7 > C8 (after B13) ; C9 (after B16) ; C10 (after B7, C4) ; C11
 D1, D2 (after B8) ; D3, D4, D5 (any time after B5) ; D6 (after B14) ; D7 (after B7, D6) ; D8 (after D4)
   ; D9 (after D6, D7, D8) ; D10 (after B18, D9) ; D11 (after D10) ; D12 (after B15)
   ; D13 (after B16, D9, D12) ; D14
 E1 (after B6) ; E2 (after E1 and decision 0020's branch) ; E3, E4 (any time after B5)
   ; E5 (after E1, D8, D10) ; E6 (after D9) ; E7 (after B7, D7, E1, E3, E5, E6) ; E8 (after E7)
   ; E9 (after E3) ; E10 (after E8, C6) ; E11 (after E9) ; E12 (after E10, D11) ; E13 (after E10, D13)
   ; E14, which ships only after F1 is accepted
 F1 any time ; F2 > F3 > F4 > F5 (after C6, C7, D3) ; F6 (after F5, D11) ; F7
```

## Parallel lanes (no shared files)

Run these at once; each lane is one subagent at a time, with its own `CARGO_TARGET_DIR`.

1. **Core lane:** B3 to B8 (`crates/engine/src/cluster/`, `store.rs`, `store_class.rs`), then E1, E6 and E7 (they change `state.rs`, `wire.rs`, `core/` and `sim/`).
2. **Transport lane:** B2 then B9 (`crates/platform/`, the new `crates/cluster-net/`).
3. **Docs lane:** B1, F1 (decision records only).
4. **Pure lane, once B5 is in:** E3, E4 (new files `fence.rs`, `outward.rs`), E5 (`promote.rs`, once D8 and D10 are in), D3 (new `snapshot/`), F2's `push.rs` without its `persist.rs` change.
5. **Git and SQLite lane:** D4 (repository state, `GIT_OPTIONAL_LOCKS=0`) and D5 (`snap_files.rs`): real git and SQLite in temp folders, no shared files.
6. **Machines lane, once B5 is in:** B15, then D12 and C8's Machines half (`crates/engine/src/machines/`).
7. **Daemon lane:** B10 to B14, then C4 to C8, D6, D9, E8 to E11, F4: one at a time, since they all touch `wire.rs`, `daemon_wiring.rs` and `policy.rs`.
8. **App lane, once the op shapes are fixed:** B16, B17, C9, D13, E13, and E2's Settings row (`crates/app/`, `crates/kit/`).
9. **Phone lane:** B18, D10, D11, E12, F5, F6 (protocol changes one at a time: the schema and its generated files are shared). D10 also changes the bridge in `crates/engine/src/bridge/` and `notchlingd`, so it holds the daemon lane's files while it runs.
10. **Settings:** E2 alone, after decision 0020's branch merges (`crates/engine/src/settings/keys.rs` is shared with that branch).

Files that must be changed by one task at a time across lanes: `crates/local-api/src/wire.rs`, `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/lib.rs`, `crates/notchlingd/src/bridge_backend.rs`, `crates/engine/src/cluster/mod.rs`, `wire.rs`, `policy.rs`, `state.rs`, `persist.rs` (the `held` and `push_devices` tables), `core/*`, `sim/*`, `crates/engine/src/inbox/model.rs` (`Kind`, `ActionId`), `crates/engine/src/settings/keys.rs`, `protocol/agentconsole.v1.schema.json` and its generated files, `docs/feature-map.md`, `crates/AGENTS.md`, `CHANGELOG.md`.

## What needs the maintainer's machines or accounts

No agent runs any of these. Each gates only the live check named; the code, tests and merges go ahead without them.

| # | What | Exact commands or steps | Gates |
| --- | --- | --- | --- |
| M1 | Tailscale on every node | Mac: the Tailscale app, signed in. VPS: `curl -fsSL https://tailscale.com/install.sh \| sh`, then `sudo tailscale up`. Check on each: `tailscale status`, `tailscale ip -4` (a 100.x address). If the tailnet has a custom ACL, allow `tcp:47823` between your devices. | B Checkpoint 3 |
| M2 | A Linux `notchling` | On the VPS: install Rust 1.99.1 with rustup, clone the branch, `cargo build --release --locked -p notchling-notchlingd`; copy `target/release/notchling` to the Mac at `~/Library/Application Support/Notchling/cluster/dist/notchling-linux-x86_64` (B Task 15 copies it from there). Version sync's signed release assets replace this later. | B Task 15 live |
| M3 | The node's user and linger | On the VPS: the user Notchling runs as (your own, or `sudo adduser --disabled-password notchling`), then `sudo loginctl enable-linger <user>`. | B Task 15's NODE_SERVICE |
| M4 | Claude Code on Linux, proven | On the VPS as that user: `curl -fsSL https://claude.ai/install.sh \| bash`; on the Mac `claude setup-token`; on the VPS `export CLAUDE_CODE_OAUTH_TOKEN=<token>` in that shell only; in a test folder `claude --bg "say hi"`, attach to it, `claude stop <id>`. Record the version and what each step printed (no token) in `docs/cluster-proofs.md`; then a PR flips `LINUX_CLAUDE_PROVEN`. | Claude Code dispatched to Linux Workers (B Tasks 3, 13) |
| M5 | Cross-host resume, proven | Same username and home path on m5 and m1. m5: in `~/proof/repo` (pushed), `claude --bg`, two turns, `claude stop <id>`. Clone the repository on m1 at the same path from its remote, copy `~/.claude/projects/<slug>/<id>.jsonl` with its `<id>/` folder to the same path (`rsync -a` over Tailscale). m1: `claude --bg --resume <id>`, one more turn that needs the earlier context. Record in `docs/cluster-proofs.md`, including which files were needed; a PR flips `RESUME_PROOF_RECORDED` and corrects D Task 3's `ResumeSet` table if needed. | D Task 9 live, all of E live |
| M6 | Optional: token without the environment | Check whether Claude Code reads its long-lived token from a file or descriptor (`claude --help`, its docs). If it does, C Task 7 switches from the environment to that. | Nothing; improves C Task 7 |
| M7 | Worker logins | Mac: `claude setup-token`, save it in Notchling's vault as `claude-worker` (Ask me first off). VPS: `codex login --device-auth`; Antigravity's own sign-in there. | C Checkpoint 2 |
| M8 | The fence hook installed | After E merges: on m5 and m1 (every Can Be Master Mac), Notchling's Install Hook (Welcome); on a headless Standby, the cluster view's Install Hook for that node. | E Checkpoint 2 |
| M9 | Firebase | A Firebase project; iOS and Android apps with the phone's bundle ids; the APNs key (.p8) uploaded; `GoogleService-Info.plist` and `google-services.json` put in `apps/flutter/ios/Runner/` and `apps/flutter/android/app/` (git-ignored); a service account allowed to send FCM; its JSON saved in the vault as `fcm-service-account` and granted to every Can Be Master machine; the phone built with `--dart-define=NOTCHLING_PUSH=1`. Steps in `docs/push-setup.md` (F Task 7). | F Checkpoint |
| M10 | Decisions and merges | Accept the five decision records (numbers given at merge; the push record before E ships); review and merge each plan's PR into `agent-run-oss`. | Each plan |
| M11 | m1 as a Standby | Same username and home path as m5 (`id -un`, `echo $HOME` on both), FileVault on, never sleeps (System Settings > Energy), Notchling installed (M13), git access to every remote the main sessions push to (the same ssh keys or credential helper as m5, checked with `git ls-remote <origin>` per repository), and Notchling's phone bridge turned on in the app (for the phone's fallback and for pushes). | D, E, F live |
| M12 | The phone reaches every Can Be Master machine | The Tailscale app on the phone in the same tailnet, or a tunnel to each machine's bridge (m1's included). | D Tasks 10 and 11 live, E phone promote, F Standby push |
| M13 | Installs | `tools/bundle-next.sh --install` on each Mac after a merge. | Every live check |
| M14 | Disk encryption on every Can Be Master machine | FileVault on Macs; LUKS on a Linux machine that should be Can Be Master (a plain VPS usually can't, so leave it a Worker). | D Task 12 passes |
| M15 | Automatic Failover | After E merges: Settings > Cluster > Automatic Failover, Off (default), After 30 Minutes or After 1 Hour. Choose with the cost in mind: with it on, m5's sessions whose code is pushed pause 2 minutes before the deadline while m5 can't reach m1 (a flight longer than the delay). | E Checkpoint 2, second run |

## Calls made where the design left room

1. **Roles in v1:** Primary, Standby, Worker, derived from the term and the Can Be Master flag. No Witness, no election, no majority. The primary is where the cluster was created, until a move (D), a promote (E) or a Take Back.
2. **Node identity:** a new v3 ECDSA node certificate per data folder, not the phone bridge's: the Swift bridge certificate is X.509 v1, which rustls' verifier refuses. The node id follows decision 0012's host id rule; the logging design's `host` should use it.
3. **Transport:** TCP and TLS 1.3 on 47823, bound to Tailscale addresses only, both sides pinned. No QUIC and no separate replication crate in v1 (the daemon is thread-based; quinn would bring an async runtime).
4. **Topology:** control (membership, dispatch, terms) through the primary; data (tails, replies, prompts) straight to the session's node. Clients and Can Be Master machines link to every member, so a Standby can relay, take snapshots and learn terms; Workers link to the primary and are dialed by the rest.
5. **Node tier:** a closed table of ops per sender role and origin tier; a client relays its own app's tier, a Worker the agent tier at most.
6. **Headless switches:** `acting`, `bridge` and `install` follow the primary's stored, app-tier choice on nodes with no app; `accounts` and `vault` never come from the cluster. Same accepted-risk class as decision 0011's note.
7. **Join tokens** can be minted at the agent tier (Machines is a separate binary, so agent tier), because every join still needs Add in the inbox.
8. **Secrets:** a per-machine allowlist, empty by default, and Any Secret off by default (confirmed 2026-11-14). Only the vault node issues, only while it is primary; standing grants already delivered keep working until they expire. A dispatch that attaches a secret while another machine is primary waits ("Waiting for m5"); a request from inside a running job fails at once (a job can't wait that long).
9. **Token renewal:** v1 renews nothing (the v1 column); option (a), Notchling as the one refresher, is the later column and not planned here.
10. **Where values meet an environment:** only the command `notchling secret` runs (today's vault rule) and `CLAUDE_CODE_OAUTH_TOKEN` on the `claude` a Worker's daemon launches (Claude Code takes it no other way that we know; M6 checks). Delivery itself is a file, never argv or env.
11. **Main sessions:** every Claude Code and Codex session on the primary, on by default (the maintainer said running sessions continue on the Standby), unless turned off with Keep on `<machine>`; sessions the phone starts are main too. Antigravity isn't main in v1. Codex sessions started in its own terminal UI, outside Notchling, can't be fenced and aren't main.
12. **Turn ends** come from the session monitor's working-to-idle change, not a new `Stop` hook slot: no extra hook install, and the monitor sees it within about a second. A `Stop` slot can be added like E's fence slot if that proves loose.
13. **Snapshots carry no code.** A set records each main session's repository facts and whether it may move (rule a); the machine that takes a session clones or fast-forwards from its remote. A folder that isn't a git repository, has no remote, or has any change or unpushed commit never moves. The user's repository is only read, with `GIT_OPTIONAL_LOCKS=0`.
14. **Encryption of sets:** Notchling doesn't encrypt them in v1; every Can Be Master machine must have disk encryption, which Machines checks (D Task 12) from the machine's own report (`fdesetup status`, `lsblk`): a machine that fails isn't ready and receives nothing. A machine could lie about it, the same accepted risk as other self-reported facts.
15. **Failover timing.** One failover target, the first ready Can Be Master machine by join order. The primary asks only it for a lease every 30 s, directly or through one other Can Be Master machine; the lease lasts the chosen delay from when it was asked, on the primary's boot clock (sleep counts; clocks need not agree). The primary's moving sessions pause `FENCE_LEAD_MS` (2 minutes) before the lease ends; the target promotes only after its own last grant plus the delay. With failover Off there is no lease: nothing pauses and nothing promotes by itself. A change of the setting takes effect through the target's next grant, so shortening or turning it off never opens an unsafe window.
16. **Terms are `(n, node id)`,** so two promotes at once never share a term; the higher wins and the other steps down.
17. **Manual promote.** The candidate asks the primary directly and through each other Can Be Master machine (10 s); any answer means relay, and nothing moves. Otherwise it promotes; when safety can't be shown (failover Off, or the target's deadline not reached), the maintainer must confirm text that says the old primary may still be running and its later work will be kept as a copy (Move Anyway). With failover Off every manual promote with an unreachable primary asks for that confirm.
18. **The fence hook** is a global `PreToolUse` `*` slot running `notchling hook --fence`, which reads one file and never opens the socket on the pass path. Each session has its own gate: `Lease` (would move), `Free` (won't move: never pauses), `Moved`, `Waiting`. An agent can rewrite the fence file: accepted, as with every file (0007).
19. **Resumed main sessions** are never sent a message; a cut outward step becomes an item with Continue (one reply) or Done.
20. **Push:** FCM HTTP v1, ids-only payloads with fixed text, sent by the primary, and by any reachable Can Be Master machine while the primary is silent to it; the item id is the collapse id on every machine, so the phone shows one alert. Needs the bridge switch on. The FCM key reaches every Can Be Master machine as a standing grant from the vault node, since a headless node never turns its vault switch on.
21. **Phone promote** confirms in a dialog; no biometric check (no plugin in the app today).
22. **Shell jobs** get a Chat tool adapter of their own (decision 0015), with Stop and no reply box.
23. **One inbox:** every node's items are mirrored on the primary and every Can Be Master machine, qualified by node, and an answer is forwarded to the owner, which keeps first-answer-wins local. The inbox table moves into `cluster.db` so ids survive a move.
24. **Code flags** `LINUX_CLAUDE_PROVEN` and `RESUME_PROOF_RECORDED` (both `false`) hold back live behavior until M4 and M5; tests pass the flag in.
25. **Windows** joins as a Worker only, for shell jobs, Codex and Antigravity; Claude Code dispatch to Windows is refused in v1.
26. **Claims.** Each moving session has a claim (machine, a counter that grows with every change of machine, term, the set it resumed from). When two copies meet, the higher claim keeps the session; the other's extra work becomes a side copy. Every member keeps the winning claims, which is how an old primary learns from a Worker what the newer primary touched. "Touched" means the newer primary let at least one tool call through in that session, recorded before the call.
27. **Side copies** stay where they are; one is moved under `<data>/cluster/copies/<session>/<set>/` only when a restore (Take Back, Continue From This Copy) needs its path. Notchling never deletes or merges one; Hide only hides it from the list.
28. **Move to `<machine>` Now** lives in the app's cluster view and on the phone, not in the Machines window: Machines is a separate binary at the agent tier, and a promote is app tier.
29. **Held commands** stay on the machine the phone reached, in order, visible on the phone; they go to the primary when it answers again, or run on this machine after it is promoted. A held answer to a question that closed meanwhile is dropped and the phone is told.
30. **Paired phones** are copied in every set and merged on restore, never removed (pairing zone).

## Open for the maintainer

Raised by the revision, not answered in the plans:

- With failover On and more than two Can Be Master machines, only the first by join order takes over by itself; the others are manual only. The decision says the primary pauses when it can't reach any Can Be Master peer; the plans pause when the target's lease can't be renewed, directly or through one other Can Be Master machine. With m5 and m1 alone the two are the same; with a third Can Be Master machine that reaches m5 but not m1, the plans pause where the decision's words wouldn't (m1 can't hear m5 then, so it would take over: pausing is the safe side).
- A primary with failover On never pauses while its target hasn't granted it a lease yet at the current term (a new target, a fresh promote): automatic promote is then off too, so it's safe, but the setting silently does nothing until the first grant. The plans show no line for whether the target is armed; whether the app should say so is open.
- "Touched" is recorded best effort: if the newer primary went off right after a tool call and before any Worker heard the record, the returning primary sees the session as untouched and continues it, and the two copies meet later as a claim conflict (the side copy keeps the other's work).
- Held commands have no expiry; a reply held for a day is still delivered when the primary returns. A time limit, or asking again on delivery, is the maintainer's call.
- Keep m1 after a failover leaves the vault on m5, which isn't primary: no new secret is issued until Take Back. Moving the vault is a later decision.
- A request for a secret from inside a running job fails at once while another machine is primary, rather than waiting.
- Main sessions on by default means every Claude Code and Codex session on m5 is snapshotted at each turn end; sets carry transcripts only, but a busy day sends many small sets.
- Sessions in folders that aren't git repositories, or in a linked worktree whose main repository isn't pushed, never move.
- m1 needs push and fetch access to every remote m5's sessions use (M11); a session whose remote m1 can't reach fails its restore and stays with m5.
- A phone's "Move to `<machine>` Now" when the primary is in fact reachable only relays; to move the primary on purpose while both are up, the maintainer uses the synced handover in the app (D Task 13).
- Machines without Can Be Master are listed in the phone's popup but can't be picked; a ready Can Be Master machine without a set can be promoted and "controls jobs only".
- Readiness (disk encryption, same user and home path) comes from each machine's own report.
- `FENCE_LEAD_MS` is 2 minutes: long enough for a laptop's clock drift over an hour, short for a turn in progress. A turn that runs past it is paused at its next tool call.
- E Task 2 waits on decision 0020's branch for the Ask class.
