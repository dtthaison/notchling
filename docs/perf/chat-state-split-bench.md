# Chat state split: bench results

The "before" column and, task by task, the numbers each step of
[the plan](../superpowers/plans/2026-11-12-chat-state-split.md) is judged by. The bench is
`--render chat` with `NOTCHLING_RENDER_BENCH=<seconds>` and `NOTCHLING_RENDER_BENCH_MODE=<mode>`
over a generated 200-turn conversation (`chat::bench_fixture`); each mode prints one
`bench-json:` line. Run it with:

```sh
cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1
```

`NOTCHLING_BENCH_SECONDS` sets each mode's length (10 s here). Each row is the median of three
runs. The counts (root, conversation, sidebar, details, rows, parsed bytes) are for the whole
10 s run: `root` is how many times `ChatView` drew, `conversation`, `sidebar` and `details` how
many times each cached part drew, `rows` how many conversation rows were built and
`parsed bytes` how many transcript bytes the engine parsed in the run. `ms/frame` is the
process's CPU time (user and system) over the run divided by the frames the window drew; `fps` is
those frames per second. `append` is the `idle` mode while the test appends one assistant line to
the open transcript every 500 ms.

## Baseline

Commit `8842ebe` (Task 0.3), a release build, three runs on 2026-11-12.

Machine: Mac17,2 (Apple M5, 10 cores, 32 GB), macOS 26.6.2, the built-in display woken for the
run (`caffeinate -d -u`, since a sleeping display draws no frames).

The Mac was not quiet. A virtual machine (`com.apple.Virtualization.VirtualMachine`) held about one
core (97-102% CPU) through all three runs; `ffmpeg` was at about 680% when run 1 started and gone
by run 2; Finder, `mds`, `fileproviderd` and `XprotectService` took short bursts (up to 36%); the
maintainer's own `/Applications/Notchling.app` ran at 6-8%, untouched. The load average fell from
45 at the start of run 1 to 4 at the end of run 3. CPU time is the render process's own, so
contention moves it less than the wall-clock numbers, but the idle mode's 5.0-8.4 ms/frame spread
across runs shows the noise: judge a later task by the counts first and by ms/frame only when it
moves by more than about a third.

| Task | Mode | ms/frame | fps | root | conversation | sidebar | details | rows | parsed bytes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0.4 baseline | idle | 5.58 | 10 | 100 | 0 | 0 | 0 | 0 | 0 |
| 0.4 baseline | append | 10.15 | 10.2 | 103 | 5 | 5 | 5 | 2500 | 10262 |
| 0.4 baseline | scroll | 82.63 | 12.2 | 122 | 122 | 0 | 0 | 61000 | 0 |
| 0.4 baseline | hover | 14.52 | 10.1 | 101 | 1 | 0 | 0 | 500 | 0 |
| 0.4 baseline | select | 39.11 | 14.9 | 149 | 64 | 64 | 64 | 32000 | 0 |
| 0.4 baseline | typing | 43.73 | 18.6 | 188 | 97 | 97 | 97 | 48500 | 0 |
| 0.4 baseline | terminal | 84.37 | 11.9 | 119 | 117 | 117 | 0 | 58500 | 0 |
| 0.4 baseline | reload | 7.66 | 11.5 | 118 | 0 | 20 | 20 | 0 | 0 |
| 0.4 baseline | menu | 31.62 | 14.3 | 144 | 49 | 49 | 49 | 24500 | 0 |

Per run (ms/frame, frames drawn, conversation draws):

