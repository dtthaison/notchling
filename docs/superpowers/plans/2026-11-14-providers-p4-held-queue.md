# Providers, Phase 4: The Held Queue for Every Tool Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While any session works, Claude Code, Codex or Antigravity, everything the user sends joins one held message that goes out when the turn ends, and can be taken back and edited. Codex stops running one turn per queued message; Antigravity's reply box stops refusing while the session works. An Antigravity session another `agy` holds stays off, with its reason, and the held queue takes nothing for it.

**Architecture:** `engine::reply::held::Held` already holds, joins and releases for Claude Code (8e73a97). It gains a tool-neutral release (`ready_for`) and `held_ops.rs` gains one send path per tool behind a small trait (`HeldSend`), so `session.reply` and `session.take_back` route by the row id to the same queue. Release is on `turn.finished` from Phase 2's `SignalWatch`, with the 1 s tick kept for the 20 s start grace. `CodexRuns` keeps one turn at a time and answers `Busy` instead of queueing; the agy runner keeps its `RunError::Busy` (call C21).

**Tech Stack:** Rust 1.99.1, the daemon's `ActingOps`, `CodexOps`, `AgyOps`, `FakeSpawner` (Codex and ACP fakes), GPUI through the kit for the composer.

**Spec:** sections 6.6 (held-queue), 7.1 (`turn.hold`, `turn.take_back`), 12 (phase 4), call 7. Phases 0 and 2 merged (the release needs `turn.finished`). Calls C20, C21 in the overview.

**Branch:** `agent/providers-p4-held-queue`.

## Global Constraints

- Phase 0 to 2 constraints hold.
- Held text is the user's: it is never logged, never put in an event other than `session.held` to the app tier (as today), never written to disk by this phase.
- One held message per session: a second send while held joins with `JOIN` (`"\n\n"`), as Claude Code does today. Take back returns the whole held text and empties it.
- The `acting` switch gates every send and release; with acting off nothing is held or sent (as today).
- Logging P2 2.18 also edits `held_ops.rs` (`session.reply` through the held queue as a log kind): rebase on whichever merged first; the log line goes where the route is decided.

## Review Focus

1. Three Codex messages sent during one turn produce one joined turn after it ends, not three turns. (Task 4.3 test `three_messages_during_a_turn_become_one_turn`.)
2. Stop on a Codex turn with a held message: the turn aborts, the held message goes out as the next turn, as the old outbox did. (Task 4.3 test `stop_lets_the_held_message_go_out`.)
3. An Antigravity session held by another `agy`: the box stays off with the holder's note, and `session.reply` refuses rather than holding. (Task 4.4 test `a_session_held_elsewhere_holds_nothing`.)
4. A held message whose turn never starts (the user sent while the session looked busy but it was idle) still goes out after the 20 s grace, for every tool. (Task 4.2 test `the_grace_releases_for_every_tool`.)
5. A failed send puts the text back and the user sees it held again, for every tool. (Task 4.2 test `a_failed_send_puts_it_back`.)

## Order and lanes

```
4.1 Held::ready_for ─► 4.2 one queue, routed by id ─┬─► 4.3 Codex ─┐
                                                    └─► 4.4 Antigravity ─┴─► 4.5 composer ─► 4.6 cells, notes
```

Lanes: 4.3 (`tools/codex/runs.rs`, `codex_ops.rs`) and 4.4 (`agy_ops.rs`) run beside each other after 4.2.

## Zones and gates

