# Git branches: every worktree in Details, every branch in a panel

- **Date:** 2026-11-17
- **Status:** Draft for the maintainer's review. Nothing here is built. The layout is decided (below); the calls only the maintainer can make are in [Needs the maintainer](#needs-the-maintainer), each with a recommendation.
- **Source:** the mockup `2026-11-17-git-branches-mockup.html` (beside this file), options A, B and C. The maintainer picked **C + B** on 2026-11-17: the Details Panel's Git section becomes numbers only, one line per branch, and a new Branches panel opens where Changes opens. Read-only in v1.
- **Base:** `origin/agent-run-oss` 02448278.
- **Touches:** decisions 0004 (the daemon runs `git`, never the app), 0007 (tiers; the local API is additive), 0018 (Chat's state per area). No decision changes, so no new record.
- **Zones:** the app's `crates/app/src/chat/` files are `auto`; the engine's `git/` and the daemon's `git_ops.rs` are protected. The maintainer merges.

## In short

1. The Details Panel's Git section shows one line per branch and no file list. First the session's own branch, marked "this session". Then one row per other worktree of the same repository. Then one row, "main and 39 branches ›".
2. Clicking a branch row opens the existing Changes panel for that branch's worktree. Clicking "›" opens a new Branches panel in the same slot.
3. The Branches panel has three tabs (Working, Not Merged, Merged), a filter, and two groups (Worktrees, No Worktree). Each row shows numbers, age, the last commit's subject, its session and status. Its buttons only go somewhere: Changes, Open Session, Show in Finder.
4. Every branch is compared with its **nearest base**: the one of a few candidates (the repository's default, the branch the main checkout is on, the branch's own upstream) it has the fewest commits on top of. In this repository a lane is 12 commits on top of `agent-run-oss` but 1,493 on top of `main`, so a single default base would make every number useless.
5. **Merged** means the branch's commits are all in the branch the main checkout is on, or in the default branch. Those two are bases and never classified themselves. A worktree with uncommitted work is never Merged.
6. Two new app-tier read ops, `git.worktrees` and `git.branch_list`, and optional arguments on three existing ones (`worktree`, `ref`, `pick`). Additive only. `APP_QUIET`. All `git` work stays in the daemon, sandboxed, off the UI thread.
7. Nothing runs `git fetch`. Remote branches come from the local remote-tracking refs, and the panel says when they were last fetched.
8. Which session works in a worktree is matched in the app, from its live session list and the selected session's subagents, so "running" is never older than the session list.
9. Cost is bounded: a `git.worktrees` call reads at most one other worktree's working tree in steady state, numbers between two commits are cached forever, and nothing is asked while the panels are hidden.
10. The phone gets nothing in v1. `agentconsole.v1` does not change.

## Builder's calls

Each is marked *Builder's call* again in its section.

| # | Call | Section |
| --- | --- | --- |
| B1 | Other worktree rows in Details: running ones first, then by path; at most 6, the rest counted in the "›" row | 7.1 |
| B2 | The section title: "Git" with one worktree (running or not); else `counted_title(Git, running others, worktrees)`: "Git · 2 running" or "Git · 3". `counted_title` alone would print "Git · 1" | 7.1 |
| B3 | The working mark is the Sidebar's working dot; "this session" is a pill like Changes' "Large diff" badge | 7.1 |
| B4 | In the Branches panel a click on a row opens its button line (one row open at a time); no hover-only buttons | 7.2 |
| B5 | A remote-only row says "GitHub only" when its remote's host is `github.com`, else "`<remote>` only"; the URL never leaves the daemon | 7.2, 7.8 |
| B6 | The filter matches every word anywhere in the branch name, worktree path, session name or subject, any case (`branch::matches`) | 7.2 |
| B7 | Selecting another session keeps Branches open and reloads it for that session's repository; it closes when the new session is in no repository | 7.2 |
| B8 | A branch name `clean_ref` refuses is listed but its Changes button is off | 7.2 |
| B9 | Line counts are printed as today (`+1240`), no grouping | 7.1 |
| B10 | Ahead and behind come from `for-each-ref %(ahead-behind:<base>)`, one process per candidate; if git refuses the atom (older than 2.41), `rev-list --left-right --count` per branch, cached | 7.5 |
| B11 | Caps: 50 worktrees, 300 refs, subject 120 characters; the numbers cache keeps 2,000 entries (least recently used); a repository not asked for 10 minutes is dropped | 7.9 |
| B12 | Work reads per `git.worktrees` call: missing ones first (up to 4), else the stalest one older than 15 s, plus up to 2 the app names in `fresh`; no new read starts after 1.5 s in a call | 7.9 |
| B13 | `git.branch_list` computes missing line counts for 1 s per call at most; the rest come back `pending` and the panel asks again after 2 s | 7.9 |
| B14 | A session's folder is its job's `worktreePath` when set, else its `cwd`; the longest worktree path that contains it wins | 7.7 |
| B15 | Notchling's own worktrees (the Machines merge worktrees) are listed like any other | 7.11 |
| B16 | File and type names (`git/worktrees.rs`, `chat/worktrees.rs`, `view/branches_panel.rs` and so on) and message ids | 11 |
| B17 | The Git section follows a session into its worktree: its folder for every `git.*` call is `worktreePath` when set, else `cwd`, as Chat already follows its transcript there | 7.1, 7.10 |
| B18 | Nearest base ties: fewest commits ahead, then fewest behind, then candidate order | 7.5 |
| B19 | The default and the integration branch are bases: never in Not Merged or Merged; without a worktree they show in Working's No Worktree group (as the mockup's `main · local · even with origin/main`) | 7.2, 7.6 |
| B20 | A worktree whose work was never read stays out of Merged until a read says it is clean; `git.branch_list` reads missing work within its budget | 7.6, 7.9 |
| B21 | A `ref` stat reply: `branch` is the ref's name without `refs/heads/` or `refs/remotes/`, `detached: false`, `head` its 7-character commit, `includes_uncommitted: false` | 7.4 |
| B22 | Changes' base menu for a `ref` target asks `git.branches` in `cwd` and leaves out the target's own name | 7.3 |
| B23 | Changes offers an `.html` page only for a new, uncommitted file of the session's own worktree | 7.1 |
| B24 | Two gates of `Gate`'s shape, one for `git.worktrees` and one for `git.branch_list`, so opening Branches never waits on a Details call | 7.9 |
| B25 | `git.branch_list` rows: `name` without the remote (`agent/ask`), `remote` the remote's name or `null`, `ref` the full ref | 7.4 |

