# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-d-standby.md

gitdb: Branch agent/cluster-v1-d-gitdb, base origin/agent-run-oss 70cda366. Lane gitdb runs D Tasks 4 and 5; lane pure runs D Task 3 on its own branch.
gitdb: Ruling R1: both lanes create crates/engine/src/cluster/snapshot/mod.rs and add `pub mod snapshot;` to cluster/mod.rs; the merge keeps both module lists — costs a trivial add/add conflict.
gitdb: Ruling R2: RepoFacts, GitCall, GitOutput live in cluster::snapshot::repo_state; plan B Task 14's cluster::workspace::RepoFacts (unmerged, other fields) stays separate — costs a later rename if they should be one.
gitdb: Ruling R3: rev-parse gains --path-format=absolute and --git-dir (to tell a linked worktree); status gains -z; rev-list is always planned and its failure means no upstream only when status shows none — read_plan can't know the upstream before status runs.
gitdb: Ruling R4: filter drivers are blanked as engine::git::read does (filter_probe, then the read calls) — status may run a clean filter; the plan's list didn't name it — costs one extra git call per read.
gitdb: Ruling R5: an http(s) origin is recorded without its userinfo; ssh and scp forms keep their user — a set must never carry a credential — Needs the maintainer (a token-in-URL remote then needs m1's own git access, M11).
gitdb: Ruling R6: snap_repo runs git through the Launcher it is given (the daemon's SandboxedLauncher, as git_ops), 10 s per call; submodules are not ignored, so a dirty submodule keeps a session from moving.
gitdb: Task 4: dispatched (base 70cda366, sonnet high).
gitdb: Task 4: implemented 80daee72 (29 engine + 16 daemon tests)
gitdb: Task 4 review (opus): Needs fixes — C1 an upstream not on origin counted as pushed; I1 a submodule's own filter ran under status; I2 submodule ignore config hid changes; I3 credentials kept on non-http schemes. Fix round 1 → resume implementer.
gitdb: Ruling R15: a session moves only when its upstream is origin/<branch>; any other upstream (local, another remote) is "has no pushed upstream" — the restore fetches from origin only — costs moving sessions that track a fork.
gitdb: Ruling R16 (replaces R6's submodule half): status runs with --ignore-submodules=all, and a repository whose index holds a gitlink never moves ("<root> has submodules") — a submodule's commit isn't covered by rule (a) and reading it safely means recursing — Needs the maintainer (sessions in repos with submodules stay on the primary in v1).
gitdb: Ruling R17: origin URLs keep a user only for ssh and scp forms; every other scheme (and remote-helper forms) loses all userinfo. An upstream that is gone is no upstream. Assume-unchanged and skip-worktree entries count as uncommitted. Parked for Task 7: a read_repo Err must become Movable::No for that session.
