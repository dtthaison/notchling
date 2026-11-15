# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-e-promote.md

pure: Lane "pure" (branch agent/cluster-v1-pure, base 70cda366): E Tasks 3, 4, then D Task 3.
pure: Ruling R1: fence JSON is serde snake_case with Gate tagged "kind"; FENCE_VERSION = 1 and parse refuses a newer v — the plan names no wire shape and Task 9 reads it with the same type — costs a format change in Tasks 8/9 if wrong.
pure: Ruling R2: test modules are registered in cluster/mod.rs next to the `pub mod` line, as every cluster module does — the lane's only shared-file edit — costs nothing.
pure: Task 3: dispatched (base 70cda366, sonnet high).
pure: Task 3: implemented (fence.rs: FenceFile, Gate, decide, decide_bytes, parse, after_wait and consts; 13 tests in fence_tests.rs).
pure: Task 3 review (opus high): REVIEW-T3
pure: Ruling R3: Artifact is outward unless its input parses as a read action; an unparseable (cut) input counts as outward — I7 safe side — costs an extra inbox item for a cut read.
pure: Ruling R4: Bash segments are matched by program and subcommand words after env/sudo wrappers; curl/wget are outward with a non-GET method or any body/upload flag — costs a missed exotic form if wrong.
pure: Task 4: dispatched (base ef98e7bf, sonnet high).
pure: Ruling R5: the Bash scan reads quotes, heredoc bodies, `$(..)`, backticks, `bash -c`/`eval` scripts and wrappers (sudo, env, nohup, time, timeout, xargs, nice), and a script nested deeper than 6 counts as outward; `gh api` with -f/-F/--field/--input and no -X counts as POST; `tofu` and `flyctl` match `terraform` and `fly`; `gh release` and `kubectl rollout` are outward whole, as the plan lists them — costs an extra inbox item for a cut read form (rollout status, release view), never a rerun.
pure: Ruling R6: aws/gcloud/az are read when one of the first four plain words is ls, show, describe*, list* or get* made only of lowercase letters, digits and dashes; `mcp__*` names are matched whole (the server part too), so a server named postgres counts as outward — costs a missed mutation named like a read, or an extra item for a read on a server whose name holds a listed word.
pure: Task 4: implemented (outward.rs with outward/shell.rs and outward/commands.rs: is_outward, CutStep, cut_steps; 23 tests in outward_tests.rs, fixture cluster/cut-push.jsonl).