## Needs the maintainer

Most important first. Each has a recommendation; the design below follows it.

1. **"Running" for lanes run as subagents.** Your lanes are mostly subagents of the main chat, not sessions. v1 matches sessions (by folder) and the selected session's running subagents (by their folder: a Claude Code subagent's newest transcript line's `cwd`, a Codex child's start `cwd`, a delegated job's folder; 7.7). A subagent that works through absolute paths while its `cwd` stays in the main checkout (as this one did) cannot be matched by anything Notchling reads; its row still shows "last 1 m ago" from file times. *Recommendation:* ship that, and ask lanes to start in their worktree (`isolation: "worktree"` or `cd` before starting) if you want the dot.
2. **What the right-hand numbers mean.** As approved, the current row's right side is its uncommitted work (+52 −9) with "vs main +501 −91" in its sub-line, while the other rows' right side is vs their base. One column then means two things. *Recommendation:* make the right side "vs base" on every row and move the uncommitted part into the current row's sub-line: `5 uncommitted +52 −9 · last 2 m ago · even with origin · vs main`. The spec is written as approved until you say.
3. **Which branch "merged" is measured against.** v1: merged into the branch the main checkout is on (here `agent-run-oss`) or into the default (`origin/main`). Only 2 of 44 remote branches are merged into `main`, 37 into `agent-run-oss`. If the main checkout is switched to a lane for a while, the tabs follow it. *Recommendation:* keep it automatic in v1; add a per-repository "Integration branch" choice later if it bites.
4. **The nearest base also becomes Changes' default.** Today a session in a lane compares with `origin/main` (1,493 commits, the 2,000-file cap). With this design it compares with `agent-run-oss` unless you pick another base in the menu. *Recommendation:* yes.
5. **Changes for a branch with no worktree** (committed work only, against its base). It needs the new `ref` argument on two ops. *Recommendation:* keep in v1: it is how you look inside a Not Merged branch.
6. **Actions, later.** v1 is read-only: no merge, delete, push, checkout, worktree add, remove or prune. Which first? *Recommendation:* "Remove Worktree" for a clean, merged worktree (the Merged tab's Worktrees group is exactly that list), behind the `acting` switch and a confirm.
7. **The phone.** *Recommendation:* not in v1. The bridge would need new ops and the phone a new screen; nothing here needs it.

## 1. Why

You run several lanes at once, each in its own git worktree. The Details Panel's Git section shows only the selected session's folder: its branch, its tracking line, up to 40 uncommitted files and one Changes row against the base. To see how the other lanes are doing you open each session, or a terminal. To see what is not merged yet you run `git` by hand.

The fix: Details shows every worktree as one short line of numbers, and a Branches panel shows every branch, grouped and filtered. Files stay in the Changes panel, which already does them well.

## 2. Goals, non-goals, anti-goals

**Goals**

- See every worktree of the session's repository at a glance: its branch, how big its work is, whether a session works there now.
- See every branch of the repository: which have a worktree, which are not merged, which are only on GitHub.
- Get from a branch to its changes, its session or its folder in one click.
- Cost nothing while the panels are hidden, and little while they show.

**Non-goals (v1)**

- Any action on a branch or worktree: merge, delete, push, pull, checkout, add, remove, prune. The buttons only go somewhere.
- Fetching. The view is as fresh as the last fetch you or an agent ran.
- Other repositories than the selected session's.
- The phone, and remote cluster rows (they already have no Git section).
- Per-file lists in Details. They are in Changes.

**Anti-goals** (failures even if everything works)

- A `git` process started from a draw or a model compute, or any file read there.
- Any `git` subcommand that writes, fetches or reaches the network. Any option passed through from a caller.
- A worktree path or ref the caller names that `git` does not list for that repository being read.
- Hover, sizes or layout of any other part changing (the Sidebar, the conversation, the Details Panel's other sections, Changes' own rows).
- A remote's URL, or anything in it, reaching the app or a log.

## 3. Constraints

- **The app runs no process and reads no repository.** Every `git` call is a daemon op, called from `ChatView` off the UI thread. Its answer is pushed into a model as an `Arc`; an unchanged one is dropped before any part hears of it (as `view/repo.rs` does today).
- **The daemon's guard stays.** `cwd` must be a known session's folder or below it. Every new argument is checked against what `git` itself lists (section 7.10).
- **Sandboxed, read only.** Every new command goes through the same `SandboxedLauncher`, with `--no-optional-locks`, the nulled `-c` options, the filter drivers blanked, a 3 s timeout and capped output.
- **No fetch, ever.** No `fetch`, `pull`, `push`, `remote update`, `ls-remote`, `gc`, `worktree add|remove|prune|repair`. A test holds the list of subcommands the git ops may run.
- **Additive.** New ops, new optional arguments, new optional reply fields. `git.diff` stays even though Details no longer calls it.
- **Words** in `locales/en/mac/chat.ftl`; a message with variables is formatted in a model compute, never in a draw. No comments in code.
- **Cost.** The maintainer's repository today: 4 worktrees, 5 local branches, 44 remote-tracking refs. A `git status` costs 40 ms in a lane and 220 ms in the main checkout; other reads 8 to 20 ms each (appendix). Status is the expensive part, so other worktrees' working trees are read rarely and one at a time.
- **Cadence.** Today's Gate asks `git.status` (and the branch stat) every 5 s while Details or Changes shows. Worktrees and branches change more slowly and cost more, so they are asked every 15 s and on events (section 7.9).

## 4. Approaches

- **A. All in Details.** The current branch with its 3 newest files, the other worktrees, a "more" row. Always visible, but the section grows with every lane and mixes two kinds of row. Rejected.
- **C. Numbers only.** One line per branch, no files; files open in Changes. The shortest. It cannot show 44 branches.
- **B. A Branches panel.** Room for tabs, a filter, groups and buttons, in the slot Changes already uses.
- **Picked: C + B.** C keeps Details short and always useful. B answers "what is not merged" and "what is only on GitHub" without crowding Details. Both reuse Changes for files, so there is one place for diffs.

## 5. What exists today

- `chat/view/repo_section.rs` draws the Git section: a branch row, a tracking line, up to `SHOWN_FILES` (40) file rows (a click opens the file's diff in the file panel, and a new `.html` file also offers its page), a "more files" line, and the Changes row (`+98 −0 vs <base>`).
- `chat/git.rs::Gate` asks `git.status` then `git.branch_diff_stat` in one background task: at once when the panel opens or the folder changes, after a turn ends (1 s floor), and every 5 s on the tick, only while Details or Changes is on screen.
- `chat/view/changes.rs` and `changes_panel.rs`: the Changes panel takes the Details Panel's place at the file panel's width; the file panel opens over it and closing the file panel shows Changes again. Its base menu asks `git.branches`.
- `notchlingd/src/git_ops.rs` serves five `git.*` ops over `notchling_engine::git`. The default base is `origin/HEAD`'s target, else `main`, else `master`.
- `engine/src/control/git.rs` has an older branch parser (`BRANCH_FORMAT` with `%(worktreepath)`) that the bridge's extras use. This design does not reuse it; its dedup rule (a remote ref whose name matches a local branch is dropped) is the same.
- Job state has `worktreePath` when a session ran EnterWorktree, but `Agent` does not carry it; only the transcript lookup uses it.

## 6. Terms

- **Main checkout:** the repository's main worktree (`git worktree list`'s first entry), unless it is bare.
- **Worktree:** any entry of `git worktree list` with a working tree, main checkout included.
- **Current worktree:** the one that contains the selected session's folder (`worktreePath` when set, else `cwd`; B17). With nested paths (lanes under `.claude/worktrees/` inside the main checkout) the longest path wins.
- **Default:** `origin/HEAD`'s target, else `main`, else `master` (today's rule). It is often a remote-tracking ref (`origin/main`). Wherever it is shown or compared with a local branch, its **short name** is used: the name without `<remote>/` (`main`).
- **Integration branch:** the branch the main checkout is on, when its name is not the default's short name.
- **Bases:** the default and the integration branch (B19).
- **Nearest base:** section 7.5.
- **Work:** a worktree's whole change against its base, uncommitted included: what `git.branch_diff_stat` counts.

