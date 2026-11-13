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

1. Node identity, joining and the cluster view (no failover yet); run a session or a job on a picked Worker and see it in Chat and on the phone.
2. Vault delivery to Workers and the single refresher.
3. Standby replication and planned handoff.
4. Election, lease, self-fencing, sudden failover, the returning Primary.
5. Vault replication and unlock from the phone.
6. Windows.

## Left to the builder

Lease and timing values, the election algorithm's details (Raft or a simpler lease protocol, as long as the majority and freshest rules hold), the wire format between nodes, the replication engine (own or Mutagen), the path rule for transcripts, how outward steps are recognised, UI layout of the cluster view and Run on column, and the order of work inside each step.