| Mode | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- |
| idle | 4.96 ms, 100 frames, 0 conv | 5.58 ms, 100 frames, 0 conv | 8.43 ms, 100 frames, 0 conv |
| append | 10.15 ms, 103 frames, 5 conv | 9.72 ms, 101 frames, 5 conv | 12.41 ms, 104 frames, 5 conv |
| scroll | 85.91 ms, 117 frames, 117 conv | 82.63 ms, 122 frames, 122 conv | 82.05 ms, 123 frames, 123 conv |
| hover | 10.9 ms, 101 frames, 1 conv | 14.55 ms, 101 frames, 1 conv | 14.52 ms, 101 frames, 1 conv |
| select | 39.11 ms, 149 frames, 60 conv | 38.78 ms, 150 frames, 65 conv | 39.57 ms, 143 frames, 64 conv |
| typing | 45.31 ms, 188 frames, 97 conv | 43.73 ms, 190 frames, 97 conv | 43.69 ms, 188 frames, 97 conv |
| terminal | 86.67 ms, 114 frames, 110 conv | 83.39 ms, 119 frames, 117 conv | 84.37 ms, 119 frames, 119 conv |
| reload | 7.66 ms, 118 frames, 0 conv | 4.63 ms, 118 frames, 0 conv | 9.13 ms, 115 frames, 0 conv |
| menu | 32.7 ms, 147 frames, 49 conv | 31.62 ms, 142 frames, 49 conv | 31.01 ms, 144 frames, 49 conv |

What the baseline says:

