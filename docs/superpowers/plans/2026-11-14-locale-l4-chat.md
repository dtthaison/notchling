# Localization L4: Chat Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every word Chat shows comes from `locales/en/mac/chat.ftl` (and `mac/units.ftl` for ages, durations and token counts), in five pull requests by part, each arming its part's literal lint. English renders are pixel-identical and Chat's bench counts don't move.

**Architecture:** Models keep their types: a `&'static str` title stays one, read from `t::chat::NAME`; text built with `format!` becomes a message with variables formatted in the model compute or the loader, never per row per frame. Cached parts compare their models by `PartialEq` as before, so they draw exactly as often.

**Tech Stack:** Rust, GPUI, `notchling-locale`.

**Spec:** `docs/superpowers/specs/2026-11-13-localization-design.md`, "Runtime in the Mac app" (Cost per draw, Numbers, dates, lists), "What is never translated", Steps L4. Calls P20, P21, P27, P28 in `2026-11-14-locale-l0-decision.md`.

**Branches:** one lane, in order, each from the previous (they all add to `mac/chat.ftl`):
`agent/locale-l4a-chat-sidebar` (from `agent/locale-l1-foundation`, or L3's branch once it merged), `agent/locale-l4b-chat-conversation`, `agent/locale-l4c-chat-reply-box`, `agent/locale-l4d-chat-details`, `agent/locale-l4e-chat-window`. Each starts only once open branches touching its files have merged. Each may be followed by `agent/locale-l4<x>-plurals`.

## Global Constraints

- English pixel-identical, wrong plurals included (call 8).
- A draw may call a plain message (`&'static str`); a message with variables is formatted in a model compute or the loader (`crates/AGENTS.md`). A cached part's model gets the formatted `String`, never a `Text` plus arguments.
- Never translated, and left as literals: part names (`.part`, `.part_at`, `.part_inside`, `.window_region`), element ids, text matched against Claude Code's screen or the daemon (`chat/prompt_flow.rs::GONE` and the dialog markers in `prompts.rs`), git's output, tool names from transcripts, fixture text in `renders.rs` and `bench_fixture.rs`.
- Key prefix `chat-`; a sub-group per part by `##` heading (`## Sidebar`, `## Conversation`, …) in `mac/chat.ftl`.
- A sentence split because a part is clickable or styled says in each part's note which messages it shows with (call 7).
- No comments in code.

## Review Focus

1. A message with a count formatted inside a list row's render (per frame): `bench_chat.rs` `scroll` and `hover` builds the same rows, and `bench_scroll.rs` the same stalls (each part's last task).
2. A model holding a `Text` and formatting at draw: forbidden by the rule above; reviewers grep the part's view files for `t::chat::` calls with arguments.
3. `format.rs::age` and `duration` boundaries (59 s, 60 s, 59 min, 24 h) keep today's English exactly (L4a Task 1, `ages_and_durations_unchanged`).
4. The dialog markers in `prompts.rs`/`prompt_flow.rs` moved into messages by mistake: the matcher would break in another language. They stay constants in the baseline (L4c Task 1, `markers_stay_literal`).
5. Copy and selection read text from the model's blocks: a word moved into a message must still copy the same (`render_copy.rs`, `render_blocks.rs` in L4b).

---

### Task 0: The five areas (in L1)

L1's `tools/locale/areas.json` lists them, `moved: false`. Paths under `crates/app/src/` (a part's PR may move a file between parts with a line in its description):

