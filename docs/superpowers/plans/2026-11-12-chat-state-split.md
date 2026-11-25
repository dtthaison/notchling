# Chat State Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Chat costs in proportion to what changed on screen, not to the length of the conversation or the number of `cx.notify()` calls: typing, terminal and simulator frames draw no cached part; scrolling, hovering and selecting lay out only the rows on screen; a reload that changed nothing in the open session draws nothing in it.

**Architecture:** Four moves, in this order. (1) Measure: bench modes and counters on `--render chat` over a generated 200-turn conversation. (2) The transcript as a log: the engine's cursors hand out `Arc<Conversation>`, the loader sends a change description (`chat::log::Change`) and the pending question comes from the same reader. (3) State per area: the Sidebar and the Details Panel draw from view models behind an equality gate, and the conversation becomes its own entity, `ConversationView`, drawing from a `ConversationInput` behind the same gate. (4) The conversation as a GPUI `list`: selection anchored to the model first (copy no longer needs every row drawn), then the list, fed by splices from the change description. Small wins (typing, terminal, simulator, notch, `window.refresh()`) land early, in parallel.

**Tech Stack:** Rust, GPUI 0.2.2 (vendored, `vendor/gpui`, not changed), crates `notchling-app`, `notchling-engine` (`src/transcript/` only), `notchling-kit`.

**Spec:** [decision 0018](../../decisions/0018-chat-state-per-area.md). Fact sheets behind the line numbers: the code at `aa74fb7` (branch `agent-run-oss`).

## Global Constraints

- Work on `agent/chat-state-split` or a branch off it. Never commit to `agent-run-oss` or `main`, never push unless the maintainer asks, never run `./build.sh --install` or `tools/bundle-next.sh --install`. Merging is the maintainer's call.
- The app builds and behaves the same after every task. A task that moves code changes no logic; a task that changes behavior says so in its goal.
- Tests never touch Keychain, real logins, `~/.claude`, `~/.codex`, `~/.gemini`, ssh or `gh`. Renders and benches read a fixture root or a generated temp root, named through `NOTCHLING_DATA_ROOT`.
- Never edit, delete or rename an existing test, fixture or golden (`WEAK-TEST-EDIT`). A behavior a later task tightens gets a new test function; an old function stays as it is. Where a signature change would break an existing test, keep the old signature (the plan says how each time).
- No app code writes files. The only new file writes are in `crates/app/tests/bench_chat.rs` (the generated temp root and the appended line), which trips `CAP-FILEWRITE`: say so in that commit's message, for the maintainer. Unit tests that need a file use `crate::testutil::Scratch::write`.
- Files: a new file stays under 600 lines; no file passes 1000 (`LINT-FILE-SIZE`). The splits below keep every touched file under its limit; if a task would push one over 600, it moves the named functions into the named new file.
- Views use kit tokens (`Ink`, `TypeRole`, kit metrics), never raw colors or sizes. New clickable things get `.pointerCursor()` through the kit styles, as today. New views get `.part_at("Name", …)` tags where they draw a named part.
- Protected paths (`docs/decisions/**`, `docs/superpowers/**`, `docs/feature-map.md`, `docs/architecture.md`, every `AGENTS.md`, any `Cargo.toml`, `vendor/`) are touched only by Task 8.1, which collects the note updates each phase lists.
- Bench numbers come from release builds (debug GPUI is far too slow to judge).

**Check** (every task, for the crates it touched):

```sh
export PATH="$HOME/.cargo/bin:$PATH" CARGO_TARGET_DIR="$PWD/target-chat-split"
cargo test --locked -p notchling-app          # plus -p notchling-engine / -p notchling-kit when touched
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
PYTHONPATH=tools/guard python3 -m guard lint
PYTHONPATH=tools/guard python3 -m guard featuremap
```

**Render** (every task that changes a Chat view): `cargo test --locked -p notchling-app --test render_chat --test render_file_panel -- --ignored`.

**Bench** (from Task 0.3 on): `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1`. Each mode prints one `bench-json:` line; record the median of three runs in `docs/perf/chat-state-split-bench.md` (created in Task 0.4, not protected) under the task's id.

## What changed from the target design

1. **No separate sessions entity (A).** `ChatView` stays the controller for sessions, the selected row, drafts, tools and panels: about 60 sites across the window read them, so moving them threads another context through those calls and redraws nothing less. The Sidebar and the Details Panel get view models behind an equality gate instead (a "selector"), which removes their redraws without reclassifying the 122 `cx.notify()` calls.
2. **`Arc<Conversation>`, not `Arc` per item (B).** `Conversation` is the daemon's wire type for `agy.conversation`; `Arc` fields need serde's `rc` feature (a protected `Cargo.toml` change) and touch every parser and many tests. The whole-conversation `Arc` plus the change description give the same gain.
3. **No reload classification by watcher paths (B).** Sidebar rows show other sessions' state (ages and status come from their files), so a reload can't skip their changes. "Reload only when the open session's files changed" holds for the conversation through the cursor cache and an `Arc` pointer compare: an unchanged file is neither parsed, compared nor drawn.
4. **The diff runs on the loader's thread (B).** `chat::log::diff` compares by item id and equality off the main thread; the main thread only checks pointers.
5. **Panels stay root-drawn (E).** The terminal, simulator and page panels are drawn by the root view, and `redraw_chrome` already redraws only the root, so their ticks just switch to it. Panel entities are optional hygiene (Phase 7), done only if the bench says the root's own draw matters.
6. **Selection ownership (C).** The conversation's selection moves into `ConversationView`; the file panel keeps today's draw-order selection with its own listener. A drag from the conversation into the file panel stops selecting across both; ⌘C copies whichever holds a selection.
7. **Scroll bar fade (E).** After the list, a fade frame lays out only the rows on screen. Task 6.4 measures it and lowers the fade's frame rate only if it still costs; a separate paint layer for the bar is out of this plan.
8. **New finding.** Every keystroke in the reply box calls `cx.notify()` (`tools.rs:207`, `draft_changed`) and so lays out the conversation, the Sidebar and the Details Panel again; Task 1.3 fixes it.

## Risks and how each is caught

