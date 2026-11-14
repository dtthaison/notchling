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

## Status

Proposed. The number is assigned at merge, and the PR that merges it renames this file. The maintainer merging plan B settles it.

## Date

2026-11-14