| Area | `chat/` files | `chat/view/` files | Elsewhere |
| --- | --- | --- | --- |
| `chat-sidebar` | `sidebar.rs`, `codex_list.rs`, `used.rs`, `closed.rs`, `cleared.rs`, `delegated.rs`, `lane_limits.rs`, `limit_ring.rs`, `format.rs` | `sidebar.rs`, `row_menu.rs`, `accounts.rs`, `inbox.rs`, `phone.rs`, `limit_ring.rs` | |
| `chat-conversation` | `rows.rs`, `steps.rs`, `message_meta.rs`, `markdown.rs`, `markdown_cache.rs`, `blocks.rs`, `paths.rs`, `subagent_view.rs`, `selection.rs`, `log.rs`, `loader.rs`, `follow.rs`, `scroll_trace.rs`, `escape.rs`, `image_size.rs` | `conversation.rs`, `conversation_list.rs`, `conversation_view.rs`, `steps_view.rs`, `markdown.rs`, `message_actions.rs`, `copy.rs`, `copy_menu.rs`, `row_blocks.rs`, `row_select.rs`, `select.rs`, `subagent.rs`, `scroll_probe.rs`, `list_probe.rs`, `list_probe_run.rs`, `tool/**` | |
| `chat-reply-box` | `composer.rs`, `prompts.rs`, `prompt_flow.rs`, `pickers.rs`, `claude_picker.rs`, `slash_menu.rs`, `drops.rs`, `outgoing.rs`, `drafts.rs`, `run_command.rs`, `activity.rs`, `tasks_bar.rs` | `composer.rs`, `prompts.rs`, `slash_menu.rs`, `drop.rs`, `tasks.rs`, `opinions.rs`, `tools.rs`, `native_spin.rs` | |
| `chat-details` | `inspector.rs`, `paging.rs`, `subagent_rows.rs`, `shell_rows.rs`, `shell_list.rs`, `git.rs`, `branch.rs`, `branch_rows.rs` | `inspector.rs`, `paged_rows.rs`, `repo.rs`, `repo_section.rs`, `changes.rs`, `changes_panel.rs`, `diff_panel.rs`, `shell_probe.rs` | |
| `chat-window` | `header.rs`, `panels.rs`, `page_panel.rs`, `file_panel.rs`, `simulator.rs`, `terminal.rs`, `data.rs`, `history.rs`, `reload.rs`, `mod.rs` | `header.rs`, `root.rs`, `mod.rs`, `load.rs`, `pane.rs`, `panels.rs`, `page_panel.rs`, `simulator/**`, `terminal.rs`, `sheets.rs`, `render_map.rs` | `sheets/new_session.rs`, `sheets/second_opinion_view.rs`, `sheets/banner.rs`, `sheets/view.rs`, `sheets/mod.rs` |

## The recipe every part follows

Every part below is three or four tasks of this shape; the part's section names its files, tests and renders.

- **Words task** (zone auto: `crates/app/**`, `locales/en/mac/*.ftl`; no expected hits): write a new `<part>_words_tests.rs` beside the part's model with one test per moved table or sentence (the model's text equals its message; a count's one and other forms equal today's English), run it to fail, write the messages with notes and run `python3 tools/locale/gen.py`, replace the literals, run `cargo test -p notchling-app chat::` to pass, commit.
- **Arm task** (zone maintainer for `areas.json` and baseline lines; hits `ZONE`, `GUARD-BASELINE` when matched text is baselined): render before and after (`NOTCHLING_RENDER_FORCE_FRAMES=1`, release, light and dark) and `cmp` every pair (P27); render en-XA and look for anything outside `⟦ ⟧`; flip the area in `areas.json`; `python3 -m guard lint` shows 0 new; run the benches and render tests; commit.
- **Existing tests**: none is edited; existing tests asserting English keep passing because English doesn't change. If one must change, it waits for the maintainer and the PR says why.

---

## L4a: the Sidebar (and units)

### Task 1: Ages, durations, token counts

**Files:**
- Modify: `locales/en/mac/units.ftl` (`units-age-now`, `units-age-minutes = { NUMBER($n) }m` and the rest of what `format::age` prints; `units-duration-*`; `units-thousands = { $value }k`, `units-millions = { $value }M`), `crates/app/src/chat/format.rs` (`age`, `duration`, `tokens` keep their signatures and return the same `String`s)
- Create: `crates/app/src/chat/format_words_tests.rs`

- [ ] **Step 1: Write the failing test** `ages_and_durations_unchanged`: a table of `(now_ms, since_ms)` and seconds at the boundaries (0, 59, 60, 3,599, 3,600, 86,399, 86,400, 7 days) and token counts (999, 1,000, 12,345, 1,234,567) asserting today's exact strings, which the test copies from the current functions' output before the change; plus `uses_unit_messages` (`tokens(12_345)` equals `t::units::thousands(&t::fmt::decimal(12.345, 1))`).
- [ ] **Step 2: Run** `cargo test -p notchling-app format_words`. Expected: `uses_unit_messages` FAILS, the table passes (it pins today).
- [ ] **Step 3: Implement** through the unit messages; `t::fmt::decimal` keeps one digit where today's code keeps one.
- [ ] **Step 4: Run** `cargo test -p notchling-app chat::format`. Expected: PASS.
- [ ] **Step 5: Commit** `chat: ages, durations and token counts from units.ftl`.

### Task 2: The Sidebar's words

**Files:** the `chat-sidebar` files of Task 0; `locales/en/mac/chat.ftl` (`## Sidebar`); `crates/app/src/chat/sidebar_words_tests.rs` (new).
Follow the words task. Notes: the footer buttons' tooltips and the account and row menus (`MenuItem` labels) are words; menu item ids are not.