- `typing`, `terminal`, `menu` and `select` lay out the whole conversation (500 rows a draw) on
  about every other frame; `typing` and `menu` also draw the Sidebar and the Details Panel each
  time, `terminal` the Sidebar (the terminal panel takes the Details Panel's place).
- `scroll` draws the conversation on every frame (500 rows each), at about 83 ms a frame. The
  Sidebar and the Details Panel stay still.
- `hover` over the message column drew the conversation once in ten seconds, so what it
  costs there is mostly the hit test (14.5 against 5.6 ms/frame idle). Hover over an element with
  a hover style or pointer cursor (a button, a link) notifies its view, and the bench's lane
  crosses few of those.
- `reload` with nothing changed draws no conversation but the Sidebar and the Details Panel on
  every reload (20 in 10 s).
- `append` (one line every 500 ms): each appended line draws the conversation, the Sidebar and the
  Details Panel again (5 draws for the 20 lines appended in 10 s; reads are spaced by the reload gate) and parses
  only the appended bytes (10 KB for the 20 lines).
- `parsed bytes` is 0 outside `append`: the engine reads an unchanged transcript for nothing.

## Phase 1 (Tasks 1.1 to 1.5)

Commit `1ce01aa`, a release build, one 10 s run per mode on 2026-11-12 (not the median of three: the
counts don't move between runs, and the Mac was busy, load average 73, so ms/frame is only a hint).
Same machine as the baseline.

| Task | Mode | ms/frame | fps | root | conversation | sidebar | details | rows | parsed bytes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1.5 (Phase 1) | idle | 9.64 | 10 | 100 | 0 | 0 | 0 | 0 | 0 |
| 1.5 (Phase 1) | append | 11.15 | 10.1 | 102 | 5 | 5 | 5 | 2500 | 9720 |
| 1.5 (Phase 1) | scroll | 81.95 | 12.3 | 123 | 123 | 0 | 0 | 61500 | 0 |
| 1.5 (Phase 1) | hover | 17.3 | 10.1 | 101 | 1 | 0 | 0 | 500 | 0 |
| 1.5 (Phase 1) | select | 38.57 | 14.6 | 146 | 63 | 0 | 0 | 31500 | 0 |
| 1.5 (Phase 1) | typing | 8.44 | 18 | 182 | 1 | 0 | 0 | 500 | 0 |
| 1.5 (Phase 1) | terminal | 7.5 | 35 | 350 | 0 | 0 | 0 | 0 | 0 |
| 1.5 (Phase 1) | reload | 9.17 | 11.2 | 115 | 0 | 20 | 20 | 0 | 0 |
| 1.5 (Phase 1) | menu | 30.57 | 14.3 | 144 | 49 | 49 | 49 | 24500 | 0 |

Each task's own 3 s count test, before and after (conversation / sidebar / details draws):

| Task | Test (mode, 3 s) | Before | After |
| --- | --- | --- | --- |
| 1.1 | `bench_terminal_draws_no_part` (terminal) | 32 / 32 / 0, 33 frames, 91 ms/frame | 0 / 0 / 0, 110 frames, 7.0 ms/frame |
| 1.3 | `bench_typing_draws_no_part` (typing) | 29 / 29 / 29, 52 frames, 49 ms/frame | 0 / 0 / 0, 57 frames, 7.9 ms/frame |
| 1.5 | `bench_select_draws_no_sidebar_or_details` (select) | 19 / 19 / 19, 43 frames, 41 ms/frame | 19 / 0 / 0, 43-45 frames, 39-42 ms/frame |

What Phase 1 did: `terminal` and `typing` draw no cached part (the 10 s `typing` run's one
conversation draw is the box growing past a line, which resizes the column), `select` draws no
Sidebar or Details Panel. `scroll`, `hover`, `reload` and `menu` are unchanged, as expected: they
are Phases 3 to 6. Tasks 1.2 (simulator) and 1.4 (notch) have no bench mode; 1.4 is held by the
`notch::drawn::tests`.

## List spike (Task 2.1)

**Decision: go.** G1 to G5 pass; G6 fails alone, which the plan still calls go (Task 6.1 then draws
the live row's spinner as a plain kit spinner view inside the row, not a slot). Task 6.1 and 6.2
stand; 6.1-alt is not needed.

The probe is `--render chat-list-probe` (`chat/view/list_probe.rs`, its checks in
`list_probe_run.rs`): a cached child view holding a bottom-aligned `ListState` (overdraw 600 pt)
over 300 made-up rows of 1 to 12 wrapped lines (`pt(14)`, 16 pt side padding), a kit spinner in the
last row, drawn through a spinner host and a `SpinLayer`, in a 600 x 600 pt column. Run it with
`caffeinate -d -u cargo test --release --locked -p notchling-app --test render_list_probe -- --ignored --nocapture`
(`NOTCHLING_RENDER_BENCH=<s>` on `--render` makes the probe run its checks and print
`list-probe: {...}`). Vendored gpui 0.2.2, a release build, 2026-11-12, two runs with identical
numbers. Each step is one frame: change the list, notify, wait for the child to paint, read.

What each check reads is what the rows recorded while they painted (their bounds and the
viewport's, in window coordinates), not the list's bookkeeping: `bounds_for_item` returns `None`
for every row while the list follows the bottom (`logical_scroll_top` is `None`, which that method
takes as "item count"), so it can't judge G1 at all. Task 6.1 must not use it for "at the
bottom" or for the last row's place; it needs the rows' own painted bounds or the list's
following state.

| Gate | What it checks | Result | Evidence | Verdict |
| --- | --- | --- | --- | --- |
| G1 follow | the last row grows by a line, ten times, while following | max gap between its painted bottom and the viewport's bottom 0.0 px (limit 1); again after the resize, 0.0 px (the last row was then 720 pt, taller than the viewport) | `bounds_for_item` was `None` all 10 times | go |
| G2 anchor | with rows above the bottom, splice a row above the first visible one; then splice that row through a remeasure (`logical_scroll_top`, splice, `scroll_to` the saved offset) | first visible row (row 241, 14 pt into it) moved 0.0 px for a splice above and 0.0 px for the remeasure (limit 1); a bare splice of that row moved 14.0 px (203.5 px after the resize): the offset resets | the same two numbers after the resize with anchor row 238: 0.0 / 0.0 | go |
| G3 cost | 60 frames of `scroll_by(+-40)`: `render_item` calls per frame against rows on screen | 5.0 calls per frame for 5.0 rows on screen (limit rows + 2) with `ListSizingBehavior::Auto` | with the default `Infer` sizing: 23.5 calls for 5.0 rows, because `request_layout` lays the rows out again | go (with `Auto`) |
| G4 resize | column 600 -> 480 pt | average painted row height 153.75 -> 242.5 pt (the heights were measured again), smallest visible row 56 pt (> 0), G1 and G2 hold again (numbers above) | | go |
| G5 at the bottom | `set_offset_from_scrollbar` to the middle, then to the end | middle: top item 271; end: `logical_scroll_top().item_ix` 300 == `item_count()` 300, last row's bottom 0.0 px from the viewport's | a fresh `max_offset_for_scrollbar` is read before each call (it counts only rows measured so far) | go |
| G6 spinners | slots a spinner host records in a frame in which the list's autoscroll retry laid the rows out twice | 2 slots for 1 spinner on screen; 1 in a plain frame before and 1 a plain frame later | `SpinSlots::len` (new, kit) | fail: fallback below |

What the Phase 6 implementer must know, from what the probe found:

- Use `ListSizingBehavior::Auto` with a sized style (`size_full()` or `flex_1()`). The default
  `Infer` lays every visible row out in `request_layout` and again in prepaint: 4 to 5 times the
  `render_item` calls here.
- A list is never "following" in `logical_scroll_top()`: `None` means follow the bottom for a
  `Bottom` list, and `bounds_for_item` gives `None` then. Decide "at the bottom" from
  `logical_scroll_top().item_ix == item_count()` (or `None` internally), and read row places from
  what the rows paint.
- A jump of 3000 pt up from the end landed on row 291 of 300: rows never drawn have no height, so
  `scroll_by` from the end can't go further than the rows measured so far (about 9 rows after the
  first frames). Walking up in wheel-sized steps reaches unseen rows (the probe does 400 pt steps
  to row 241). The scroll bar's thumb is approximate for the same reason.
- A splice of the row at the scroll top resets its offset (a 14 px jump here, 203 px after a
  resize). Save `logical_scroll_top()`, splice, `scroll_to(saved)`: the row stays put.
- A width change measures every row again by itself; no stale height survived the resize.
- A row that asks for autoscroll (a text editor's cursor) makes the list prepaint twice, and a
  spinner slot pushed in the first prepaint stays: two slots for one spinner (G6). Task 6.1's
  fallback applies: draw the live row's spinner as the plain kit spinner view inside the row, and
  clear or de-duplicate nothing in the host.
- `canvas(...).absolute().size_full()` needs `.top_0().left_0()` too: without them GPUI puts it
  after its siblings, and the bounds it records are the wrong place (the first run of the probe
  read a 584 px gap for exactly that reason).

## Running the bench while the screen is locked

A locked screen (or a sleeping display) sends a window no display-link frames, so every mode printed
`frames: 0` and `bench_chat`'s `check` failed, although `--render` still captured a PNG (its one
frame comes from AppKit's `displayLayer:`, which GPUI answers by running the window's frame
callback). The bench now does the same on a timer: `mac/bench.rs` first makes the window dirty
(`ChatView::bench_chrome`, which wakes only the root) and, when no frame arrives in 300 ms, calls
`displayLayer:` on the window's view every 16 ms (`bench_events::FrameDriver`) for the rest of the
run. GPUI draws only when the window is dirty, so a forced frame draws what a display-link frame
would. `NOTCHLING_BENCH_FORCE_FRAMES=1` forces it always and `=0` never; the `bench-json:` line says
`"forced":true` when it did. Nothing else changes outside bench mode, and `vendor/` is untouched.

What is comparable: the counts (`conversation`, `sidebar`, `details`, `rows`, `parsed_bytes`),
which come from what the window draws and not from how it is paced. What is not: `ms/frame` and
`fps` (a forced frame is presented with a transaction and restarts GPUI's display link each time, so a
frame costs more, and a heavy mode draws fewer of them), so judge a task on counts. A run takes the
first 1.5 s as warm-up before it counts (it was 0.4 s; the window's first reloads land in the
first second or two, and a draw of every part from one of them showed up in some runs).

Verified on 2026-11-12 with the screen unlocked, by `NOTCHLING_BENCH_FORCE_FRAMES=1` against the
display link's own frames (the Mac was not locked when this was written, so the automatic
detection was only seen choosing "not forced"; the forced loop itself is what `=1` runs). Counts
per mode, 10 s each, median of 2 forced runs against the median of 3 display-link runs:
`terminal` 0 / 0 / 0 and 0 / 0 / 0, `typing` 0 or 1 / 0 / 0 (the box growing past a line) in both,
`select` 59.5 / 0 / 0 and 58 / 0 / 0, `scroll` 99 / 0 / 0 and 101 / 0 / 0, `reload` 0 / 19 / 19 and
0 / 20 / 20, `menu` 49.5 / 49.5 / 49.5 and 49 / 49 / 49, `hover` 1 / 0 / 0 in both, `append` 5 / 5 / 5
in both.

A run on a busy Mac shows a stray draw of every part once or twice (a 3 s count test asserting 0
failed about one run in six, with or without forced frames), and under load a forced run draws
fewer frames (one set of runs gave `scroll` 82 and `menu` 44). Repeat a run before believing a
single 1.

## Phase 3 (Tasks 3.1 to 3.4)

Commit `10b2c82` plus the forced-frame bench, a release build, 2026-11-12, 10 s per mode. The first
set is median of 3 display-link runs (`forced: false`, the screen unlocked); the second median of 2
runs with every frame forced (`NOTCHLING_BENCH_FORCE_FRAMES=1`), whose ms/frame is not comparable.
Same machine as the baseline.

| Task | Mode | ms/frame | fps | root | conversation | sidebar | details | rows | parsed bytes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 3.4 (Phase 3) | idle | 6.00 | 10.0 | 100 | 0 | 0 | 0 | 0 | 0 |
| 3.4 (Phase 3) | append | 10.27 | 10.3 | 103 | 5 | 5 | 5 | 2500 | 5408 |
| 3.4 (Phase 3) | scroll | 100.18 | 10.0 | 101 | 101 | 0 | 0 | 50500 | 0 |
| 3.4 (Phase 3) | hover | 11.63 | 10.1 | 101 | 1 | 0 | 0 | 500 | 0 |
| 3.4 (Phase 3) | select | 45.06 | 13.7 | 137 | 58 | 0 | 0 | 29000 | 0 |
| 3.4 (Phase 3) | typing | 5.46 | 18.3 | 183 | 1 | 0 | 0 | 500 | 0 |
| 3.4 (Phase 3) | terminal | 4.43 | 36.5 | 365 | 0 | 0 | 0 | 0 | 0 |
| 3.4 (Phase 3) | reload | 6.50 | 11.4 | 118 | 0 | 20 | 20 | 0 | 0 |
| 3.4 (Phase 3) | menu | 40.37 | 13.1 | 134 | 49 | 49 | 49 | 24500 | 0 |
| 3.4 (Phase 3, forced frames) | idle | 6.46 | 9.5 | 95 | 0 | 0 | 0 | 0 | 0 |
| 3.4 (Phase 3, forced frames) | append | 12.67 | 10.2 | 102.5 | 5 | 5 | 5 | 2500 | 5269.5 |
| 3.4 (Phase 3, forced frames) | scroll | 91.53 | 10.2 | 101.5 | 99 | 0 | 0 | 49500 | 0 |
| 3.4 (Phase 3, forced frames) | hover | 14.54 | 9.2 | 92 | 1 | 0 | 0 | 500 | 0 |
| 3.4 (Phase 3, forced frames) | select | 43.60 | 13.6 | 136.5 | 59.5 | 0 | 0 | 29750 | 0 |
| 3.4 (Phase 3, forced frames) | typing | 6.05 | 17.6 | 176.5 | 0.5 | 0 | 0 | 250 | 0 |
| 3.4 (Phase 3, forced frames) | terminal | 4.56 | 28.2 | 282 | 0 | 0 | 0 | 0 | 0 |
| 3.4 (Phase 3, forced frames) | reload | 6.38 | 11.4 | 114.5 | 0 | 19 | 19 | 0 | 0 |
| 3.4 (Phase 3, forced frames) | menu | 43.68 | 12.4 | 125.5 | 49.5 | 49.5 | 49.5 | 24750 | 0 |

Against Phase 1 the counts are the same: Phase 3 moved work off the main thread (the engine hands out
`Arc<Conversation>`, the loader diffs), which no count here shows. `parsed bytes` in `append` is
about half of Phase 1's because the run now starts counting after the longer warm-up, while the test
appends a line every 500 ms from `bench: ready`.

## Phase 4 (Tasks 4.1 to 4.4)

The Sidebar, the Details Panel and the conversation each draw from a model or input of what they
show, and only when it changed (`chat/view/pane.rs`, `conversation_view.rs`). A release build, the
screen locked, so every frame forced (`forced: true`), 10 s per mode, median of 3 runs. As in the
locked-screen section above, `ms/frame` and `fps` are not comparable with the display-link rows;
judge by the counts. Same machine as the baseline.

| Task | Mode | ms/frame | fps | root | conversation | sidebar | details | rows | parsed bytes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4.4 (Phase 4) | idle | 9.92 | 10.0 | 100 | 0 | 0 | 0 | 0 | 0 |
| 4.4 (Phase 4) | append | 13.29 | 10.1 | 101 | 5 | 0 | 5 | 2500 | 5406 |
| 4.4 (Phase 4) | scroll | 97.70 | 8.8 | 88 | 88 | 0 | 0 | 44000 | 0 |
| 4.4 (Phase 4) | hover | 11.78 | 10.1 | 101 | 1 | 0 | 0 | 500 | 0 |
| 4.4 (Phase 4) | select (see below) | 9.37 | 10.0 | 100 | 0 | 0 | 0 | 0 | 0 |
| 4.4 (Phase 4) | typing | 8.69 | 17.9 | 181 | 1 | 0 | 0 | 500 | 0 |
| 4.4 (Phase 4) | terminal | 8.36 | 31.7 | 317 | 0 | 0 | 0 | 0 | 0 |
| 4.4 (Phase 4) | reload | 8.60 | 11.5 | 116 | 0 | 0 | 0 | 0 | 0 |
| 4.4 (Phase 4) | menu | 9.11 | 14.0 | 141 | 0 | 0 | 0 | 0 | 0 |

Before (Phase 3, same counts method): `reload` 0 / 20 / 20 (conversation / sidebar / details),
`menu` 49 / 49 / 49, `append` 5 / 5 / 5. After: `reload` 0 / 0 / 0, `menu` 0 / 0 / 0,
`append` 5 / 0 / 5 (the Sidebar's rows don't change when a session's text grows; the Details Panel
shows counts from it).

Per task, from the 3 s count tests (debug build): 4.1 `reload` Sidebar 6 to 0 and `menu` Sidebar
15 to 0; 4.2 `reload` Details Panel 6 to 0 and `menu` Details Panel 15 to 0; 4.3 moves only; 4.4
`menu` conversation draws to 0 (a run with the input check turned off gave 4 in 3.4 s of a debug
build, which draws few frames).

`select` shows 0 conversation draws here because the bench's mouse press does not reach the window
while the screen is locked (true of the base build too), so its 58 draws of Phase 3 are not
reproduced; rerun it on an unlocked Mac. `hover` and `typing` keep their one draw (the first
hover; the box growing past a line). `scroll` is the conversation scrolling, which draws it by
design.

## Task 5.2 (selection anchored to the model)

A release build over the generated 200-turn root, the screen locked, so every frame forced
(`forced: true`; `ms/frame` is not comparable with the display-link rows, judge by the counts),
10 s per run, three runs each, the commit before the task (with the bench fix below) against the
task. Columns: frames / conversation / sidebar / details draws / rows built.

| Mode | Before (3 runs) | After (3 runs) |
| --- | --- | --- |
| select | 121 / 57 / 0 / 0 / 28500; 125 / 61 / 0 / 0 / 30500; 132 / 61 / 0 / 0 / 30500 | 133 / 62 / 0 / 0 / 31000; 126 / 60 / 0 / 0 / 30000; 114 / 55 / 0 / 0 / 27500 |
| scroll | 93 / 93 / 0 / 0 / 46500; 82 / 82 / 0 / 0 / 41000; 93 / 93 / 0 / 0 / 46500 | 89 / 89 / 0 / 0 / 44500; 91 / 91 / 0 / 0 / 45500; 97 / 97 / 0 / 0 / 48500 |
| hover | 100 / 1 / 0 / 0 / 500 (x3) | 101 / 1 / 0 / 0 / 500 (x3) |

The counts don't move, as expected: the selection already redrew only the conversation, and the
conversation still lays out every row on a draw until Task 6.1. What changes is what a draw
needs: a copy reads the model's blocks, not the drawn notes, so a row that isn't drawn is copied
like any other (Task 6.1 will stop drawing those).

**The `select` mode under a locked screen.** It showed 0 conversation draws there (the Phase 4
section) because the injected press landed before the conversation's first draw. The press
reaches the window's view fine when the screen is locked (AppKit events handed to our own window's
view, as for `scroll`); `prepare` now waits until a row has been built (up to 5 s) and 300 ms more
before pressing. The new test `bench_select_draws_the_conversation_only` asserts the conversation
drew (so a press that missed can't pass), the Sidebar and the Details Panel didn't.

## Task 6.2 (the kit's scroll bar over a list)

The list probe now draws its list through `scrollable_list` and gate G7 reads where the bar put its
thumb (`Scrollable::record_thumb`). A release build, the screen locked (frames forced), 2026-11-12:
G1 to G5 pass as before (G6 still fails alone, as in the spike), and

| Gate | What it checks | Result |
| --- | --- | --- |
| G7 bar | the thumb's bottom against the track's bottom: while following, while the last row grows, scrolled up 1500 pt, and back at the end | 0.0 px following, 0.0 px while growing, 45.7 px scrolled up (thumb 541 to 571 of a track ending at 617), 0.0 px back at the end: pass |

One thing the probe taught: a list at its end counts as scrolled to the end of its content (the
scroll top is past the last scroll position by a viewport), so a `scroll_by` of less than a
viewport from there leaves it following. Phase 6's code that scrolls the list by a step must start
from `scroll_px_offset_for_scrollbar`/`logical_scroll_top`, not assume the end is a scroll position.

## Phase 6 (Tasks 6.1 to 6.4)

### Task 6.1: the conversation as a GPUI list

The conversation's rows are a `list` (`chat/view/conversation_list.rs`): a draw builds the rows on
screen plus any overdraw not measured yet, and a load splices the rows it changed. A release
build over the generated 200-turn root, the screen locked, so every frame forced (`forced: true`),
10 s per mode, on the same machine as the baseline. The "before" is one run of the commit before
the task (`22b25b4`; the counts don't move between runs); the "after" is the median of 3 runs. Both
sides are forced frames on the same Mac in the same state, so `ms/frame` and `fps` are
comparable with each other here (not with the display-link rows above). `rows/draw` is `rows`
divided by `conversation`.

| Mode | ms/frame before → after | fps before → after | conversation draws | rows built | rows/draw | parsed bytes |
| --- | --- | --- | --- | --- | --- | --- |
| idle | 8.01 → 3.81 | 10.0 → 10.0 | 0 → 0 | 0 → 0 | - | 0 → 0 |
| append | 11.95 → 3.93 | 10.1 → 10.3 | 5 → 5 | 2500 → 5 | 500 → 1 | 5406 → 5406 |
| scroll | 93.06 → 5.24 | 9.1 → 46.3 | 92 → 463 | 46000 → 1569 | 500 → 3.4 | 0 → 0 |
| hover | 9.54 → 4.20 | 10.1 → 10.1 | 1 → 1 | 500 → 1 | 500 → 1 | 0 → 0 |
| select | 46.31 → 4.55 | 12.6 → 18.7 | 60 → 99 | 30000 → 99 | 500 → 1 | 0 → 0 |
| typing | 7.89 → 3.43 | 18.0 → 17.5 | 1 → 1 | 500 → 1 | 500 → 1 | 0 → 0 |
| terminal | 4.68 → 4.21 | 32.4 → 33.2 | 0 → 0 | 0 → 0 | - | 0 → 0 |
| reload | 7.71 → 3.60 | 11.2 → 11.6 | 0 → 0 | 0 → 0 | - | 0 → 0 |
| menu | 8.27 → 3.83 | 13.7 → 13.9 | 0 → 0 | 0 → 0 | - | 0 → 0 |

`sidebar` and `details` draws are unchanged (0 everywhere but `append`'s 5 Details Panel draws, as in
Phase 4). What the table says:

- A conversation draw builds the rows on screen, not 500: 1 in `append`, `hover`, `select` and
  `typing` (a message of this fixture is about 800 pt tall, so one or two fill the viewport), 3.4
  in `scroll`, which also moves across rows the list hasn't measured yet.
- `scroll` is the big one: a wheel event used to lay out all 500 rows (93 ms a frame, 9 fps); it
  now takes 5 ms a frame and the window keeps up with the 60 a second the driver sends (46 fps).
- `append` builds one row per draw (the appended one, spliced at the tail), the engine parses the
  same 5.4 KB (only new bytes), and the conversation is still at its end afterwards (`at_end` in the
  `bench-json:` line, new; `bench_append_draws_only_rows_on_screen_and_follows` asserts it).
- `hover`'s and `typing`'s one conversation draw is the first hover and the box growing past a line,
  as before.

The 3 s count tests (before the task they failed at 500 rows a draw, after they pass):
`bench_scroll_draws_only_rows_on_screen` 15500 rows over 31 draws → 489 over 144,
`bench_hover_draws_only_rows_on_screen` 500 over 1 → 1 over 1, `bench_select_draws_only_rows_on_screen`
9500 over 19 → 29 over 29, `bench_append_draws_only_rows_on_screen_and_follows` 1000 over 2 → 2
over 2, `at_end` true.

### Task 6.3: drag-select scrolls the list at its edges

A behavior change, not a speed one: the `select` bench drags inside the column, so its numbers are
the 6.1 numbers (the table above; a run after 6.3: 99 conversation draws, 99 rows, 3.2 ms a frame).
The scroll itself is a 16 ms timer that moves the list by `autoscroll_speed` points through the
scroll bar's offset (`set_offset_from_scrollbar`, `scroll_target`), not `ListState::scroll_by`:
`scroll_by` from a list that follows its end first moves it a whole viewport's worth of
unmeasured offset, while the bar's offset moves it from where it stands.

### Task 6.4: the scroll bar's fade

`bench_scroll_fade` (mode `scroll-fade`: 1.5 s of warm-up, 1 s of the `scroll` wheel, then 2 s of
nothing while the bar holds for 0.7 s and fades over 0.3 s; the numbers are those 2 s). Release
build, three runs, the screen awake (`forced: false`, the window's own frames):

| Run | frames | conversation draws | rows built | cpu ms | ms/frame |
| --- | --- | --- | --- | --- | --- |
| 1 | 27 | 9 | 9 | 39 | 1.43 |
| 2 | 28 | 10 | 10 | 107 | 3.83 |
| 3 | 28 | 9 | 9 | 97 | 3.47 |

The fade draws the conversation 9 or 10 times (the hold's wake-up and the fade's steps at 30 a
second), one row each, and `ms_per_frame` stays under 4 ms in every run, so the fade rate is left
as it is: `FADE_STEP_SECONDS` stays 1/30 and `the_fade_steps_at_fifteen_frames_a_second` was not
added. (Before the list a fade step drew all 500 rows; the plan's 4 ms line is for the list.)

## Native spinner spike

After the split, a session that works still cost the window about 100 frames in 10 s: the
Activity Line's spinner (`SpinLayer`, ten steps a second) asked GPUI to draw the root and present
the window for each step, about 3.8 ms each, to turn a 12 pt ring. The spike draws that ring as a
Core Animation layer instead: a `CAShapeLayer` (Lucide's `loader-circle` arc, same size, stroke and
ink) under a repeating rotation the render server turns by itself, so the app asks for no frame to
turn it. Verdict: GO.

`bench_idle_working_requests_few_frames` (mode `idle`, a working session on the generated 200-turn
conversation, 10 s, release, the window's own frames):

| | root draws (frames asked for) | cpu ms over the run |
| --- | --- | --- |
| GPUI spinner (`NOTCHLING_NATIVE_SPINNER=0`) | about 100 | 164 to 286 |
| Core Animation spinner | 10 to 11 | 67 to 82 |

The ten or eleven draws left are the Activity Line's clock ("Writing... (26s)"), redrawn once a
second by the tick (`activity::clock_due`) through `redraw_chrome`, which draws no part. The
conversation, the Sidebar and the Details Panel draw 0 or 1 times in the run. The test counts
`root` (the draws the app asked for), not a rate, so it holds on a locked screen, where the bench
draws frames by hand but only a window made dirty draws anything. `native_rings` in the
`bench-json:` line is the number of Core Animation rings the window holds; the test requires one.

Not verified by a machine: that the ring looks identical to the GPUI one and turns at the same
speed (this environment takes no screenshots; the probe below reads the layers' rotation, not
pixels), and the ring under every overlay. `NOTCHLING_RENDER_NATIVE_PROBE=1` on `--render chat`
prints `native-spin:` with each ring's place and its presentation layer's rotation at three times
150 ms apart; the rotation moves between samples when Core Animation is turning it.
