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
- git: Task 5 review (agy hydrogen high): Approved.
- git: Task 6: complete 55edf934 (5 new tests; sessions:: 75, notchlingd git_ops 23; workspace clippy clean). Workspace run under heavy load (other lanes' builds): file-watcher timeouts only, in `platform watch::tests`, app `chat::reload` watcher test, notchlingd `codex_children_tests::a_rollout_written_after_start_is_found`, `log_audit_e2e`; none in code this lane touches (platform has no diff). To recheck on a quiet machine.
- git: Task 7: dispatched (sonnet high).
- git: Task 6 review (agy hydrogen high, second run: the first stopped on a declined shell command): Approved. The workspace build is the literal check (cargo check --all-targets clean).
- git: Task 7: complete 67cddc8f (6 new tests; glance 19, contract:: 49, chat::subagent_rows 11, subagent_reads 5; engine suite green). App lib: one watcher-timing failure `chat::reload::tests::steady_writes_still_make_the_watcher_report` under load (not touched here); the app's other targets rerun with Task 15.
- git: Task 15: dispatched (sonnet high).
- git: Task 7 review (agy hydrogen high): Approved.
- git: Task 15: complete 1b51be9f (8 tests, red seen with a todo! stub; app package all 28 targets pass but the lib's `chat::reload::tests::steady_writes_still_make_the_watcher_report`, a 3.5 s watcher-timing test, fails under load avg 16-43 from other lanes; not touched here). Ruling R2: `fresh_after` names only paths present in both lists whose value changed; a new worktree is not "fresh" (its work is missing, so the daemon reads it first anyway). The pending retry is due on Tick only.
- git: Task 3: dispatched (sonnet high).
- git: Task 15 review (agy hydrogen high): Approved.
- git: Task 3: complete 84d3b0c2 (nearest_tests 12, branch_list_tests 22, red seen; engine suite green). Ruling R3 (implementer's reading where the spec is silent, accepted): a pick 0 ahead that is not a merge base (e.g. its own upstream) gets zero `lines` without a read; no candidates gives `base: null` with neither `pending` nor `unrelated`; a remote-only row is a base only when `<remote>/<name>` equals the base rev exactly; `merged_into` carries the rev (D4), the app shows it with `shown_base`; base rows carry no numbers (spec 7.2: bases are never classified).
- git: Task 4: dispatched (sonnet high).
- git: Task 3 review (agy hydrogen high): Approved.
- git: Task 4: complete e1087e1e (reader_real_tests 20, 6 reader hardening traps, the sandbox trap test; git:: 209; engine suite green). Accepted deviations: `remotes` reads `^remote\..*\.url$` so a remote named with a dot is kept; `commit_facts` adds `--no-show-signature` (no gpg). The implementer's "duplicated line in parse_left_right" does not hold (checked).
- git: Task 9: merged origin/agent-run-oss first; dispatched (sonnet high).
- git: Task 4 review (agy hydrogen high): Approved.
- git: Task 9: complete d1807546 (git_args_tests 11, red seen with todo! stubs; notchlingd lib 2102 pass, two timing tests failed under load and pass alone: codex_children a_rollout_written_after_start_is_found, outbox_verify a_real_clear_waiter_wakes_on_the_successor_file). `FRESH_READS` lives in git_args.rs; Task 8 uses it from there. `fresh_args` drops duplicates.
- git: Task 13: dispatched (sonnet high).
- git: Task 9 review (agy hydrogen high): Needs fixes, 1 finding, verified: `git_args_tests` call `std::os::unix::fs::symlink` with only `#[cfg(test)]`, so the Windows CI leg would not compile. Fixed by the coordinator (one line): the module is `#[cfg(all(test, unix))]`, as `reader_real_tests` is.
- git: Task 13: complete 37e04b67 (worktrees 31, changes_target 5, git 25; app lib 1613; gen.py --check ok). Accepted: `GUARD-LOCALE-SUBSET` needs a select to be the whole value, so `chat-git-default-and` and `chat-git-more-worktrees` wrap the whole text in the select (ids and variables as planned). `SubPart` joins with one space, `Faint("·")` separators, needs input is a `Running` string; occupancy kind priority Running, then NeedsInput, then Idle.
- git: Task 8: dispatched (sonnet high).
- git: Task 13 review (agy hydrogen high; the first run failed to start, the retry ran): Approved. Coordinator finding: with more than 6 other worktrees and no branch without a worktree, the "›" row is left out, so the hidden worktrees are counted nowhere (B1 says the rest are counted in the "›" row). Ruling R4: then the row reads "M more worktrees" alone (a new word), and still opens Branches; a fix round follows Task 8.
- git: Task 8: complete 682b0105 (git_cache_tests 19; notchlingd all integration targets pass, lib's two known timing tests flaked under load and pass alone). Noted for Task 10: `Budget::new(start, duration)`; `pick_reads` does not filter `fresh` against `eligible`, so the op passes only non-current, non-missing fresh paths. `SLOW_EVERY_MS`/`PENDING_AGAIN_MS` are also defined in git_cache.rs (app values); harmless.
- git: Task 13 fix round (R4): dispatched (sonnet high).
- git: Task 8 review (agy hydrogen high): Approved.
- git: Task 13 fix round: f168556b (R4; new word chat-git-more-worktrees-only; worktrees 32 pass). Approved by the coordinator (one-function change, test red then green).