### Task 3: Arm `chat-sidebar`

Renders: `chat`, `chat` with `NOTCHLING_RENDER_ROW_MENU=job` and `=confirm`, `NOTCHLING_RENDER_ACCOUNT_MENU=1`, `NOTCHLING_RENDER_SIDEBAR_WIDTH=200` (footer on two rows). Benches: `bench_chat.rs`. Commit `chat: the Sidebar has moved`.

## L4b: the conversation

### Task 1: The conversation's words

**Files:** the `chat-conversation` files; `## Conversation` in `chat.ftl`; `crates/app/src/chat/conversation_words_tests.rs`.
Notes: step titles, tool summaries made by our code ("Read 3 files") are words; tool names, file paths and anything read from a transcript are not. `message_meta.rs` dates use `units.ftl` from L4a. Selection and Copy read blocks from the model (`blocks.rs::plain_blocks`): a moved word is still a model string, so the copy is unchanged.

### Task 2: The tool adapters

**Files:** `crates/app/src/chat/view/tool/{claude,codex,antigravity,mod}.rs`; `chat.ftl` under `## Tools`. Words test in the same new file.

### Task 3: Arm `chat-conversation`

Renders: `chat`, `chat-list-probe`. Tests: `bench_chat.rs` (`scroll`, `hover`, `select`, `append` counts), `bench_scroll.rs`, `render_blocks.rs`, `render_copy.rs` (`cargo test --release --locked -p notchling-app --test <name> -- --ignored --nocapture --test-threads=1`). Commit `chat: the conversation has moved`.

## L4c: the reply box

### Task 1: The reply box's words

**Files:** the `chat-reply-box` files; `## Reply box` in `chat.ftl`; `crates/app/src/chat/reply_words_tests.rs`.
Notes: `prompt_flow.rs::GONE` and every string matched against Claude Code's screen or a daemon refusal stay constants: a test `markers_stay_literal` asserts each marker constant is not equal to any English message value (`notchling_locale::testing::messages("en")`), so a later move into `chat.ftl` fails. Pickers' model names and slash command names come from data and stay. The Activity Line's clock text ("(26s)") is formatted where `activity_drawn` notes it, once a second, as today.

### Task 2: Arm `chat-reply-box`

Renders: `chat` with an open question (the fixture's) and the slash menu if a `NOTCHLING_RENDER_*` opens it (grep `crates/app/src/chat`). Benches: `bench_chat.rs` `typing`, `menu`, `sending` (`root <= 16`, `native_rings >= 1`). Baseline: the markers. Commit `chat: the reply box has moved`.

## L4d: the Details Panel

### Task 1: The Details Panel's words

**Files:** the `chat-details` files; `## Details Panel` in `chat.ftl`; `crates/app/src/chat/details_words_tests.rs`.
Notes: section titles with counts ("Subagents · 3 running") are formatted in `inspector.rs`'s model compute, which already builds them; the Load N More row's text too. Git's status letters and branch names are data. The Changes panel's `+98 −0` and `vs <base>` are words with variables.

### Task 2: Arm `chat-details`

Renders: `chat`, `chat-git`, `chat-git` with `NOTCHLING_RENDER_CHANGES=1`, `=expanded`, `=menu`. Benches: `bench_chat.rs` `reload`, `bench_subagents.rs` (`parsed_bytes` unchanged). Commit `chat: the Details Panel has moved`.

## L4e: the window and its sheets

### Task 1: The window's words

**Files:** the `chat-window` files; `## Window` and `## Sheets` in `chat.ftl`; `crates/app/src/chat/window_words_tests.rs`.
Notes: New Session and Second Opinion sheets, the banner; the terminal and simulator panels' headers; the file and page panels' empty states. The render map's badges are numbers, not words.

### Task 2: Arm `chat-window`

Renders: `chat`, `new-session`, `trust-folder`, `banner`, `shell`, `chat` with `NOTCHLING_RENDER_FILE=crates/app/fixtures/files/plan.md`, `NOTCHLING_RENDER_PARTS=1`. Benches: `bench_chat.rs` all modes, `panels` included. Commit `chat: the window and its sheets have moved`.

### Checkpoint, after each part

- [ ] `cargo test --workspace --locked`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check`; `python3 tools/locale/gen.py --check`; `PYTHONPATH=tools/guard python3 -m guard lint` (0 new).
- [ ] The part's renders `cmp` equal; en-XA renders in the PR; bench counts as on the base commit.
- [ ] Push; PR against `merge/settings-batch`; the next part branches from this one.