| Risk | Caught by |
| --- | --- |
| The `list` doesn't stay pinned to the bottom while the last row grows (a tools group gaining steps) | Spike gate G1 (Task 2.1); `bench_append_*` follow check (Task 6.1) |
| Rows never drawn count as 0 px, so the Latest pill or a reload's auto-follow misjudges "at the bottom" and yanks the reader down | Task 6.1 decides "at the bottom" from the list's following state and the last row's bounds, never pixel sums; unit test `at_bottom_ignores_unmeasured_rows`; spike gate G5 |
| A splice at or above the first visible row moves what the reader is looking at | Spike gate G2; `ConversationView::remeasure` keeps the scroll-top offset; test `a_splice_at_the_scroll_top_keeps_its_offset` |
| A width change leaves stale heights | Spike gate G4 (resize) |
| Spinners inside list rows leave duplicate or missing slots (the list's prepaint can retry inside `window.transact`) | Spike gate G6; fallback in Task 6.1 (a plain kit spinner in that row) |
| `plain_blocks` drifts from what a row draws, so copy differs from what was highlighted | Shared text helpers used by both (Task 5.1); render check `NOTCHLING_RENDER_CHECK_BLOCKS=1` (Tasks 5.1, 5.2) |
| A view model misses a field, and a part shows a stale value | Draw functions take only the model (compiler); explicit `redraw_parts` for globals (appearance, folds); render tests; bench counters; the manual checklist in Task 8.2 |
| Item ids renumber when the engine starts a fresh cursor, and splices hit the wrong rows | `chat::log` resets on misaligned ids (tests in Task 3.2) |
| A reload that changed the open session is missed | Unchanged: the watcher plus `ReloadGate`'s 30 s full reload; tests `an_appended_line_is_one_splice` (Task 3.3) |
| The pending question from the conversation reader differs from the snapshot's | Parity test over every fixture transcript (Task 3.4) |
| A task edits an existing test to fit a new signature | Old signatures kept (`read`, `Open::apply` generic); `guard lint` and the maintainer gate on `WEAK-TEST-EDIT` |
| A file passes 600 or 1000 lines | Splits in Tasks 0.1 and 4.3; `guard lint` |
| The notch's ages go stale once it redraws only on change | Test `the_notch_redraws_when_an_age_changes` (Task 1.4) |
| Typing, hover or a terminal frame starts drawing parts again later | Count-based bench tests added per task (`bench_typing_draws_no_part`, …) |

## Phases at a glance

| Task | What | Size | After | May run in parallel with |
| --- | --- | --- | --- | --- |
| 0.1 | Split `chat/view/mod.rs` and `conversation.rs` (moves only) | M | — | — |
| 0.2 | Counters (rows drawn, root draws, bytes parsed) and the `bench-json:` line | S | 0.1 | 2.1, 3.2, 5.1 |
| 0.3 | Bench modes, the 200-turn fixture, `tests/bench_chat.rs` | M | 0.2 | 2.1, 3.1, 3.2, 5.1 |
| 0.4 | Baseline numbers | S | 0.3 | 2.1, 3.1, 3.2, 5.1 |
| 1.1 | Terminal frames and selection redraw only the root | S | 0.4 | 1.2-1.5, 2.1, 3.x, 5.1 |
| 1.2 | Simulator frames redraw only the root | S | 0.4 | 1.1, 1.3-1.5, 2.1, 3.x, 5.1 |
| 1.3 | Typing and the slash menu redraw only the root | S | 0.4 | 1.1, 1.2, 1.4, 1.5, 2.1, 3.1, 3.2, 3.4, 5.1 (not 3.3: both touch `tools.rs`) |
| 1.4 | The notch redraws only when what it draws changed | S | 0.4 | 1.1-1.3, 1.5, 2.1, 3.x, 4.x, 5.1 |
| 1.5 | Scroll bar, Details fold and selection notify their own view, not the window | S | 0.4 | 1.1-1.4, 2.1, 3.x, 5.1 |
| 2.1 | `list` spike: probe view and go/no-go | M | 0.1 | everything up to 6.2 |
| 3.1 | Engine: `Arc<Conversation>` from the cursors | S | 0.2 | 0.3, 0.4, Phase 1, 2.1, 3.2, 5.1 |
| 3.2 | `chat::log`: the change description | M | 0.1 | 0.2-0.4, Phase 1, 2.1, 3.1, 5.1 |
| 3.3 | Loader and `ChatView` carry `Arc` and the change | M | 3.1, 3.2 | 1.1, 1.2, 1.4, 1.5, 2.1, 4.1, 4.2, 5.1 |
| 3.4 | The pending question from the conversation reader | S | 3.3 | Phase 1, 2.1, 4.1, 4.2, 5.1 |
| 4.1 | Model parts; the Sidebar draws from `SidebarModel` | M | 1.5 | 1.1-1.4, 2.1, Phase 3, 5.1 |
| 4.2 | The Details Panel draws from `DetailsModel` | S | 4.1 | 1.1-1.4, 2.1, Phase 3, 5.1 |
| 4.3 | `ConversationView` entity (move, same redraws) | M | 4.2 | 2.1, 3.4, 5.1 |
| 4.4 | `ConversationView` draws from `ConversationInput` | L | 4.3, 3.3 | 2.1, 5.1, 6.2 |
| 5.1 | `plain_blocks` and the shared text helpers | M | 0.1 | Phases 0-4 after 0.1 |
| 5.2 | Selection anchored to the model, owned by `ConversationView` | L | 4.4, 5.1, 1.5 | 6.2 |
| 6.2 | Kit scroll bar tracks a `ListState` | M | 2.1 (go) | Phases 3-5 |
| 6.1 | The conversation as a `list` | L | 5.2, 6.2, 2.1 (go) | — |
| 6.3 | Drag-select scrolls the list at its edges | S | 6.1 | 6.4, Phase 7 |
| 6.4 | Scroll bar fade: measure, lower its rate if it costs | S | 6.1 | 6.3, Phase 7 |
| 7.x | Optional: terminal, simulator, page panels as entities | M each | 6.1 | each other |
| 8.1 | Notes: `crates/AGENTS.md`, `docs/feature-map.md` (protected) | S | each phase | anything |
| 8.2 | Manual check on the maintainer's screen | S | 6.1 | — |

Two tasks that touch the same file never run at once even when the table allows it; the files are listed in each task.

If Task 2.1 says no-go, Tasks 6.1 and 6.2 are replaced by Task 6.1-alt (below) and the rest stands.

## The bench

Modes (`NOTCHLING_RENDER_BENCH_MODE`, with `NOTCHLING_RENDER_BENCH=<seconds>` on `--render chat`):

| Mode | What the driver does each step | Rate |
| --- | --- | --- |
| `idle` | nothing (the working session's spinners still turn) | — |
| `scroll` | after scrolling 2000 pt up from the end once, a wheel event of ±60 pt alternating every 30 steps, over the conversation's centre | 60/s |
| `hover` | a mouse move over the conversation, stepping 7 pt down and wrapping | 30/s |
| `select` | press on a row, drag down 7 pt per step, release at the end | 30/s |
| `typing` | inserts `a` into the reply box (`TextEditor::insert`) | 10/s |
| `terminal` | opens the render terminal and feeds one line to it | 30/s |
| `reload` | `reload_now` (nothing on disk changed) | 2/s |
| `menu` | opens or closes the account menu by the reply box (`toggle_account_menu`; button and menu are root-drawn): a `ChatView` change that no part shows | 5/s |

The test `bench_append` runs `idle` and appends one assistant line to the open transcript every 500 ms from the test process.

`bench-json:` fields: `mode`, `seconds`, `frames`, `cpu_ms`, `ms_per_frame`, `root` (ChatView renders), `conversation`, `sidebar`, `details` (part renders), `rows` (conversation rows built), `parsed_bytes`.

Results file `docs/perf/chat-state-split-bench.md` (Task 0.4 creates it; each task appends its rows):

| Task | Mode | ms/frame | fps | root | conversation | sidebar | details | rows | parsed bytes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0.4 baseline | idle | | | | | | | | |
| 0.4 baseline | … one row per mode, `append` included | | | | | | | | |

Expected direction, to judge a task by: Phase 1 takes `conversation`, `sidebar` and `details` to 0 in `typing` and `terminal`, and `sidebar` and `details` to 0 in `select`; Phase 4 takes all three to 0 in `menu`, and `sidebar` and `details` to 0 in `reload` (an age may tick over once); Task 6.1 takes `rows` per conversation draw from about 500 to the rows on screen (about 10-25) in `scroll`, `hover`, `select` and `append`.

---

### Task 0.1: Split `chat/view/mod.rs` and `conversation.rs` (moves only)

**Goal:** room for the work ahead. `mod.rs` is 979 lines and `conversation.rs` 854; both are touched by later tasks.

**Files:**
- Create `crates/app/src/chat/view/load.rs`: one `impl ChatView` block holding, unchanged, `show_started_job` (`mod.rs:525`), `show_session` (`:541`), `start_load` (`:557`), `apply` (`:586-702`), `reload_now` (`:740`), `select` (`:745`), `tick` (`:769-830`).
- Create `crates/app/src/chat/view/root.rs`: `impl Render for ChatView` (`mod.rs:833-979`), unchanged.
- Create `crates/app/src/chat/view/steps_view.rs`: from `conversation.rs`, `hairline` (`:146`), `diff_line` (`:150`), `diff_view` (`:205`), `step_detail` (`:266`), `step_row` (`:307`), `step_chip` (`:398`), `step_group` (`:453`), `thinking` (`:486`), through line 533, unchanged.
- Modify `crates/app/src/chat/view/mod.rs`: `mod load; mod root; mod steps_view;`; `conversation.rs` imports what it calls from `steps_view`.

**Interfaces:** none new. Functions called across the new files become `pub(super)`; nothing else changes.

- [ ] **Step 1:** Move the blocks (cut and paste, then fix `use` lines and visibility only).
- [ ] **Step 2: Run** Check and Render. Expected: PASS. `git diff -M --color-moved=zebra` shows only moved blocks, `use` lines and `pub(super)`.
- [ ] **Step 3: Commit** "Chat: split the view's loading, its root draw and the step rows into files of their own (moves only)".

**Done when:** `mod.rs` and `conversation.rs` are under 600 lines, every new file under 600, Check and Render pass.

### Task 0.2: Counters and the `bench-json:` line

**Goal:** the numbers every later task is judged by.

**Files:**
- Modify `crates/engine/src/transcript/tail.rs`: `pub static PARSED_BYTES: AtomicU64`; `Cursors` gets a field `parsed: AtomicU64` and `pub fn parsed_bytes(&self) -> u64`; `Cursors::read` (`:191`) adds `consumed` to both after each `read_chunk` (the reusable path near `cursor.offset += consumed`, and the fresh path).
- Modify `crates/app/src/chat/view/pane.rs`: `pub static ROWS: AtomicU64` and `pub static ROOT_RENDERS: AtomicU64` beside `RENDERS` (`:30-32`).
- Modify `crates/app/src/chat/view/conversation.rs`: `ROWS.fetch_add(1, Relaxed)` at the top of `row_with_opinions` (`:723`).
- Modify `crates/app/src/chat/view/root.rs`: `ROOT_RENDERS.fetch_add(1, Relaxed)` at the top of `render`.
- Modify `crates/app/src/mac/render.rs`: `bench` (`:75-95`) reads the new counters too and prints, after the unchanged `bench:` line, `bench-json: {…}` with the fields listed in "The bench" (`mode` is `"idle"` until Task 0.3).
- Create `crates/app/src/chat/parse_cost_tests.rs` (`#[cfg(test)] mod parse_cost_tests;` in `chat/mod.rs`).

- [ ] **Step 1: Write the failing test** `an_unchanged_reread_parses_nothing_and_an_append_parses_only_its_line`: `Scratch::new("parse-cost")`; write a 3-line transcript ending in a newline (copy three lines from `crates/app/fixtures/chat/claude/projects/-Users-me-work-notchling/*.jsonl` into the test as a `const`); a local `Cursors::new(CHAT_TAIL_BYTES, 4, |p: &Parser| p.conversation())`; first `read` → `parsed_bytes() == text.len()`; second `read` → unchanged; write again with one more line → the increase equals that line's length plus its newline.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-app parse_cost` — Expected: FAIL (`parsed_bytes` missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check (`-p notchling-engine -p notchling-app`) — Expected: PASS. Run `NOTCHLING_RENDER_BENCH=3 cargo run --release -p notchling-app -- --render chat --out /tmp/c.png` with `NOTCHLING_DATA_ROOT=crates/app/fixtures/chat`: both lines print.
- [ ] **Step 5: Commit** "Chat bench: count rows drawn, root draws and transcript bytes parsed, and print them as JSON".

**Done when:** the test passes and the bench prints a parseable `bench-json:` line.

### Task 0.3: Bench modes, the 200-turn fixture, `tests/bench_chat.rs`

**Goal:** the modes in "The bench", over a conversation long enough to show the cost.

**Files:**
- Create `crates/app/src/mac/bench.rs`: move `bench` and `cpu_ms` from `render.rs` (keeps `render.rs` small); add `pub enum Mode { Idle, Scroll, Hover, Select, Typing, Terminal, Reload, Menu }`, `Mode::parse(&str) -> Option<Mode>`, `pub const MODE_ENV: &str = "NOTCHLING_RENDER_BENCH_MODE"`, and `async fn drive(mode, window: AnyWindowHandle, chat: Option<Entity<ChatView>>, until: Instant, cx: &AsyncApp)`. Input goes through the window handle, `window.update(cx, |_, window, cx| window.dispatch_event(PlatformInput::…, cx))` (`vendor/gpui/src/window.rs:3581`); positions come from `chat::view::bench_point(size) -> Point<Pixels>` (the conversation column's centre from `SIDEBAR_WIDTH` and `inspector::WIDTH`).
- Modify `crates/app/src/mac/render.rs`: `run` keeps `window.window_handle()` and the built view (`views::build`) and passes them to `bench`; `bench` prints `bench: ready` once the settle time is over, then drives the mode for the bench seconds.
- Modify `crates/app/src/chat/view/mod.rs`: `pub fn bench_point`, `pub fn bench_type(&mut self, text: &str, cx)` (`self.tools.editor.update(cx, |e, cx| e.insert(text, cx))`), `pub fn bench_reload(&mut self, cx)` (`reload_now`), `pub fn bench_menu(&mut self, cx)` (`toggle_account_menu`).
- Modify `crates/app/src/chat/view/terminal.rs`: `pub(crate) fn bench_terminal_start(&mut self, cx)` (`render_terminal(false, cx)` then `tick_terminals(cx)`) and `pub(crate) fn bench_terminal_feed(&mut self)` (feeds `"bench line\r\n"` to the active shell's screen and sets its `dirty`, as the pty reader does).
- Create `crates/app/src/chat/bench_fixture.rs` (`pub mod bench_fixture;` in `chat/mod.rs`): pure, `pub fn files(turns: usize, now_ms: i64) -> Vec<(String, String)>` giving relative paths and texts for a data root with one working Claude Code job (`claude/jobs/<id>/state.json` shaped like `crates/app/fixtures/chat-steps`'s, `"state": "working"`) whose transcript has `turns` turns, plus three done jobs with two turns each. A turn is: a user line (about 100 characters), an assistant line with a Bash `tool_use`, its `tool_result` (20 lines of output), and an assistant reply whose text has a heading, a paragraph, a three-item list, an 8-line code block and a 3×3 table. Ids come from the turn number and timestamps count back from `now_ms`, so the same arguments give the same output and the sessions are recent for a render (the bench passes the current time). The root is generated at run time, never checked in: a 200-turn transcript under `crates/` would pass `LINT-FILE-SIZE`.
- Create `crates/app/tests/bench_chat.rs` (`#![cfg(target_os = "macos")]`, every test `#[ignore = "opens a window; a Mac with a display"]`): `fn root(turns) -> TempRoot` writes `bench_fixture::files(turns, now)` under `std::env::temp_dir()` (the plan's only file writes; removed on drop); `fn run(mode, seconds, root, during: impl FnMut(&Path))` spawns `CARGO_BIN_EXE_notchling-app --render chat --out <tmp png>` with `NOTCHLING_DATA_ROOT`, `NOTCHLING_RENDER_BENCH`, `NOTCHLING_RENDER_BENCH_MODE`, reads stdout line by line, calls `during` after `bench: ready`, and returns the parsed `bench-json:` object.

**Tests to write first:**
- `bench_fixture` unit tests: `the_fixture_lists_one_working_job_first` (`DataRoot::fixture` over a `Scratch` copy of `files(5, T)` written with `Scratch::write`: `sessions(T)` lists 4, the first is the working one); `a_long_fixture_fills_the_conversation` (`files(200, T)`'s transcript fed line by line to `conversation::Parser`: at least 400 items, at most the parser's 500); `the_fixture_is_the_same_every_time` (`files(3, T) == files(3, T)`).
- `Mode::parse` unit test `every_mode_parses_and_nothing_else_does`.
- `tests/bench_chat.rs`: one test per mode, `bench_<mode>`, asserting only that the run succeeded and `frames > 0`; plus `bench_append`, whose `during` appends one assistant line to the working job's transcript every 500 ms (`OpenOptions::append`) until the run ends.

- [ ] **Step 1:** Write the unit tests. **Run** `cargo test --locked -p notchling-app bench_fixture mac::bench` — Expected: FAIL.
- [ ] **Step 2: Implement** the fixture, the modes and the driver.
- [ ] **Step 3: Run** Check — Expected: PASS. **Run** Bench — Expected: every mode prints a `bench-json:` line.
- [ ] **Step 4: Commit** "Chat bench: scroll, hover, select, typing, terminal, reload and menu modes over a generated 200-turn conversation" and say in the body: "tests/bench_chat.rs writes its generated root and the appended line under the temp folder (CAP-FILEWRITE, maintainer)".

**Done when:** each mode runs over the 200-turn root and prints numbers; nothing outside the temp folder is written.

### Task 0.4: Baseline numbers

**Goal:** the "before" column.

- [ ] **Step 1:** Run Bench three times on the maintainer's kind of machine (a Mac with a display, nothing else heavy running).
- [ ] **Step 2:** Create `docs/perf/chat-state-split-bench.md` with the results table from "The bench", the machine (model, macOS version), the commit, and the medians as the `0.4 baseline` rows.
- [ ] **Step 3: Commit** "docs: Chat bench baseline before the state split".

**Done when:** every mode has a baseline row.

### Task 1.1: Terminal frames and selection redraw only the root

**Goal (behavior change: fewer redraws, same pixels):** a shell printing at 30 frames a second stops laying out the conversation, the Sidebar and the Details Panel. The terminal panel is drawn by the root view, so `redraw_chrome` is enough.

**Files:** Modify `crates/app/src/chat/view/terminal.rs`: in `tick_terminals` (`:372-400`) replace `cx.notify()` (`:388`) with `view.redraw_chrome(cx)`. The four `window.refresh()` calls of the terminal's selection (`:677, 739, 768, 829`) sit in window mouse handlers that `screen` (`:568`) installs; `screen` gets a `chat: WeakEntity<ChatView>` argument (from `render`, `:869`, `cx.entity().downgrade()`), and each refresh becomes `chat.update(cx, |v, cx| v.redraw_chrome(cx)).ok()` (the handlers' `cx`, unused today). A root-drawn panel needs no more: `redraw_chrome` draws the root. Opening, closing and resizing keep `cx.notify()`.

- [ ] **Step 1: Write the failing bench test** `bench_terminal_draws_no_part` in `tests/bench_chat.rs`: mode `terminal`, 3 s; `conversation == 0 && sidebar == 0 && details == 0`, and `frames >= 30`.
- [ ] **Step 2: Run** it — Expected: FAIL (about 90 of each today).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render, Bench — Expected: PASS; record the `terminal` row.
- [ ] **Step 5: Commit** "Chat: a terminal's output and its selection redraw only the window's outer parts, not the conversation, the Sidebar or the Details Panel".

**Done when:** the new test passes and a terminal still updates live (Render with `NOTCHLING_RENDER_TERMINAL=1`).

### Task 1.2: Simulator frames redraw only the root

**Goal (behavior change: fewer redraws):** the simulator panel is root-drawn; its frames needn't draw the parts.

**Files:** Modify `crates/app/src/chat/view/simulator/mod.rs`: in `watch_simulator` the per-frame `cx.notify()` (`:391`) becomes `v.redraw_chrome(cx)`; `:312` (list loaded) too. Menus, pick, zoom and errors keep `cx.notify()`.

- [ ] **Step 1: Write the failing test** `a_simulator_frame_redraws_only_the_chrome` as a unit test in a new `crates/app/src/chat/view/simulator/frame_tests.rs`, if the frame path can be reached without a simulator; otherwise state in the commit that it is covered by Render with `NOTCHLING_RENDER_SIMULATOR` and the manual check (Task 8.2). Do not start `xcrun`.
- [ ] **Step 2: Implement.** **Run** Check and Render — Expected: PASS.
- [ ] **Step 3: Commit** "Chat: iOS Simulator frames redraw only the window's outer parts".

**Done when:** Check and Render pass; the simulator render still shows its frame.

### Task 1.3: Typing and the slash menu redraw only the root

**Goal (behavior change: fewer redraws):** the reply box, the slash menu and the pickers are root-drawn and no part shows a draft, so a keystroke needn't lay out the conversation.

**Files:** Modify `crates/app/src/chat/view/tools.rs`: `draft_changed` (`:194-208`) ends with `self.redraw_chrome(cx)` instead of `cx.notify()`. Modify `crates/app/src/chat/view/slash_menu.rs`: `pick_slash` (`:56`) and `capture_slash_key` (`:91`) likewise.

- [ ] **Step 1: Write the failing bench test** `bench_typing_draws_no_part`: mode `typing`, 3 s; `conversation == 0 && sidebar == 0 && details == 0`.
- [ ] **Step 2: Run** — Expected: FAIL (about 30 each today).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render (with `NOTCHLING_RENDER_TYPED=1` and `NOTCHLING_RENDER_SLASH=/`), Bench — Expected: PASS.
- [ ] **Step 5: Commit** "Chat: typing in the reply box no longer lays out the conversation, the Sidebar and the Details Panel".

**Done when:** the test passes; the box still grows from one to eight lines (the conversation's box then changes size, which draws it, as it should).

### Task 1.4: The notch redraws only when what it draws changed

**Goal (behavior change: fewer redraws):** the notch's 2 s poll notifies only on a visible change.

**Files:**
- Modify `crates/app/src/notch/model.rs`: `#[derive(Clone, Debug, PartialEq)] pub struct Drawn { agents: Vec<Agent>, snapshots: Vec<(String, Snapshot)> (sorted by id), requests: Vec<(String, HookRequest)> (sorted), state: NotchState, tab: AgentGroup, notch: Size, width: NotchWidth, show_last_reply: bool, daemon_note: Option<&'static str>, answered: Vec<String> (sorted), card_height: Option<f64>, ages: Vec<String> }` and `pub fn drawn(&self) -> Drawn`, where `ages` are the strings the rows show, made by the same function the rows call for each agent's age. Derive `PartialEq` on the types that lack it (only those named here).
- Modify `crates/app/src/notch/panel.rs`: before the update in the poll (`:245-267`) take `let before = view.model.drawn();`, and replace `cx.notify()` (`:267`) with `if view.model.drawn() != before { cx.notify(); }`.
- Create `crates/app/src/notch/drawn_tests.rs` (`model.rs` is 556 lines and `model_tests.rs` is 445).

- [ ] **Step 1: Write the failing tests:** `the_same_poll_twice_draws_nothing_new` (two `update`s with the same agents: `drawn()` equal); `the_notch_redraws_when_an_age_changes` (same agents, `now_ms` moved past the next age step: `drawn()` differs); `a_new_snapshot_line_is_a_change`; `a_peek_is_a_change`.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-app drawn_tests` — Expected: FAIL.
- [ ] **Step 3: Implement.** **Run** Check and `cargo test --locked -p notchling-app --test render_notch -- --ignored` — Expected: PASS.
- [ ] **Step 4: Commit** "Notch: a poll that changed nothing on screen doesn't draw the notch again".

**Done when:** the four tests pass and the notch renders as before.

### Task 1.5: Scroll bar, Details fold and selection notify their own view

**Goal (behavior change: fewer redraws):** no `window.refresh()` in these paths, so the other cached parts keep their caches.

**Files:**
- Modify `crates/kit/src/ui/scrollbar.rs`: in `paint`, take `let view = window.current_view();` (as the fade already does, `:252`) and give it to the drag, hover, press and release handlers; `window.refresh()` at `:316, 327, 363, 377` becomes `cx.notify(view)`.
- Modify `crates/app/src/chat/view/inspector.rs`: the fold header's click (`:229-232`) calls `chat.update(cx, |v, cx| v.redraw_details(cx))` through the panel's `chat: WeakEntity<ChatView>` (pass it into the fold helper) instead of `window.refresh()`.
- Modify `crates/app/src/chat/view/pane.rs`: `pub(super) fn redraw_details(&self, cx)` beside `redraw_sidebar` (`:182`).
- Modify `crates/app/src/chat/view/select.rs`: the listener's paint takes `let view = window.current_view();` and the three `window.refresh()` (`:196, 219, 241`) become `cx.notify(view)`. The root redraws on any frame, so the file panel's wash still follows.

- [ ] **Step 1: Write the failing bench test** `bench_select_draws_no_sidebar_or_details`: mode `select`, 3 s; `sidebar == 0 && details == 0`.
- [ ] **Step 2: Run** — Expected: FAIL (a refresh per move today).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check (`-p notchling-kit -p notchling-app`), Render, Bench — Expected: PASS. In a release build over `crates/app/fixtures/chat` (`--open-chat`): drag the conversation's bar, fold a Details section, select text in the conversation and in an open file panel; each still updates at once.
- [ ] **Step 5: Commit** "Chat and kit: the scroll bar, a Details fold and a selection drag redraw their own view, not the whole window".

**Done when:** the test passes and the four interactions still update immediately.

### Task 2.1: `list` spike: probe view and go/no-go

**Goal:** decide, before any conversation code depends on it, whether GPUI's `list` (`vendor/gpui/src/elements/list.rs`) does what the conversation needs.

**Files:**
- Create `crates/app/src/chat/view/list_probe.rs` (render-only view, kept as a regression probe) and `crates/app/src/chat/renders.rs` with `pub const VIEWS: [ViewSpec; 1]` (`chat-list-probe`, 900×700); chain it in `crate::views::every()` and add `.or_else(|| crate::chat::view::list_probe::build_render(other, cx))` in `crates/app/src/mac/views.rs`.
- Create `crates/app/tests/render_list_probe.rs` (ignored, macOS).

**The probe:** an outer view holding a cached child view (as `ConversationView` will be) with `ListState::new(300, ListAlignment::Bottom, px(600.))`, 300 synthetic rows of 1 to 12 lines (deterministic from the index), each with `pt(14)`, a kit spinner in the last row, `list(state, cx.processor(|this, ix, window, cx| this.row(ix, window, cx)))`. A scripted timeline (a spawned task, one step per frame) records, from `bounds_for_item` and `viewport_bounds`:
- **G1 follow:** while following, grow the last row 10 times (change its text, `splice(299..300, 1)`, notify). Max gap between the last row's bottom and the viewport's bottom: ≤ 1 px.
- **G2 anchor:** `scroll_by(px(-3000.))`. Note the first visible row and its top. Splice a row above it with a new height, then the first visible row itself through `remeasure` (save `logical_scroll_top()`, splice, `scroll_to` the saved offset). Its top moves ≤ 1 px each time.
- **G3 cost:** 60 frames of `scroll_by(±40)`. Average `render_item` calls per frame ≤ visible rows + 2.
- **G4 resize:** change the probe column's width (600 → 480 pt, a style on its container). G1 and G2 still hold after it, and every visible row's bounds are taller than 0.
- **G5 at the bottom:** `set_offset_from_scrollbar` to the middle, then to the end: `logical_scroll_top().item_ix == item_count()`, or the last row's bottom within 1 px of the viewport's.
- **G6 spinners:** after a splice that triggers the list's autoscroll retry, the spinner slots recorded for the frame equal the spinners on screen (1). Add `SpinSlots::len` to `kit::ui::spin_layer` if it has no way to count them.

It prints `list-probe: {"g1":…,"g2":…,"g3":…,"g4":…,"g5":…,"g6":…}` and exits 0.

- [ ] **Step 1: Write the ignored test** `the_list_does_what_the_conversation_needs`: runs `--render chat-list-probe`, parses the line, asserts each gate.
- [ ] **Step 2: Implement** the probe. **Run** Check and `cargo test --release --locked -p notchling-app --test render_list_probe -- --ignored --nocapture`.
- [ ] **Step 3:** Write the result in `docs/perf/chat-state-split-bench.md` ("List spike": each gate's number, go or no-go).
- [ ] **Step 4: Commit** "Chat: a list probe that checks GPUI's list for the conversation (follow, anchor, cost, resize, spinners)".

**Go:** G1-G5 pass. G6 failing alone is still go: Task 6.1 then draws the live row's spinner as a plain kit spinner view inside the row, not a slot.

**No-go:** any of G1-G4 fails. Then Task 6.1-alt replaces 6.1 and 6.2, and the maintainer decides whether a patch to `vendor/gpui/src/elements/list.rs` is worth trying instead (protected).

**Done when:** the gates are measured and the decision is written down.

### Task 3.1: Engine: `Arc<Conversation>` from the cursors

**Goal:** an unchanged transcript costs a pointer copy, and Chat can tell "unchanged" by pointer.

**Files:**
- Modify `crates/engine/src/transcript/conversation/mod.rs`: `session_cursors()` (`:32-35`) and `subagent_cursors()` (`:37-41`) hold `Arc<Conversation>` (`Cursors::new(CHAT_TAIL_BYTES, 60, |p: &Parser| Arc::new(p.conversation()))`); add `pub fn read_shared(path: &str) -> Arc<Conversation>` and `pub fn read_subagent_shared(path: &str) -> Arc<Conversation>`; `read` and `read_subagent` keep their signatures and return `(*read_shared(path)).clone()`.
- Modify `crates/engine/src/transcript/codex/mod.rs`: `cursors()` (`:441-448`) likewise; `pub fn read_shared`; `read` (`:451`) unchanged in signature.
- Modify `crates/engine/src/transcript/subagents.rs`: `pub fn read_shared(path) -> Arc<Conversation>` beside `read` (`:215`).
- Modify `crates/engine/src/transcript/mod.rs`: the table line for `tail::Cursors` names `read_shared`.

- [ ] **Step 1: Write the failing tests** (in each module's test file, inline transcripts only, no new file writes in the engine): `an_unreadable_path_reads_as_an_empty_shared_conversation` (`read_shared("/nonexistent")` has no items); and in the app crate, `crates/app/src/chat/parse_cost_tests.rs`: `an_unchanged_file_gives_the_same_arc` (`Scratch` transcript; two `conversation::read_shared` calls are `Arc::ptr_eq`; after an append they are not).
- [ ] **Step 2: Run** `cargo test --locked -p notchling-engine transcript && cargo test --locked -p notchling-app parse_cost` — Expected: FAIL.
- [ ] **Step 3: Implement.** **Run** Check (`-p notchling-engine -p notchling-app`) — Expected: PASS, every existing `read` test unchanged.
- [ ] **Step 4: Commit** "Transcripts: the conversation readers hand out a shared conversation, so an unchanged file costs a pointer".

**Done when:** both tests pass; no existing test changed.

### Task 3.2: `chat::log`: the change description

**Goal:** what changed between two conversations, by item, computed off the main thread.

**Files:** Create `crates/app/src/chat/log.rs` and `crates/app/src/chat/log_tests.rs`; `pub mod log;` in `chat/mod.rs`.

**Interfaces (produces):**

```rust
/// Items `old` in the previous conversation became items `new` in this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Splice { pub old: Range<usize>, pub new: Range<usize> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Same `Arc`, or equal: nothing the column shows changed.
    Same,
    /// Splices in old-item order, none overlapping; `other` says a field besides `items` changed
    /// (tasks, facts, queued, activity, turn start or tokens, finished agents, shells, pending question).
    Items { splices: Vec<Splice>, other: bool },
    /// The ids don't line up (another session, a fresh cursor renumbered them): start over.
    Reset,
}

pub const RESET_ABOVE: usize = 64;
pub fn diff(old: &Arc<Conversation>, new: &Arc<Conversation>) -> Change;
```

Rules: `Arc::ptr_eq` → `Same`. Either side empty while the other isn't → `Reset`. Find the first new item's id in the old items at `k` (not found → `Reset`): old `0..k` is a front trim (`Splice { old: 0..k, new: 0..0 }`). Walk the aligned pairs `old[k+i]`, `new[i]`: an id mismatch → `Reset`; an unequal pair is a one-item splice, and neighbouring ones merge; more than `RESET_ABOVE` unequal pairs → `Reset`. The rest: old longer → `Splice { old: k+m..old.len(), new: m..m }`; new longer → `Splice { old: old.len()..old.len(), new: m..new.len() }`. No splices and `other == false` → `Same`.

- [ ] **Step 1: Write the failing tests** in `log_tests.rs` (conversations built from `ChatItem`s with ids `i1…`): `the_same_arc_is_same`; `equal_copies_are_same`; `an_appended_item_is_one_splice_at_the_end` (`[Splice { old: 3..3, new: 3..4 }]`); `a_changed_last_item_is_replaced_in_place` (a tools group gained a step: `old: 2..3, new: 2..3`); `a_front_trim_and_an_append_are_two_splices`; `renumbered_ids_reset`; `an_empty_side_resets`; `more_than_reset_above_changes_resets`; `only_the_queue_changed_is_items_with_no_splices_and_other`.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-app log_tests` — Expected: FAIL.
- [ ] **Step 3: Implement** `diff`.
- [ ] **Step 4: Run** Check — Expected: PASS.
- [ ] **Step 5: Commit** "Chat: describe what changed between two loads of a conversation, by item".

**Done when:** the nine tests pass.

### Task 3.3: Loader and `ChatView` carry `Arc` and the change

**Goal:** no conversation copy on the main thread, no item-by-item compare there, and the change ready for the list.

**Files:**
- Modify `crates/app/src/chat/data.rs`: `pub fn conversation_shared(&self, session) -> Arc<Conversation>` (Claude → `conversation::read_shared`, Codex → `codex::read_shared`, Antigravity → `Arc::default()`); `conversation` (`:307-318`) stays and returns `(*self.conversation_shared(session)).clone()`.
- Modify `crates/app/src/chat/loader.rs`: `Loaded.conversation: Arc<Conversation>` (`:25`); `Loaded.subagent: Option<(String, Arc<Conversation>)>`; new `pub change: Option<(Arc<Conversation>, log::Change)>` (the conversation the change starts from, and the change); `Loader` keeps `last: Option<(String, Arc<Conversation>)>` (session id, conversation) and fills `change` in `load_live` (`:89-156`) when the selected session is the same as last time, else `None`. The existing loader tests read `.conversation.items` through `Deref` and stay unchanged.
- Modify `crates/app/src/chat/subagent_view.rs`: `Open.conversation: Option<Arc<Conversation>>`; `pub fn apply(&mut self, listed: &[Subagent], read: Option<(String, impl Into<Arc<Conversation>>)>)`, so the existing test that passes `Conversation::default()` compiles unchanged.
- Modify `crates/app/src/chat/view/mod.rs` and `load.rs`: `ChatView.conversation: Arc<Conversation>`; in `apply` (`load.rs`, from `mod.rs:671`) `self.loading || self.conversation != loaded.conversation` becomes `self.loading || content_changed`, with

```rust
let content_changed = match &loaded.change {
    // The loader compared against the conversation on show: trust it.
    Some((from, change)) if Arc::ptr_eq(from, &self.conversation) => *change != Change::Same,
    // Anything else (first load, Antigravity, a subagent in between): pointer, then content.
    _ => !Arc::ptr_eq(&self.conversation, &loaded.conversation)
        && *self.conversation != *loaded.conversation,
};
```

  Keep `loaded.change` in a new field `pending_change: Option<(Arc<Conversation>, Change)>` (read in Task 4.4). `tools.rs:817` (Antigravity) wraps its conversation in `Arc::new`. Other readers of `view.conversation` keep compiling through `Deref` (`&*view.conversation` where a `&Conversation` is asked for).

- [ ] **Step 1: Write the failing tests** in `loader.rs`'s neighbour `crates/app/src/chat/loader_change_tests.rs`: `an_unchanged_reload_is_same_and_the_same_arc` (`Scratch` root built from the `chat` fixture's files with `Scratch::write`; two `Loader::load(selected, now)` calls: `Arc::ptr_eq` and `Change::Same`); `an_appended_line_is_one_splice` (write again with one more assistant line: `Items` with one splice at the end); `another_session_has_no_change` (switch the selected id: `change == None`).
- [ ] **Step 2: Run** `cargo test --locked -p notchling-app loader_change` — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render, Bench — Expected: PASS; record `reload` and `append`.
- [ ] **Step 5: Commit** "Chat: loads share the conversation and say what changed, so the main thread no longer copies or compares it item by item".

**Done when:** the three tests pass; no existing test changed; `reload`'s `conversation` count stays 0.

### Task 3.4: The pending question from the conversation reader

**Goal:** one reader for the conversation and its open question; both come from the same bytes.

**Files:**
- Modify `crates/engine/src/transcript/snapshot.rs`: `QuestionItem` and `PendingQuestion` (`:29-43`) derive `Serialize, Deserialize` (`rename_all = "camelCase"` as `Conversation`'s neighbours do).
- Modify `crates/engine/src/transcript/model.rs`: `Conversation` (`:182-200`) gets `#[serde(skip_serializing_if = "Option::is_none")] pub pending_question: Option<PendingQuestion>`, so `agy.conversation` answers serialize as before.
- Modify `crates/engine/src/transcript/conversation/parser.rs`: fields `last_question: Option<PendingQuestion>`, `last_question_answered: bool`; in `tool_use` (`:290`), for `AskUserQuestion` with an id, `snapshot::question(id, input)` sets them as `SnapshotParser::tool_use` does (`snapshot.rs:211-216`); in `tool_result` (`:399`), before its early returns, a result for that id sets `last_question_answered = true` (as `snapshot.rs:161-167`); `conversation()` (`:517`) sets `pending_question` to the question unless answered. `parser.rs` stays under 600 lines.
- Modify `crates/app/src/chat/loader.rs`: `question` (`:123-126`) comes from `conversation.pending_question.clone()` under the same filter (Claude, not a command job), instead of `snapshot::read`.
- Create `crates/app/src/chat/question_parity_tests.rs`.

- [ ] **Step 1: Write the failing tests:** engine, inline lines into `Parser::consume`: `an_unanswered_question_is_pending`, `an_answered_question_is_not`, `the_last_question_wins`, `a_sidechain_question_is_not_the_sessions`. App: `the_conversation_and_the_snapshot_agree_on_every_fixture` (every `*.jsonl` under `crates/app/fixtures/**/claude/projects/`, `crates/app/fixtures/notch/claude/projects/-work-site/sess-release.jsonl` among them: `conversation::read(p).pending_question == snapshot::read(p).question`).
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.** **Run** Check (`-p notchling-engine -p notchling-app -p notchling-notchlingd`), Render — Expected: PASS (no golden changes).
- [ ] **Step 4: Commit** "Transcripts: the conversation reader finds the open question too, so Chat reads one file once per load".

**Done when:** parity holds on every fixture; no golden changed.

### Task 4.1: Model parts; the Sidebar draws from `SidebarModel`

**Goal (behavior change: fewer redraws):** the Sidebar draws only when what it shows changed.

**Files:**
- Modify `crates/app/src/chat/view/pane.rs`: keep today's `Part` for the conversation as `RawPart` (until Task 4.3); add

```rust
/// What a part draws, computed from the chat on each of its notifies.
type Select<M> = fn(&ChatView, &App) -> M;
/// Draws a part from its model; listeners still reach ChatView through `cx`.
type DrawFrom<M> = fn(&M, Appearance, &mut Context<ChatView>) -> AnyElement;

pub struct ModelPart<M: PartialEq + 'static> {
    chat: WeakEntity<ChatView>,
    select: Select<M>,
    draw: DrawFrom<M>,
    /// The model drawn last; `None` asks for a fresh one at the next draw.
    model: Option<M>,
    renders: &'static AtomicU64,
    spins: SpinSlots,
    _changes: Subscription, // observe(chat): compute the model; notify only when it differs
    _release: Subscription,
}
impl<M: PartialEq + 'static> ModelPart<M> {
    /// Computes the model again and draws only if it changed.
    pub fn refresh(&mut self, chat: &ChatView, cx: &mut Context<Self>);
    /// Draws again whatever the model says (for what the model can't see: the appearance, folds).
    pub fn redraw(&mut self, cx: &mut Context<Self>); // model = None; notify
}
```

  `Render for ModelPart<M>` takes the model out (computing it if `None`), draws it inside `chat.update(cx, |_, ccx| draw(&model, a, ccx))`, and puts it back. `Parts.sidebar: Entity<ModelPart<SidebarModel>>`. `redraw_sidebar` (`:182`) and `redraw_all_but_conversation` (`:188`) call `refresh` (so a reload that changed nothing shown draws nothing); new `ChatView::redraw_parts(cx)` calls `redraw` on all three, from the appearance observer in `with_source` (`mod.rs:371`).
- Modify `crates/app/src/chat/sidebar.rs` (pure): 

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarModel {
    pub sections: Vec<Section>,
    /// Each row's account, in row order across the sections.
    pub accounts: Vec<Option<RowAccount>>,
    pub selected: Option<String>,
    pub daemon_note: Option<&'static str>,
}
pub fn sidebar_model(sessions: &[Session], now_ms: i64, selected: Option<&str>,
                     snapshot: &Snapshot, daemon_note: Option<&'static str>) -> SidebarModel
```
- Modify `crates/app/src/chat/view/sidebar.rs`: `pub(super) fn model(view: &ChatView, cx: &App) -> SidebarModel` (from `view.sessions`, `view.now_ms`, `view.selected`, `view.accounts.snapshot()`, `view.daemon_note`); `render` (`:194`) becomes `render(m: &SidebarModel, a, cx: &mut Context<ChatView>)` and reads only `m` (the listeners keep `cx.listener`). The part names need nothing: `set_parts` refreshes every window.

**Moves:** none; `ChatView` keeps every field. The model is computed from `sessions` (115), `now_ms` (126), `selected` (119), `accounts` (212), `daemon_note` (144).

- [ ] **Step 1: Write the failing tests** in `chat/sidebar.rs`'s tests: `the_model_is_equal_when_nothing_shown_changed` (same inputs, a different draft or menu state: equal); `a_new_age_or_status_changes_the_model`; `a_changed_row_account_changes_the_model`; `selecting_another_row_changes_the_model`. Bench tests `bench_reload_draws_no_sidebar` (mode `reload`, 3 s: `sidebar <= 1`, since an age may tick over once) and `bench_menu_draws_no_sidebar` (mode `menu`: `sidebar == 0`).
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render (also `NOTCHLING_RENDER_SIDEBAR_HIDDEN=1` and the row menu render), Bench — Expected: PASS.
- [ ] **Step 5: Commit** "Chat: the Sidebar draws from its own model and only when that changes".

**Done when:** tests pass; the Sidebar still follows selection, new rows, ages, accounts, the daemon note, dark/light switching and the part names.

### Task 4.2: The Details Panel draws from `DetailsModel`

**Goal (behavior change: fewer redraws):** same for the Details Panel.

**Files:**
- Modify `crates/app/src/chat/view/inspector.rs`: 

```rust
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DetailsModel {
    pub i: Inspector,
    /// The session's folder as shown (`~/…`), and as a path.
    pub folder: Option<(String, String)>,
    pub agent_ids: Vec<String>,
}
pub(super) fn model(view: &ChatView, cx: &App) -> DetailsModel   // what `render` (:57-70) computes today
pub fn render(m: &DetailsModel, a: Appearance, cx: &mut Context<ChatView>) -> impl IntoElement
```
  The fold state stays in its global; a fold click already redraws through `redraw_details` (Task 1.5), which now calls `ModelPart::redraw`.
- Modify `crates/app/src/chat/view/pane.rs`: `Parts.details: Entity<ModelPart<DetailsModel>>`.

- [ ] **Step 1: Write the failing tests:** `the_details_model_is_equal_for_the_same_inputs`; `a_new_task_changes_the_details_model`; `a_shell_writer_appearing_changes_the_details_model` (the `writers` closure answers a pid for the shell's output path); bench tests `bench_reload_draws_no_part` (mode `reload`, 3 s: `conversation == 0 && sidebar <= 1 && details == 0`) and `bench_menu_draws_no_details` (mode `menu`: `details == 0`).
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render (with `NOTCHLING_RENDER_DETAILS_HIDDEN=1` too), Bench — Expected: PASS.
- [ ] **Step 5: Commit** "Chat: the Details Panel draws from its own model and only when that changes".

**Done when:** tests pass; folds, shell writers (Output, Stop), subagent rows and the folder line still update.

### Task 4.3: `ConversationView` entity (move, same redraws)

**Goal:** the conversation's own state lives in its own entity. Behavior and redraws stay exactly as today (it still observes `ChatView`).

**Files:**
- Create `crates/app/src/chat/view/conversation_view.rs` (under 300 lines):

```rust
pub struct ConversationView {
    chat: WeakEntity<ChatView>,
    pub(super) s: ConversationState,
    spins: SpinSlots,
    _changes: Subscription, // observe(chat) → notify, as RawPart did
    _release: Subscription,
}

pub(super) struct ConversationState {
    pub scroll: ScrollHandle,
    pub markdown: RefCell<markdown_cache::Cache>,
    pub expanded: HashSet<String>,
    pub expand_all: bool,
    pub actions_preview: bool,
    pub latest_preview: bool,
    pub copied: Option<(String, i64)>,
    pub hovered_message: Option<String>,
    pub drawn_ms: i64,
    pub follow_sent: bool,
    pub scroll_to_end: bool,
}

impl ConversationState {
    pub fn is_open(&self, id: &str) -> bool;            // from ChatView::is_open (mod.rs:345)
    pub fn toggle(&mut self, id: &str);                  // from ChatView::toggle (mod.rs:355), no notify
    pub fn hover(&mut self, id: &str, hovered: bool) -> bool; // from message_hover (mod.rs:333): true = redraw
    pub fn follow_content(&mut self, session_changed: bool);  // from pane.rs:198
    pub fn latest_pill(&self, loading: bool) -> bool;    // from pane.rs:211
    pub fn jump_to_latest(&mut self);                    // from pane.rs:225, no notify
    pub fn scroll_to_end_now(&mut self);                 // the `scroll_to_bottom()` callers use this
}
```

  `Render for ConversationView`: `RENDERS += 1`, `spins.clear()`, `s.drawn_ms = now_ms()`, then `chat.update(cx, |chat, ccx| draw(chat, &self.s, a, ccx))` with today's `draw_conversation` body (`pane.rs:106-115`).
- Modify `crates/app/src/chat/view/pane.rs`: `Parts.conversation: Entity<ConversationView>`; delete `RawPart` and `draw_conversation`; move `follow_content`, `latest_pill`, `jump_to_latest` (`:198-230`) into `ConversationState` (above).
- Modify `crates/app/src/chat/view/mod.rs`: remove the fields below from `ChatView`; `toggle`, `message_hover`, `is_open` and `tasks_open` forward to `self.parts.conversation` (`update` for writes, then `cx.notify()` as today; `read(cx)` for reads).
- Modify every reader listed in the move table: `conversation.rs`, `steps_view.rs`, `message_actions.rs`, `tasks.rs`, `subagent.rs`, `tools.rs`, `prompts.rs`, `page_panel.rs`, `load.rs`. The draw functions in `conversation.rs`, `steps_view.rs` and `message_actions.rs` get a second argument `s: &ConversationState` and read the moved fields from it; listeners that changed them call the `ChatView` forwarders (`this.toggle(&id, cx)` stays as written).

**Field moves (`ChatView` → `ConversationState`):**

| Field (`mod.rs` line) | New name | Readers and writers to update |
| --- | --- | --- |
| `conversation_scroll` (150) | `scroll` | `conversation.rs:787`, `pane.rs:113,199,215,227`, `prompts.rs:197`, `subagent.rs:74,83`, `tools.rs:377`, `load.rs` (was `mod.rs:678`): all `scroll_to_bottom()` calls become `scroll_to_end_now()` |
| `markdown` (152) | `markdown` | `conversation.rs:652,674`, `load.rs` (`reloaded`, `switched`), `page_panel.rs:660` |
| `expanded` (163) | `expanded` | `mod.rs:345-360` forwarders |
| `expand_all` (166) | `expand_all` | `new` (`mod.rs:264`) sets it on the entity |
| `actions_preview` (175) | `actions_preview` | `message_actions.rs:184,234`; `new` |
| `latest_preview` (173) | `latest_preview` | `pane.rs:216,226`; `new` |
| `copied` (177) | `copied` | `message_actions.rs:79`; `tick` (`load.rs`, was `mod.rs:805-808`) |
| `hovered_message` (180) | `hovered_message` | `message_hover`; `tick` (was `mod.rs:811`) |
| `drawn_ms` (183) | `drawn_ms` | `pane.rs:108` (now the entity's render), `tick`, `tools.rs:621,634` |
| `follow_sent` (185) | `follow_sent` | `pane.rs:204`, `prompts.rs:196`, `tools.rs:376,501` |
| `scroll_to_end` (135) | `scroll_to_end` | `load.rs` (was `mod.rs:656,668,758`), init `true` |

Stay on `ChatView` (read by other areas or by the load): `conversation`, `subagents`, `open_subagent`, `loading`, `tasks_seen`, `tasks_open_preview`, `drop_preview`, `prompt`, `selected`.

- [ ] **Step 1: Write the failing tests** (pure, `crates/app/src/chat/view/conversation_state_tests.rs`): `toggle_opens_then_closes`; `expand_all_opens_everything`; `hover_asks_for_a_redraw_only_when_times_are_stale`; `the_pill_shows_when_scrolled_up_and_not_while_loading` (a `ScrollHandle` with no layout reads as at the bottom: no pill; `latest_preview` forces it).
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement** the move. No logic change: each moved body is the old one with `self.` paths adjusted.
- [ ] **Step 4: Run** Check, Render (`NOTCHLING_RENDER_EXPAND=1`, `NOTCHLING_RENDER_LATEST=1`, `NOTCHLING_RENDER_ACTIONS=1`, `NOTCHLING_RENDER_TASKS_OPEN=1` too), Bench — Expected: PASS and the same counts as after Task 4.2 (within noise).
- [ ] **Step 5: Commit** "Chat: the conversation's own state (scroll, open groups, hover, Copy, Markdown cache) lives in its own view (moves only)".

**Done when:** `ChatView` has 11 fields fewer, the renders look the same, and the bench counts match Task 4.2's.

### Task 4.4: `ConversationView` draws from `ConversationInput`

**Goal (behavior change: fewer redraws):** the conversation draws only when its input or its own state changed; its draw reads nothing from `ChatView`.

**Files:**
- Modify `crates/app/src/chat/view/conversation_view.rs`:

```rust
#[derive(Clone)]
pub(super) struct ConversationInput {
    /// What the column shows: the open subagent's conversation or the session's.
    pub shown: Arc<Conversation>,            // compared by Arc::ptr_eq first (PartialEq impl below)
    pub subagent: Option<SubagentShown>,     // { agent_id: String, running: bool, loaded: bool }
    pub session: Option<SessionShown>,       // { id: String, tool: Tool, cwd: String, working: bool }
    pub held_working: bool,                  // selected_held_working()
    pub outgoing: Vec<String>,               // outgoing_texts(), empty while a subagent is open
    pub loading: bool,
    pub empty_note: Option<&'static str>,    // the note conversation.rs:823-836 picks for empty rows
    pub subagents: Vec<Subagent>,            // for an Agent step's Open
    pub tasks_inset: bool,                   // tasks::shown(view)
    pub selected: Option<String>,            // the opinions store's key
}
impl ChatView { pub(super) fn conversation_input(&self, cx: &App) -> ConversationInput }
```

  A hand-written `PartialEq for ConversationInput` compares `shown` by `Arc::ptr_eq(a, b) || **a == **b` and the rest by field (`SubagentShown` and `SessionShown` derive `Clone, PartialEq`). `empty_note` needs Acting: `ChatView::acting` takes `&mut Context<Self>` but only reads the acting store, so add `fn acting_in(&self, cx: &App) -> Acting` beside it (`tools.rs:189`) and use that in `conversation_input`. The `_changes` observer computes the input and notifies only when it differs (the `ModelPart` pattern). The entity keeps `input: ConversationInput` and `chat: WeakEntity<ChatView>`.
- Modify `crates/app/src/chat/view/conversation.rs`, `steps_view.rs`, `message_actions.rs`, `opinions.rs` (`cards`), `subagent.rs` (Agent step Open): draw functions take `(input: &ConversationInput, s: &ConversationState, a, cx: &mut Context<ConversationView>)`. Listeners that change conversation state use `cx.listener` on `ConversationView` (toggle, hover, Copy, Latest); listeners that act on the session capture `chat: WeakEntity<ChatView>` and call `chat.update(cx, |v, cx| v.open_subagent(&id, cx))` (also `open_shell_output`, `retry_stalled`, `open_second_opinion`, opinions cancel/remove, `page_panel::follow_link` callers).
- Modify `crates/app/src/chat/view/mod.rs` and `load.rs`: the forwarders from Task 4.3 notify the entity, not `ChatView` (`self.parts.conversation.update(cx, |v, cx| { v.s.toggle(id); cx.notify() })`); `tick`'s Copy and hover clocks and `opinions::any_running` notify the entity; `apply` hands `pending_change` to the entity (`v.take_change(change)`, kept for Task 6.1) before notifying.
- `latest_pill` moves from the root (`root.rs`, was `mod.rs:872`) into the entity's render, absolutely placed as today, so it follows scrolling.

- [ ] **Step 1: Write the failing tests:** `the_input_is_equal_when_only_a_draft_or_a_menu_changed` (unit, from two `ChatView`-free constructions of the input); `a_new_conversation_arc_changes_the_input`; bench tests `bench_menu_draws_no_part` (mode `menu`, 3 s: `conversation == 0 && sidebar == 0 && details == 0`; about 15 conversation draws before this task) and `bench_typing_still_draws_no_part`.
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render (all the conversation renders from Task 4.3), Bench — Expected: PASS. In a release build over `crates/app/fixtures/chat-steps` (`--open-chat`): open and close step groups and thoughts, hover a message (actions show, the time refreshes), Copy (check mark goes after a moment), open a subagent and come back, send nothing; each behaves as before.
- [ ] **Step 5: Commit** "Chat: the conversation draws from its own input and state, and only when one of them changed".

**Done when:** `ChatView` notifications that change nothing in the conversation draw 0 conversations (bench), and the checks above behave as before.

### Task 5.1: `plain_blocks` and the shared text helpers

**Goal:** each row's selectable texts, from the model, in the order the row draws them, without drawing it.

**Files:**
- Create `crates/app/src/chat/blocks.rs` and `crates/app/src/chat/blocks_tests.rs`; `pub mod blocks;` in `chat/mod.rs`.

```rust
/// What decides which of a row's texts are drawn.
pub struct Shown<'a> {
    pub is_open: &'a dyn Fn(&str) -> bool,
    /// This row is the live step group (always open, its running call spinning).
    pub live: bool,
    pub is_file: &'a dyn Fn(&str) -> bool,
}
/// The row's selectable texts in draw order. `message` is the row's parsed Markdown
/// (`markdown_cache::Cache::message`) for an assistant row, else `None`.
pub fn plain_blocks(row: &Row, message: Option<&[Part]>, shown: &Shown) -> Vec<String>
```

  Per row: `User` → the words from `drops::split_images(text, is_file)` when the bubble draws them (`!words.is_empty() || pictures.is_empty()`, `conversation.rs:535`), else nothing. `Assistant` → per Markdown part in `markdown.rs` `render` order (`:435`): heading and paragraph → the inline text; list → each item's inline text in order; quote → its inline text; code → the code text; table → header cells, then each row's cells; rule → nothing. `Tools` → when `live || is_open(group id)`: per step whose detail is open (`is_open(step id)` and `steps::detail` is `Text`), its input text then its output text. `CommandOutput` → its text. `Thinking`, `TurnEnd`, `Notice` → nothing.
- Modify `crates/app/src/chat/markdown_cache.rs`: `impl Inline { pub fn text(&self) -> String }` (the concatenation `view/markdown.rs:27-40` builds); `view/markdown.rs` `inline` calls it.
- Modify `crates/app/src/chat/steps.rs`: `pub fn input_text(step: &ToolRow) -> Option<String>` and `pub fn output_text(step: &ToolRow) -> Option<String>` (the strings `step_detail` gives `mono_box`, the output with its `"\n… +{n} lines"`); `view/steps_view.rs` `step_detail` calls them.
- The render check `NOTCHLING_RENDER_CHECK_BLOCKS=1` (with `NOTCHLING_RENDER_EXPAND=1`): `crates/app/src/chat/view/select.rs` gets `pub fn noted() -> Vec<(String, String)>` (key and text, in note order); `ChatView::check_blocks(&self, cx: &App) -> Result<usize, String>` (in `conversation.rs`) compares them, with no file panel open, to the concatenation of `plain_blocks` over the shown rows (Markdown from the cache, `is_open` from the open groups); `crates/app/src/mac/render.rs` calls it after settle and prints `blocks-check: ok <n>`, or `blocks-check: mismatch at <i>: drawn <key> <text> model <text>` and exits 1.
- Create `crates/app/tests/render_blocks.rs` (ignored): runs the check over `fixtures/chat`, `fixtures/chat-steps` and the bench fixture.

- [ ] **Step 1: Write the failing tests** in `blocks_tests.rs`: `a_user_row_is_its_words_without_image_paths`; `a_picture_only_message_has_no_block`; `markdown_parts_give_blocks_in_draw_order` (heading, paragraph, list of 3, code, 2×2 table, rule: 1+1+3+1+(2+4)+0 blocks in that order); `a_closed_group_has_no_blocks_and_an_open_step_has_input_then_output`; `the_live_group_counts_as_open`; `a_long_output_ends_with_its_more_lines_note`; `thinking_turn_ends_and_notices_have_none`.
- [ ] **Step 2: Run** `cargo test --locked -p notchling-app blocks` — Expected: FAIL.
- [ ] **Step 3: Implement**, including the two helpers and their callers in the draw code (the draw output doesn't change).
- [ ] **Step 4: Run** Check, Render, and `cargo test --release --locked -p notchling-app --test render_blocks -- --ignored` — Expected: PASS (`blocks-check: ok`).
- [ ] **Step 5: Commit** "Chat: each row's selectable texts come from the model too, pinned to what the row draws".

**Done when:** the unit tests and the render check pass over all three fixtures.

### Task 5.2: Selection anchored to the model, owned by `ConversationView`

**Goal (behavior change):** a selection names rows, blocks and bytes; copy comes from the model; the drag redraws only the conversation. The file panel keeps its own draw-order selection. Selecting from the conversation into the file panel in one drag stops working.

**Files:**
- Keep `crates/app/src/chat/view/select.rs` for the file panel only: its draw-order selection is unchanged (only its module doc says it now serves the file panel), and its `listener()` moves from the conversation's column to the first child of the file panel's body (`page_panel.rs:531-534`). Its file and its test keep their names, so no test is renamed.
- Create `crates/app/src/chat/view/row_select.rs` (under 500 lines) for the conversation:

```rust
/// One end of a selection: a row (item id, or `queued-<k>` / `sending-<k>` for the tail rows),
/// a block in that row (its index in `plain_blocks`) and a byte offset in the block's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot { pub row: SharedString, pub block: u16, pub byte: usize }

#[derive(Default)]
pub struct Selection { pub anchor: Option<Spot>, pub focus: Option<Spot>, pub unit: Unit, pub dragging: bool }

/// Shared by the column's selectables and the listener, per ConversationView.
pub type SelState = Rc<RefCell<SelFrame>>;
pub struct SelFrame {
    pub selection: Selection,
    /// This frame's drawn texts, for hit tests.
    pub notes: Vec<Note>,
    /// Row key → its place in the rows on show.
    pub rows: HashMap<SharedString, usize>,
}

pub fn at(spot: &Spot, rows: &HashMap<SharedString, usize>) -> Option<selection::At> // order = (row << 16) | block
pub fn copied(sel: &Selection, rows_in_order: &[(SharedString, Vec<String>)]) -> Option<String>
pub fn listener(state: SelState, view: WeakEntity<ConversationView>) -> impl IntoElement
pub fn selectable(row: SharedString, block: u16, text: String, layout: TextLayout,
                  child: AnyElement, a: Appearance, state: &SelState) -> Selectable
```

  The listener's handlers update `selection` and call `view.update(cx, |_, cx| cx.notify())`. `row_select` reuses `chat::selection` (`At`, `ordered`, `range_in`, `copied`, `floor`, `word_at`) and copies `select.rs`'s hit test (`spot_at`, `widen`) and wash painting (`rects`). `Selectable` paints the wash from `state.borrow().selection` (as today from the thread-local) and notes itself for hit tests.
- Modify `crates/app/src/chat/view/markdown.rs`: `render`, `inline`, `plain`, `mono_box` take a `sel: &Sel` where `pub enum Sel<'a> { Row { row: SharedString, next: &'a Cell<u16>, state: &'a SelState }, File }`. In a row, each selectable takes the next block number from `next`, in draw order, which is `plain_blocks` order by Task 5.1's check. `Sel::File` keeps today's string keys for `select`. Callers: `conversation.rs` and `steps_view.rs` (rows), `page_panel.rs:463,478` (`Sel::File`).
- Modify `crates/app/src/chat/view/conversation.rs`: tail rows get keys `queued-<k>` and `sending-<k>` (today both are `"queued"`, `:723-756`); the column installs the new `listener` first.
- Modify `crates/app/src/chat/view/conversation_view.rs`: `ConversationState.sel: SelState`; `pub fn copied(&self, cx: &App) -> Option<String>` (rows from the shown conversation, `plain_blocks` per row with Markdown from the cache, then `row_select::copied`); `pub fn clear_selection(&mut self)`.
- Modify `crates/app/src/chat/view/root.rs`: ⌘C in `capture_key_down` (was `mod.rs:924`) copies `parts.conversation.read(cx).copied(cx)`, else `select::copied()` (the file panel's). `load.rs` `select` calls `clear_selection` instead of `select::clear()` (`mod.rs:750`).
- Extend the render check: every conversation note's key parses to a row and block, and the block equals its index in that row's `plain_blocks`.

- [ ] **Step 1: Write the failing tests** (`crates/app/src/chat/view/row_select_tests.rs`, pure): `ends_order_by_row_then_block_then_byte`; `copy_takes_rows_that_are_not_drawn` (three rows' blocks, anchor in the first, focus in the third: the middle row's text is in the copy); `a_selection_on_a_trimmed_row_is_dropped`; `a_drag_backwards_copies_the_same_text`; `queued_rows_have_distinct_keys`; `the_file_panel_keeps_its_own_selection` (a file note and no conversation selection: ⌘C's choice is the file's).
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render, `render_blocks`, Bench (`select` row) — Expected: PASS. In a release build over `fixtures/chat-steps`: drag across three messages and ⌘C, double-click a word, triple-click a paragraph, select in an open step's output, select in an open file panel and ⌘C, press elsewhere to clear; each as before.
- [ ] **Step 5: Commit** "Chat: a selection names rows and bytes, so copying no longer needs every message drawn; the file panel keeps its own".

**Done when:** tests and both render checks pass, and the manual checks behave as before.

### Task 6.2: Kit scroll bar tracks a `ListState`

**Goal:** the kit's overlay bar over a GPUI list, with the same look and behavior as over a div.

**Files:**
- Modify `crates/kit/src/ui/scrollbar.rs` (431 lines; move `Geometry` and `scroll_to` into a new `crates/kit/src/ui/scrollbar_track.rs` if it passes 600):

```rust
enum Track { Div(ScrollHandle), List(ListState) }
enum Inner { Div(Stateful<Div>), Any(AnyElement) }
/// The bar over a GPUI list; `list` must fill the box the bar sits in.
pub fn scrollable_list(id: impl Into<ElementId>, list: List, state: &ListState) -> Scrollable
```

  `Geometry::of(bounds, &Track)`: for a list, `content = viewport + max_offset_for_scrollbar().height`, and `scrolled = list_scrolled(following, -scroll_px_offset_for_scrollbar().y, max)`, where `following` is `logical_scroll_top().item_ix >= item_count()`. `scroll_to(&Track, scrolled)` calls `set_offset_from_scrollbar(point(px(0.), px(scrolled)))` for a list. A thumb press calls `scrollbar_drag_started()`, its release `scrollbar_drag_ended()`. `prepaint`'s "does it scroll" reads the track.
- Add `pub(crate) fn list_scrolled(following: bool, offset: f32, max: f32) -> f32` (a following list is at `max`; otherwise `offset` clamped to `0..=max`) in the kit's scroll bar math module.
- Modify `crates/app/src/chat/view/list_probe.rs`: draw the probe through `scrollable_list` and add gate G7: while following, the thumb's bottom is at the track's bottom.

- [ ] **Step 1: Write the failing tests:** `a_following_list_is_scrolled_to_the_end`, `a_list_offset_is_clamped` (kit math); probe gate G7 in `render_list_probe`.
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check (`-p notchling-kit -p notchling-app`), the kit gallery render, `render_list_probe` — Expected: PASS.
- [ ] **Step 5: Commit** "Kit: the overlay scroll bar also tracks a GPUI list".

**Done when:** the div path renders as before (kit gallery, Chat) and the probe's bar sits at the end while following.

### Task 6.1: The conversation as a `list`

**Goal (behavior change: same look, fewer rows drawn):** a draw lays out only the rows on screen; the follow rules, the Latest pill, the Tasks inset, open groups, hover, selection and the subagent view work as before.

**Files:**
- Modify `crates/app/src/chat/view/conversation_view.rs` (add a `conversation_list.rs` beside it for the list plumbing if the file passes 500 lines):

```rust
pub const OVERDRAW: f32 = 600.0;
/// List items: [top spacer] + rows + [bottom spacer].
struct ListRows {
    state: ListState,               // ListState::new(0, ListAlignment::Bottom, px(OVERDRAW))
    rows: Vec<Row>,                 // with_outgoing(rows(shown), outgoing): items, queued, sending; kept in step with the splices
    keys: Vec<SharedString>,        // each row's key (item id, queued-<k>, sending-<k>)
    live: Option<usize>,            // activity::live_group(shown, working)
    tasks_inset: bool,
}
/// No window needed: these work on the ListState alone, so they are unit-tested.
impl ListRows {
    fn sync(&mut self, old: &ConversationInput, new: &ConversationInput,
            change: Option<(Arc<Conversation>, Change)>, follow_sent: bool);
    fn remeasure(&self, items: Range<usize>);   // splice list items, keeping the scroll-top offset
    fn at_bottom(&self) -> bool;                // following, or the last row's bottom within follow::TOLERANCE
    fn scroll_to_end(&self);                    // scroll_to(ListOffset { item_ix: item_count, offset_in_item: 0 })
}
impl ConversationView {
    pub fn scroll_to_end_now(&mut self);        // list.scroll_to_end(); the old ScrollHandle callers use it
    fn render_item(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement;
}
```

- Modify `crates/app/src/chat/rows.rs`: make `row`, the per-item function `rows` maps (`:243`), `pub` as `row_of`; `rows` is unchanged.

**Rules `sync` implements:**
- Another session or subagent (by `session.id`, `subagent.agent_id`), or `change` missing or `Reset`, or `change`'s start isn't the conversation on show (`!Arc::ptr_eq`): rebuild `rows` and `keys`, `state.reset(rows.len() + 2)` (the list follows the end). For `Reset` within the same session while scrolled up, keep the first visible row by key: after the reset, `scroll_to` it if it still exists.
- `Items { splices, .. }`: apply each splice back to front to `rows`/`keys` (`row_of` for the new items) and to the list (`splice(old.start + 1 .. old.end + 1, new.len())`).
- The tail rows (queued, sending) changed: splice the tail range.
- `live` moved: `remeasure` the old and the new live row (an off-screen row's height changed).
- `tasks_inset` changed: `remeasure` the top spacer.
- Before applying: `let follow = at_bottom() || follow_sent || session changed` (`follow::decide`'s rule, with `at_bottom` instead of pixel sums); after: if `follow`, `scroll_to_end()`. The caller takes `s.follow_sent`.

**Rules `render_item` implements:** item 0 is the top spacer (`h` = `tasks::INSET + COLUMN_PADDING_VERTICAL - ROW_GAP` when `tasks_inset`, else `COLUMN_PADDING_VERTICAL - ROW_GAP`); the last item is the bottom spacer (`COLUMN_PADDING_VERTICAL`); row `r = ix - 1` is `div().px(SIDE_PADDING).pt(ROW_GAP).child(row_with_opinions(…))` (counted in `ROWS`). The empty states (loading, no session, empty rows, a subagent still loading) draw instead of the list, as today. The column is `div().size_full().relative()` with the selection listener first, then `scrollable_list("conversation", list(state, cx.processor(Self::render_item)), &state)`, then the Latest pill (`!at_bottom() && !loading`, or `latest_preview`). `jump_to_latest` is `scroll_to_end_now` plus notify.
- A visible row needs no splice to change: the list lays out the rows on screen on every draw. Toggles, hover, Copy, selection and opinions just notify.
- If the spike's G6 failed: the live row's spinner is a plain kit spinner view, not a `SpinLayer` slot.

- [ ] **Step 1: Write the failing tests** (`crates/app/src/chat/view/conversation_list_tests.rs`, `ListRows` over a `ListState` with no window): `an_append_splices_one_row_before_the_bottom_spacer`; `a_front_trim_removes_rows_from_the_top`; `another_session_resets_to_the_end`; `the_tail_rows_follow_the_queue`; `the_live_row_is_remeasured_when_it_moves`; `at_bottom_ignores_unmeasured_rows` (a list with unmeasured rows above and `logical_scroll_top` past the end: at the bottom); `a_splice_at_the_scroll_top_keeps_its_offset`. Bench tests: `bench_scroll_draws_only_rows_on_screen` (`rows / conversation <= 30`), `bench_hover_draws_only_rows_on_screen`, `bench_append_draws_only_rows_on_screen_and_follows` (`rows` per conversation draw ≤ 30, and `parsed_bytes` ≈ the appended lines' bytes, within 10 %).
- [ ] **Step 2: Run** — Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** Check, Render (every conversation env from Task 4.3, plus `NOTCHLING_RENDER_SCROLLBARS=1`), `render_blocks`, Bench — Expected: PASS. Release build over the bench fixture (`--open-chat` on its temp root, or a copy in `/tmp`): scroll from the end to the top and back; follow while lines are appended (`echo … >> transcript` from a shell); scroll up, let lines append (no jump, the Latest pill shows, clicking it goes to the end); open and close groups near the top; select across rows off screen with a drag and the wheel, then ⌘C; open a subagent and come back; resize the window narrower and wider.
- [ ] **Step 5: Commit** "Chat: the conversation is a list that lays out only the rows on screen".

**Done when:** the tests pass, the bench rows drop to the rows on screen, and the manual checks behave as before.

### Task 6.1-alt (only on no-go): a windowed column with our own height cache

**Goal:** the same result without GPUI's list.

**Files:** `crates/app/src/chat/view/conversation_window.rs` (new): keep the `Stateful<Div>` scroll container and the kit bar; draw only rows whose cached top is within the viewport ± `OVERDRAW`, between two spacers whose heights are the sums of the cached heights above and below; `heights: HashMap<(SharedString, u32 /*width px*/), f32>` filled from each drawn row's bounds (a `canvas` after each row); an unknown height is estimated from the row's text length at the column's width; after a draw that measured rows above the first visible one, the scroll offset moves by the difference, so the first visible row stays put. Follow and the pill use the same `at_bottom` rule as Task 6.1. Same tests and bench gates as Task 6.1, plus `the_offset_absorbs_heights_measured_above`. Size L.

### Task 6.3: Drag-select scrolls the list at its edges

**Goal (behavior change, new):** dragging a selection past the top or bottom of the conversation scrolls it, and the selection follows.

**Files:** Modify `crates/app/src/chat/view/row_select.rs` and `conversation_view.rs`. Pure `pub fn autoscroll_speed(pointer_y: f32, top: f32, bottom: f32) -> f32` (0 inside; beyond an edge, `(distance / 4).clamp(2, 40)` points per tick, negative above). While dragging and the speed isn't 0, one 16 ms timer at a time calls `state.scroll_by(px(speed))`, sets the focus to the spot at the edge row, and notifies; it stops on release or when the pointer comes back inside.

- [ ] **Step 1: Write the failing tests:** `no_speed_inside`, `speed_grows_with_distance_and_is_capped`, `above_scrolls_up`.
- [ ] **Step 2: Implement.** **Run** Check, Render, Bench (`select`) — Expected: PASS. Manual: drag below the window's bottom edge; the conversation scrolls and the selection grows.
- [ ] **Step 3: Commit** "Chat: dragging a selection past the conversation's edge scrolls it".

### Task 6.4: Scroll bar fade: measure, lower its rate if it costs

**Goal:** know what the fade costs after the list; act only if it matters.

- [ ] **Step 1:** Add `bench_scroll_fade` (mode `scroll` for 1 s, then 2 s idle while the bar fades): record `ms_per_frame` and `rows` during the fade.
- [ ] **Step 2:** If the fade's `ms_per_frame` is above 4 ms, set the fade's frame interval in the kit's scroll bar math (`math::next_frame_in`) to 1/15 s instead of 1/30 s, add the test `the_fade_steps_at_fifteen_frames_a_second`, and record the new row. Otherwise record the numbers and change nothing.
- [ ] **Step 3: Commit** with the numbers in the message.

### Tasks 7.1-7.3 (optional): the terminal, simulator and page panels as entities

**Goal:** the root's own draw gets lighter when a panel is open. Do these only if, after Phase 6, the `terminal` bench's `ms_per_frame` with the panel open is above 4 ms.

Each: a new entity (`TerminalPanel` in `chat/view/terminal_panel.rs`, `SimulatorPanel`, `PagePanel`) owning that panel's state (`terminal` (240), `simulator` (242), `page` (238) and `page_covered` (190)), drawn cached in the root's place, with `ChatView` forwarding its calls; its ticks notify only the panel. Tests: the panel's existing renders unchanged; a bench test that its tick draws `root == 0` beyond the panel. Size M each; they may run in parallel.

### Task 8.1: Notes (protected: maintainer)

**Goal:** the notes say how Chat works after each phase. One commit per phase, or one at the end; each needs the maintainer (`crates/AGENTS.md`, `docs/feature-map.md`).

- After Phase 0: `crates/AGENTS.md`, "Build, run, test": the bench modes (`NOTCHLING_RENDER_BENCH_MODE`), `bench-json:`, `tests/bench_chat.rs` (release, `--test-threads=1`) and `docs/perf/chat-state-split-bench.md`.
- After Phase 1: "Things that bit before": typing called `cx.notify()` and laid out the whole conversation per keystroke; terminal and simulator frames did the same at 30 a second; root-drawn panels redraw through `redraw_chrome`; never `window.refresh()` in a handler, notify the view captured at paint.
- After Phase 2: the list probe and its gates (`--render chat-list-probe`), and why the conversation is or isn't a GPUI list.
- After Phase 3: `conversation::read_shared` and `Arc<Conversation>`; `chat::log::Change`; the open question comes from the conversation reader.
- After Phase 4: "Architecture in one pass": `ChatView` is the controller; the Sidebar and the Details Panel are `ModelPart`s drawn from `SidebarModel`/`DetailsModel`; the conversation is `ConversationView`, drawn from `ConversationInput` and its own state; a draw function reads only its model; globals (appearance, folds) redraw through `redraw_parts`/`redraw_details`. `docs/feature-map.md` (Chat row, line 100): name `conversation_view.rs`, `load.rs`, `root.rs`, `steps_view.rs` beside `pane.rs`.
- After Phase 5: the conversation's selection is per `ConversationView` and anchored to rows (`view/row_select.rs`, `chat::blocks::plain_blocks`, `NOTCHLING_RENDER_CHECK_BLOCKS`); `view/select.rs` serves only the file panel.
- After Phase 6: the conversation is a list (or the windowed column); unmeasured rows count as 0 px, so "at the bottom" comes from the list's state; a visible row needs no splice; the bar tracks a `ListState`. Feature map: `chat/blocks.rs`, `chat/log.rs`, `conversation_list.rs`.

- [ ] **Step 1:** Write the notes for the phases done. **Run** `PYTHONPATH=tools/guard python3 -m guard lint && PYTHONPATH=tools/guard python3 -m guard featuremap && PYTHONPATH=tools/guard python3 -m guard claudemd` — Expected: PASS.
- [ ] **Step 2: Commit** "notes: Chat's state per area (phase N)".

### Task 8.2: Manual check on the maintainer's screen

A checklist for the maintainer's release build over real sessions (the maintainer runs it; agents don't install): typing in a long session stays smooth; a printing terminal doesn't slow scrolling; hovering and scrolling a 200-turn conversation; selection across messages and ⌘C; follow while a session works; the Latest pill; the Tasks bar; subagents; the file panel's own selection; the Sidebar and Details update (new session, ages, account move, folds, shells); dark and light switching; the part names (?).

## Out of this plan

- Making `Parser::conversation()` incremental (it still clones up to 500 items when a file changes).
- A paint layer for the scroll bar's fade.
- Splitting `terminal.rs` (934), `composer.rs` (932), `simulator/mod.rs` (876), `simulator/parts.rs` (839) and `tools.rs` (837) beyond what the tasks above need.
- Search inside the conversation and jump to a message (the list's `scroll_to_reveal_item` makes them possible later).
