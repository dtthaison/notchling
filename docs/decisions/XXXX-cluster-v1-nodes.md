# XXXX: Cluster v1 nodes: one primary, standbys and workers over Tailscale

## Context

Machines (Macs, Linux boxes) should join one cluster, so one app and one phone see every node's sessions and a job can run on another node. Plan B of cluster v1 is the nodes: the pure core (`engine::cluster`), a deterministic simulator and `cluster.db`. Sockets, certificates and the daemon wiring come after it (plan B tasks 9 and later). Facts as of 2026-11-14:

- [0012](0012-remote-computers.md) says nothing is replicated and computers never relay. A cluster needs a replicated member list, a term and a relay.
- [0007](0007-local-api-tiers.md) has two tiers, app and agent. A call that comes from another node needs a third origin, and a way to carry the app tier from a client node.
- The revised 2026-11-14 model: m5 (the laptop) is the primary and m1 is its standby. Terms ride on every Hello, Call and Members message. Can Be Master is a flag per member. There is no short lease.
- Version sync (`agent/versions-p1-see`) says a tool version that differs from the reference Mac warns at dispatch and refuses only from its Phase 6.

## Decision

- Roles in v1: one primary (the home), Standbys, Workers. No witness and no election. A member's Can Be Master switch (on by default for Macs, off for other machines) makes it a Standby; any number may have it. The primary is the node the cluster was created on until a planned move or a promote. Terms are `(n, node)`, only grow and fence every call: a stale call is refused with the newer term and never run, and a home that hears a newer term steps down.
- Identity: a node id is the first 16 lowercase hex characters of the SHA-256 fingerprint of its certificate, one certificate per data folder. Ids on the wire are qualified `<node>/<id>` ([0012](0012-remote-computers.md)'s host-qualified ids).
- Topology: every node links to the primary; a client node (a Mac with the app, marked at approval) and every Can Be Master node link to every member. Control goes through the primary; data goes to the session's owner.
- Node tier: a closed table of ops one node may ask of another, by the sender's role and the origin's tier (`cluster::policy::RULES`). A client relays its own app's tier; a Worker relays the agent tier at most; `origin.node` must equal the sender.
- State: one journaled `ClusterState` (members, jobs, term, home) changed only by `apply`, which returns the journal entry and what changed. The core is sans-IO: `NodeCore::step(Input) -> Vec<Effect>`. A call is deduplicated by `(from, id)`.
- Joins need the maintainer's Allow in the Inbox; join tokens are one-time and last 10 minutes. The joiner is pinned only while its join is open.
- A removed node is never pinned, never dispatched to, and wipes its cluster state when it hears it was removed.
- `cluster.db` (SQLite, in `<data>/cluster/`) holds the state and its journal. A database from a newer schema is refused untouched. Tables of `notchling.db` are classed Node or Cluster (`store_class`); only `inbox` is Cluster.
- Dispatch compatibility (`cluster::versions::compat`): another node protocol or schema, a missing tool or a missing `dispatch` feature refuses; a tool or Notchling version that differs warns.
- Correctness is held by a deterministic simulator (seeded, with delay, duplication, partitions, clock drift, crash and suspend) and invariants: no two homes in a term, no term goes down, no stale call accepted, no job started twice or on two nodes, no dispatch to a removed node. Each invariant has a planted-bug test.

## Consequences

- Amends [0012](0012-remote-computers.md): "Nothing is replicated" is superseded for cluster members, and "computers never relay" and "on the wire nothing changes" change for the home and client nodes.
- Amends [0007](0007-local-api-tiers.md): a third origin, the node, comes in through the table only.
- `cluster.db` is a new file write in a protected area, so `maintainer-gate` stops on it (`CAP-FILEWRITE`) until the maintainer approves.
- The core cannot undo state after a failed save: the daemon rebuilds the core from the last saved state.
- Joining over TLS needs a short window where the joiner is not yet pinned (`Effect::Pins.join_open`); the transport must open it only for an open join.
- Still to build: the platform clock and addresses, the TLS transport, the daemon ops and the app and phone views.

## Note, 2026-11-14: joins and restarts on headless nodes

The maintainer decided the three points the daemon wiring (plan B tasks 9 to 11) left open:

1. **`cluster.join` from an agent, only while no app is attached.** Kept as built: an app-tier join turns the `cluster` switch on, an agent's join does too while no app is attached (a headless machine Machines joins over ssh), and an agent's join while an app is attached is refused with the off message. Every switch a join turns on is audited `settings.set_switch cluster` with `cluster.join (app)` or `(headless)`.
2. **A headless member rejoins its home after a restart.** A node that is an active member of a cluster, never had an app attached, and is not the home turns the `cluster` switch on at daemon start and dials its home (decision 0007's note of 2026-11-14 records the exception):
   - "Never had an app attached" is the member's role as the home recorded it, not a local flag: the joiner says in its `Join` whether it joined headless (an agent's join with no app attached), the home stores it as `Member.headless` at the join and keeps it through the approval, and it reaches the node in `JoinOk` and `Members` like every other field. A join from the app is never headless, so a Mac with the app keeps the old behavior: off until the app turns it on, even when its daemon starts before the app attaches. A client member (marked by the home) never rejoins by itself either. The home's app clears the mark with `cluster.member.set {node, headless: false}` when a headless machine gets the app later.
   - The rule is `ClusterState::rejoin_home`: this node is an active, headless, non-client member and the home is another active member. The daemon reads it once from `cluster.db` at start and turns the switch on only if no app is attached at that moment, audited `cluster_set:cluster:on` with the home as `peer` and the reason `restart: a headless member rejoins its home`.
   - Dialing home grants nothing by itself. The peer must present the pinned certificate (the home's fingerprint in `cluster.db`), so a rewritten address reaches no one who passes the pin and a rewritten fingerprint refuses the real home (tests: `a_tampered_home_certificate_fails_closed`, `a_tampered_home_address_fails_closed`). `acting`, `bridge` and `install` still come only from the home's `Members`/`HelloOk` (`node.switch.set`, `cluster_switches.rs`), only while no app is attached; `vault` and `accounts` never follow the cluster.
   - **Known risk, accepted with the decision.** `cluster.db` and the node identity are files an agent with a shell can write. An agent that rewrites the whole home record (id, fingerprint and address) and runs a node of its own at that address becomes this node's "home" and can turn on `acting`, `bridge` and `install`. That is not new: under point 1 the same agent can already wipe `cluster.db` and join a cluster of its own while no app is attached. It is the class of decision 0007's notes of 2026-11-08 and 2026-11-13 (files at rest are not a tier boundary) and closes only when the home's pin has an anchor an agent can't write.
3. **The phone may Allow a join.** Kept as built: the `node_join` inbox item takes Add from `mac` or `phone`, as a secret approval does (decision 0007's note of 2026-11-13).

## Status

Proposed. The number is assigned at merge, and the PR that merges it renames this file. The maintainer merging plan B settles it.

## Date

2026-11-14
