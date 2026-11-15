# XXXX: Cluster secrets and Worker logins: one vault, files for delivery, nothing renewed

## Context

A job on a Worker needs a token or a password: a GitHub token for a push, an API key for a test. It must get that without the value going through a chat, a log, a mirrored transcript or the journal. The cluster also has Workers that run Claude Code, Codex and Antigravity, and each needs a login. Facts as of 2026-11-16:

- [Cluster v1 nodes](XXXX-cluster-v1-nodes.md) gives one primary (the home), Standbys and Workers, a node tier (`cluster::policy::RULES`) and `ClusterState.vault`, the node whose vault holds the values. It is the Mac the cluster was created on, and it never changes in v1.
- The vault ([0007](0007-local-api-tiers.md)) is one Keychain item on one Mac, with Ask me first and Always Allow. Only this build of the app can read it without macOS asking.
- Refresh tokens rotate. A tool that renews a login another process still holds signs that process out (the Codex and Claude Code notes in `CLAUDE.md`).
- The maintainer decided on 2026-11-14 (proposals 3a and 8): the allowlist is per machine, and another primary must not issue grants while the vault stays where it is.

## Decision

- **The vault stays in the vault node's Keychain, and only that node, while it is the primary, issues grants.** The vault node reads a value through the same release path `vault.use` takes (vault switch, Ask me first, Always Allow), writes the audit entry, then sends `grant.deliver` to the Worker running the job. If another machine is primary (after a failover or a planned move), no new secret is issued. A dispatch that names a secret is refused with "`<vault node>` holds the vault; dispatch again when it is primary", and a dispatch whose attach still comes back waiting fails the job the same way, closes its inbox notice and revokes what was sent (G191). A request from inside a running job is refused, naming the vault node, or answers "no job" unless the vault node dispatched the job itself. Grants already delivered keep working until they expire. Copying the vault to a Standby is a later decision and needs its own record.
- **The allowlist is per machine and empty by default.** Each member has `secrets: {names, any}`. `names` is the vault entries a job on that machine may carry. `any` (Any Secret) allows every entry by name on that machine and is off by default. The home journals it (`SetAllow`, set by `cluster.allow.set` from the app). A name off the list is refused before the vault is read, and the refusal is audited.
- **A value is delivered as a file, never in env or argv.** `grant.deliver {job, name, value, expiresMs}` crosses the pinned TLS link and lands in `<cluster>/grants/<job>/<name>.json` on the Worker (folder 0700, file 0600, made with `O_EXCL` and `O_NOFOLLOW`, never replaced in place). It expires 12 hours after release at the latest. A Worker knows its jobs from its own daemon, since the home's state sync carries none: a shell job from spawn to reap, a Claude or Antigravity job from its start op. A grant for a job that has not started yet is held pending for 8.5 minutes (the attach wait plus one call) and deleted if the job does not start in that time. A shell job's grants are deleted at reap. A Claude or Antigravity job has no end signal on the Worker, so its grants go when the home revokes them, at expiry or when the daemon restarts. A janitor sweeps every 30 s. A Worker cut off from the cluster keeps the file until it expires. The grants folder is excluded from snapshots.
- **A value meets an environment in two places only.**
  1. The child environment of the one command `notchling secret NAME -- <command>` runs on the Worker, as on the vault node today. The CLI streams the output through a redactor that replaces each value with `[secret:<name>]`.
  2. `CLAUDE_CODE_OAUTH_TOKEN` in the environment of a `claude --bg` the daemon launches for a dispatched Claude job. This second path is built and tested but **switched off** in this build (`CLUSTER_LOGIN_ON = false`): Claude Code's background host inherits the environment of the first `claude --bg` and outlives wipe, expiry and replace, so the token would reach every later session in that config folder. It stays off until cluster jobs have a config folder (and host) of their own.
- **What streams back is masked where it leaves the node.** `node.tail`, `agy.conversation`, `prompts.list` and `node.sessions` carry a job's text to other nodes, and the Worker masks each value (and its common escaped forms, such as JSON and HTML escapes) in them. Each text is masked with every value live on that node, whichever job it was granted to, because every job on a Worker runs under the daemon's uid (G190). A job whose masks were lost (a restart) is refused rather than served unmasked. A job's shell log is masked as it is written, and a delegate job's `out` and `err` are masked the same way. The open items below say what is not masked.
- **Workers renew nothing.** No node in the cluster renews an OAuth token. A Worker reports whether Codex and Antigravity are signed in, with read-only probes: Codex's `login status` command, and whether the Antigravity token file exists and is not empty (never its contents). The Claude Code login is reported only while the cluster login file stands, which is off in this build. The home refuses to dispatch to a tool that is known to be signed out. A signed-out tool raises an inbox notice that says what to run on that machine. Option (a), Notchling as the one refresher of logins for the whole cluster, is not in v1.
- **Inbox items come from every node.** A node sends its inbox items to the home and to every linked Can Be Master machine (`node.inbox`), which show them in one list. An answer to a mirrored item is forwarded to the owner, which takes the first answer only (`inbox.answer`). A Mac or a phone may Allow a secret approval, as 0007's note of 2026-11-13 says.
- **Another machine's vault is reached only from the app.** `vault.list`, `vault.save` and `vault.remove` take a `node` argument that names the vault node, from the app, over the node link. There is no op that returns a list of values.

