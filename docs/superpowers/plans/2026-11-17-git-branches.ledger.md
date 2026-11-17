# Git branches build ledger

Append-only: add new rulings and log lines at the end of their section; never edit or remove an earlier one.

## Rulings

- Maintainer 2026-11-17: the spec `2026-11-17-git-branches-design.md` is approved with every recommendation in its "Needs the maintainer" taken. 1 ship the running dot by session and subagent folder; lanes start inside their worktree to get it; 2 every row's right side is "vs base", and the current branch's uncommitted count and lines move into its sub-line (spec 7.1, 7.2 and the mockup updated); 3 Merged is automatic: into the main checkout's branch or the default; 4 Changes' default base becomes the nearest base; 5 Changes for a branch with no worktree, committed work only, in v1; 6 no actions in v1; the first after v1 is "Remove Worktree"; 7 no phone in v1.

## Log
- 2026-11-17 plan check (Antigravity, gemini-4-argon, high, as `delegate-models --for spec-check` picks): "ready after fixes", 14 findings (6 errors, 7 should-fix, 1 nit), each verified against the code and fixed in the plan: unconfirmed nearest-base picks for unrelated histories (D5 rewritten, `Reader::related`, sticky unrelated marks, an `unrelated` reply field), wave order (Task 8 needs Tasks 1 to 3), two test filters, the root read in Task 5, `GitModel` raw fields and its test literals, the `ref` base menu (B22), detached facts and upstream candidates in the snapshot, `lines` keyed by commit, Task 18 files and the kit icon test, `offer_page` window, Changes from Branches for the own target, "just now" ages, missing Branches rules, the unborn head, ms units and `read_ref_stat` without a base.
- git: 2026-11-17 lane start: branch agent/git-branches from origin/agent-run-oss 02448278 merged with origin/agent/git-branches-plan ce287b14 (fast-forward). Maintainer items accepted as the plan proposes: D1, D2, D5, D6.
- git: Ruling R1: the tasks run one implementer at a time (sonnet, high) in the lane worktree, in wave order 1, 2, 5, 6, 7, 15, 3, 4, 9, 13, 8, 14, 17, 10, 16, 11, 12, 18, 19: one shared target dir allows one build at a time, and parallel implementers in one worktree break each other's builds. Each task's review (agy, as `delegate-models --for review` picks) runs while the next implementer works; fix rounds go in between tasks.
- git: Task 1: complete b9a00204 (9 tests in git::worktrees_tests; engine suite green, clippy clean). Review (agy gemini-4-hydrogen high): Approved, no findings.
- git: Task 2: dispatched (sonnet high).
- git: Task 2: complete 301080ac (15 tests in git::refs_tests; git:: 125 pass; engine suite one load flake `machines::tests_node_logins` that passes alone). Binary-only shortstat prints ` 1 file changed, 0 insertions(+), 0 deletions(-)` on git 2.54; the parser reads tokens and takes both forms. Review (agy hydrogen high): Approved.
- git: Task 5: dispatched (sonnet high).
- git: Task 5: complete dfc8a45e (git:: 148 pass; engine suite green; notchlingd git_ops 22 pass). Only the three planned `calls.len()` growths in existing tests. Implementer wrote code before tests (no red seen for the new tests); accepted, the tests check behavior against real git.
- git: Task 6: merged origin/agent-run-oss (d7749377) first, as the merge notes ask; dispatched (sonnet high).
