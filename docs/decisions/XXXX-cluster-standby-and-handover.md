# XXXX: Cluster v1 Standby and handover: sets at every turn end, only pushed sessions move, a Standby relays or holds the phone's commands

## Context

A Standby (a Can Be Master machine that isn't the primary) can carry on the primary's sessions only if it holds what they need, and a planned handover needs that copy brought up to date first. Plan D of cluster v1 is how. Facts as of 2026-11-16:

- [XXXX-cluster-v1-nodes](XXXX-cluster-v1-nodes.md) gives one primary, Standbys and Workers, and terms `(n, node)` that only grow. [XXXX-cluster-failover-and-fencing](XXXX-cluster-failover-and-fencing.md) (plan E) promotes a Standby when the primary is gone, and it needs a copy on that Standby to resume from.
- The spec's review found that a live `.git` folder and a live SQLite file can't be copied while they're written (issue 6), that paths differ between machines (issue 7), and that a copy of the primary's transcripts on another disk needs encryption (issue 8).
- The inbox lived in `notchling.db`, which never leaves the machine, so its items and ids would not survive a handover.
- The maintainer decided on 2026-11-14: a set at every turn end of every main session (c), sessions move only inside git with a pushed remote, and no shadow repository in v1 (d, proposal 2a), disk encryption instead of Notchling's own (proposal 1), and a held command older than an hour asks "Still send?" (a).

## Decision

- **Sets, not live files.** The primary sends a **set** (a consistent copy) of its main sessions to every ready Can Be Master member. It sends one at every turn end of a main session, every 5 minutes as a backstop, when a member becomes ready, and once more (the Final set) during a handover. A set holds each main session's transcripts, its session folder, Claude Code's edit history and to-do files for it (for Codex, its rollout and its line of `session_index.jsonl`), plus the shared parts: `cluster.db`, the paired phones' `devices.json` and the local copies of Artifact pages. It is taken only at quiet points: no write in the last 10 seconds and no git lock in the session's repository. A file that changed while it was copied is retried. A transcript goes as the bytes after the length the target acknowledged, and a database goes through SQLite's backup API. `EXCLUDED_DIRS` and files over 100 MB are left out. A set is staged under `<data>/cluster/outgoing` and sent over the cluster link.
- **A set is current only when it is complete.** A Standby stages a set as `.partial`, checks every part against the manifest's hash, and only then makes it current. It keeps the two newest sets, never holds one ahead of what the primary staged (invariant I4), and treats everything from the wire as hostile: no `..`, no links, no overlapping parts.
- **Ready.** A Can Be Master member gets sets only when it is ready. That means disk encryption is on (FileVault or LUKS; unknown counts as not ready), it has the primary's home folder and user, its free space is at least twice the newest set and 5 GB, and its Claude Code and Codex versions equal the primary's. Machines shows the same four checks as steps.
- **Code never travels in a set, and only pushed sessions move (rule a).** For a session's repository a set carries only facts: the root, `origin` (without a password or token), the branch and the commit. It also records whether the session may move, which needs all of these:
  - a clean tree: no change, no untracked file that isn't ignored, and no edit hidden by skip-worktree or assume-unchanged;
  - no submodules;
  - a branch whose upstream is on `origin` and holds every commit.

  Git is only read, with optional locks off and filter drivers blanked. A repository that holds a file the set carries (a dotfiles repository at `~`) makes its sessions No. The machine that takes a session clones or fetches the code from `origin` at the recorded commit, and refuses a folder that has changes of its own. Every other session stays where it is.
- **Identical paths.** A set restores only at the same absolute paths, under the same home folder; nothing is rewritten. Repository roots are refused in folders where code could run, such as dot folders, `Library`, `Applications`, `bin` and folders on the PATH. A local transcript that differs from the set's is never written over: the restore is refused, or the local copy is moved under `<data>/cluster/copies` first.
- **No encryption by Notchling.** Sets cross the cluster's pinned TLS 1.3 link and sit 0600 in 0700 folders. At rest they are protected by the disk encryption that readiness requires.
- **Main sessions are on by default.** Every Claude Code and Codex session on the primary that has a folder is main, with these exceptions:
  - `claude -p` sessions and command jobs;
  - Codex sessions that `codex exec` didn't start (Notchling's runner and `delegate` start theirs with it);
  - Antigravity sessions;
  - sessions the maintainer set to Keep on <machine> (`cluster.main.set {main: false}`, journaled).

  An ended session stays main for a day.
- **The inbox lives in `cluster.db`.** When a node creates or joins a cluster, its inbox moves there in one transaction, keeping ids and order. When it leaves, the inbox moves back. If an id is in both databases, the row with the larger `updated_ms` wins. So inbox items travel in the set's database and keep their ids after a handover.
- **The handover is a journaled saga.** The maintainer starts it with Move to <machine> Now (`cluster.move_home {to}`). Its state is journaled in `move-saga.json` (fsync, then rename), so a restart picks it up where it was. The steps are:
  - **Prepare.** The target must be ready, online and Can Be Master, and must pass the promote check. The resume proof must be recorded: `RESUME_PROOF_RECORDED` is false until the maintainer's proof, so the live op refuses today.
  - **CatchUp.**
  - **Freeze.** Replies to moving sessions are refused while the saga waits up to 10 minutes for their turn ends. Move Now (`cluster.move.now`) ends the wait and stops working sessions mid-turn. They are named in `move.cut`.
  - **FinalSnapshot.** The primary is fenced: no dispatch, new session, main change or return. One Final set must be acknowledged by the target, and a session that set can't move stays.
  - **StopOld.**
  - **Switch.** The target restores the shared parts and the moving sessions into a staging folder and checks them. Then it takes term `n + 1` and swaps the staged database in right before the takeover persists.
  - **Resume.** The target runs `claude --bg --resume` with each session's recorded flags and sends no message.
  - **Announce.** The inbox gets the notice "<to> is primary now".

  Sessions that stay keep running on the old primary, which is now a Standby, as worker-hosted sessions.
