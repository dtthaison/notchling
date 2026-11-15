# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-b-nodes.md

Spec: docs/superpowers/specs/2026-11-13-notchling-cluster-design.md ("Scope: v1 first" step 1, "Work placement", "Sending a session to a Worker", "Node connections and joining", "Software architecture")

Other lanes of plan B append to this file on their own branches; every line is prefixed with its lane name (`daemon:` for the notchlingd lane), so a merge keeps them apart.

## Tasks

daemon: Branch agent/cluster-v1-b-daemon, start 70cda366 (origin/agent-run-oss; B1-B11 merged).
daemon: Task 12: dispatched (base 70cda366, sonnet high).
daemon: Task 12: implemented 7c8c7371 (notchlingd lib 800 passed, cluster_forward 13, cluster_sessions 13, cluster_mirror 14, engine cluster:: 107, engine sessions:: 64, local-api 94+1; clippy, fmt, guard clean; concerns: log_audit_e2e daemon_run_writes_sealed_month is timing-flaky on this Mac, the base daemon also takes 7-20 s to exit so the test's 10 s stop deadline fails at random; prompt events are not yet pushed to a follower (B17); node.sessions, node.tail and node.follow are consts in the daemon, not wire.rs ops, as B11 did for node.switch.set).
