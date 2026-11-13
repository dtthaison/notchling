# Cluster v1: the plans, their order, and what needs the maintainer

Plan A (the inbox, step 0) is merged. These plans build steps 1 to 5 of `docs/superpowers/specs/2026-11-13-notchling-cluster-design.md`, "Scope: v1 first". Every plan is written for one subagent per task (superpowers:subagent-driven-development), on its own branch off `agent-run-oss`, merged by the maintainer only.

| Plan | Step | File | Tasks | Checkpoints |
| --- | --- | --- | --- | --- |
| B | 1. Nodes: join over Tailscale with pinned mTLS, node tier, versions, cluster view, work on a picked Worker, seen in Chat and on the phone | `2026-11-13-cluster-v1-b-nodes.md` | 19 | 3 (the third is live) |
| C | 2. Per-job secret grants and Worker logins | `2026-11-13-cluster-v1-c-secrets.md` | 11 | 2 |
| D | 3. Standby snapshots and the planned move of the home | `2026-11-13-cluster-v1-d-standby.md` | 13 | 2 |
| E | 4. Promote and fencing | `2026-11-13-cluster-v1-e-promote.md` | 12 | 2 |
| F | 5. Phone push | `2026-11-13-cluster-v1-f-push.md` | 7 | 1 |

62 tasks in all. The spec's "Must exist first" row is covered by: the resume proof (M5 below, then `RESUME_PROOF_RECORDED`, D Task 9); `claude --bg` and attach on Linux (M4, then `LINUX_CLAUDE_PROVEN`, B Tasks 3 and 13); node tier and headless switch policy (B Tasks 4 and 11); the `notchling.db` split (B Task 8, D Task 1); the version handshake (B Tasks 3, 6, 10); the deterministic simulator (B Task 7, before any lease code in E).

## Cross-plan notes

