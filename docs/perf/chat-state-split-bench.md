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