## Consequences

- Amends [XXXX cluster v1 nodes](XXXX-cluster-v1-nodes.md): the node tier gains `grant.*`, `login.*`, `node.inbox`, `inbox.*` and the vault relay. `Member` gains `secrets`.
- A Worker's own agents can no longer use `notchling secret` from outside a cluster job while the machine is a member and not the vault node: the call is refused with "this command isn't part of a cluster job" and never falls back to a local vault (ruling G48).
- Open items, each decided by the maintainer later:
  - **Claude and Antigravity jobs on a Worker can't use grants.** A job's process root is recorded for shell jobs only, since a Claude job runs under Claude Code's own host, not under the daemon. Their granted values are delivered, masked and swept, but `notchling secret` inside them is refused (G49). Every row is masked with the values of every live job on that Worker, tied to its job or not, and after a restart that lost a job's masks that job's rows are refused, as is a row tied to no job while an unidentified lost job exists (G57, G65, G72, G168, G190).
  - **A job on a Worker can read another job's grant file** (G190, qualifies G49). Every job there runs under the daemon's uid, so job B can open `<data>/cluster/grants/<jobA>/*.json`. The masks use every value live on the node, so the value does not stream back through a tail, a shell log or a delegate reply, but masking is not isolation. Real isolation needs a separate uid or a sandbox per job.
  - **The Worker Claude login ships off** (G100). Until cluster jobs get their own config folder, a Worker signs in to Claude Code by hand (`/login` in `claude` on that machine).
  - **`workspace.return` ships a job's repository changes unmasked.** A secret a job wrote into a file travels with its branch (G74).
  - **Any linked client or Standby may answer a secret approval** it does not own, through the forwarded `inbox.answer`, and the owner cannot tell which surface the client stands for. A compromised client node could approve a release (G82). The alternative is a per-node answerer allowlist.
  - **Whichever node is primary applies `cluster.allow.set`** (G82). A Standby acting as primary can set Any Secret on a Worker, though an Ask me first entry still asks.
  - **A dispatch the vault node can't serve fails instead of waiting** (G191, supersedes G21's wait). Replicating a waiting job to the vault node, so it is attached when that node becomes primary, is a maintainer decision.
  - **A home restart during an attach leaves the job Dispatching** (G192). The engine never re-drives a dispatch that was in flight at a restart, with secrets or without, and it can't tell an attach from a start that was already sent. The grants expire on the Worker.
  - **A Worker has no end signal for a Claude or Antigravity job** (G162, G169). Its grants last until the home revokes them, the 12 hour expiry or a restart, and the last held prefix of a value in its output stays held back (never served raw) until a failed start or a restart ends the job here.
  - **A Worker still stores a stale release in a split-brain window** (G138, G146, G172). A release under an older term, made by a primary that was cut off from a newer one, can be stored by a Worker. The simulator counts it and does not fail it. A Worker should refuse a `grant.deliver` whose release term is older than the highest term it has seen.
  - **A grant for a made-up job id from the vault node is stored until its pending window passes** (G161).
  - **`grant.request` on a vault node that is not the primary answers "no job"** unless that node dispatched the job itself, because its state has no other jobs (G165, G169, G191).
  - **An unkeyed lost job is released from blanket refusal 12 hours after its end** (G72). Its own rows are then masked with other jobs' values only.
  - **The simulator models the daemon, it does not run it** (G140, G170). A later daemon change must be mirrored by hand until the Worker's decisions are extracted into the engine.
  - **The value passes through call arguments as an unwiped string** on the vault node and the Worker. `Debug` of those arguments prints nothing, but the heap copy is not wiped (G23, G35).
  - A Windows Worker gets no grants, and a Linux Worker needs glibc 2.32 or newer.
  - Escape forms the masks do not cover stay visible in a tail or log: triple escapes, JS-in-Go, base64 and other encodings of a value (G18).
  - A signed-out check can read "can't tell", and then the entry is absent and nothing is refused. A Mac's Antigravity login may be in the Keychain, which is never read, so a signed-out Mac Antigravity is never refused (G113). Codex output Notchling can't read leaves the entry absent (G117).
- The simulator checks these:
  - No planted marker value leaves its allowed paths, and a value is bound to the one node its release named.
  - A grant lives only on its job's node and is gone within 32 s of its job's end on that Worker (35 s after a restart, and the pending window plus 2 s for a job that never starts).
  - Nothing refreshes a login on a Worker.
  - An item is answered once.
  - A release is judged by the releasing node's own view of being the vault node, the home and holding its own term, and a Worker must not store a release older than the term its own core holds.
  - An ask-first name carries the approval that allowed it, and one Allow Once covers one release.
  - A login probe only runs read-only commands.
  - The daemon's broker, the Worker handlers and the inbox mirror are modelled in the simulator, not run, so their own tests cover them.

## Status

Proposed. The number is assigned at merge, and the PR that merges it renames this file. The maintainer merging plan C settles it.

## Date

2026-11-16
