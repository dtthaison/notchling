# Notchling cluster: one Notchling across many machines

Status: design, waiting for the maintainer's approval. Date: 2026-11-13.
Supersedes the "Nothing is replicated" rule of [0012](../../decisions/0012-remote-computers.md) (Proposed); 0012's identity, pairing, host-qualified ids, cursors and revocation stay.

## Intent

The maintainer works on a laptop and owns other machines (a Mac mini "m1", Linux VPSes such as "vps-sing-1", possibly Windows PCs). Today Machines only sets those up for the overnight sd worker. The maintainer wants every machine added to Notchling to become part of the one Notchling they use:

- send any piece of work (a Claude Code, Codex or Antigravity session, or a long shell job) to a machine they pick, and keep using its CPU and RAM while the laptop does other things;
- keep the main session (the one that hands work out) going from the phone when the laptop sleeps, dies or is switched off;
- use one Secret Vault and one set of logins everywhere, the way 1Password is one vault on many devices;
- run on macOS, Linux and Windows.

## Goals

1. A cluster of nodes with three roles: one **Primary**, any number of **Workers**, and Workers the maintainer marks as **Standby**.
2. Work placement is **manual**: the maintainer (or a session naming a machine) picks the node. Notchling never moves work to another node on its own, except the Primary's own duties on failover.
3. **Failover:** when the Primary dies (planned or sudden), a Standby becomes the **Acting primary** and the phone continues there. When the Primary returns it catches up, waits for a quiet moment, and takes the role back.
4. **Realtime replication** from the Primary to every Standby: files of the main sessions' worktrees (including `.git`), transcripts and tool state, Notchling data, the vault and logins. Not through git.
5. **Vault like 1Password:** one encrypted vault on the Primary and every Standby, synced both ways, unlocked after a reboot by Face ID on the phone or Touch ID on the Mac. Workers never hold the vault.
6. **One refresher for logins:** only the node holding the Primary role renews OAuth tokens; Workers get access tokens only.
7. All tools: Claude Code, Codex, Antigravity CLI, shell jobs. All three systems.

## Non-goals

- Automatic placement by load. Nodes show CPU, RAM and OS; the choice is the maintainer's.
- Splitting one session over several nodes. A session runs whole on one node at a time.
- Several Primaries acting at once (multi-leader). There is one leader at a time.
- Keeping work going when the Primary and every Standby are off. Workers finish what they hold; the main session waits.

## Anti-goals (failure even if it "works")

- The same session running on two nodes at once (double commits, double pushes, double deploys).
- A sign-out caused by two nodes renewing the same login.
- A secret value written in clear to a disk, a log, argv or an environment variable that outlives its job.
- A cut turn that pushed or deployed being run again without asking.
- A VPS port opened to the internet for the cluster.

## Terms

| Term | Meaning | UI label (vi) |
| --- | --- | --- |
| Node | A machine running `notchlingd` and joined to the cluster | Máy |
| Primary | The node the maintainer chose to coordinate: main sessions, phone bridge, vault, token renewal, handing work out. Usually the laptop. | Máy chính |
| Worker | A node that only runs work it is given | Máy chạy việc |
| Standby | A Worker that also keeps a replica of the Primary's state and may be elected | Dự phòng |
| Acting primary | The Standby elected while the Primary is away | Máy chính tạm |
| Leader | Whichever node holds the Primary role now (the Primary or the Acting primary) | — |
| Witness | A Worker marked to vote in elections without holding a replica | Trọng tài |
| Main session | A session the maintainer runs on the leader to coordinate work | Session chính |

The word "slave" is not used anywhere; "master" is not used either (Primary/Standby, as in PostgreSQL).

## Roles and the leader

