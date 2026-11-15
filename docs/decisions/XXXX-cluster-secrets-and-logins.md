# XXXX: Cluster secrets and Worker logins: one vault, files for delivery, nothing renewed

## Context

A job on a Worker needs a token or a password: a GitHub token for a push, an API key for a test. It must get that without the value going through a chat, a log, a mirrored transcript or the journal. The cluster also has Workers that run Claude Code, Codex and Antigravity, and each needs a login. Facts as of 2026-11-16:

- [Cluster v1 nodes](XXXX-cluster-v1-nodes.md) gives one primary (the home), Standbys and Workers, a node tier (`cluster::policy::RULES`) and `ClusterState.vault`, the node whose vault holds the values. It is the Mac the cluster was created on (m5) and never changes in v1.
- The vault ([0007](0007-local-api-tiers.md)) is one Keychain item on one Mac, with Ask me first and Always Allow. Only this build of the app can read it without macOS asking.
- Refresh tokens rotate. A tool that renews a login another process still holds signs that process out (the Codex and Claude Code notes in `CLAUDE.md`).
- The maintainer decided on 2026-11-14 (proposals 3a and 8): the allowlist is per machine, and another primary must not issue grants while the vault stays where it is.

## Decision

- **The vault stays in the vault node's Keychain, and only that node, while it is the primary, issues grants.** The vault node reads a value through the same release path `vault.use` takes (vault switch, Ask me first, Always Allow), writes the audit entry, then sends `grant.deliver` to the Worker running the job. If another machine is primary (after a failover or a planned move), no new secret is issued. A dispatch that names a secret stays Dispatching with the detail "Waiting for `<vault node>`" and is retried when the vault node is primary again. A request from inside a running job is refused, naming the vault node. Grants already delivered keep working until they expire. Copying the vault to a Standby is a later decision and needs its own record.
- **The allowlist is per machine and empty by default.** Each member has `secrets: {names, any}`. `names` is the vault entries a job on that machine may carry. `any` (Any Secret) allows every entry by name on that machine and is off by default. The home journals it (`SetAllow`, set by `cluster.allow.set` from the app). A name off the list is refused before the vault is read, and the refusal is audited.
- **A value is delivered as a file, never in env or argv.** `grant.deliver {job, name, value, expiresMs}` crosses the pinned TLS link and lands in `<cluster>/grants/<job>/<name>.json` on the Worker (folder 0700, file 0600, made with `O_EXCL` and `O_NOFOLLOW`, never replaced in place). It expires at the earlier of 12 hours from release and the job's end, is deleted at job end, expiry and `grant.revoke`, and a janitor sweeps every 30 s. The grants folder is excluded from snapshots.
- **A value meets an environment in two places only.**
  1. The child environment of the one command `notchling secret NAME -- <command>` runs on the Worker, as on the vault node today. The CLI streams the output through a redactor that replaces each value with `[secret:<name>]`.
  2. `CLAUDE_CODE_OAUTH_TOKEN` in the environment of a `claude --bg` the daemon launches for a dispatched Claude job. This second path is built and tested but **switched off** in this build (`CLUSTER_LOGIN_ON = false`): Claude Code's background host inherits the environment of the first `claude --bg` and outlives wipe, expiry and replace, so the token would reach every later session in that config folder. It stays off until cluster jobs have a config folder (and host) of their own.
- **What streams back is masked where it leaves the node.** `node.tail`, `agy.conversation`, `prompts.list` and `node.sessions` carry a job's text to other nodes, and the Worker masks each value (and its common escaped forms, such as JSON and HTML escapes) in them. A job whose masks were lost (a restart) is refused rather than served unmasked. A job's shell log is masked as it is written.
- **Workers renew nothing.** No node in the cluster renews an OAuth token. A Worker reports whether Claude Code, Codex and Antigravity are signed in, with read-only probes (a status command, or the existence of a file, never its contents), and the home refuses to dispatch to a tool that is signed out. A signed-out tool raises an inbox notice that says what to run on that machine. Option (a), Notchling as the one refresher of logins for the whole cluster, is not in v1.
- **Inbox items come from every node.** A node sends its inbox items to the home and to every linked Can Be Master machine (`node.inbox`), which show them in one list. An answer to a mirrored item is forwarded to the owner, which takes the first answer only (`inbox.answer`). A Mac or a phone may Allow a secret approval, as 0007's note of 2026-11-13 says.
- **Another machine's vault is reached only from the app.** `vault.list`, `vault.save` and `vault.remove` take a `node` argument that names the vault node, from the app, over the node link. There is no op that returns a list of values.

## Consequences

- Amends [XXXX cluster v1 nodes](XXXX-cluster-v1-nodes.md): the node tier gains `grant.*`, `login.*`, `node.inbox`, `inbox.*` and the vault relay. `Member` gains `secrets`.
- A Worker's own agents can no longer use `notchling secret` from outside a cluster job while the machine is a member and not the vault node: the call is refused with "this command isn't part of a cluster job" and never falls back to a local vault (ruling G48).
- Open items, each decided by the maintainer later:
  - **Claude and Antigravity jobs on a Worker can't use grants.** A job's process root is recorded for shell jobs only, since a Claude job runs under Claude Code's own host, not under the daemon. Their granted values are delivered, masked and swept, but `notchling secret` inside them is refused (G49). Output of a Claude job whose `session` is not recorded is served unmasked from the Worker (G52); its output through `notchling secret` is still hidden by the CLI.
  - **The Worker Claude login ships off** (G100). Until cluster jobs get their own config folder, a Worker signs in to Claude Code by hand (`/login` in `claude` on that machine).
  - **`workspace.return` ships a job's repository changes unmasked.** A secret a job wrote into a file travels with its branch (G74).
  - **Any linked client or Standby may answer a secret approval** it does not own, through the forwarded `inbox.answer`, and the owner cannot tell which surface the client stands for. A compromised client node could approve a release (G82). The alternative is a per-node answerer allowlist.
  - **The value passes through call arguments as an unwiped string** on the vault node and the Worker. `Debug` of those arguments prints nothing, but the heap copy is not wiped (G23, G35).
  - A Windows Worker gets no grants, and a Linux Worker needs glibc 2.32 or newer.
  - Escape forms the masks do not cover stay visible in a tail or log: triple escapes, JS-in-Go, base64 and other encodings of a value (G18).
  - A Codex or Antigravity signed-out check can read "can't tell": an Antigravity login in a Mac's Keychain is never read (G113).
- The simulator checks that no marker value leaves its paths, that a grant lives only on its job's node and ends with the job, that nothing refreshes on a Worker, that an item is answered once and that a release happens only on the vault node while it holds the current term.

## Status

Proposed. The number is assigned at merge, and the PR that merges it renames this file. The maintainer merging plan C settles it.

## Date

2026-11-16
