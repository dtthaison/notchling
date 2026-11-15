# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-d-standby.md

gitdb: Branch agent/cluster-v1-d-gitdb, base origin/agent-run-oss 70cda366. Lane gitdb runs D Tasks 4 and 5; lane pure runs D Task 3 on its own branch.
gitdb: Ruling R1: both lanes create crates/engine/src/cluster/snapshot/mod.rs and add `pub mod snapshot;` to cluster/mod.rs; the merge keeps both module lists — costs a trivial add/add conflict.
gitdb: Ruling R2: RepoFacts, GitCall, GitOutput live in cluster::snapshot::repo_state; plan B Task 14's cluster::workspace::RepoFacts (unmerged, other fields) stays separate — costs a later rename if they should be one.
gitdb: Ruling R3: rev-parse gains --path-format=absolute and --git-dir (to tell a linked worktree); status gains -z; rev-list is always planned and its failure means no upstream only when status shows none — read_plan can't know the upstream before status runs.
gitdb: Ruling R4: filter drivers are blanked as engine::git::read does (filter_probe, then the read calls) — status may run a clean filter; the plan's list didn't name it — costs one extra git call per read.
gitdb: Ruling R5: an http(s) origin is recorded without its userinfo; ssh and scp forms keep their user — a set must never carry a credential — Needs the maintainer (a token-in-URL remote then needs m1's own git access, M11).
gitdb: Ruling R6: snap_repo runs git through the Launcher it is given (the daemon's SandboxedLauncher, as git_ops), 10 s per call; submodules are not ignored, so a dirty submodule keeps a session from moving.
gitdb: Task 4: dispatched (base 70cda366, sonnet high).