- The cluster has exactly one configured Primary, chosen by the maintainer and changeable in settings.
- **Voters** are the Primary, every Standby and every Witness. A node may act as leader only while it holds a **lease** granted by a majority of voters. The lease is short (the builder picks, on the order of 10 to 30 s) and renewed continuously.
- **Self-fencing:** a leader that cannot renew its lease stops acting (stops its main sessions' turns, stops renewing tokens, stops answering the phone as leader) before the lease can expire for anyone else. This is what keeps two leaders from acting at once.
- **Election** when the leader's lease expires: Standbys stand in the maintainer's priority order, but a Standby with an older replica than another reachable Standby may not win. The winner needs a majority of voters. It becomes the Acting primary.
- **Fewer than a majority reachable:** nobody leads. Workers keep running what they hold. The app and phone say the cluster has no leader. The phone offers **Take over manually** on a Standby, with a warning that it may run sessions twice if the old leader is still alive; the maintainer's tap is the only way past the majority rule.
- **Return of the Primary (preferred leader):** it joins as a Standby, catches up from the leader, must stay reachable and stable for a while (default 5 minutes; a laptop that sleeps and wakes must not flip the cluster), then takes the role back by a **planned handoff** at a quiet moment (between turns of every main session). The Acting primary goes back to Standby.
- **Planned handoff** (the maintainer shuts the laptop down, or the Primary returns): the old leader stops new turns, finishes or waits out the turn in flight, pushes its last changes, hands the lease over, and the new leader resumes every main session. Nothing is lost.
- **Recommended shape:** the Primary plus at least two voters among Standbys and Witnesses, with at least one always-on node (a VPS). With only the Primary and one Standby, the Standby can't tell a dead Primary from a cut network; the app says so and suggests a Witness.

## Work placement

- New Session gets a **Run on** column listing nodes (name, OS, online or not, CPU and RAM in use, which tools are installed and signed in). The maintainer picks.
- Delegation names the node: `notchling delegate --machine <node> …` from an agent; an unknown or offline node is an error, never a silent fallback.
- Each running thing belongs to one node. Its id is host-qualified (`<host>/<id>`, from 0012).

| Kind | How it runs on a node | If its node dies |
| --- | --- | --- |
| Claude Code | `claude --bg`; Chat reads the transcript and replies through the node's daemon (attach on that node) | On a Worker: reported failed to the leader. A main session on the leader: the new leader runs `claude --resume` from the replica. |
| Codex | one `codex exec --json` per turn (`resume <id>` after the first) | Next turn resumes from the replicated rollout |
| Antigravity | `agy acp` per open session | Reopened with `session/load` from the replica. Its SQLite files are replicated only at quiet points (no mid-write copy). |
| Shell job | a process whose output streams to Chat | Marked **interrupted**; rerun by a tap, or automatically if the job was marked "safe to rerun" |

## Resuming after a sudden failure

The new leader resumes each main session from the last replicated point. The step that was running when the old leader died has no result:

- a **safe** step (read, search, test, build) is left to the tool to redo;
- a step that **may have acted outward** (`git push`, a deploy, a publish, sending a message, a write API call) is **not** rerun. The session waits with a notice on the phone and in Chat ("The laptop dropped while `git push` was running; check, then Continue"). The classification of outward steps is the builder's, starting from command patterns and tool names.

Lines written in the last moment before a crash may be lost; the design accepts that.

## Replication

- **Direction follows the lease:** only the leader writes the replicated state, so replication is one way, leader to every Standby, and reverses on failover. There is no two-way merge of files.
- **Change detection:** the OS file events (FSEvents, inotify, ReadDirectoryChangesW); send the changed ranges of a changed file, not the whole file. The builder decides between writing our own in Rust (preferred, since it is tied to the lease) and embedding a tool such as Mutagen (MIT).
- **What is replicated:** the worktrees of main sessions including `.git`; Claude Code transcripts and session state, Codex rollouts, Antigravity conversation data; Notchling data (pages library, settings, `notchling.db`); the vault; the login store.
- **Not replicated:** build output and caches (`target/`, `node_modules/`, and an editable ignore list); worktrees of sessions that run on Workers (their code moves with git when the session is sent there, see below).
- **`.git` consistency:** a burst of changes is sent only after git's lock files are gone and the burst is quiet for a short moment, so a replica never holds a half-written index.
- **Paths:** Claude Code keys transcripts by project folder, and Mac and Linux home paths differ. The design requires the same path for a replicated project on every Primary-capable node, or a mapping applied on resume; the builder decides which, and must prove `claude --resume` works on the other node.
- **A cut network:** the old leader may write for a few seconds before it fences itself. On rejoin, writes it made after losing the lease are kept as conflict copies for the maintainer to look at, never merged or written over.

## Sending a session to a Worker

A session started on, or sent to, a Worker gets its code by git: the Worker clones or fetches at the sender's commit, then receives unpushed commits and the ignored files the maintainer picks (`.env` and the like). Work comes back as a branch (or a PR, if the maintainer asks), never merged into the maintainer's tree. Git on the Worker runs without prompts and without hooks from the sender. `.claude/`, `CLAUDE.md` and `.mcp.json` are not copied back.

## Vault

- **One vault, encrypted,** on the Primary and every Standby, synced both ways (a Standby may be the leader and edit). A conflicting edit made while two nodes were apart keeps the newest and puts the other in the item's history.
- **Keys (1Password's layering):** a vault key encrypts the items; an unlock key encrypts the vault key. The unlock key lives in the Mac's Keychain/Secure Enclave behind Touch ID and on the phone behind Face ID. A Standby keeps only ciphertext on disk.
- **Unlock after a reboot:** a Standby that restarts is locked. It asks the cluster; the phone shows "vps-sing-1 asks to unlock the vault"; Face ID approves; the phone sends the unlock key over the paired, encrypted connection; the node keeps it in memory only. The Mac can approve the same way with Touch ID.
- **Workers never hold the vault.** For a job they ask the leader for the named secrets; the leader delivers each into a mode-600 file with an expiry, in a folder no replication or sync carries, over the node connection (never argv or env), and the output that streams back is redacted. Entries marked **Ask me first** go to the phone for approval at each use.
- **Optional "This Mac only" flag** on an entry: never replicated; unavailable while the Mac is off.
- **Recovery code** (like 1Password's Emergency Kit) for when every approving device is lost.
- **Limit stated plainly:** anyone with root on a running, unlocked node can read secrets in use there. The design protects disks, backups and snapshots, not a compromised live node.

## Logins and tokens

- Logins are taken from the leader with the session; a Worker never signs in on its own.
- **One refresher:** only the leader renews OAuth refresh tokens (Claude Code, Codex, Antigravity). It renews early (before expiry) and pushes new access tokens to Standbys and to every Worker using that account. Workers receive **access tokens without refresh tokens**, so they cannot rotate a login and sign anyone out.
- A Worker whose token is rejected asks the leader for a fresh one; it never renews.
- A Worker that needs a login it doesn't have reports "m1 needs a Codex login for job X"; the leader's Chat and the phone show it; the maintainer signs in on the leader (browser), the login goes into the login store and is delivered.
- Claude Code's long-lived token (`claude setup-token`) stays a fallback per account.
- A returning Primary never uses its own stale copy of a login; it takes the current one from the leader before it leads.
- Accounts share their usage limits across nodes; limit watching and auto-switch run on the leader for the whole cluster.
- **To verify before building:** whether a long-running `claude` and `agy acp` pick up a replaced access token mid-run. If not, the leader restarts the process between turns. Codex runs one process per turn and reads `auth.json` each time.

## Phone and desktop app

- The phone pairs with the cluster through any node (0012's pairing, per node certificate) and keeps every Primary-capable node's address. It talks to the **leader**; on failover it finds the new leader and carries on with the same host-qualified ids and cursors.
- The phone bridge (`agentconsole.v1`, LAN or each node's own Cloudflare tunnel) runs on the leader. Other nodes answer only "not leader, try <node>".
- The desktop app on the Primary talks to its local daemon as today. On other nodes (or when the Primary is away) the app connects to the leader like a phone, using 0012's transport.
- Chat shows every node's sessions with a node label, and a cluster view (nodes, roles, leader, lease, replication lag, vault locked or not).

## Node connections and joining

- Nodes talk over mutually authenticated TLS, identity = certificate fingerprint (0012). Routes: LAN, Tailscale, or an ssh tunnel the node opens outward. No inbound port on a VPS beyond ssh.
- **Joining:** the Machines window adds a machine as today (ssh, Checks and Fixes), installs `notchlingd` as a service (LaunchAgent, systemd user unit, Task Scheduler), and the new node is approved from a trusted device (phone or Primary). The maintainer picks its role: Worker, Standby or Witness.
- Removing a node revokes its certificate everywhere and wipes its replica and vault copy on its next contact.

## Platforms

`notchlingd` builds for macOS, Linux and Windows. Known gaps to close or route around:

- the local API's Windows named pipe is a stub;
- `claude --bg` and `claude attach` on Windows are unproven; if they don't work, Windows nodes drive Claude Code another way (the builder finds one) or start as Codex/Antigravity/shell Workers only;
- per-OS secret storage: Keychain on macOS; on Linux and Windows the unlock key is held in memory only, so their Standbys always need the phone or Mac to unlock.

## Conflicts and how each is closed

| Where | Closed by |
| --- | --- |
| Token renewal | One refresher (the leader); Workers hold no refresh token |
| Two leaders | Majority lease + self-fencing; manual takeover only by the maintainer's tap |
| Files during a network split | One-way replication; post-lease writes kept as conflict copies |
| Vault edited on two nodes apart | Newest wins, the other kept in history |
| Git pushes | Only the node running a session pushes its branch |
| Account limits | Shared by design; limit watching runs on the leader |

## Decision record

Write a new record superseding 0012's "Nothing is replicated" paragraph and ARCHITECTURE.md's "Remote computers" row: roles, election and lease, replication scope, vault on Standbys, single refresher. 0012's identity, transport, ids, cursors and revocation stay.

## Suggested order

0. The notifications inbox (works on one Mac too).
1. Node identity, joining and the cluster view (no failover yet); run a session or a job on a picked Worker and see it in Chat and on the phone.
2. Vault delivery to Workers and the single refresher.
3. Standby replication and planned handoff.
4. Election, lease, self-fencing, sudden failover, the returning Primary.
5. Vault replication and unlock from the phone.
6. Windows.

## Left to the builder

Lease and timing values, the election algorithm's details (Raft or a simpler lease protocol, as long as the majority and freshest rules hold), the wire format between nodes, the replication engine (own or Mutagen), the path rule for transcripts, how outward steps are recognised, UI layout of the cluster view and Run on column, and the order of work inside each step.

## Calls made by the design author (after the builder check)

These were open after an independent read; each is the author's call, for the maintainer to check.

- **Main session** means a session the maintainer marks as main (default: none; a session started from the phone on the leader is marked main). Only main sessions are replicated and resumed on failover. Other sessions on the leader behave like sessions on a Worker: when the leader dies they are reported interrupted. A main session's replicated folder is the git repository of its working folder; other repos it touches are not replicated.
- **Identity across failover:** main sessions get a cluster-level host id (from a cluster key made when the cluster is created), not the node certificate's, so their `<host>/<id>` stays the same on any leader. Sessions on a node keep that node's host id. The phone learns which node leads through one additive `agentconsole.v1` field; older phones keep working against whichever node they paired with. Paired-device tokens replicate to Standbys so a phone paired once is accepted by every leader.
- **Replicas are encrypted at rest** on Standbys: transcripts, worktrees, `notchling.db` and the login store are under the vault key, like the vault. Consequence: a Standby that rebooted cannot lead until someone unlocks it (Face ID or Touch ID). The cluster view warns when no unlocked Standby is reachable.
- **OS user on a node:** cluster work runs as a dedicated, non-admin account (`notchling`), beside the sd worker's account, never root and never the maintainer's own login on a shared machine. On the maintainer's own Macs it is the maintainer's user. Its tools' configs and logins live in that account.
- **Moving a live session:** a session can be moved from the leader to a Worker and back with the journalled move (copy, quiet point, stop, final sync, resume on the target), using the same transcript path rule as failover. In step 1 a session is only started on a node; moving is step 3, with Standby replication.

## Maintainer's answers (2026-11-13)

- **Secrets for Workers:** a Worker's job may ask for any vault entry by name; there is no per-node allowlist. An entry marked **Always ask** (the vault's Ask me first) is released only after the maintainer allows it: the request goes to the leader, which shows it in Chat and on the phone, and nothing is delivered until Allow.
- **Token renewal: option (a).** Notchling takes renewal over completely. Every tool on every node, the leader included, gets access-only credentials, and the leader's daemon alone holds and renews refresh tokens (Claude Code, Codex, Antigravity CLI logins and lanes). This replaces today's "never renew a token in use" rule: no tool renews any more, so Notchling's renewal can't race one. Claude Desktop's cookies and the Antigravity app's own login stay outside the cluster for now.
- **Signing (author's call, not yet confirmed):** the design does not assume Developer ID signing. Without it the Mac keeps the vault's unlock key in the Keychain as today, and unlocking a Standby is approved mainly from the phone (Face ID). Secure Enclave behind Touch ID comes when the app is signed.

## Notifications (in-app inbox)

Asked for by the maintainer on 2026-11-13. One place that collects what needs the maintainer, where each item can be acted on directly.

- **Where:** a bell with an unread count in Chat (bottom of the Sidebar, next to Machines and Vault), a badge in the notch, a Notifications tab on the phone, and a macOS notification for items that need an answer (with the in-app banner as the fallback when macOS refuses).
- **Item:** title, one line of detail, where it comes from (node, session or job), time, and its actions. Newest first; answered or expired items move to a "Done" section and are pruned after a while.
- **Actions on the item itself:** the buttons the kind needs, e.g. **Allow / Deny** (vault "Always ask", a permission), **Continue / Cancel** (a paused outward step after failover), **Done** (acknowledge). Clicking the item body opens the detail as a popup or sheet: the vault form, the sign-in sheet, the conflict-copy viewer, the session.
- **One state everywhere:** items live in the leader's daemon (stored, and replicated to Standbys) and are pushed to every client; answering on one surface clears it on all, the way a question card answered in the notch clears in Chat. The first answer wins.
- **Requests wait, then expire:** an item that holds work (a secret request, a paused step) keeps that work waiting until answered, and turns into a refusal after its timeout (the requester gets a clear "not allowed in time").
- **Kinds at first:** secret request (Always ask), login needed on a node, failover happened, paused outward step, no leader / not enough voters, Standby locked (Unlock with Face ID), conflict copies to review, limit switch, a failed action, a job finished (optional, per kind on/off).
- **Order:** the inbox is useful before the cluster exists (vault Ask First, errors, limit switches today), so it can be built first, as step 0.

## Software architecture

### Principles

1. **Functional core, imperative shell**, as the repo already does: every cluster decision (who leads, who owns a session, what to replicate, which notification to raise, what a job's next state is) is a pure function in the engine, tested on every system; the daemon only performs effects.
2. **Control plane apart from data plane.** Small, strongly consistent metadata (members, roles, the leader, ownership, jobs, notifications, vault item versions) goes through consensus. Bulk bytes (worktrees, transcripts, databases) go through a separate replication stream and never through the consensus log.
3. **One owner per resource, now per cluster** (0011 generalised): every session, job, login refresh and replica stream has exactly one owning node at a time, named by an **epoch** (the leader's term). Any effect made under an old epoch is refused.
4. **Ports and adapters:** the cluster core sees the world through traits (`Transport`, `Clock`, `Store`, `Blobs`, `ProcessRunner`, `SecretBox`, `Notifier`). Production adapters are per OS; tests use deterministic fakes. Nothing in a test opens a socket, starts ssh or touches Keychain (the repo's rule).
5. **Additive, versioned wire.** Node-to-node messages are versioned like `agentconsole.v1`: additive only, with a feature list exchanged at connect, so nodes on different builds keep working during an upgrade.

### Layers

```
┌───────────────────────── clients ─────────────────────────┐
│ GPUI app (Chat, notch)   phone (agentconsole.v1)   CLI    │
└───────────────┬──────────────────────────────────┬────────┘
                │ local API (tiers, 0007)          │ bridge (any node; non-leader redirects)
┌───────────────▼──────────────────────────────────▼────────┐
│ notchlingd on every node                                  │
│  Gateway ─ Cluster service ─ Executor ─ Broker ─ Inbox    │
│     │            │              │         │        │      │
│     │      consensus log   tool providers  vault    outbox│
│     │      (control plane)  (0016)       + tokens         │
│     └──── Node link (mTLS, QUIC or ssh tunnel) ───────────┤
│           Replicator (data plane, leader → Standbys)      │
└───────────────────────────────────────────────────────────┘
```

| Component | Job | Where it lives |
| --- | --- | --- |
| **Membership** | Nodes, certificates, roles (Primary/Standby/Worker/Witness), join and removal | `engine::cluster::members` (pure) |
| **Consensus and lease** | A replicated log of control-plane commands; leader election with priority and freshness; the lease and its epoch | `engine::cluster::consensus` over a library such as `openraft` (MIT/Apache-2.0, fits `deny.toml`); the builder may choose a smaller lease protocol if the rules hold |
| **Control state machine** | Applies commands to the cluster state: placement, session ownership, job states, notifications, vault versions, login versions | `engine::cluster::state` (pure) |
| **Executor** | Runs a job on this node through the tool's provider (0016) and reports its state | `notchlingd`, using `engine::providers` |
| **Replicator** | Watches the leader's replicated folders, sends content-addressed chunks to Standbys, applies them atomically there | new `crates/replica` (data plane), adapters per OS for file events |
| **Broker** | The vault (key hierarchy, unlock, per-job delivery) and the single OAuth refresher | `engine::vault`, `engine::accounts` + `notchlingd` |
| **Inbox** | Actionable notifications with one state everywhere | `engine::cluster::inbox` (pure) + an op group in `notchlingd` |
| **Gateway** | Bridge and node API on every node; a non-leader answers with a redirect to the leader | `engine::bridge` + `notchlingd` |
| **Node link** | Mutually authenticated TLS between nodes; LAN, Tailscale, or an outbound ssh tunnel | new `crates/cluster-net` (`rustls`, `quinn`) |

### Design patterns, and where each applies

| Pattern | Applied to | Why |
| --- | --- | --- |
| **Replicated state machine** | The control plane | Every node agrees on who owns what, in the same order |
| **Leader lease + fencing token** | Leader duties, every side effect | A node acting under an old epoch is refused by the others (job start, token refresh, replica write, bridge answers), so split brain can't produce effects |
| **Leadership transfer (preferred leader)** | The Primary's return | Failback at a quiet point, not by a new election race |
| **Event sourcing with projections** | The control log | The audit trail comes for free; the app reads projections (sessions, jobs, inbox) instead of polling nodes |
| **Saga / process manager with a journal** | Session move, planned handoff, failover resume, joining a node | Each step is journalled; before the switch point every step has a compensation, after it the saga only goes forward (OpenMausBot's move) |
| **Transactional outbox** | Notifications, job events, replies to the phone | An event is stored with the state change and delivered at least once; receivers dedupe |
| **Idempotency keys** | Notification answers, job dispatch, replies (steered / refused / indeterminate) | A retry over a flaky link never acts twice; "indeterminate" is never resent |
| **Strategy (provider registry)** | Per tool (Claude Code, Codex, Antigravity, shell) and per OS | Already the repo's shape (0016, `ProviderRegistry`); the cluster adds no `match tool` |
| **Capability tokens** | Node-to-node calls | A short-lived, scoped grant per call kind (run job, deliver secret, replicate) replaces the local socket's tiers for remote callers; least privilege for a compromised node |
| **Envelope encryption / key hierarchy** | Vault and replicas | Unlock key → vault key → item and replica keys; rotating one level doesn't re-encrypt everything |
| **Lease (resource leases with TTL)** | Secrets and access tokens on Workers, the sessions' ownership | Everything handed out expires on its own |
| **Circuit breaker + backoff, bulkheads** | Node links; per-node job slots | A dead node doesn't stall the leader; one node's flood doesn't starve others |
| **Content-addressed chunking** | Replicator | Only changed chunks travel (BLAKE3 hashes); a replica can be verified and resumed |
| **Deterministic simulation testing** | Consensus, failover, handoff, replication | Run thousands of seeded scenarios with a fake clock, fake network partitions and node crashes, checking invariants after each (no two owners per epoch, no lost acknowledged answer, no effect under a stale epoch), the way OpenMausBot checks its runtime invariants |

### Invariants (checked after every simulated step and asserted in production logs)

- I1. At most one node holds the lease for an epoch, and epochs only grow.
- I2. A session or job has one owner per epoch; no effect is accepted from a non-owner.
- I3. Only the lease holder renews refresh tokens; no refresh token exists on a Worker.
- I4. A Standby's replica never runs ahead of the leader's acknowledged state.
- I5. A notification is answered at most once; the first answer wins and is the one applied.
- I6. A secret value never reaches a log, argv, env or replicated folder in clear.
- I7. An outward step cut by a failover is never re-run without an answer.

### Data ownership

| Data | Owner | Replicated to | Encrypted at rest |
| --- | --- | --- | --- |
| Control log (members, ownership, jobs, inbox, versions) | Consensus | All voters | Yes, on non-Mac nodes |
| Main sessions' repos, transcripts, tool state | Leader | Standbys | Yes |
| Vault items | Leader writes, Standbys may write when leading | Standbys | Always |
| Refresh tokens | Leader | Standbys | Always |
| Access tokens, delivered secrets | Leader | The Worker running the job, with TTL | Mode-600 file, deleted at job end |
| Worker sessions' repos and transcripts | That Worker | Nobody (code returns by git) | Node's choice |
| Notchling app data (`notchling.db`, pages) | Leader | Standbys | Yes |

## Review (2026-11-13): issues the design must answer

An independent review (Fable) and the maintainer-owner pass found these. They hold whatever shape is chosen.

1. **Self-fencing doesn't reach the tools.** `claude --bg` turns run in Claude Code's own background host, not under `notchlingd`; a suspended or crashed daemon can't stop them, so a woken laptop may finish a `git push` the Standby already resumed. Fencing must happen at the tool call: the `PreToolUse` hook denies when this node's term isn't current (checked on a suspend-aware clock), and a takeover first tries `claude stop` on the old node.
2. **Clocks across sleep.** Rust's `Instant` doesn't count sleep on macOS, so a lease looks valid after hours asleep; wall clocks skew. Leases are judged by the granting majority's clocks, with a suspend-aware clock locally (`CLOCK_BOOTTIME`, `mach_continuous_time`) and a stated skew bound.
3. **Headless leaders.** Switches turn on only from an app-tier call (`crates/notchlingd/src/switches.rs`) and approvals fail closed without an app (`crates/local-api/src/tier.rs`). A node tier and a headless switch policy must be defined before a VPS can lead.
4. **Vault off macOS.** The vault store is one Keychain item and unsupported elsewhere (`crates/engine/src/vault/store.rs`); a file-backed encrypted store is new work.
5. **Cluster state vs node-local state.** `notchling.db` mixes settings, the machines registry and the cost cache (0014). Replicating the whole file would overwrite a Standby's own state; split it, with a schema version.
6. **Live `.git` and SQLite.** "Lock files gone and quiet" races with git and with gc; a WAL database copied file by file can tear; `agy acp` holds its databases open. Replicate consistent snapshots (git objects plus a private index, SQLite's backup API), not live files.
7. **Resume needs identical paths and more than the transcript.** Transcripts embed absolute paths the model reuses, so only identical paths work. `claude --resume` also needs folder trust in `~/.claude.json`, hooks pointing at the local binary, MCP servers, plugins and file-history; `sessions/<pid>.json` and jobs' `state.json` are machine-local. Prove a cross-host resume first.
8. **Access-only credentials are unproven** for all three tools; on Linux the CLIs keep tokens in plain files. Google refresh tokens don't rotate, so Antigravity doesn't need a single refresher. The Antigravity and ChatGPT apps on the Mac also refresh the same logins. Whether a third party refreshing with Claude Code's client id, from several IPs, fits the providers' terms needs the maintainer's own reading.
9. **Revocation.** A single shared unlock key means removing a node really requires rotating the vault key, every secret and every login. Wrap the vault key per node public key and write the runbook.
10. **Version skew.** Nodes need a handshake that refuses dispatch or promotion on a schema or tool-version mismatch (reply keys and parsing are pinned to observed Claude Code versions, `crates/engine/src/reply/`).
11. **Notifications without the leader.** The phone has local notifications only (`flutter_local_notifications`), so "failover happened" or "no leader" reaches nobody unless the app is connected. Decide on push (APNs/FCM) or an always-connected fallback; notification ids must survive failover.
12. **Platform gaps.** File watching is FSEvents on macOS and polling elsewhere (`crates/platform/src/watch.rs`); replying through `claude attach` is proven on macOS only. Windows can't share Mac paths, so it is Worker-only.
13. **Network.** Two NATed nodes have no rendezvous with an outbound ssh tunnel; a WireGuard mesh (Tailscale) as the v1 network, with pinned mTLS on top, solves NAT, Windows and "no open port".
14. **A deterministic simulator** (fake clock, network, disk) must exist before any lease or replication code runs on a real node.