- **The undo line.** Any failure before Switch undoes the move: replies are unfrozen, stopped sessions start again with the same flags, and the notice "Couldn't move the primary: <reason>" is raised. At Switch, only a first attempt that surely wasn't delivered (answered `refused` or `not_sent`) may undo. Once a switch may have been sent, the saga only rolls forward: it re-sends the same term every second until the target holds it, with a notice at the 150th try. After Switch nothing is undone, and a session that fails to resume raises an item of its own.
- **A Standby relays or holds the phone's commands.** This covers `session.message`, `session.sendNow`, `prompt.answer` and `session.new`.
  - **Primary reachable.** The command is relayed and answered `relayed: true`.
  - **Primary unreachable.** The command is held in the Standby's own `cluster.db`:
    - kept in order, per phone, and seen only by the phone that sent it;
    - deduplicated by the command's `key`, else by its request id plus a hash of the op and arguments;
    - limited to 50 per phone and 200 in all;
    - never run against the Standby's own copy.
  - **Delivery.** Held commands go once, in order, when the primary is back or another machine becomes primary. Delivery is at most once: a command is recorded as delivered before it is sent, and one whose send timed out is reported, never resent. Held rows survive a handover's database swap.
  - **Stale commands.** A command held over an hour waits as Waiting until the phone confirms ("Still send?") or cancels it.
  - **Reads.** A read the Standby answers from its newest set carries `stale`, the set's time.
- **Paired phones travel in sets.** A Standby accepts the phones in the newest set's `devices.json`. It reads that file only, never writes or merges it, so a phone revoked on the primary is refused there with the next set. This is the same accepted risk as `devices.json` today ([0007](0007-local-api-tiers.md)): a file an agent with a shell can write.
- **The phone falls back, pinned.** The paired Mac's hello lists every Can Be Master machine with its bridge port and fingerprint, which each node publishes in its versions.
  - **Dialing.** The phone dials only a 64-hex fingerprint it learned, on an address that carries its own port, and refuses a certificate that doesn't match before it sends its token.
  - **Learning.** Pins, the term and the primary change only from a hello over the paired Mac's own LAN route, with a term at least as new as the stored one. A fallback machine's hello stores nothing.
  - **Staying.** The phone stays on a machine that holds its commands, since `held.confirm` and `held.cancel` must reach it, and goes back to the paired Mac when that Mac answers. It shows "Still send?" only after a probe of the paired Mac succeeded.
  - **Tunnel.** There is no fallback over the Cloudflare tunnel.

## Consequences

- A session with uncommitted, untracked or unpushed work never moves; it keeps running where it is. To make a session movable, the maintainer pushes its branch. A Standby needs its own git access to every `origin` the primary's sessions use (M11).
- A set goes out at every turn end. Only new transcript bytes travel. The database, pages and session folders are staged again each time, but a part the Standby already holds unchanged is linked there, not sent again.
- A Standby's disk holds copies of the primary's transcripts. They include what tools read or printed, and the edit history holds earlier versions of files the session changed. Only disk encryption and file modes protect them.
- At most once means a command whose send timed out may not have arrived: the phone is told to check, and nothing is sent twice.
- Revoking a phone on the primary reaches a Standby only with the next set. Phone continuity works only on the LAN.
- The handover is off in the code until the maintainer records the resume proof (M5). It can't be cancelled once Move is pressed: Move Now ends the wait, and only a failure before Switch undoes it.
- Built: the inbox in `cluster.db`, main sessions, the planner and its runners, sending and the Standby's store, the core and simulator, the restore, the handover, the relay and held queue, the phone's fallback, readiness, and the app's replication lines, Move sheet and Keep on <machine>.
- Not built yet:
  - The phone doesn't show `stale`.
  - Move Now's cut steps are named in `cluster.status` only, not raised as plan E's `paused_step` items.
  - A handover writes no fence file of plan E's, so the fence hook doesn't stop a session on the old primary.
  - A Standby restored from a set keeps its own claims and side copies.
  - A folder on the login shell's PATH but not the daemon's isn't refused as a repository root.

## Open items for the maintainer

- Accept this record, and record the resume proof (M5) that turns `RESUME_PROOF_RECORDED` on.
- A machine removed from the cluster keeps its sets (`<data>/cluster/standby/`) and set-aside copies (`<data>/cluster/copies/`) until you delete them, since removal deletes only `cluster.db`, the mirror and the transfer folders. Should removal delete the sets too?
- These decisions are as built:
  - an `http(s)` origin is recorded without its userinfo, so a token-in-URL remote needs the Standby's own access (R5);
  - sessions in repositories with submodules never move (R16);
  - a Codex session counts as main only when `codex exec` started it (R35).

## Status

Proposed. The number is assigned at merge, and the PR that merges it renames this file.

Ship gate: the maintainer accepts this record and records the resume proof; the live check with m5 and m1 (Checkpoint 2 of the plan) passes.

## Date

2026-11-16