## 7. The design

### 7.1 The Details Panel's Git section

Shown when `git.status` says the folder is in a repository, as today. Rows, top to bottom:

1. **This session's branch.** Branch icon, the branch name (or "Detached at abc1234"), a "this session" pill, and on the right the uncommitted totals (`+52 −9`, the sum of `git.status`'s rows). Sub-line: `5 files changed · last 2 m ago · even with origin · vs main +501 −91`.
   - "5 files changed": `git.status`'s change count ("500+" when `truncated`).
   - "last 2 m ago": the newer of the head commit's time (`committed_at` of the current entry in `git.worktrees`) and the newest change time of the changed files (`changed_at`, a new field on `git.status`). Left out when neither is known.
   - "even with origin": the upstream line, short. "2 to push", "3 behind", "2 to push, 3 behind", "no upstream". The remote name alone when the upstream has the branch's own name, else the upstream (`origin/agent-run-oss`).
   - "vs main +501 −91": the branch stat, as the old Changes row showed it. Its base is the nearest base unless a base was picked in Changes.
   - (See question 2: the recommended swap of the two pairs of numbers.)
2. **One row per other worktree.** Branch icon, the branch name, on the right `+N −M` of its work against its base. Sub-line: `23 files · 2 ahead`, then `· running 41 m` with the working dot when a session or subagent works there (7.7), else `· last 6 m ago`. "2 ahead" counts commits on top of its base; "2 ahead, 1 behind" when the base moved on. A worktree whose work was never read yet shows its branch and "2 ahead" with no line counts. A missing folder (prunable) shows "folder missing", faint, and does nothing on click.
3. **The "›" row.** `main and 39 branches ›`: the default's short name and the count of the other branches with no worktree (local and remote-only, after the dedup in 7.2). "39 other branches ›" when a worktree has the default's short name checked out. With more than 6 other worktrees: `4 more worktrees · main and 39 branches ›`. Left out when the repository has no other branch.

*Builder's call (B1):* other worktree rows are running ones first, then by path, so rows do not jump while agents work; at most 6. *Builder's call (B2):* the title, as in the table. *Builder's call (B3, B9):* inks and number format as stated in the table. *Builder's call (B17):* a session that ran EnterWorktree is "this session" in its worktree, and its Git section, `git.status` and Changes read there; otherwise row 1 would show the main checkout while row 2 showed the same session running in the lane.

Clicks: the current row, or any worktree row, opens Changes for that worktree (7.3). The "›" row opens Branches. Rows keep the look of today's Changes row: `mx(-4) px(4) py(2)`, `RADIUS_SM`, Raised wash at 0.6 on hover, `MARK_BOX` icon column, `ROW_LINE` line height; a two-line row is laid out like today's file rows (name line, faint second line). The file rows, the "more files" line and the Changes row go away. Nothing else in the Details Panel changes.

