# SDD ledger — plan: docs/superpowers/plans/2026-11-13-cluster-v1-e-promote.md

pure: Lane "pure" (branch agent/cluster-v1-pure, base 70cda366): E Tasks 3, 4, then D Task 3.
pure: Ruling R1: fence JSON is serde snake_case with Gate tagged "kind"; FENCE_VERSION = 1 and parse refuses a newer v — the plan names no wire shape and Task 9 reads it with the same type — costs a format change in Tasks 8/9 if wrong.
pure: Ruling R2: test modules are registered in cluster/mod.rs next to the `pub mod` line, as every cluster module does — the lane's only shared-file edit — costs nothing.
pure: Task 3: dispatched (base 70cda366, sonnet high).
pure: Task 3: implemented (fence.rs: FenceFile, Gate, decide, decide_bytes, parse, after_wait and consts; 13 tests in fence_tests.rs).
