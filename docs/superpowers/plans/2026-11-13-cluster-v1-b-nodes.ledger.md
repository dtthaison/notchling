# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-b-nodes.md

Spec: docs/superpowers/specs/2026-11-13-notchling-cluster-design.md ("Scope: v1 first" step 1, "Work placement", "Sending a session to a Worker", "Node connections and joining", "Software architecture")

Other lanes of plan B append to this file on their own branches; every line is prefixed with its lane name (`daemon:` for the notchlingd lane), so a merge keeps them apart.

## Tasks

daemon: Branch agent/cluster-v1-b-daemon, start 70cda366 (origin/agent-run-oss; B1-B11 merged).
daemon: Task 12: dispatched (base 70cda366, sonnet high).
daemon: Task 12: implemented 7c8c7371 (notchlingd lib 800 passed, cluster_forward 13, cluster_sessions 13, cluster_mirror 14, engine cluster:: 107, engine sessions:: 64, local-api 94+1; clippy, fmt, guard clean; concerns: log_audit_e2e daemon_run_writes_sealed_month is timing-flaky on this Mac, the base daemon also takes 7-20 s to exit so the test's 10 s stop deadline fails at random; prompt events are not yet pushed to a follower (B17); node.sessions, node.tail and node.follow are consts in the daemon, not wire.rs ops, as B11 did for node.switch.set).
daemon: Task 12 review (opus): Needs fixes — I1 remote rows unvalidated and ignore the tools filter, I2 Workers pull and list other nodes' sessions, I3 symlinked mirror folder written through. Fix round 1 → resume implementer.
daemon: Ruling R1: only an app-tier caller forwards a qualified id; an agent-tier caller gets Forbidden locally; a Node caller never forwards again (no relay); origin is {this node, App} — the node tier lets a client relay its own app's tier only — costs an agent-tier remote path if wrong.
daemon: Ruling R2: node-only ops (node.sessions, node.tail, node.follow) are daemon consts, not wire.rs ops, as B11 did for node.switch.set; only cluster.follow is local API — costs a move to wire.rs if wrong.
daemon: Ruling R3: can_answer is not widened; hook.request uses Room::can_answer_prompts (an app attached, or a client leasing node.follow {topic: prompts}, 60 s) — can_answer also gates switches, join and approvals — B17 must push prompt events to the follower in the same change that first sends node.follow.
daemon: Ruling R4: only a client node pulls node.sessions and merges remote rows, decided from the current member record; a Worker's lists never carry another node's sessions — keeps the maintainer's sessions from agents on Workers — costs a Worker-side view if wrong.
daemon: Ruling R5: remote rows must deserialize as engine::sessions::Agent; non-Claude remote rows only in tools: all — the app reads the lists as Vec<Agent> — costs nothing.
daemon: Ruling R6: a mirror file is named by sha256(qualified id)[:16] plus its extension and capped at 256 MiB (the follow stops, nothing deleted) — costs a long transcript's tail if wrong.
daemon: Task 12 fix round 1: df72deb6 (I1 rows validated as Agent and tools-filtered, I2 only a client node pulls and merges, I3 mirror folders must be real directories, M4 hashed file names, M5 one Publisher reading the fresh local list, M6 follows cleared before the mirror folder is removed, M7 gen kept across a lapsed follow, M8 256 MiB cap, M9 peer events off the sessions topic dropped; notchlingd lib 812 passed, engine cluster::/sessions:: 172, local-api 94+1; clippy, fmt, guard clean; log_audit_e2e daemon_run_writes and log_bus push_never_blocks are timing-flaky under load, both pass alone).
daemon: Task 12 re-review (agy gemini-4-hydrogen high; first run declined git commands and returned nothing, rerun told to read only): Needs fixes — comments added in fix round 1; is_real_file and file_len follow a symlinked cluster or mirror folder. Both verified against the code.
daemon: Task 12 fix round 2: 4ec3570b (18 comment lines removed from cluster_mirror, cluster_sessions, cluster_wiring, cluster_driver_links; is_real_file and file_len now check cluster, mirror and node folders with symlink_metadata, file_len None on a link so the pull stops; 2 new planted-file tests; strip_comments --check passes, clippy, fmt, guard clean; notchlingd lib 813 passed on rerun, one timing flake on first run).
daemon: Task 12: complete (7c8c7371, df72deb6, 4ec3570b; round 2 fix checked by the coordinator: three-folder check in is_real_file and file_len, comments gone, cluster_ 102 passed).
daemon: Task 13: dispatched (base b8d95c0d, sonnet high).
daemon: Ruling R7: cluster.dispatch refuses tool codex ("Codex jobs on another machine come in a later version") — overview: Codex-in-cluster deferred, not v1 — costs one refusal to remove later if wrong.
daemon: Ruling R8: job ids and the key dedupe come from the core (LocalCmd::Dispatch, jobs.rs); the daemon builds none — one job per key is a core invariant — costs nothing.