- **Base.** `origin/agent-run-oss` (79f4b62 when written). One branch per plan, `agent/cluster-v1-<letter>-<name>`; a plan's later tasks may need a rebase once `merge/settings-batch` lands, since both touch `crates/local-api/src/wire.rs` and `crates/notchlingd/src/daemon_wiring.rs`.
- **Decision numbers.** 0019 and 0020 are taken on unmerged branches (`agent/agents-setup-decision`, `agent/agent-settings-cli`). The plans use 0021 (nodes), 0022 (secrets), 0023 (Standby), 0024 (promote), 0025 (push); take the next free number at merge.
- **Settings classes (decision 0020).** None of these plans adds a settings key: cluster configuration is journaled in `cluster.db`. A builder who needs one gives it class Never, and no switch is ever reachable through `settings.set` (B's headless switches go through `node.switch.set`, never settings).
- **Version sync (`agent/version-sync-design`, `docs/local/2026-11-13-version-sync-design.md`).** B Task 3 builds the minimum of `engine::versions` (`Version`, CLI output parsing) under that design's names, or uses its Phase 1 if merged first. B's `compat` refusal is that design's §17.5 text ("m1 runs Claude Code 2.1.323, the Mac 2.1.324"), with Run Anyway in the app only and never for an agent. Its Phase 5 (rollout to workers) needs B's node link and node tier, and its Phase 6 `NodeHost` is B's `ClusterNode::local(Forward)`. Its fleet simulation should reuse B Task 7's `World` rather than build a second one. D Task 11's `STANDBY_TOOLS` leaves the update itself to version sync.
- **Logging (`agent/logging-design`).** Its envelope `host` becomes B Task 9's node id once a node certificate exists (its `host.identity` event records the change; its Phase 7 "cluster host ids" consumes it). C Task 4's release is its first commit-before-effect event; until the event log lands, the audit line is written first and a failed write stops delivery. Events to add when it lands: `cluster.join`, `cluster.dispatch`, `cluster.refused`, `grant.release`, `snap.set`, `move.step`, `promote.step`, `fence.deny`, `push.sent`.
- **Invariants by plan** (the spec's I1 to I7, checked in `engine::cluster::sim` after every step): B: I1 as far as terms go, I2 (one start per job), no dispatch to a removed node, one job per key. C: I3 (nothing renews; no refresh on a Worker), I5 across nodes, I6 with planted marker values. D: I4 (a Standby never ahead, never torn). E: I1 with leases, I2' (the fence passes on one node at a time per main session), I7 (a cut step never rerun).
- **The node tier** is one table, `engine::cluster::policy::RULES`, appended by each plan: B (sessions, replies, prompts, tails, dispatch, membership, switches, workspace, blobs), C (grants, vault relay for names and saves, inbox relay, allowlist), D (snapshot transfer, move steps, main sessions), E (leases, fence now, promote, install from the home), F (none: push goes to Google, not between nodes). A Worker never starts work on another node; a client relays its own app's tier only.
- **`notchling.db` today** (for B Task 8's class table): tables `meta`, `settings`, `drafts`, `docs`, `inbox`; docs `daemon/limits`, `daemon/cost-cache`, `accounts/codex`, `accounts/antigravity`, `accounts/ids`, `app/second-opinions`, `daemon/accounts`, `accounts/antigravity-usage`, `machines/pending-updates`, `machines/registry`, `daemon/vault`. All Node, except `inbox` (Cluster; it moves in D Task 1).

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
   ; D9 (after D6, D7, D8) ; D10 (after B18) ; D11 (after B15) ; D12 (after B16, D9) ; D13
 E1 (after B5) ; E2, E3 (any time) ; E4 (after D8) ; E5 (after B7, D7, E1) ; E6 > E7 > E8 > E9 (after D9)
   ; E10, E11 (after E8) ; E12 ; ships only after F1 is accepted
 F1 any time ; F2 > F3 > F4 > F5 (after C6, C7) ; F6 (after F5) ; F7
```

## Parallel lanes (no shared files)

Run these at once; each lane is one subagent at a time, with its own `CARGO_TARGET_DIR`.

1. **Core lane:** B3 to B8 (`crates/engine/src/cluster/`, `store.rs`, `store_class.rs`).
2. **Transport lane:** B2 then B9 (`crates/platform/`, the new `crates/cluster-net/`).
3. **Docs lane:** B1, F1 (decision records only).
4. **Pure lane, once B5 is in:** E2, E3 (new files `fence.rs`, `outward.rs`), D3 (new `snapshot/`), F2's `push.rs` without its `persist.rs` change.
5. **Git and SQLite lane:** D4 (`snapshot/git_snap.rs`, `notchlingd/src/snap_git.rs`) and D5 (`snap_files.rs`): real git and SQLite in temp folders, no shared files.
6. **Machines lane, once B5 is in:** B15, then D11 and C8's Machines half (`crates/engine/src/machines/`).
7. **Daemon lane:** B10 to B14, then C4 to C8, D6, D9, E6 to E9, F4: one at a time, since they all touch `wire.rs`, `daemon_wiring.rs` and `policy.rs`.
8. **App lane, once the op shapes are fixed:** B16, B17, C9, D12, E11 (`crates/app/`, `crates/kit/`).
9. **Phone lane:** B18, D10, E10, F5, F6 (protocol changes one at a time: the schema and its generated files are shared).

Files that must be changed by one task at a time across lanes: `crates/local-api/src/wire.rs`, `crates/notchlingd/src/daemon_wiring.rs`, `crates/notchlingd/src/lib.rs`, `crates/engine/src/cluster/mod.rs`, `wire.rs`, `policy.rs`, `state.rs`, `core/*`, `sim/*`, `crates/engine/src/inbox/model.rs` (`Kind`, `ActionId`), `protocol/agentconsole.v1.schema.json` and its generated files, `docs/feature-map.md`, `crates/AGENTS.md`, `CHANGELOG.md`.

## What needs the maintainer's machines or accounts

No agent runs any of these. Each gates only the live check named; the code, tests and merges go ahead without them.

| # | What | Exact commands or steps | Gates |
| --- | --- | --- | --- |
| M1 | Tailscale on every node | Mac: the Tailscale app, signed in. VPS: `curl -fsSL https://tailscale.com/install.sh \| sh`, then `sudo tailscale up`. Check on each: `tailscale status`, `tailscale ip -4` (a 100.x address). If the tailnet has a custom ACL, allow `tcp:47823` between your devices. | B Checkpoint 3 |
| M2 | A Linux `notchling` | On the VPS: install Rust 1.99.1 with rustup, clone the branch, `cargo build --release --locked -p notchling-notchlingd`; copy `target/release/notchling` to the Mac at `~/Library/Application Support/Notchling/cluster/dist/notchling-linux-x86_64` (B Task 15 copies it from there). Version sync's signed release assets replace this later. | B Task 15 live |
| M3 | The node's user and linger | On the VPS: the user Notchling runs as (your own, or `sudo adduser --disabled-password notchling`), then `sudo loginctl enable-linger <user>`. | B Task 15's NODE_SERVICE |
| M4 | Claude Code on Linux, proven | On the VPS as that user: `curl -fsSL https://claude.ai/install.sh \| bash`; on the Mac `claude setup-token`; on the VPS `export CLAUDE_CODE_OAUTH_TOKEN=<token>` in that shell only; in a test folder `claude --bg "say hi"`, attach to it, `claude stop <id>`. Record the version and what each step printed (no token) in `docs/cluster-proofs.md`; then a PR flips `LINUX_CLAUDE_PROVEN`. | Claude Code dispatched to Linux Workers (B Tasks 3, 13) |
| M5 | Cross-host resume, proven | Same username and home path on the laptop and m1. Laptop: in `~/proof/repo`, `claude --bg`, two turns, `claude stop <id>`. Copy the repo and `~/.claude/projects/<slug>/<id>.jsonl` with its `<id>/` folder to the same paths on m1 (`rsync -a` over Tailscale). m1: `claude --bg --resume <id>`, one more turn that needs the earlier context. Record in `docs/cluster-proofs.md`, including which files were needed; a PR flips `RESUME_PROOF_RECORDED` and corrects D Task 3's `ResumeSet` table if needed. | D Task 9 live, all of E live |
| M6 | Optional: token without the environment | Check whether Claude Code reads its long-lived token from a file or descriptor (`claude --help`, its docs). If it does, C Task 7 switches from the environment to that. | Nothing; improves C Task 7 |
| M7 | Worker logins | Mac: `claude setup-token`, save it in Notchling's vault as `claude-worker` (Ask me first off). VPS: `codex login --device-auth`; Antigravity's own sign-in there. | C Checkpoint 2 |
| M8 | The fence hook installed | After E merges: on the home, Notchling's Install Hook (Welcome); on a headless Standby, the cluster view's Install Hook for that node. | E Checkpoint 2 |
| M9 | Firebase | A Firebase project; iOS and Android apps with the phone's bundle ids; the APNs key (.p8) uploaded; `GoogleService-Info.plist` and `google-services.json` put in `apps/flutter/ios/Runner/` and `apps/flutter/android/app/` (git-ignored); a service account allowed to send FCM; its JSON saved in the vault as `fcm-service-account`; the phone built with `--dart-define=NOTCHLING_PUSH=1`. Steps in `docs/push-setup.md` (F Task 7). | F Checkpoint |
| M10 | Decisions and merges | Accept 0021 to 0025 (0025 before E ships); review and merge each plan's PR into `agent-run-oss`. | Each plan |
| M11 | m1 as a Standby | Same username and home path as the laptop (`id -un`, `echo $HOME` on both), FileVault on, never sleeps (System Settings > Energy), Notchling installed (M13). | D, E live |
| M12 | The phone reaches the Standby | The Tailscale app on the phone in the same tailnet, or a tunnel to m1's bridge. | D Task 10 live, E phone promote |
| M13 | Installs | `tools/bundle-next.sh --install` on each Mac after a merge. | Every live check |
| M14 | Disk encryption on Standbys | FileVault on Macs; LUKS on a Linux Standby (a plain VPS usually can't, so v1's Standby is m1). | D Task 11 passes |

## Calls made where the design left room

1. **Roles in v1:** Home, Standby, Worker. No Witness, no election, no majority. The home is where the cluster was created, until a move (D) or a promote (E).
2. **Node identity:** a new v3 ECDSA node certificate per data folder, not the phone bridge's: the Swift bridge certificate is X.509 v1, which rustls' verifier refuses. The node id follows decision 0012's host id rule; the logging design's `host` should use it.
3. **Transport:** TCP and TLS 1.3 on 47823, bound to Tailscale addresses only, both sides pinned. No QUIC and no separate replication crate in v1 (the daemon is thread-based; quinn would bring an async runtime).
4. **Topology:** control (membership, dispatch, terms) through the home; data (tails, replies, prompts) straight to the session's node. A client Mac links to every node; others link to the home.
5. **Node tier:** a closed table of ops per sender role and origin tier; a client relays its own app's tier, a Worker the agent tier at most.
6. **Headless switches:** `acting`, `bridge` and `install` follow the home's stored, app-tier choice on nodes with no app; `accounts` and `vault` never come from the cluster. Same accepted-risk class as decision 0011's note.
7. **Join tokens** can be minted at the agent tier (Machines is a separate binary, so agent tier), because every join still needs Allow in the inbox.
8. **Secrets allowlist:** the v1 table and the brief say "each node has an allowlist"; the maintainer's answers say "any vault entry by name, no allowlist". The plans build a per-node allowlist, empty by default, with an Any Secret switch per node that gives the maintainer's answer on that node. **The maintainer should confirm the default.**
9. **Token renewal:** v1 renews nothing (the v1 column); option (a), Notchling as the one refresher, is the later column and not planned here.
10. **Where values meet an environment:** only the command `notchling secret` runs (today's vault rule) and `CLAUDE_CODE_OAUTH_TOKEN` on the `claude` a Worker's daemon launches (Claude Code takes it no other way that we know; M6 checks). Delivery itself is a file, never argv or env.
11. **Main sessions:** Claude Code and Codex sessions on the home that the maintainer marks main, plus sessions the phone starts there. Antigravity isn't main in v1. Codex sessions started in its own terminal UI, outside Notchling, can't be fenced.
12. **Turn ends** come from the session monitor's working-to-idle change, not a new `Stop` hook slot (the spec says Stop hook): no extra hook install, and the monitor sees it within about a second. A `Stop` slot can be added like E's fence slot if that proves loose.
13. **Snapshots** go through a shadow repository with alternates and a private index, as bundles; a restore leaves every change unstaged, as it was. The user's repository is never written.
14. **Encryption of sets:** Notchling doesn't encrypt them in v1; Standbys must have disk encryption (D Task 11). The spec's data table asks for encryption at rest, which this meets through the disk, not the app.
15. **Fencing needs a time bound in v1.** The spec's v1 row says terms, with the lease later; but a sleeping laptop never hears a new term, and review issues 1 and 2 ask for a suspend-aware clock. So v1 has a lease that every other voter (the Standbys) grants the home, measured on the home's boot clock from when it asked. **Cost the maintainer should know:** a home that can't reach its Standby (a laptop on a plane) pauses its main sessions' tool calls after about 15 seconds; sessions not marked main never pause. Turning Standby off for a session, or having no Standby, avoids it.
16. **Terms are `(n, node id)`,** so two promotes at once never share a term; the higher wins and the other steps down.
17. **Take Over Anyway** is the one override, offered only when safety can't be shown (the Standby isn't a voter at the current term, or another voter can't be reached), and the confirm text says what may happen.
18. **The fence hook** is a global `PreToolUse` `*` slot running `notchling hook --fence`, which reads one file and never opens the socket on the pass path. An agent can rewrite the fence file: accepted, as with every file (0007).
19. **Resumed main sessions** are never sent a message; a cut outward step becomes an item with Continue (one reply) or Done.
20. **Push:** FCM HTTP v1, ids-only payloads with fixed text, sent by the home and by a Standby for "home isn't answering"; needs the bridge switch on. The FCM key reaches Standbys as a standing grant, since a headless node never turns its vault switch on.
21. **Phone promote** confirms in a dialog; no biometric check (no plugin in the app today).
22. **Shell jobs** get a Chat tool adapter of their own (decision 0015), with Stop and no reply box.
23. **One inbox:** every node's items are mirrored on the home, qualified by node, and an answer is forwarded to the owner, which keeps first-answer-wins local. The inbox table moves into `cluster.db` so ids survive a move.
24. **Code flags** `LINUX_CLAUDE_PROVEN` and `RESUME_PROOF_RECORDED` (both `false`) hold back live behavior until M4 and M5; tests pass the flag in.
25. **Windows** joins as a Worker only, for shell jobs, Codex and Antigravity; Claude Code dispatch to Windows is refused in v1.