| Task | Zone | Gates expected | Existing tests edited |
| --- | --- | --- | --- |
| 4.1 | Maintainer (`reply/**`) | none | none: `held_tests` unchanged |
| 4.2 | Maintainer (`notchlingd/**`) | none | none |
| 4.3 | Maintainer (`tools/codex/**`, `notchlingd/**`) | none of `CAP-*` | in-crate, not tripped by the gate, **flagged for the maintainer by spec call 7**: in `crates/engine/src/tools/codex/runs_tests.rs` `a_message_sent_mid_turn_waits_and_goes_out_next`, `stop_leaves_the_waiting_messages_to_go_out`, `forget_drops_the_waiting_messages_too`, `a_failed_turn_reports_what_it_printed_last_and_the_queue_goes_on`, `a_waiting_message_that_cannot_start_is_reported_and_the_rest_dropped`, `a_waiting_message_without_a_folder_runs_where_the_session_does`, `a_waiting_message_does_not_start_once_starting_is_off`, `nothing_is_sent_or_queued_while_starting_is_off`, `a_message_queued_while_a_turn_is_starting_runs_in_its_folder` (each either asserts `Busy` now or is removed, with its behavior covered by a new joined-queue test in `crates/notchlingd/src/held_codex_tests.rs`); in `crates/notchlingd/src/codex_ops_tests.rs:214` `a_message_behind_a_running_turn_is_queued` (now held and joined) |
| 4.4 | Maintainer (`notchlingd/**`) | none | none: `tools/agy/runner_tests.rs` `a_prompt_runs_a_turn_holds_the_permission_and_never_approves_it_itself` keeps asserting `RunError::Busy` (the runner is unchanged) |
| 4.5 | Auto (`app/**`) | none | in-crate, listed: `app/src/chat/view/tool/mod.rs` `only_codex_queues_through_the_daemon_so_far` becomes `every_tool_holds_through_the_daemon` (the trait's `queues` answers true for all three); `tools/live.rs` tests unchanged |
| 4.6 | Maintainer (`contract/**`, `AGENTS.md`); auto (matrix) | `ZONE` | none |

## Maintainer only (Phase 4)

- Sending is the maintainer's zone: every task but 4.5.
- The Codex test changes in 4.3 (call 7): the behavior changes from one turn per message to one joined turn.

---

### Task 4.1: A tool-neutral release

**Files:**
- Modify: `crates/engine/src/reply/held.rs`
- Test: `crates/engine/src/reply/held_for_tests.rs` (new)

**Interfaces:**
- Produces: `pub struct TurnState { pub id: String, pub idle: bool, pub can_send: bool }`; `Held::ready_for(&mut self, states: &[TurnState], now_ms: i64) -> Vec<(String, String)>` (a held id whose state is idle and can send, or whose hold is older than `START_GRACE_MS` with no turn seen working, is released and removed); `Held::seen_working(&mut self, id: &str)` (marks that the turn started, so the grace no longer applies); `ready(rows: &[Agent], ..)` becomes a wrapper that maps Claude Code rows to `TurnState` and calls `ready_for` (its tests unchanged).

- [ ] **Step 1: Write the failing tests:** `an_idle_session_releases_its_held_text`, `a_working_session_keeps_it`, `the_grace_releases_a_turn_that_never_started`, `a_turn_seen_working_is_not_released_by_the_grace`, `a_session_that_cannot_send_keeps_it`.
- [ ] **Step 2: Run** `cargo test -p notchling-engine reply::held_for_tests`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-engine reply`. Expected: PASS (`held_tests` unchanged).
- [ ] **Step 5: Commit** `git commit -m "held: release by turn state, for any tool"`.

### Task 4.2: One held queue, `session.reply` and `session.take_back` routed by id

**Files:**
- Create: `crates/notchlingd/src/held_send.rs`
- Modify: `crates/notchlingd/src/held_ops.rs` (`HeldState` serves every tool; `reply_or_hold` takes a `&dyn HeldSend`; `flush_held` and the `turn.finished` listener route by `contract::Tool::of_row`), `crates/notchlingd/src/acting_ops.rs` (`session.reply` and `session.take_back` with a `codex:` or `antigravity:`/`agy:` id go to the held queue and the tool's sender; Claude Code ids unchanged), `crates/notchlingd/src/daemon_wiring.rs` (the senders are handed to `ActingOps`)
- Test: `crates/notchlingd/src/held_send_tests.rs`

**Interfaces:**
- Produces: `pub trait HeldSend: Send + Sync { fn tool(&self) -> Tool; fn state(&self, id: &str) -> Option<TurnState>; fn send(&self, id: &str, text: &str) -> Result<(), ApiError>; fn refusal(&self, id: &str) -> Option<String>; }` with `ClaudeSend` (over `ActingOps::play` and `plan::reply`, as now), `CodexSend` (over `CodexOps`), `AgySend` (over `AgyOps`). `refusal` answers before anything is held (Antigravity held elsewhere).
- Replies keep today's shape: `{"sent": true}` or `{"sent": true, "state": "queued", "waiting": 1, "held": "<all>"}`; `session.held` events as today.

- [ ] **Step 1: Write the failing tests** (fake senders): `a_codex_id_is_held_while_working`, `an_antigravity_id_is_held_while_working`, `agy_and_antigravity_ids_share_one_hold` (call C30), `take_back_returns_the_whole_text_for_every_tool`, `the_grace_releases_for_every_tool`, `a_failed_send_puts_it_back`, `acting_off_holds_nothing`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd held_send`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "held: one queue for every tool, session.reply routed by id"`.

### Task 4.3: Codex joins the held queue

**Files:**
- Modify: `crates/engine/src/tools/codex/runs.rs` (`outbox` removed; `send` while a turn runs or starts answers `SendError::Busy`), `crates/notchlingd/src/codex_ops.rs` (`codex.send` for an existing session goes through the held queue: hold while working, send when idle; `forget` drops the held text with the session), `crates/notchlingd/src/held_send.rs` (`CodexSend::state` from `CodexRuns::is_running` and the rollout's `turn_open`)
- Test: `crates/notchlingd/src/held_codex_tests.rs` (new); the edits listed in the zones table

**Interfaces:**
- Consumes: `FakeSpawner` (`tools/codex/fake.rs`). `codex.send` stays as an alias op until Phase 6.

- [ ] **Step 1: Write the failing tests** in `held_codex_tests.rs`: `three_messages_during_a_turn_become_one_turn` (the fake sees one `exec resume` whose prompt is the three joined), `stop_lets_the_held_message_go_out`, `forget_drops_the_held_text`, `a_failed_turn_lets_the_held_message_go_out`, `a_held_message_without_a_folder_runs_where_the_session_does`, `nothing_is_held_while_starting_is_off`, `a_message_held_while_a_turn_is_starting_runs_in_its_folder`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd held_codex`; expected FAIL.
- [ ] **Step 3: Implement**; then edit the listed `runs_tests.rs` and `codex_ops_tests.rs` tests: those whose behavior moved to the held queue are removed with a line in the pull request naming the new test that covers each; those about `CodexRuns` alone assert `Busy`.
- [ ] **Step 4: Run** `cargo test -p notchling-engine tools::codex`, `cargo test -p notchling-notchlingd`. Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "codex: messages sent during a turn join one held message"`. The commit body lists every test removed or changed and its replacement.

### Task 4.4: Antigravity holds instead of refusing

**Files:**
- Modify: `crates/notchlingd/src/agy_ops.rs` (`agy.send` for an existing session: `refusal` first (held elsewhere: `failed(holder.note())`, as now), then the held queue: hold while working, prompt when idle), `crates/notchlingd/src/held_send.rs` (`AgySend::state` from the runner's busy flag and `elsewhere`)
- Test: `crates/notchlingd/src/held_agy_tests.rs` (new)

**Interfaces:**
- The runner is unchanged and still answers `RunError::Busy` to a prompt during a turn; `agy_ops` never sends it one (it holds).

- [ ] **Step 1: Write the failing tests** (ACP `FakeSpawner`): `a_message_during_a_turn_is_held_and_sent_after`, `two_messages_join`, `a_session_held_elsewhere_holds_nothing`, `cancel_lets_the_held_message_go_out`.
- [ ] **Step 2: Run** `cargo test -p notchling-notchlingd held_agy`; expected FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p notchling-notchlingd`, `cargo test -p notchling-engine tools::agy` (`runner_tests` unchanged). Expected: PASS.
- [ ] **Step 5: Commit** `git commit -m "antigravity: the box holds while a turn works"`.

### Task 4.5: The composer for every tool

**Files:**
- Modify: `crates/app/src/chat/view/tool/mod.rs` (`queues` answers true by default), `crates/app/src/chat/view/tool/antigravity.rs` (`refusal` keeps only the elsewhere note; `AGY_BUSY` goes), `crates/app/src/chat/view/tool/codex.rs`, `crates/app/src/chat/view/composer.rs` (the held line `Held by Notchling until the turn ends` and Take Back for every tool, from `session.held` as for Claude Code), the locale file if L1 has merged (call C25)
- Test: `crates/app/src/chat/view/tool/held_tests.rs` (new); the inline test edit listed in the zones table

**Interfaces:**
- No new model field if the held text already reaches `ConversationInput` or the root's composer draw through `tools.live`; otherwise the held text joins the composer's input (the root draws the reply box, so no cached part is affected).

- [ ] **Step 1: Write the failing tests:** `an_antigravity_session_that_works_takes_a_message`, `a_session_held_elsewhere_stays_off_with_its_note`, `the_held_line_shows_for_codex`.
- [ ] **Step 2: Run** `cargo test -p notchling-app tool::held_tests`; expected FAIL.
- [ ] **Step 3: Implement**; sizes and hover of the composer unchanged.
- [ ] **Step 4: Run** `cargo test -p notchling-app`; on the Mac `bench_chat` (`typing`, `sending` counts unchanged); render `--render chat` over a fixture with a held Codex message.
- [ ] **Step 5: Commit** `git commit -m "chat: every tool's box holds while its session works"`.

### Task 4.6: Cells, matrix, notes

**Files:**
- Modify: `contract/codex/support.rs` (`turn.take_back` Fallback `HeldQueue`: `The held message is Notchling's, so Notchling edits it`), `contract/antigravity/support.rs` (`turn.hold` Fallback `HeldQueue`: `agy acp takes one prompt per turn; Notchling holds the rest`; `turn.take_back` as Codex's), `contract/*/mod.rs` (`WIRED` gains `turn.hold` and `turn.take_back` for all three), `gaps.json` (−3), `unwired.json` (−3), `contract/conformance_turns.rs` (new; Fallback checks: the port answers `Unsupported`, the registry answers `HeldQueue`, the held queue holds and joins over fakes), `docs/provider-matrix.md`, `crates/notchlingd/AGENTS.md` (`held_ops.rs`, `held_send.rs`), `crates/engine/AGENTS.md` (`tools/codex/runs.rs`: no outbox), `crates/AGENTS.md` (the held queue for every tool)

- [ ] **Step 1:** declarations and files; **Step 2:** `cargo test -p notchling-engine` (summary `gaps 18`, `unwired 55`); **Step 3:** regenerate the matrix; **Step 4:** notes, `guard claudemd`, `strip_comments --check`; **Step 5:** commit `git commit -m "contract: the held queue for every tool"`.

## Checkpoint P4

- `cargo test --workspace --locked`, clippy (Linux and the Mac), fmt, deny.
- Conformance summary `gaps 18`, `unwired 55`.
- By hand over a fixture root with a fake `codex` that sleeps 5 s per turn: send three messages during a turn; one joined turn follows; take back during the turn returns all three.
- The pull request lists each Codex test changed or removed and the test that now covers its behavior (call 7).