The `.html` page offer that today's file rows have moves to Changes: its "Open in File Panel" (`open_branch_diff`) on a new, uncommitted `.html` file of the session's own worktree also offers the page, as `open_git_row` did (*Builder's call B23*). `offer_page` looks the file up in `git_for_details()` today; it must take the Changes row's file instead.

### 7.2 The Branches panel

Takes the slot Changes takes: a window-drawn panel at the file panel's width, with its drag edge, `Page Panel` slot and part tag "Branches Panel" (`.window_region` before the tag). Its own entity, drawn as a cached part, with its model pushed in by `ChatView`, like `ChangesPanel`.

- **Header** (36 pt): branch icon, "Branches", a pill with the repository's name (the main checkout's folder name), and Close. Under it, faint: "Remote branches as of 2 h ago" (last fetch, 7.8), or "Never fetched".
- **Tabs:** Working (n), Not Merged (n), Merged (n). They are views, not a partition:
  - **Working:** every worktree, and the bases that have no worktree (B19).
  - **Not Merged:** every branch whose commits are not all in a base, plus every worktree with uncommitted work. Never a base.
  - **Merged:** every branch whose commits are all in a base, without a worktree or with a clean one (B20). Never a base. A merged, clean worktree is a finished lane you could remove.
- **Filter:** a single-line `TextEditor` as in Changes. *Builder's call (B6).*
- **Groups:** "Worktrees" and "No Worktree", in each tab that has rows for them. Worktrees: this session's first, then running, then by path. No Worktree: newest commit first.
- **A row:** branch icon (a cloud icon for remote-only), the name, right side `+N −M` against its base (empty for a merged branch, "…" while pending). Sub-line, each part only when known:
  - where: "main checkout", the worktree path relative to the main checkout (`.claude/worktrees/lane-ask`) or absolute when outside it, "local", or "GitHub only" (*Builder's call B5*);
  - the session: its name in quotes and "running" or its state;
  - "last commit 6 m ago “ask: store and wait”";
  - push state: "even with origin", "2 to push", "upstream gone";
  - "not merged" pill, or "merged into agent-run-oss";
  - files and commits: "23 files · 2 ahead".
- **Button line** (*Builder's call B4*): a click on a row opens it, one at a time. Buttons: **Changes** (any branch; off for a name `clean_ref` refuses, B8, and for a missing folder), **Open Session** (only with a matched session or subagent: selects it as a Search hit does, or opens the subagent as its Details row does), **Show in Finder** (only a worktree whose folder exists: `cx.reveal_path`).
- **Changes from Branches:** Changes replaces Branches in the slot. Closing Changes shows Branches again, as closing the file panel shows Changes again. Closing Branches shows Details.
- **Session switch** (*Builder's call B7*): Branches reloads for the new session's repository and closes if it has none. The open row and the filter clear.

Commit subjects and remote branch names are other people's text. They are plain labels: one line, control characters dropped, cut at 120 characters, never markdown, never a link.

### 7.3 Changes for another worktree or branch

Changes gets a **target**: the session's own folder (today), another worktree (`worktree: <path>`), or a branch with no worktree (`ref: refs/heads/x` or `refs/remotes/origin/x`).

- **Two states, not one.** Today `repo::State::branch` and `base_choice` feed both Changes and the Details row. The session's own `BranchModel`, `Summary` and `base_choice` stay where they are and keep feeding Details. A target other than the session's own has its own `BranchModel` and `base_choice`, used only by Changes, and cleared when Changes closes or the selected folder changes. Otherwise closing Changes would show another branch's numbers on "this session".
- The header shows the target's branch → base. The base menu lists that target's branches: `git.branches` with the same `worktree`, or in `cwd` for a `ref` target, leaving out the target's own name (*B22*).
- For a worktree target, the stat covers its work, uncommitted included, as today. For a `ref` target, committed work only: the counts line says no uncommitted files and the reply has `includes_uncommitted: false`.
- The Gate's background task asks the stat for the target while Changes shows it, at the Gate's cadence. `git.status` for the session folder is still asked while Details shows.
- `details_visible()` also needs Branches closed. Hiding the Details Panel closes the page, then Changes (back to Branches if it came from there), then Branches, in that order.
- An opened file's patch and the file panel's refresh pass the same target (`diff_via` gains it), so `git.branch_diff_file` reads the right tree.
- A target the daemon refuses (a worktree removed meanwhile, a branch deleted) shows Changes' "Nothing to compare." and the reason, and the next Gate round goes back to the session's own folder.

### 7.4 The daemon ops

All app tier, read only, no switch, `OpSpec::APP_QUIET` (asked on a timer; failures and refusals still audited). Shapes go into `docs/local-api-ops.md`.

**New: `git.worktrees {cwd, fresh?: [path]}`** — for Details.

```
{repo: false}
| {repo: true, name, default, integration, fetched_at, no_worktree, truncated,
   worktrees: [{path, real, main, current, branch, head, detached, locked, missing,
                upstream, upstream_ahead, upstream_behind, upstream_gone,
                base, ahead, behind, committed_at,
                work: {added, removed, files, uncommitted_files, changed_at, read_at} | null}]}
```

- `name`: the main checkout's folder name. `default`, `integration`: as in section 6, `null` when none. `fetched_at`: 7.8. `no_worktree`: the count for the "›" row.
- `path` as `git` lists it, `real` canonical. `current`: contains `cwd` (longest path). `missing`: `git` says prunable. A bare main entry is left out.
- `base`, `ahead`, `behind`: 7.5. `committed_at`: the head commit's time (ms).
- `work`: the last read of that worktree's work against `base` (as `git.branch_diff_stat` counts it), from the daemon's cache, with when it was read. `null` until read. Here the current worktree's `work` is always `null`: the app has it from `git.status` and the stat (`git.branch_list` does return it, from the cache that stat fills).
- Which worktrees get read in a call: B12.

**New: `git.branch_list {cwd}`** — for the Branches panel.

```
{repo: false}
| {repo: true, name, default, integration, fetched_at, truncated, pending,
   branches: [{name, ref, local, remote, remote_only, on_github, is_base,
               worktree: {path, real, main, current, missing, work} | null,
               head, detached, committed_at, subject,
               upstream, upstream_ahead, upstream_behind, upstream_gone,
               base, ahead, behind, merged_into,
               lines: {added, removed, files} | null, pending}]}
```

- One row per local branch, and per remote-tracking ref whose name (minus its remote) is no local branch's name; `origin/HEAD` is skipped. A detached worktree is a row too (`name` null, `detached`, `head` set). `name`, `remote`, `ref`: B25.
- `worktree`: set when the branch is checked out somewhere, with the same fields and `work` object as in `git.worktrees`, current worktree included, from the same cache. Missing work is read within the call's budget (B20); the panel needs no second call.
- `is_base`: the default or the integration branch (B19). `merged_into`: the base that contains it, or `null` (7.6). `lines`: committed work against `base`, `null` for a merged branch; `pending` when not computed yet (B13).
- `on_github`: the remote's host is `github.com` (B5).

Every `git.branch_diff_stat` that reads a worktree with its nearest base (the session's own, every 5 s) also stores its result as that worktree's `work` in the cache. So the current worktree's work is never read twice.

**Changed, additively:**

- `git.status`: new reply field `changed_at`, the newest modification time among its changed files that exist (one `symlink_metadata` each under the repository root, at most the 500 listed; the root costs one `rev-parse --show-toplevel`, run only when there are changes), `null` when none. `read_branch_stat` fills the same field for `work`.
- `git.branch_diff_stat`: new optional `worktree` (read that worktree instead of `cwd`'s), `ref` (committed only; reply fields as B21) and `pick: "nearest"` (use the nearest base when no `base` is given; the reply's `base` names it). Without them, nothing changes. With `ref`, `read_branch_stat` reads no working tree: no `status`, no untracked files; it diffs `<merge base> <ref> --`.
- `git.branch_diff_file`: new optional `worktree` and `ref`, as above. With `ref`, `path` must be one of the files the stat lists for that ref and base, and the patch is `diff <merge base> <ref> -- :(literal)<path>`; no untracked-file path.
- `git.branches`: new optional `worktree`.
- `sessions.list` rows: new optional `worktreePath` (the job state's), so the app can place a session that ran EnterWorktree (7.7).

New `git` subcommands, all reads, fixed in code: `worktree list --porcelain -z`, `rev-parse --path-format=absolute --git-common-dir`, `for-each-ref` with `--merged=<ref>` and a fixed `--format`, `rev-list --left-right --count`, `log -1 --format=%ct%x00%s <commit>` and `merge-base --is-ancestor` (both only for a detached head, which no `for-each-ref` lists), `diff --shortstat` with the same `DIFF_FLAGS` as today's diffs (`--no-color --no-ext-diff --no-textconv --ignore-submodules=all -M`) and a trailing `--`, `config -z --get-regexp ^remote\.[^.]*\.url$` (only to tell GitHub; the URL is parsed for its host in the daemon and dropped).

The two ops are registered like the others: constants in `local-api/src/wire.rs` (`ops`), `GIT_OPS` in `git_ops.rs`, and `HOMES` with `Home::Quiet` in `notchlingd/src/op_kinds.rs`.

### 7.5 The nearest base

A branch's base is one of these candidates, never the branch itself or a ref pointing at its own name:

1. the default;
2. the integration branch (the local branch, since merges land there first);
3. the branch's upstream, when the upstream has another name (a lane made with `git worktree add -b agent/x origin/agent-run-oss` tracks `origin/agent-run-oss` until it is pushed).

The base is the candidate with the fewest commits in `candidate..branch`; then the fewest behind; then the order above (*B18*). A branch made at `main` is 0 ahead of both `main` and `agent-run-oss` but 1,482 behind the second, so it gets `main`. A branch that shares no history with any candidate has no base: no numbers, "no common history".

`%(ahead-behind:<base>)` prints ahead then behind; `rev-list --left-right --count <base>...<head>` prints behind then ahead. The parsers keep the two apart.

Why: in this repository `agent/ask` is 12 commits on top of `agent-run-oss` and 1,493 on top of `main`; `agent-run-oss` itself is 1,482 on top of `main`. One base for all would make every lane look like the whole integration branch. The candidates are few and fixed, so the cost is one `for-each-ref %(ahead-behind:<candidate>)` per candidate for all branches at once (*Builder's call B10*). A detached head is no ref, so it takes `rev-list --left-right --count` per candidate. The choice is cached by (head commit, every candidate's commit), so while nothing moved the stat Changes asks every 5 s with `pick: "nearest"` adds only the one `for-each-ref` that reads those commits (about 15 ms).

The same rule picks Changes' default base (`pick: "nearest"`), so the Details row and Changes always agree. A base picked in Changes' menu still wins for that target (question 4).

### 7.6 Merged

A branch is merged when its head is in the integration branch or in the default (`for-each-ref --merged=<each>`: two processes for all branches; `merge-base --is-ancestor` for a detached head). `merged_into` names the first that contains it. Rules on top:

- `--merged=X` lists X itself. A branch is never merged into itself or into a ref with its own short name (`agent-run-oss` is not "merged into `origin/agent-run-oss`").
- The bases are never classified (B19). Without that, `main` would read "merged into agent-run-oss" and `agent-run-oss` "not merged, +1,482".
- A worktree with uncommitted work is never Merged; one whose work was never read waits for a read (B20).
- A branch with no commits of its own (a lane just made) is merged by this rule; while it has a worktree it shows under Working, and under Not Merged once it has uncommitted work.

Why both: lanes merge into the integration branch (37 of 44 remote branches here), and the integration branch merges into `main` now and then (2 of 44). Measured against `main` alone, almost everything would read Not Merged. Measured against the selected session's branch, the answer would change with every session you select. The main checkout's branch is the same for every session in the repository.

### 7.7 Sessions in worktrees, and "running"

Matched in the app, in the model compute, with no I/O:

- **Sessions:** every row of the app's session list (Claude Code, Codex, Antigravity; not remote rows). Its folder is `worktreePath` when set, else `cwd` (*Builder's call B14*).
- **Subagents:** the selected session's running subagents. Running subagents are always on the first page, so they are read. Neither `Subagent` nor `SubagentRow` has a folder or a start time today; both gain `cwd` and `started_ms`, from each kind's own source:
  - a Claude Code subagent: the `cwd` of the newest line, kept by the glance that already reads its tail (`Peeked`);
  - a Codex child: its `session_meta`'s `cwd`, as `codex::meta` reads it;
  - a delegated job: its record's `folder`.
- A folder belongs to the worktree whose `path` or `real` is the longest component-wise prefix of it.
- **Running:** a matched session whose group is Working, for `now − since_ms` ("running 41 m"); or a matched running subagent, for `now − started_ms`. Needs Input shows "needs input" instead. Several matches: the one running longest is named, the count follows ("+1").
- Membership comes from the daemon's paths; state comes from the live session list, so the dot changes as soon as the Sidebar does.

Known limit (question 1): a subagent whose `cwd` stays in the main checkout while it edits a worktree through absolute paths cannot be placed. Its worktree still shows "last 1 m ago".

### 7.8 Remote branches without fetching

Nothing runs `fetch`. Remote branches are the local `refs/remotes/*` as they stand. They change when you or an agent fetch, pull or push. `fetched_at` is the modification time of `<common dir>/FETCH_HEAD` (one `stat`), `null` when it does not exist; the panel says "Remote branches as of 2 h ago" or "Never fetched". A push updates its own remote ref without touching `FETCH_HEAD`, which is fine: that ref is then fresh.

### 7.9 Caching and cadence

**Daemon caches** (in `GitOps`, keyed by the repository's canonical common dir):

- The worktree list: 5 s, also used by the `worktree` argument's check.
- **Numbers between two commits** (`ahead`, `behind`, `lines`) keyed by (base commit, head commit): commits never change, so an entry is right forever. 2,000 entries, least recently used (*B11*).
- **Work** per worktree path: the last `read_branch_stat` result with `read_at`. Read only by B12's rule: missing ones first (up to 4 a call), else the stalest one older than 15 s, plus up to 2 the app names in `fresh`; no read starts after 1.5 s in one call. With 3 other worktrees, each is read about every 45 s in steady state, and at once when its session ends a turn.
- A repository not asked for 10 minutes is dropped.
- The cache lock covers the maps only, never a `git` run. A work read in flight is marked, so a second call (Details and Branches at once) skips it and returns the last result.
- The app waits 5 s for a call (`CALL_TIMEOUT`). The 1.5 s stop bounds when reads start, not a read already running (each `git` run has its own 3 s timeout), so a slow call can still time out in the app; its reads finish and fill the cache, and the next call returns them. The app shows the last good reply meanwhile.
- Each app call opens its own connection (`actions.rs::Socket`), so a slow `git.branch_list` holds up no other call.

**App cadence:**

| What | Asked | While |
| --- | --- | --- |
| `git.status` + stat (session or Changes target) | as today (Gate: open, folder change, turn end 1 s floor, tick 5 s) | Details or Changes shows |
| `git.worktrees` | panel opens or folder changes at once; turn end or a matched session's state change (1 s floor, that worktree in `fresh`); tick every 15 s | Details shows |
| `git.branch_list` | panel opens at once; every 15 s; after 2 s while `pending > 0` | Branches shows |

Two more gates of `Gate`'s shape, one per op, hold the 15 s cadence and one call in flight each (*B24*), so opening Branches never waits on a Details call. Hidden panels, a fixture root and a render root make no call. Ages ("41 m", "2 m ago") are recomputed only when the minute changes, so a running clock never redraws the part.

**Cost per call on this Mac** (appendix): `git.worktrees` ≈ 60 ms without a work read, ≤ 0.4 s with one; `git.branch_list` ≈ 80 ms plus 20 ms per new line count (6 Not Merged branches here). Every 15 s while visible.

### 7.10 Guard and safety

- `cwd` is checked as today and `git` runs in its canonical path. The known folders (`live_folders`) gain each session row's `worktreePath`, since the Git section now asks there (B17).
- `worktree` must be absolute, have no `..`, and canonicalize to exactly one of the non-bare, non-missing paths `git worktree list` gives for `cwd`'s repository. `git` then runs in that canonical path. A worktree is the user's own checkout of the same repository; the reads are sandboxed either way.
- `ref` must pass `clean_ref` (so it never starts with `-`), start with `refs/heads/` or `refs/remotes/`, and be one of the refs `for-each-ref` lists. It is passed as a revision, before `--` (after `--` git reads paths), and only to `merge-base` and `diff`.
- `fresh` paths are matched the same way as `worktree`; an unknown one is ignored, not refused.
- The URL from `remote.*.url` is parsed for its host inside the daemon. It is never in a reply, an error or the Audit.
- The `crates/notchlingd/AGENTS.md` rule stands: no way to run any other subcommand or to pass an option through.

### 7.11 Failures and edge cases

- **Not a repository:** `{repo: false}`; no Git section, as today. `git.worktrees` is asked only after `git.status` said `repo: true`.
- **A failure** (timeout, `can't read this repo safely`, refused): Details keeps the last good rows; with none, it shows the current row only, without noise. Branches shows "Can't read the branches." and the reason.
- **No other worktrees:** Details shows the current row and the "›" row. Branches' Working tab shows the main checkout and "No other worktrees."
- **Detached HEAD:** "Detached at abc1234", nearest base by its commit, no upstream; a row in Worktrees.
- **Bare main repository:** no main checkout row, no integration branch; linked worktrees as usual.
- **No commit yet:** the branch name and uncommitted count; no base, no numbers.
- **Missing worktree folder** (prunable): "folder missing", no numbers, Changes and Show in Finder off.
- **Very many worktrees:** 50 listed (`truncated`), 6 in Details, all in Branches; work reads stay bounded by B12.
- **Very many refs:** 300 (`truncated`, "Not all branches are shown.").
- **Nested worktrees:** a lane under `.claude/worktrees/` is inside the main checkout's path; the longest path wins for sessions and for `current`.
- **Machines merge worktrees:** listed like any other (*B15*).
- **Remote cluster rows:** no Git section, as today (`remote_refused`).

## 8. UI states

| State | Details Git section | Branches panel |
| --- | --- | --- |
| Loading (first ask) | current row from `git.status`; no other rows, no "›" row yet | header, tabs without counts, "Reading the branches…" |
| Ready | as 7.1 | as 7.2 |
| Numbers pending | worktree row with "2 ahead", no line counts | "…" where the numbers go |
| Error, nothing shown before | current row only | "Can't read the branches." + reason |
| Error after a good reply | last good rows stay | last good rows stay |
| One worktree | current row, "›" row | Working: main checkout, "No other worktrees." |
| Empty tab | — | "No branches here." |
| No filter match | — | "No branches match." |
| Detached HEAD | "Detached at abc1234" | row with `head`, "detached" |
| Bare repository | linked worktrees only | no main checkout row |
| Not a repository | no section | closes |

## 9. The phone

Not in v1 (question 7). Nothing in `protocol/` changes. `sessions.list`'s new `worktreePath` is on the local API only; the bridge maps `Agent` to its own `Session` and leaves it out.

## 10. Tests that will prove it

**Engine, pure** (`git/*_tests.rs`, fixture text):

- `worktree list --porcelain -z`: main, linked, bare, detached, locked, prunable, a path with spaces and a newline.
- `for-each-ref` records: upstream track (`ahead 2, behind 1`, `gone`, none), `%(ahead-behind)` output, the remote-ref dedup, `origin/HEAD` skipped, the 300 cap.
- `diff --shortstat` lines under `LC_ALL=C`: insertions only, deletions only, both, one file, binary only.
- Nearest base: fewest ahead wins, then fewest behind, then order; a branch made at `main`; the branch never its own base; an upstream with its own name not a candidate; `origin/main` and `main` compared by short name; no common history.
- `rev-list --left-right --count` and `%(ahead-behind)` give the same ahead and behind for the same pair.
- Merged: into either; never into itself or its own short name; bases never classified; the uncommitted-work and never-read rules; the tab views from one list.
- B12's choice of work reads with a fake clock: missing first, then stalest, `fresh`, the 1.5 s stop.

**Engine, real temp repositories** (`branch_real_tests.rs` style, no network): a main checkout and two linked worktrees, one detached, one with untracked files, a remote-only ref made with `update-ref`, a merged branch. Check every number against `git`'s own. `read_hardening_tests.rs` and `sandbox_tests.rs` gain the new commands: the trap per repository program (filter, hook, fsmonitor, textconv) never fires.

**Daemon** (`git_ops_worktrees_tests.rs`, `FakeLauncher` recording each command):

- A `cwd` that is no session folder is refused; a `worktree` not in the list, with `..`, a link to elsewhere, or missing is refused; an unknown `fresh` path is ignored.
- A `ref` outside `refs/heads/` and `refs/remotes/`, failing `clean_ref` or not listed is refused; an accepted one is always before `--` in the recorded command; a `ref` stat runs no `status`.
- `diff --shortstat` carries every `DIFF_FLAGS` option (the textconv and external-diff traps in `read_hardening_tests.rs` stay silent).
- A detached worktree gets its row from `log -1` and `merge-base --is-ancestor`.
- A session's `worktreePath` is a known folder.
- A stat of the session's own worktree fills the work cache: the next `git.branch_list` reads no `status` for it.
- **No fetch:** every command any git op runs is in a fixed allowlist of subcommands; `fetch`, `pull`, `push`, `remote`, `ls-remote`, `gc` and `worktree add|remove|prune|repair` never appear.
- The numbers cache: a second `git.branch_list` with the same heads runs no `diff` and no `rev-list`.
- A second `git.worktrees` inside 15 s runs no `status`.
- The remote URL is in no reply and no audit line (a URL with a user and a token in the fixture).
- The quiet-op test classifies the two new ops as quiet.
- `local-api/src/server_audit_tests.rs`: a quiet success leaves no audit line, a refusal does.

**App, pure** (`chat/worktrees_tests.rs`, `chat/branch_list_tests.rs`):

- Replies to models: the current row's texts from `git.status` + stat + `git.worktrees`, every sub-line part present and absent, "500+", the "›" row's text in its three forms.
- Session matching: `worktreePath` over `cwd`, longest path wins, `real` and `path`, each subagent kind's `cwd`, two matches, needs input.
- Row order (B1), the 6 cap, the title with one worktree ("Git"), tab counts, groups, bases in Working, filter.
- The two new gates: 15 s tick, events with the 1 s floor, one in flight each, nothing while hidden or under a fixture root.
- Changes on another target and back: the Details row keeps the session's own numbers and base the whole time.
- An unchanged reply makes no new `Arc` and no redraw.

**App, renders and benches:**

- `--render chat-git` shows the new section over made-up data; `NOTCHLING_RENDER_BRANCHES=working|not-merged|merged|open-row|loading|error|empty` shows the panel.
- A render of Details before and after with the same session shows no change outside the Git section (sizes and positions of the other sections' rows).
- `bench_chat.rs`: the map changes no part's count; a session list change redraws the Git section only when a matched worktree's dot or text changed.
- `crates/app/tests/subagent_reads.rs`-style check: building the Details model and the Branches model reads no file and starts no process.

## 11. Left to the builder

- File, type and message names (B16). Suggested: engine `git/worktrees.rs`, `git/branch_list.rs`, `git/nearest.rs`; daemon arms in `git_ops.rs`; app `chat/worktrees.rs`, `chat/branch_list.rs`, `chat/view/branches.rs`, `chat/view/branches_panel.rs`.
- The kit pieces for tabs (an existing segmented control if the kit has one, else Ghost buttons with a selected state) and the cloud icon (a vendored Lucide SVG).
- The words. At least: this session, files changed (plural), last … ago, even with / to push / behind / no upstream, ahead / behind (plural), running …, needs input, folder missing, "› row" (three forms, plural), Branches, Working, Not Merged, Merged, Filter Branches, Worktrees, No Worktree, main checkout, local, GitHub only, `<remote>` only, last commit … ago, merged into …, not merged, Changes, Open Session, Show in Finder (reuse `chat-show-in-finder`), Remote branches as of …, Never fetched, Reading the branches…, Can't read the branches., No other worktrees., No branches here., No branches match., Not all branches are shown., no common history.
- Notes, in the same change: `docs/notes/chat-details.md` (Changes gains targets; a Branches entry), `docs/local-api-ops.md` (`git_ops.rs` section), `docs/feature-map.md` (the two Git rows), `crates/app/AGENTS.md` and `crates/notchlingd/AGENTS.md` (the why of `WorktreeGate`, the caches, the guard for `worktree` and `ref`). Notes changes wait for the maintainer.
- Registering the two ops (`wire.rs`, `GIT_OPS`, `op_kinds.rs` `HOMES` as `Home::Quiet`); `worktreePath` on `Agent` and in `live_folders`; `cwd` and `started_ms` on `Subagent` and `SubagentRow`.
- Order of work: engine parsers and nearest base with their tests; the daemon ops and guard; `worktreePath` on `Agent`; the app models; Details; Changes targets; Branches.

## 12. Builder check

Two passes: the author's own cold read, then Antigravity (gemini-4-argon, high, the `spec-check` pick of `notchling delegate-models`) reading the spec and the code it builds on. Every finding was checked against the code or `git` before it went in. The questions, in three piles.

**Errors the check found, now fixed in the text:**

- `counted_title("Git", 0, 1)` prints "Git · 1", not "Git" (B2).
- `git.branch_list` rows lacked the worktree fields and `work`, and the current worktree's work was never cached; now every stat of the session's own worktree fills the cache (7.4).
- `for-each-ref --merged=X` lists X itself, so the integration branch read "merged into itself"; bases are now never classified (7.6, B19).
- The default is `origin/main` while worktrees check out `main`; compared by short name now (6, 7.1).
- `rev-list --left-right --count` prints behind first, `%(ahead-behind)` ahead first (7.5).
- `for-each-ref` never lists a detached head; `log -1` and `merge-base --is-ancestor` cover it (7.4, 7.6).
- "`ref` goes after `--`" was wrong: after `--` git reads paths. A `ref` stat must also read no working tree, and `diff --shortstat` needs every `DIFF_FLAGS` option or the textconv trap fires (7.4, 7.10).
- `Subagent` and `SubagentRow` have no folder or start time, and only Claude Code lines carry `cwd`; Codex and delegated jobs have their own sources (7.7).
- Changes on another target would have overwritten the Details row's numbers and base, which share `repo::State::branch` today; two states now (7.3).
- `live_folders` lacks `worktreePath` (7.10, B17).
- New ops must also be in `wire.rs` and `op_kinds.rs` (7.4, 11).

**Answered already** (now in the text above):

- Where does each number come from, and which base? 7.1, 7.5.
- Does the app match sessions or the daemon? The app, with no I/O; the daemon gives `path` and `real`. 7.7.
- How does Changes open another worktree when its folder is no session's? The `worktree` argument, checked against `git worktree list`. 7.3, 7.10.
- How fresh is GitHub without fetching? Local remote refs, with "as of" from `FETCH_HEAD`. 7.8.
- What about the `.html` page offer from today's file rows? Moved to Changes' open action. 7.1.
- Is `git.diff` removed? No; ops are never removed. 3.
- How many processes per refresh, and how often? 7.9 and the appendix.
- What does a click on a missing worktree do? Nothing; buttons off. 7.11.
- Does the phone change? No. 9.
- What do tabs contain when a branch fits two? Tabs are views; a worktree can be in Working and in one of the others. 7.2.

**Minor, decided** (marked *Builder's call*, table at the top): row order and cap, the title, inks, buttons on click, the GitHub label, the filter's fields, session switch, refused names, number format, the `ahead-behind` fallback, caps and cache sizes, the work-read rule, the line-count budget, `worktreePath` over `cwd`, Notchling's own worktrees, names (B1 to B16); and from the second pass: the Git section follows a session into its worktree, nearest-base ties, bases in Working, unread work kept out of Merged, a `ref` reply's fields, the base menu for a `ref`, the `.html` offer's condition, one gate per op, row names (B17 to B25).

**The maintainer's:** the seven in [Needs the maintainer](#needs-the-maintainer). The check raised two more, decided here as builder's calls because they follow from existing behavior and are cheap to change: which worktree is "this session" after EnterWorktree (B17), and where the bases go in the tabs (B19, as the mockup drew `main`).

## Appendix: measurements

Measured 2026-11-17 on the maintainer's Mac, git 2.54 (Apple Git-157), unsandboxed (`sandbox-exec` adds its own start cost per process; measure it in the daemon), mean of 5 runs:

| Command | ms |
| --- | --- |
| `config -z --get-regexp ^filter\.` | 8 |
| `worktree list --porcelain -z` | 9 |
| `for-each-ref` over all 49 refs with 7 fields | 15 |
| `for-each-ref %(ahead-behind:agent-run-oss)` over all refs | 13 |
| `for-each-ref --merged=agent-run-oss` | 10 |
| `rev-list --count a..b` | 9 |
| `diff --shortstat a...b` | 20 |
| `diff --numstat -z -M <base>` in a lane | 20 |
| `status --porcelain=v2 -z --untracked-files=all` in a lane | 40 |
| the same in the main checkout | 222 |

The repository: 4 worktrees (the main checkout on `agent-run-oss`, three lanes), 5 local branches, 44 remote-tracking refs. `origin/main` is 1,482 commits behind `origin/agent-run-oss`. `agent/ask` is 12 ahead and 1 behind `agent-run-oss`, 1,493 ahead of `main`. 37 remote refs are merged into `agent-run-oss`, 2 into `origin/main`.
