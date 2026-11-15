# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-c-secrets.md

grants: Branch agent/cluster-v1-c-grants, start 70cda366 (origin/agent-run-oss). Lane runs C1 > C2 > C3; C4 onward waits for B13.
grants: Ruling G1: Member.secrets needs `secrets: SecretAllow::default(),` in the one Member literal in core/join.rs — one additive line, approved by the coordinator — the daemon lane's merge should expect it.
grants: Ruling G2: C1's RULES additions (policy.rs) and Dispatch calling `attachable` (core/dispatch.rs) wait for C4 / the daemon lane, which owns those files; `dispatch_with_a_refused_attachment_fails_before_start` tests `attachable` directly — costs one call site in C4.
grants: Ruling G3: C3 ships `hide_forms` and `TailMasks` (cluster_redact.rs); the node.tail and shell-log wiring waits until cluster_mirror.rs and shell_jobs.rs exist (not on agent-run-oss yet) — costs two call sites later.
grants: Task 1: dispatched (base 70cda366, sonnet high).
grants: Task 1: implemented in the commit "engine: secret grants, allowlist and expiry" (15 tests in cluster::grants, cluster:: total 121 passing).
grants: Ruling G4: `SetAllow` re-lowercases names in `apply` (via `SecretAllow::new`) so a replayed or hand-built command can't store a mixed-case name — costs nothing if wrong.
