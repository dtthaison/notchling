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
gitdb: Task 5: dispatched (base b9df0a4e, sonnet high), in parallel with Task 4's review.
gitdb: Ruling R7: Manifest, Part, PartKind and dir_digest live in engine cluster::snapshot::manifest — Tasks 6 and 8 are engine code and need them; the plan named only snap_files.rs — costs nothing if wrong.
gitdb: Ruling R8: Task 3's SetPlan, SessionEntry, Trigger, excluded, MAX_FILE and plan C's EXCLUDED_DIRS are stand-ins in cluster::snapshot::stand_in with the plans' shapes, re-exported from snapshot/mod.rs; consumers import only cluster::snapshot::{..}. At merge with lane pure: delete stand_in.rs and re-export pure's names (SessionEntry must use repo_state's RepoFacts and Movable); with plan C: EXCLUDED_DIRS comes from cluster::grant_files.
gitdb: Ruling R9: stage_set(plan, stage, meta: &StageMeta { seq, term, home, created_ms, quiet_ms, now_ms }) — the Manifest's term, home and created_ms aren't in SetPlan; stage is <data>/cluster/outgoing — costs a signature change if wrong.
gitdb: Ruling R10: parts staged under <set>/parts/<absolute path>, manifest.json written last by rename (a set without it is incomplete); 0700 folders and 0600 files created so, never narrowed after.
gitdb: Ruling R11: a directory in plan.files is one Dir part (len the sum, sha256 = dir_digest over "<sha256> <len> <relpath>" lines); symlinks are never followed or staged; excluded entries inside are skipped.
gitdb: Ruling R12: one Busy file or db fails the whole stage and removes its folder; a Rewritten tail is staged whole as Tail { from: 0 }; a missing file is skipped, a missing db is an error; a path inside a session's repository is refused (code never travels).
gitdb: Ruling R13: copy_tail is Rewritten when shorter than `from` or its inode changed during the copy; a recorded inode across sets would be Task 6's (it keeps lengths only) — costs a missed replace-with-longer-file if wrong.
gitdb: Ruling R14: backup_db steps 256 pages / 5 ms and, after 10 s or repeated restarts under a busy writer, finishes with one step(-1) under one read lock — a stepped backup of a db written all the time can restart forever; rusqlite becomes a notchlingd dependency with "backup", plus sha2.
gitdb: Task 5: implemented a3f88969 (6 engine + 22 daemon tests)
gitdb: Task 4 re-review (opus): Approved (981fcc71). Parked minor → final fix round: a local branch named origin/<x> passes the origin/ check; count `rev-list --count HEAD --not --remotes=origin` instead.
gitdb: Task 4: complete (80daee72, 981fcc71).
gitdb: Task 5: implemented a3f88969, 22e9514c (review on 22e9514c).
gitdb: Task 5 review (opus): Needs fixes — I1 copy_tail took a tail of a file rewritten in place during the copy; minors M1–M13. Fix round 1 → resume implementer.
gitdb: Ruling R18: the source db's -wal/-shm may appear beside a cleanly closed WAL db on the backup's read-only open; accepted, the sources are dbs the daemon owns — costs two small files.
gitdb: Ruling R19: a session's repository root that holds a plan path (a dotfiles repo at ~ holds every transcript) fails every stage, closed — Task 3/7 must leave such a session's repo facts out and mark it No — Needs the maintainer (until then a repo at ~ blocks sets).
gitdb: Ruling R20: MAX_FILE counts a tail's bytes (a set sends only what a transcript gained); a tail over it is skipped, and Task 8 must refuse a session whose transcript isn't in the store — costs a session that can't move after a 100 MB turn.
gitdb: Notes for Task 6: Tail { from: 0 } replaces the whole stored file; a target whose acked length is past the staged from gets a re-sliced, re-hashed tail; staged_path is in engine manifest.
gitdb: Task 5 fix round 1: M8 uses a symlink_metadata check on the source db instead of SQLITE_OPEN_NOFOLLOW, which refuses any symlinked path component (macOS /var and /tmp); snap_files.rs is split into snap_copy.rs (copies), snap_db.rs (backup) and snap_files.rs (staging).
