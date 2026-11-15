# Outbox: every message reaches its session, fast, once, in order

- **Date:** 2026-11-15
- **Status:** Approved design, written up for the build. The maintainer approved the design on 2026-11-15 ("finalize the mailbox, ready to plan and implement"), and answered [question 1](#for-the-maintainer) the same day: a message only in Claude Code's queue is not Sent; its row stays as **Queued in the session** (`in_queue`) until the model takes it. This text is amended for that answer, and again after the plan review of 2026-11-15 (rulings R12 to R22 in `docs/superpowers/plans/2026-11-15-outbox.ledger.md`). Nothing here is built.
- **Source:** the design page *Outbox & Ask*, Part 1 (published artifact v8; local copy `docs/local/2026-11-15-outbox-ask-design.html`, not in git). Its rules, bubble states, retention table, two build details and the table of known problems 1 to 10 are decisions and are not reopened here. Part 2 (Ask: agents asking you questions) is **not in scope**; it comes later.
- **Base:** `origin/agent-run-oss` 5f48fb0c, which holds the two fixes the design builds on: `agent/held-busy-background` (0617c60e: `turn_is_over` counts `busy` with `inFlight.tasks > 0` as "may be at its prompt") and `agent/reply-leftover-box` (1a73db2e: `Clearing`, `reply_attempts.rs`, `clear_box`, `ApiError::data`).
- **Touches:** decisions 0004 (the daemon owns what acts), 0007 (tiers, additive API), 0011 (one owner per resource), 0014 (app data in SQLite), 0018 (Chat's parts), the typed event stream (XXXX), the cluster v1 design (a mailbox stays on the machine that owns the session). It needs a decision record of its own (plan task "Decision record").
- **Gaps** the source left open are settled here and marked **Builder's call**. They are collected in [Builder's calls](#builders-calls). Questions only the maintainer can answer are at the end, in [For the maintainer](#for-the-maintainer).

## In short

1. **Send never waits.** Chat, the notch, the phone and old clients put a message in the session's **Outbox**, a table in `notchling.db`, and get its id back in milliseconds. Nothing is typed on the caller's thread.
2. **One postman per session** delivers that session's messages in the order they were sent. A slow session never holds up another.
3. **Deliver like the terminal.** The postman types a message as soon as the session's input box can take text, even mid-turn. It waits only for an open dialog, or for an earlier message (ours or one the person typed) still unread in Claude Code's own queue. Slash commands, Codex and Antigravity wait for the turn to end.
4. **Never twice.** After Return the postman watches the transcript (10 s, then one more look at 30 s). Taken by the model (a user line or a `queued_command` attachment): the row is deleted and the transcript shows the message. Only in Claude Code's queue (an `enqueue` line): **Queued in the session**; the row stays until the model takes it, and comes back as May already be in if Claude Code drops it (its queue is memory). Neither: **May already be in**, with Send Again and Drop. A daemon that stops while typing reloads that row as May already be in (or Queued in the session), never as Queued.
5. Leftover text of ours in the session's box is cleared (the leftover-box fix); anyone else's text stops the session's mailbox at **Box has other text**, with Clear Box and Send.
6. The Outbox is a mailbox, not a history: a row lives only until its delivery is settled. Unsent work is never deleted silently; after 7 days it is raised once in the Inbox with Send and Discard.
7. The ops are additive: `session.send` and `outbox.*`, an app-tier `outbox` topic. `session.reply` stays for old clients and runs on the Outbox. Held text (`held_ops.rs`) goes away.
8. Only character counts reach the Audit and Activity; the text lives only in the Outbox row and on the app-tier topic.

## Builder's calls

Each is marked *Builder's call* again in the section it belongs to.

| # | Call | Section |
| --- | --- | --- |
| B1 | The table is `outbox` in `notchling.db` (schema 3 to 4), class Node; one persisted state machine, order by an AUTOINCREMENT column; the daemon is the only reader and writer | [8](#8-data-model) |
| B2 | `sending` is one persisted state for "keys may be going out or Return was pressed and we are verifying"; Edit and Remove refuse it | [8](#8-data-model), [9](#9-states-and-what-moves-them) |
| B3 | The head row of a session blocks the rows behind it, whatever its state, so order holds; a stuck head is visible and has its own actions | [9](#9-states-and-what-moves-them) |
| B4 | Edit and Remove also work on Box has other text, Session stopped and Session not found rows (they are not being typed) | [9](#9-states-and-what-moves-them) |
| B5 | Verification counts from the attempt's start, not the queue time, in two counts (`Seen { enqueued, taken }`): `seen_before` (taken) and `queued_before` (enqueued) are the counts since `attempt_ms − 2 s` stored when the row is taken; the row is Sent when the taken count since that instant grows, and In Claude Code's queue when only the enqueued count does. It is the design's "count after queue time" made safe for rows that waited hours past the 1 MB transcript window | [10.6](#106-verifying) |
| B6 | "After typing" means after Return. A failure before Return (nothing submitted) never goes to May already be in: it retries, or stops at Box has other text | [10.5](#105-what-a-try-ends-with) |
| B7 | A session that disappears from every lane is never deleted from the Outbox by itself: its rows go to Session not found; only Notchling's own remove (or Discard) deletes them | [9](#9-states-and-what-moves-them) |
| B8 | Session stopped = the job's `state` is `stopped`; the postman never attaches to it. Resume and Send runs `claude respawn <job>` (`control::session::plan_respawn`) and then queues the row | [10.3](#103-the-gate-claude-code) |
| B9 | One `claude attach` per session is kept open between messages and closed after 10 s with nothing to type, on any failure, on acting off and at daemon close. Other acting ops (answer, permission, interrupt, take back, the wake nudge) take the same per-session lock and attach on their own | [10.4](#104-attach-reuse-and-the-per-session-lock) |
| B10 | The global `ActingOps::typing` mutex becomes a per-session lock map | [10.4](#104-attach-reuse-and-the-per-session-lock) |
| B11 | The safe rule (turn end) is the held path's existing gate: a fresh job row, `restart::restarted`, a 3 s settle and the check at the first key. It is used for slash commands, for a session whose last typed message showed no queue line within 5 s, and while the session's own transcript tail holds a queue line of a shape Notchling doesn't know (per session, on each read; not the versions soak) | [10.3](#103-the-gate-claude-code) |
| B12 | Only prompt-like entries in Claude Code's queue make the next message wait (`typed_prompt` rules); task notifications and agent messages don't | [10.3](#103-the-gate-claude-code) |
| B13 | Codex and Antigravity are delivered by their runners, not typed: Sent when the runner started the turn. A new `CodexRuns::send_if_idle` refuses instead of queueing in memory. Draft first messages (`codex-new:`, `agy-new:`) keep today's path | [10.8](#108-codex-and-antigravity) |
| B14 | `session.send` is refused while acting is off, like every acting op; rows already in the Outbox wait | [11.1](#111-sessionsend) |
| B15 | Text is at most 100 KB; at most 200 rows per session | [8](#8-data-model) |
| B16 | `session.reply` = `session.send` plus a wait of up to 50 s for the row to settle; a row that ends needing the user is removed and returned as an error, as today's reply errors are; a row that reaches Claude Code's queue answers at once with the queued shape and stays | [11.3](#113-sessionreply-kept) |
| B17 | The 7-day notice is one Inbox item per session (not per message), with no message text (the `inbox` table is class Cluster), raised once per row and not raised again after a restart | [13](#13-the-7-day-notice) |
| B18 | Held text is memory only today, so nothing carries over at the first start; `session.held` is no longer published and `session.held.discard` discards the session's removable rows | [12](#12-migration-from-held-text) |
| B19 | The notch has no reply box in the Rust app; it shows only a "needs you" hint. The phone's `session.message` goes into the Outbox and answers `{"sent": true}` as today; the phone shows no Outbox states in v1 and acts through the 7-day Inbox notice | [14](#14-what-the-user-sees) |
| B20 | Sessions in Notchling's own terminal host: the Rust daemon has no host-terminal client yet, so there is nothing to deliver to; the courier interface has room for one. Sessions in another terminal app are refused at Send | [10.9](#109-terminal-sessions) |
| B21 | Remote sessions (cluster) keep using `session.reply`, which the cluster already forwards to the owner node, where it runs on that node's Outbox. Cluster rules for `session.send` and `outbox.*` come after cluster b merges | [11.6](#116-cluster) |
| B22 | A merged prompt (ours and the person's taken together by Claude Code) is labelled on the transcript message in Chat, not on an Outbox row, since the row is settled by then: a merged user line counts as a take for each entry it joined, so the row is Sent (ruling R12, from a recorded merge) | [10.7](#107-merged-prompts) |
| B23 | New event kind `session.outbox` for row life (queued, sent, unsure, removed …); `session.reply` stays the per-try record; `session.reply_held` stays registered and is no longer emitted | [11.5](#115-events-and-audit) |
| B24 | A row is keyed by its job, not by the `sessions.list` row id: a Claude Code job's short id (`AgentTarget::Job`), which survives `/clear`, the first `sessionId`, `claude respawn` and a move; `codex:<thread>` and `agy:<conversation>` for the others. Ops take the row id callers know and resolve it *(settled after the builder check)* | [8](#8-data-model) |
| B25 | The phone's send reaches Codex and Antigravity sessions too, through `session.send` (the bridge already names them with `codex:`/`agy:` job ids and today answers "Session not found.") *(settled after the builder check)* | [11.4](#114-the-phone) |

## Contents

1. [Why](#1-why)
2. [Goals, non-goals, anti-goals](#2-goals-non-goals-anti-goals)
3. [Constraints](#3-constraints)
4. [Approaches](#4-approaches)
5. [What exists today](#5-what-exists-today)
6. [Terms](#6-terms)
7. [The design as decided](#7-the-design-as-decided)
8. [Data model](#8-data-model)
9. [States and what moves them](#9-states-and-what-moves-them)
10. [The postman](#10-the-postman)
11. [Ops, topic, events](#11-ops-topic-events)
12. [Migration from held text](#12-migration-from-held-text)
13. [The 7-day notice](#13-the-7-day-notice)
14. [What the user sees](#14-what-the-user-sees)
15. [Tests, rule by rule](#15-tests-rule-by-rule)
16. [Risks](#16-risks)
17. [Where the code goes](#17-where-the-code-goes)
18. [For the maintainer](#for-the-maintainer)

## 1. Why

Pressing Send in Chat or the notch makes Notchling type the text into the session's terminal, the way a person would. On 2026-11-15 that went wrong four ways:

- **Stuck in "Queued" for hours.** A session running background agents: Claude Code writes `status: busy` while anything runs, not only the model, so Notchling held the message until every agent ended (job be50d36a; fixed for that shape by `agent/held-busy-background`, but the guess remains).
- **Refused: "input box holds text".** Leftover text in the session's box made every reply fail `not_cleared`, with no way out (job 3dc1c4be; `agent/reply-leftover-box` clears Notchling's own leftovers and adds Clear Box and Send).
- **Lost.** Held text lives in the daemon's memory only (`held_ops.rs`); a restart drops it.
- **Slow.** `session.reply` returns only after the box let go of the text, about 4 s and up to 15 s, and Chat waits with it (`SESSION_CALL_TIMEOUT` 60 s).

The terminal never has these problems: Claude Code's input box takes text at any time. Idle, the text starts a turn; working, Claude Code queues it and the model reads it at its next step.

The cause is three choices: guessing "is a turn running?" from a flag that means something else, holding the message in memory, and making Send wait for the typing. The Outbox removes all three.

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. A message sent from Chat, the notch or the phone is saved before anything else and survives a daemon restart.
- G2. Send returns in milliseconds, whatever the session is doing.
- G3. A message reaches the session as soon as the terminal would take it, and steers a running turn the way typing in the terminal does.
- G4. Each session's messages arrive in the order sent, each as its own prompt, and a slow session never holds up another.
- G5. No message is typed twice by Notchling on its own. When Notchling can't tell, the person decides.
- G6. Every state a message can be in is on its bubble, and every state that needs the person has an action. No dead ends.
- G7. Codex and Antigravity sessions use the same mailbox.

**Non-goals**

- Ask (part 2 of the source): agents asking the person questions.
- A history of sent messages: the transcript is the record.
- Phone UI for Outbox states (v1 shows only the Inbox notice on the phone).
- A reply box in the Rust notch.
- Delivery into sessions in another terminal app, or into Notchling's terminal host before the daemon has a client for it.
- Mailboxes for sessions on worker machines (comes with the cluster work).
- Sending to a session from an agent (`session.send` is app tier).

**Anti-goals** (things this must never do)

- Type into a box holding text Notchling didn't put there, unless the person pressed Clear Box and Send.
- Type into an open question or permission dialog.
- Retype a message on its own when it may already be in the session.
- Resume a stopped session on its own (that spends the person's limits).
- Write message text to the Audit, Activity, `reply-screen.txt`, the Inbox or any log.
- Delete an unsent message without the person's action.

## 3. Constraints

- The local API and the phone protocol change additively only (`crates/AGENTS.md`, Where new things go). The phone protocol is not changed at all in v1.
- Acting is off at every daemon start until the app turns it on; nothing types while it is off.
- `reply/pin_tests.rs` pins `plan::reply`'s keys and timings to the Swift app; a new plan may not change them.
- Every key still waits on the input box (`terminal/input_box.rs`, `reply/exec.rs`): no key without the session's own prompt on screen, the paste only into an empty box, Enter only on the message and nothing else.
- Never hold a lock across I/O, a pipe or a Keychain call (Things that bit before). The Outbox's SQLite lock covers single statements only.
- Files under 1,000 lines (`LINT-FILE-SIZE`), no comments in code, tests in `<module>_tests.rs`. `acting_ops.rs` is already 906 lines after the leftover fix: new code goes in new files.
- Tests never touch real sessions, `~/.claude`, Keychain or a real `claude attach`; the fakes are `FakePrompt` (engine) and `acting_ops_tests::setup`'s fake terminal.
- A new `notchling.db` table needs a class in `store_class.rs`; the Outbox is Node.
- A new event kind changes the golden and `docs/log-kinds.md`; fields have no text (`LINT-LOG-NO-TEXT`).
- Two parallel cluster lanes edit `crates/local-api/src/wire.rs`, `crates/notchlingd/src/daemon_wiring.rs`, `daemon.rs`, `acting_ops.rs`, `lib.rs`, `op_kinds.rs`, `engine/src/cluster/policy.rs`, `docs/local-api-ops.md` and `crates/AGENTS.md` (cluster b tasks 12 to 14). Outbox edits there are additive blocks; the plan names each.

## 4. Approaches

**A. Hold until the turn ends (today).** Type at once if the session looks idle, else keep the text in memory, joined into one entry per session, and type it when a fresh read of the job files, the transcript and a 3 s settle say the turn is over. It fails exactly the four ways in [1](#1-why): the "turn over" guess rests on `status`, `state` and `inFlight.tasks`, which mean other things and lag; memory loses the text; the caller waits for the typing; and joined messages become one prompt. Each fix so far (`shell`, busy with tasks) narrowed one wrong guess and left the guess.

**B. Deliver like the terminal, one of ours in Claude Code's queue at a time (chosen).** Save first, then type as soon as the input box takes text, mid-turn included. Wait only for a dialog (text would answer it) and for an earlier prompt still unread in Claude Code's queue (Claude Code would merge the two). The transcript already says what is in that queue (`queue-operation` lines and `queued_command` attachments; Chat parses them today). No turn guess for ordinary messages; the guess survives only where it is needed (slash commands, Codex and Antigravity runners, and a safe fallback when the queue lines can't be read).

**C. Claude Code's own queue only.** Type every message at once and let Claude Code queue them. Simple, but Claude Code merges queued texts into one prompt, its queue is in memory (a Claude Code restart drops it, `crates/AGENTS.md`, "Queued for good after Claude Code restarted"), dialogs still eat text, and Notchling keeps no record, so a lost message is silent.

B keeps C's speed and steering, keeps the order and the one-prompt-per-message property C loses, and keeps a record that survives restarts, which A and C both lack.

## 5. What exists today

| Part | Where | What the Outbox does with it |
| --- | --- | --- |
| Held text | `notchlingd/src/held_ops.rs`, `engine/src/reply/held.rs` | Removed. Its gate (`calm_now`, `new_turn_since`, `SETTLE_MS`) becomes the safe rule |
| Turn gate pieces | `notchlingd/src/held_guard.rs` (`Rest`, `FromFiles`, `Guarded`), `engine/src/sessions/fresh.rs::job_row`, `transcript/restart.rs` | Kept, renamed `turn_gate.rs`; used by the safe rule |
| Typing a reply | `engine/src/reply/plan.rs::reply`, `exec.rs::execute_clearing`, `terminal/input_box.rs` | Reused unchanged; a new `plan::reply_in` for an attach that is already open |
| Leftover clearing | `notchlingd/src/reply_attempts.rs`, `exec::Clearing` (leftover-box fix) | Reused: every try notes its text there; `clear_box` comes from the row |
| `not_cleared` with the box text | `acting_ops.rs::play_inner` (`data {why, box}`) | Reused for the Box has other text state |
| Sent-since check | `engine/src/transcript/sent.rs::sent_since` | Joined by `count_since` (two counts, enqueued and taken, not yes/no) |
| Claude Code's queue | `engine/src/transcript/conversation/queue.rs` (`Queue`), `parser.rs::attachment`, `typed.rs::typed_prompt` | Reused by a light `queue_state` read |
| The turn's live flags | `sessions/jobs.rs::turn_is_over`, `Agent::at_prompt`, `turn_running`, `takes_held_text` | Read by the safe rule only |
| Codex's queue | `engine/src/tools/codex/runs.rs` (`state.outbox`, in memory) | Not used for existing sessions any more; `send_if_idle` added |
| Antigravity's turn | `notchlingd/src/agy_ops.rs::send` (busy is `failed`) | The Outbox waits instead |
| Chat's sending bubble | `app/src/chat/outgoing.rs` (`Outgoing`: copies before the send, arrived when more) | Kept for the moment between Sent and the reload that shows it |
| Chat's held bubble | `tools/live.rs` (`held`, `held_note`), `view/conversation.rs::user_row`, `view/tools.rs::discard_held` | Replaced by Outbox bubbles |
| Box-holds row | `app/src/chat/box_holds.rs`, `view/composer.rs::box_holds_row` (leftover fix) | Replaced by the Box has other text bubble |
| Phone's send | `engine/src/bridge/router_ops.rs::session_message` → `notchlingd/src/bridge_backend.rs::send_message` → `session.reply` | Calls `session.send` instead |
| Inbox | `engine/src/inbox/`, `notchlingd/src/inbox_ops.rs` | Raises the 7-day notice |

## 6. Terms

- **Row**: one message in the Outbox.
- **Session key**: what a row is keyed by (*B24*): a Claude Code job's short id (the `AgentTarget::Job` value, not the row id, which becomes the conversation's `sessionId` once Claude Code writes one), `codex:<thread>`, `agy:<conversation>`.
- **Head**: the session's oldest row still in the Outbox.
- **Postman**: the thread that delivers one session's rows.
- **Courier**: the per-tool part of the postman that knows how to deliver (type into a Claude Code job, start a Codex turn, prompt an Antigravity session).
- **Take**: the postman's one database step that marks the head `sending` and reads its text.
- **Try**: one take and what follows it.
- **Claude Code's queue**: text typed while the model works, held by Claude Code until its next step; written to the transcript as `queue-operation` lines. It is memory: a Claude Code restart drops it.
- **Taken**: the model read the message: a typed user line (after a `dequeue`, or with none when Claude Code was idle) or a `queued_command` attachment naming it, or for a slash command its `local_command` line. An `enqueue` line alone is not a take.
- **In Claude Code's queue** (`in_queue`): a row whose text Claude Code queued (an `enqueue` line) and the model has not taken yet.
- **Safe rule**: wait for the turn to end before typing (today's held gate).
- **Turn**, **transcript**, **busy**, **daemon**: as in the source's glossary.

## 7. The design as decided

From the approved source, restated; nothing in this section is open. The maintainer's answer of 2026-11-15 to question 1 ([For the maintainer](#for-the-maintainer)) amends rule 4, the bubble and retention tables, the first build detail and problem 10; the amended words are marked *(amended 2026-11-15)*.

### 7.1 Four rules

1. **Saved first.** The message is written to `notchling.db` before anything else, so a restart never loses it. Send returns in milliseconds; Chat shows the bubble as Queued.
2. **Deliver like the terminal.** Type the message as soon as the session can take text, even mid-turn. Only two things make it wait: an open permission or question dialog, and an earlier Notchling message still unread in Claude Code's own queue.
3. **One postman per session.** Only one thing types into a given terminal, in the order sent. A slow session never holds up another.
4. **Never deliver twice.** After typing, check the transcript. Taken by the model: Sent. Only in Claude Code's queue: kept as "Queued in the session" until the model takes it *(amended 2026-11-15)*. Unsure: don't retype; show "May already be in the session" with Send Again and Drop. Old text in the box: show it with Clear Box and Send.

### 7.2 Bubble states

| Bubble | Means | Actions |
| --- | --- | --- |
| Queued | In the mailbox, waiting its turn | Edit, Remove |
| Waiting: dialog open | The session is asking you something first | Answer the dialog |
| Sending | Being typed now | none (it takes a second) |
| Queued in the session | In Claude Code's queue: kept until the model takes it *(amended 2026-11-15)* | none |
| Sent | The transcript shows the model took it | none |
| May already be in | Typed, the transcript doesn't confirm it yet | Send Again, Drop |
| Box has other text | Shows that text | Clear Box and Send |

Plus, from the known problems: Session stopped (Resume and Send), Waits for the current turn (Codex, Antigravity, slash commands).

### 7.3 How long a row stays

| When | The row |
| --- | --- |
| Sent (the transcript shows the model took it) | Deleted at once; Chat shows the transcript's message |
| In Claude Code's queue | Kept until the model takes it (then Sent); if Claude Code drops it, May already be in *(amended 2026-11-15)* |
| You Remove or Drop it | Deleted at once |
| The session is removed | Deleted with it |
| Queued, Waiting, May already be in | Kept until sent or you act; unsent work is never deleted silently |
| Unsent after 7 days | Kept, raised once in the Inbox with Send and Discard |

At most 200 waiting rows per session. Only the character count goes to the Audit and Activity, never the text.

### 7.4 Two build details

- A message being typed when the daemon stopped comes back as May already be in, never as Queued; one the transcript shows only in Claude Code's queue comes back as Queued in the session *(amended 2026-11-15)*.
- Identical messages ("ok", "go on") are told apart by counting how many times the text appears in the transcript after each was queued, not by the text alone; that needs only the waiting rows, so sent rows can go at once. (Made precise in [10.6](#106-verifying), B5: two counts, enqueued and taken.)

### 7.5 Known problems and their solutions

| # | Problem | Solution | Here |
| --- | --- | --- | --- |
| 1 | Knowing one of ours is still in Claude Code's queue | Read the queue lines (enqueue, dequeue, remove, popAll, `queued_command`). Ours has left the queue when one names it, or a turn ends after it (*amended 2026-11-15*: "left", since a turn end is not a take, [6](#6-terms)). No queue line within 5 s of typing: the safe rule. A test per Claude Code version; the unknown-lines count switches to the safe rule | [10.3](#103-the-gate-claude-code) |
| 2 | Your own typing in the terminal | Text in the box is the Clear Box case. A message you typed sitting in Claude Code's queue makes ours wait. If they merge anyway, the bubble says so (the merged line counts as taken for ours, R12) | [10.3](#103-the-gate-claude-code), [10.7](#107-merged-prompts) |
| 3 | An attach per message is slow | One attach per session while it has rows; close after 10 s idle; the dialog check uses the same screen | [10.4](#104-attach-reuse-and-the-per-session-lock) |
| 4 | Slash commands | Only at turn end. Confirmed by the `local_command` line; `/clear` by its new transcript. Menu commands never go through the Outbox | [10.3](#103-the-gate-claude-code), [10.6](#106-verifying) |
| 5 | Long pastes and images | Match the transcript's full text, whitespace trimmed; images by path and caption | [10.6](#106-verifying) |
| 6 | A stopped session | Session stopped, Resume and Send; never resumed on its own | [10.3](#103-the-gate-claude-code) |
| 7 | Terminal sessions | Notchling's own terminal host takes text like a background job; another terminal app is refused at Send | [10.9](#109-terminal-sessions) |
| 8 | Codex and Antigravity | Same mailbox; the runner's own turn state; waits for the turn to end | [10.8](#108-codex-and-antigravity) |
| 9 | Edit or Remove while typing | The take is one database step; Edit and Remove only on Queued or Waiting rows | [9](#9-states-and-what-moves-them) |
| 10 | Too many May already be in | Watch the transcript (file events) up to 10 s, look once more at 30 s; a line showing the model took it (a user line, `queued_command`) is Sent; an `enqueue` line alone keeps the row as Queued in the session, watched until the model takes it or Claude Code drops it *(amended 2026-11-15)* | [10.6](#106-verifying) |

## 8. Data model

*Builder's call B1.* One table in `notchling.db`, created by schema version 4 in `engine/src/store.rs::migrate` (today 3), class **Node** in `store_class.rs::table_class`. The daemon is its only reader and writer (decision 0011): the app and the phone see rows only through ops and the topic. An older daemon opening a version-4 file ignores the table (its `migrate` stops at `version >= SCHEMA`); rows wait for the newer daemon.

```sql
CREATE TABLE IF NOT EXISTS outbox (
    n INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    session TEXT NOT NULL,
    tool TEXT NOT NULL,
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    options TEXT,
    via TEXT NOT NULL,
    state TEXT NOT NULL,
    why TEXT,
    clear_box INTEGER NOT NULL DEFAULT 0,
    queued_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    attempt_ms INTEGER,
    seen_before INTEGER,
    queued_before INTEGER,
    tries INTEGER NOT NULL DEFAULT 0,
    next_ms INTEGER,
    noticed_ms INTEGER
);
CREATE INDEX IF NOT EXISTS outbox_session ON outbox(session, n);
CREATE INDEX IF NOT EXISTS outbox_age ON outbox(queued_ms);
```

| Column | Meaning |
| --- | --- |
| `n` | Order. AUTOINCREMENT so a number is never reused (SQLite's `sqlite_sequence` is skipped by the class test's `sqlite_%` filter) |
| `id` | `o` + 32 hex characters from the OS's random source (as the vault's waits) |
| `session` | The session key (*B24*, [6](#6-terms)). The ops take the `sessions.list` row id callers already use and resolve it through the daemon's session list (`Agent::outbox_key()`: the `Job` short id, else the row id); Chat matches rows to a session the same way. Since the key survives `/clear`, the first `sessionId`, `claude respawn` and `session.move` (the job keeps its id in its new lane), a row is never re-keyed; a move that reports another job id calls `OutboxStore::rekey` |
| `tool` | `claude`, `codex`, `antigravity` |
| `kind` | `prompt`, or `command` for text starting with `/` (Claude Code only; Codex and Antigravity commands are prompt text as today) |
| `text` | The message as sent (Chat's `text\n\n<picture paths>` included), at most 100 KB (*B15*) |
| `options` | JSON or null: Codex `{model, effort}` as `codex.send` takes them, checked again at delivery |
| `via` | `chat`, `notch`, `phone`, `agent` (the existing `ReplyVia` words) |
| `state` | [9](#9-states-and-what-moves-them) |
| `why` | A reason code for `waiting` (`dialog`, `queue`, `turn`, `retry`), `unsure` (`not_found`, `restart`, `dropped`: Claude Code's queue lost it untaken), `box` (`not_cleared`, `mixed`); none for `in_queue`; never text |
| `clear_box` | The person pressed Clear Box and Send; used by the next try only, then reset |
| `queued_ms`, `updated_ms` | Times in ms since 1970 |
| `attempt_ms`, `seen_before` | Set by the take: when the try began, and the **taken** count (user lines, `queued_command`, a command's `local_command` line) since `attempt_ms − 2 s` that the verification compares with (B5) |
| `queued_before` | Set by the take beside `seen_before`: the **enqueued** count (`queue-operation` `enqueue` lines) since `attempt_ms − 2 s`; a growth of it alone makes the row `in_queue` (B5, [10.6](#106-verifying)) |
| `tries` | Takes so far; drives the retry back-off |
| `next_ms` | Earliest next try after a failure that typed nothing |
| `noticed_ms` | When the 7-day notice was raised for this row |

**Retention** is the source's table: a row is deleted when it is Sent, Removed, Dropped, Discarded or its session is removed by Notchling. Nothing expires by age. *B15:* `session.send` refuses a session that already has 200 rows (`failed`: "200 messages are already waiting for this session.") and text over 100 KB (`bad_request`).

**Access.** `engine/src/outbox/store.rs` keeps one `rusqlite::Connection` behind a mutex, opened with `store::open`; each call is one statement or one `BEGIN IMMEDIATE` transaction (insert with the cap count, take, edit, re-key), and nothing else (no pty, no file read) runs while the mutex is held. The busy timeout stays 5 s.

## 9. States and what moves them

*Builder's calls B2, B3, B4, B7.*

| State | Bubble | Who moves it out |
| --- | --- | --- |
| `queued` | Queued | The postman (take), Edit, Remove |
| `waiting` | `dialog`: Waiting: dialog open; `turn`: Waits for the current turn; `queue`, `retry`: Queued | The postman, Edit, Remove |
| `sending` | Sending | The postman only |
| `in_queue` | Queued in the session (no buttons; not a "needs you" state) | The postman only: Sent (delete) when the model takes it, `unsure`/`dropped` when Claude Code drops it ([10.6](#106-verifying)); `outbox.discard` and the 7-day notice's Discard delete it |
| `unsure` | May already be in | Send Again (to `queued`), Drop (delete) |
| `box` | Box has other text: «what the box holds» | Clear Box and Send (to `queued` with `clear_box`), Edit, Remove |
| `stopped` | Session stopped | Resume and Send (respawn, then `queued`), Edit, Remove |
| `gone` | Session not found | Remove; the postman re-checks it every `GONE_RECHECK_MS` (10 s) with a fresh job read and puts it back to `queued` if the session comes back (R15) |

- **The head blocks.** Only the head of a session is ever taken. Rows behind a head in any state are `queued` and show Queued. A head that needs the person (`unsure`, `box`, `stopped`, `gone`) stops the session's mailbox until they act; that is the order rule, and the bubble says why (*B3*). An `in_queue` head stops it too, until the model takes it or Claude Code drops it: a later row typed meanwhile would sit behind it in Claude Code's queue and could be merged with it.
- **The take** is `UPDATE outbox SET state='sending', attempt_ms=?, seen_before=NULL, queued_before=NULL, tries=tries+1, updated_ms=? WHERE id=? AND state IN ('queued','waiting') AND (next_ms IS NULL OR next_ms <= ?)` in one transaction; zero rows changed means Edit or Remove won, and the postman starts over (problem 9). Right after it, before any key, the postman counts both baselines ([10.6](#106-verifying)) and stores them with `OutboxStore::set_seen(id, seen: Seen)` (`seen_before` = `taken`, `queued_before` = `enqueued`).
- **Edit** (`outbox.edit`) and **Remove** (`outbox.remove`) change or delete a row only `WHERE state IN ('queued','waiting','box','stopped','gone')` (*B4*: those rows are not being typed). On `sending` they answer `failed` "It's being sent." and on `in_queue` `failed` "It's in the session's queue." (both `Change::Busy`), and Chat keeps the buttons off. Remove on `unsure` is Drop. Edit resets `tries`, `next_ms` and `clear_box`.
- **Send Again** (`outbox.resend {id}`) on `unsure` sets `queued`, clears `attempt_ms`, `seen_before` and `queued_before`. The next try's `Clearing.earlier` includes the row's text, so if the text is still in the box it is cleared, not doubled (`exec.rs::submit`'s existing rule).
- **Gone** (*B7*): a session missing from the fresh read and from every lane's `jobs/` for 60 s (`GONE_AFTER_MS`) moves its head to `gone`; rows are never deleted for it. The postman re-checks a `gone` head on the tick every `GONE_RECHECK_MS` (10 s) with a fresh job read (`sessions::fresh::job_row`, `job_state`); when the session is back the head goes to `queued` and the postman wakes (R15). An `in_queue` head goes to `unsure`/`dropped` instead, since its copy left with Claude Code's queue: the job counts as gone after `GONE_AFTER_MS` (60 s) missing, and then the 30 s grace of [10.6](#106-verifying) runs, so the row is `unsure`/`dropped` 90 s after the job vanished at the earliest; a stopped job or a changed Claude Code process starts the 30 s grace at once (R21). Codex and Antigravity rows go `gone` when their rollout or summaries row is gone.
- **Session removed**: `session.remove` (and `codex.delete`) that succeeds deletes the session's rows and publishes `outbox.removed` with `why: "session_removed"`.

## 10. The postman

### 10.1 Shape

`notchlingd/src/outbox_postman.rs`. A `Postmen` registry keyed by session id; a postman thread exists while its session has rows, at most `POSTMEN_MAX` (32) at once (the rest start as threads end; the 1 s tick retries). Each loops on a channel with `recv_timeout(next deadline)`:

- **Wake** messages: a new row, an action on a row, acting turned on, the 7-day notice's Send.
- **Tick**: at most every second while it has something timed (a settle, a retry, a verification, an `in_queue` head's watch and its verdict deadline `gone_since_ms + VERIFY_LOOK_MS` (R13), a `gone` head's 10 s re-check (R15), the attach's idle close).
- It ends when its session has no rows and its attach is closed.

Nothing runs while acting is off: a postman parks, closes its attach, and is woken by `settings.set_acting`. A plan already typing finishes (as today).

### 10.2 One pass

```
head = first row of the session (by n)
none                          → close the attach after 10 s idle, then end
unsure | box | stopped       → park until an action wakes it
gone                          → park; every 10 s re-read the job fresh: back → queued, wake (R15)
in_queue                      → watch (10.6): taken → Sent; dropped for 30 s → unsure/dropped;
                                else wait for the tick (a transcript or job change, or the
                                timer armed for gone_since_ms + 30 s, R13)
                                (the attach closes after 10 s, as with nothing to type)
queued | waiting              → gate (10.3 or 10.8)
  Wait(why, recheck)          → persist waiting/why if it changed, sleep until recheck
  NeedsUser(state)            → persist state (box, stopped, gone)
  Go { guarded }              → take; courier.deliver; outcome (10.5); verify (10.6)
```

### 10.3 The gate (Claude Code)

Pure, in `engine/src/outbox/gate.rs`: facts in, a decision out. The daemon gathers the facts for a `queued` or `waiting` head only (an `in_queue` head is watched, not gated, [10.6](#106-verifying)). The checks run in this order, the first that holds decides (R20: the same order as `gate.rs`, so a row in back-off keeps `why: retry` until its `next_ms`):

1. **Fresh job row**: `sessions::fresh::job_row` by the row's key (never the 5 s cache, so it works before the first scan fills `sessions.list`, as at start-up) and the job's own `state` (a small new `sessions::fresh::job_state`).
2. **Gone**: no row and no `jobs/<id>/` in any lane for `GONE_AFTER_MS` → `NeedsUser(gone)` (*B7*).
3. **Stopped** (*B8*): `state == "stopped"` → `NeedsUser(stopped)`. Never attached. Resume and Send (`outbox.resend {id, resume: true}`) runs `control::session::plan_respawn` (`claude respawn <job>` with the job's `CLAUDE_CONFIG_DIR`) through the daemon's launcher, then sets the row `queued`. A respawn that fails leaves the row `stopped` and returns the error to the caller.
4. **No replies here**: `!agent.can_reply()` can't happen for a row (refused at Send, [10.9](#109-terminal-sessions)); if a session changes into one, `NeedsUser(gone)`.
5. **Dialog**: the live status is `waiting`, or the last screen the postman read showed a dialog → `Wait(dialog, 30 s)`; re-checked on the tick from the fresh row, and by the plan's `RefuseDialog` on the open attach (problem 3: same screen).
6. **Retry back-off**: `next_ms` in the future → `Wait(retry, next_ms)`.
7. **Command** (`kind == command`, problem 4) → the safe rule.
8. **Safe mode** (*B11*) → the safe rule. A session is in safe mode when its last typed message showed neither a new `enqueue` line nor a new taken line naming it within `QUEUE_LINE_MS` (5 s) of Return, until a later message shows one within 5 s; and while the read in item 9 finds a queue line it doesn't know (`QueueState::unknown > 0`: a `queue-operation` whose `operation` is outside `enqueue`, `dequeue`, `remove`, `popAll`, or a `queued_command` attachment whose `prompt` is neither text nor blocks). That count is per session and fresh on every read, so one odd line elsewhere never turns steering off everywhere, and it doesn't depend on version checks being on. *Settled after the builder check:* the versions soak (`versions/soak.rs`) latches every kind of unknown line until Claude Code changes version and runs only while version checks are on, so the gate does not read it. *R18:* the session's safe-mode memory is one `SafeModes` behind an `Arc`, shared by the Claude Code courier (which reads it for `Facts.safe`) and the transcript verifier (which notes each 5 s test in it).
9. **Claude Code's queue** (problems 1 and 2): `transcript::queue_state(path)` (new, `engine/src/transcript/queue_state.rs`) reads the last 1 MB and runs the existing `conversation::queue::Queue` over it, plus two rules: an entry leaves the list on a `queued_command` attachment naming it, and on a `turn_duration` written after it. (Leaving this list is not always a take: a `turn_duration`, `remove` or `popAll` is not one, [6](#6-terms), [10.6](#106-verifying).) Its `waiting` list keeps only prompt-like entries (`typed_prompt`'s rules: not `<task-notification>`, not `<agent-message`, *B12*). Any waiting entry, ours or the person's, → `Wait(queue, on change)`; the postman re-reads when the transcript's size changes (a stat on the tick).
10. Otherwise **Go**, unguarded.

**The safe rule** is today's held gate, moved: a fresh row that `takes_held_text()`, for a Working-with-`at_prompt` row a transcript with no turn after the last `turn_duration` (`restart::restarted`), the reading standing for `TURN_SETTLE_MS` (3 s), then **Go { guarded: true }**: the `Guarded` pty (`held_guard.rs`, renamed `turn_gate.rs`) asks again at the first key and refuses with nothing typed (`not_idle`). While it isn't met: `Wait(turn, 1 s)`.

### 10.4 Attach reuse and the per-session lock

*Builder's calls B9, B10.*

- `ActingOps::typing` (one mutex for every session today) becomes `TypingLocks`: a map from job id to `Arc<Mutex<()>>`, created on first use and dropped when unused. `play`, `play_guarded`, `play_clearing`, `interrupt`, `permission_on_screen`, `question`, `take_back`, the wake loop's nudge and the postman take the lock of the session they type into, and never two at once. A slow attach to one session no longer holds up another (rule 3).
- The postman keeps the `Box<dyn Pty>` from `Attacher::attach` between rows. The first row runs `plan::reply` (attach wait included); later rows on the same open attach run `plan::reply_in(text)`, a new plan with `plan::reply`'s steps from the first `RefuseDialog` on, and no detach. `plan::reply` and the pinned timings are unchanged.
- The postman holds the session's typing lock only while a plan runs, not while the attach sits open. Another op that needs the session attaches on its own (two attaches to one job is what a person with a terminal open already causes).
- The attach is closed (its plan's detach wait, then drop) after `ATTACH_IDLE_MS` (10 s) with no row to type, after any failure (the screen's state is unknown), on acting off, and in `close()`.

### 10.5 What a try ends with

*Builder's call B6.* The postman's pty wrapper (the `Guarded` one, generalised) notes two facts: `pasted` (a bracketed paste went out, as `typed` today) and `entered` (Return went out). Only a try that pressed Return can have submitted anything.

| Result | Row goes to |
| --- | --- |
| `Ok` | Verify ([10.6](#106-verifying)) |
| `Failure::Dialog` before the paste | `waiting`/`dialog` |
| `Failure::NotCleared` | `box`/`not_cleared`; the box text is read off the last screen (`input_box::typed_text`, cut at `BOX_SHOWN_MAX`), kept in memory only and sent on the topic |
| `Failure::Mixed` (ours pasted beside someone's text, no Return) | `box`/`mixed`, same; our text is noted in `reply_attempts` |
| Any failure with `entered` (`NotSent`, a timeout or pty error after Return) | Verify |
| Refused at the first key (`not_idle`, safe rule) | `waiting`/`turn` |
| Any other failure without Return (`NoInput`, `NotTaken`, `Timeout`, attach failed, pty error) | `waiting`/`retry`, `next_ms` = now + 30 s, 60 s, 120 s, then 300 s; if `pasted`, the text is noted in `reply_attempts` so the next try clears it as our leftover |

Every try's end is a `session.reply` event (via, chars, images, result, why, `waited_ms` = `attempt_ms − queued_ms`) and, for a failure, `reply-screen.txt` as today (masked; never the text). `reply_attempts` is kept as the leftover-box fix made it; it is still memory only.

### 10.6 Verifying

*Builder's call B5, amended 2026-11-15 (the maintainer's answer to question 1).* `engine/src/transcript/sent.rs::count_since(path, message, since_ms) -> Option<Seen>`, with `pub struct Seen { pub enqueued: usize, pub taken: usize }`, counts, in the last 1 MB, the lines at or after `since_ms` that carry the message, in two counts:

- **`enqueued`**: a `queue-operation` `enqueue` line naming it (its `content`). Claude Code queued it; the model has not read it.
- **`taken`**: a typed user line (`typed_prompt` rules: not a tool result, not a subagent line, not meta; `<bash-input>` counts); an `attachment` of type `queued_command` naming it (its `prompt`, text blocks joined); for a command, its `local_command` system line or an older `<command-name>` user line; and a **merged** user line (R12): when Claude Code dequeues several entries together it writes one user line holding their texts joined with `"\n"` (recorded on Claude Code, 2026-11-09: `enqueue "hi"`, `enqueue "hi"`, a `stop_hook_summary`, two `dequeue` lines at the same ms, then one user line `"hi\nhi"`). `count_since` keeps the enqueued texts in queue order, pairs each `dequeue` with the oldest one, and a user line that follows such dequeues and equals their texts joined with `"\n"` (squeezed) counts one take for each joined entry that matches the message. A merged line is a take, not a drop.

A `dequeue` line carries no text, so it counts only through the user line that follows it. A `popAll` or `remove` line counts in neither: `popAll` is Up pulling the queue back into the box (`crates/AGENTS.md`), so the text left the queue without the model reading it, and `remove` takes it out. It is `None` when the 1 MB window starts after `since_ms` (as `sent_since`). Matching (problem 5): both sides whitespace-squeezed; a message's picture paths (Chat's `drops::with_attachments` form; the path rule moved from `app/src/chat/drops.rs::split_images` into the engine) are cut out and counted, and a transcript line's `[Image #N]` chips are cut out and its `image` blocks counted, so a message matches a line with the same caption and at least as many pictures; a `!cmd` message matches `cmd` in an enqueue line and `<bash-input>cmd</bash-input>` in a user line; a command matches its `local_command` system line (or an older `<command-name>` user line) by name and arguments; `/clear` is confirmed by a successor transcript (`transcript::locate`'s successor lookup) newer than the try, which counts as taken. Long pastes need nothing special: the transcript holds the full text.

**Counting.** Right after the take, before any key, the postman stores both baselines from `count_since(path, text, attempt_ms − 2 s)`: `seen_before` = its `taken`, `queued_before` = its `enqueued` (`OutboxStore::set_seen(id, seen: Seen)`). A count of `None` at the take (no transcript yet, or the 1 MB window already past `attempt_ms − 2 s`) stores `Seen::default()` (both 0) with `set_seen` and the try verifies against it (R17), so a row never reaches verification without baselines. The row is Sent once the `taken` count since that instant is above `seen_before`; it is In Claude Code's queue once only the `enqueued` count is above `queued_before`. Two "ok" rows: the second's take counts the first's lines in its baselines, so only a new line confirms it. Only the waiting row is needed; sent rows are gone (source detail 2). Counting from the try's start rather than the queue time is the same rule for a row taken at once, and stays inside the 1 MB window for a row that waited behind a dialog for hours.

**Timing** (problem 10): after the plan ends, a `platform::watch::Watcher` on the transcript's folder (debounce 100 ms, max latency 500 ms) wakes a count on each batch naming the file, for `VERIFY_WATCH_MS` (10 s), then one more count at `VERIFY_LOOK_MS` (30 s) after Return. Each count is judged by `engine/src/outbox/verdict.rs::verdict`, `Verdict { Sent, InQueue, Pending, Unsure }`:

- `taken > seen_before` → **Sent**: delete the row, publish `outbox.removed {why: "sent"}`, emit `session.outbox` `sent`, forget the session's `reply_attempts`.
- else `enqueued > queued_before` → **InQueue**: the row goes to `in_queue` at once (persisted, `outbox.changed`, `session.outbox` `in_queue`) and stays; the watch below takes over (a taken line inside the same 10 s still makes it Sent).
- else before 30 s → **Pending**; at 30 s → **Unsure**: `unsure`/`not_found`. A `None` count is neither taken nor enqueued.

The session's next row waits for this verdict, and behind an `in_queue` head for as long as it stays there.

**Watching a row in Claude Code's queue.** The postman keeps watching an `in_queue` head: on the tick it re-reads when the transcript's size changes (a stat) or the job's files change, with `count_since` (same `since`, same baselines) and `transcript::queue_state` ([10.3](#103-the-gate-claude-code) item 9). A new taken line (`taken > seen_before`) → Sent, as above (delete, `outbox.removed {why: "sent"}`, `session.outbox` `sent`). The row goes to `unsure` with `why: "dropped"` (`Why::Dropped`) when Claude Code's queue no longer holds it and no taken line appeared within `VERIFY_LOOK_MS` (30 s) of first seeing that. Claude Code's queue no longer holds it when any of these holds:

- `queue_state`'s `waiting` no longer lists the entry: a `remove`, a `popAll`, the lost-queue rule after a Claude Code restart (a typed prompt with no `dequeue` before it), or a later `turn_duration` with no take;
- the job is stopped, or gone: no row and no `jobs/<id>/` in any lane for `GONE_AFTER_MS` (60 s), the same rule as [9](#9-states-and-what-moves-them)'s Gone (R21); a shorter absence is not gone;
- the job's Claude Code process changed: its pid or start time, from the job's files if they record one, against what the postman noted when the row went `in_queue` (kept in memory only).

So `still_listed` is true only while `queue_state` lists the entry **and** the job is not stopped, not gone and its process is unchanged (R13). `gone_since_ms` is set the first time `still_listed` is false and is cleared only when all four hold again; an entry listed again in a job that stopped or changed process keeps it. A stopped job or a changed process starts the 30 s grace at once; a vanished job starts it once it has been missing for 60 s, so such a row is `unsure`/`dropped` 90 s after the job vanished at the earliest (R21). The postman keeps a timer armed for `gone_since_ms + VERIFY_LOOK_MS`, so the verdict runs at 30 s even when no file changes (R13). The 30 s grace covers a take written just after the queue line that ended it (a `turn_duration`, then `dequeue` and the user line). `None` (the 1 MB window has passed the attempt) never counts as a take. The rule is pure, `verdict.rs::queued_verdict(seen: Option<Seen>, before: Seen, still_listed: bool, gone_since_ms: Option<i64>, now_ms: i64) -> QueuedVerdict { Sent, Stay, Unsure }`: `seen.taken > before.taken` → Sent; `still_listed` → Stay; `gone_since_ms` (when the postman first saw `still_listed` false) at least `VERIFY_LOOK_MS` before `now_ms` → Unsure; else Stay. The implementer may refine the signature. A `dropped` row is an ordinary `unsure` one: Send Again, Drop, the 7-day notice.

The 5 s queue-line test for safe mode ([10.3](#103-the-gate-claude-code) item 8) is read from the same counts and is unchanged: did a new `enqueue` or a new taken line naming the message appear within 5 s of Return (`enqueued > queued_before` or `taken > seen_before`).

**At daemon start** (`OutboxOps::load`, called between `Server::bind` and `server.spawn()` like `InboxOps::load`): every Claude Code row in `sending` is counted once with its stored `attempt_ms`, `seen_before` and `queued_before`: taken grew → deleted as Sent; else enqueued grew → `in_queue` (its postman then watches it); else, or `None`, or no baselines stored → `unsure`/`restart` (source detail 1). A row already `in_queue` stays `in_queue` and its postman resumes watching it; the Claude Code process it was queued in was kept in memory only, so a Claude Code restart while the daemon was down is caught only by the queue read (the lost-queue rule) or the job's state ([16](#16-risks)). Codex and Antigravity rows in `sending` become `unsure`/`restart` without a count: only Claude Code is verified in the transcript (R14, [10.8](#108-codex-and-antigravity)). At most one row per session is `sending`, and each count is one 1 MB tail read.

### 10.7 Merged prompts

*Builder's call B22.* If the person types in the terminal while ours sits in Claude Code's queue, Claude Code may take both as one prompt (problem 2, "if they merge anyway, the bubble says so"). Our row is `in_queue` then. *Amended by R12 (overrides the earlier text here, which treated a merged line as no take):* a real merge was recorded on 2026-11-09: `enqueue "hi"`, `enqueue "hi"`, a `stop_hook_summary`, two `dequeue` lines at the same ms, then one user line `"hi\nhi"`. Claude Code joins the texts of the entries it dequeues together with `"\n"`. `count_since`'s `taken` counts such a merged line once for each joined entry (the enqueued texts those dequeues took, in queue order) that matches the message ([10.6](#106-verifying)), so a row merged with the person's text is **Sent**, not May already be in: the model did read it. The row is not the place for the label (it is deleted as Sent), so the label goes on the transcript message: the conversation parser marks a typed user line (or `queued_command` attachment) that was taken after two or more `dequeue` lines and holds their entries' texts joined with `"\n"` as `merged`, and Chat's user row shows a caption "Sent together with another message". The fixtures (`crates/engine/tests/fixtures/outbox/merged.jsonl` and `crates/engine/tests/fixtures/queue/merged.jsonl`) copy the recorded shape with synthetic text.

### 10.8 Codex and Antigravity

*Builder's call B13.* A courier per tool behind one trait (`notchlingd/src/outbox_courier.rs`): `facts`, `deliver`, and `verifies() -> bool` (R14). Only Claude Code is verified in the transcript: the Claude Code courier answers `true`; Codex, Antigravity and the `Refuse` placeholder answer `false`. With a courier that doesn't verify, a try that ends in `After::Verify` is Sent at once (the runner started the turn), with no transcript count or watch; at daemon start such a row in `sending` becomes `unsure`/`restart` ([10.6](#106-verifying)).

- **Gate**: Codex is busy while `CodexRuns::is_running(session)`; Antigravity while `AgyOps`' runner `is_working(session)`. Busy → `Wait(turn, 1 s)` ("Waits for the current turn", problem 8). A session another `agy` holds (a delegation, `elsewhere`) is refused at Send as today; one that becomes held later waits as `turn`.
- **Deliver**: Codex calls a new `CodexRuns::send_if_idle`, which starts the turn or answers `Busy` without queueing (the in-memory `state.outbox` stays only for drafts); Antigravity calls the runner's `prompt`. Started → Sent: the prompt is an argument or a JSON-RPC request, not typing, so there is no box and no doubt. These rows are never `in_queue` (unchanged by the answer to question 1). `Busy` → `waiting`/`turn`. Any other error → `waiting`/`retry` with the error kept in memory for the bubble.
- **Sent means the turn went out** (*R24*). A Codex row is Sent only once the process was spawned with the prompt and its follow thread is running; an Antigravity row once its `session/prompt` request was written to the agent, not while it waits behind a session load. A try that did not dispatch waits and is never Sent or `unsure`: acting going off before dispatch, or a runner that refuses, waits like a busy session (`turn`, no note, no back-off); a load error is `waiting`/`retry` with the error as the row's note. A try cut by us after it dispatched (acting going off interrupts the turn, a follow-thread spawn failing after the process started) is `unsure` with `why` `restart` (May already be in) and is never retried by itself. An Antigravity session another `agy` holds waits as `turn` without being taken every second: the courier reads a cached `elsewhere` state, refreshed every few seconds outside any lock.
- **Options**: the row's `options` (`model`, `effort`) are checked again at delivery with `codex.send`'s rules.
- **Drafts** keep today's path: a first message to `codex-new:`/`agy-new:` (or `draft_folder`) starts the session directly and is not an Outbox row; its later messages are.
- `codex.send {id}` and `agy.send {id}` to an existing session now insert a row and answer at once (`codex.send`: `{"id", "session", "state": "queued", "waiting": <position>, "outboxId"}`; `agy.send`: `{"id", "sent": true, "outboxId"}`).

### 10.9 Terminal sessions

*Builder's call B20.* Rows exist only for targets the daemon can deliver to: a background job (`AgentTarget::Job` with `can_reply()`), a Codex session, an Antigravity session. A session in another terminal app (`AgentTarget::Process`, `Resume`) or an `exec` command job is refused at Send with `bad_request` "Reply in its terminal." and nothing goes in the mailbox (problem 7). Notchling's own terminal host (`notchling-ptyhost`) has no client in the Rust daemon today (only the phone's `ConsolePty` control refers to it); when one is added it becomes a courier with the same input-box checks, and such sessions take rows like background jobs.

## 11. Ops, topic, events

All additive. Names go in a new block at the end of `wire.rs`'s `ops` and `topics` (the cluster lanes add theirs at the same places; the plan's merge note covers it). Every op below is app tier (`OpSpec::APP`) and, except `outbox.list`, behind the `acting` switch like every acting op. They are served by `OutboxOps` (`notchlingd/src/outbox_ops.rs`, an `OpGroup`).

### 11.1 `session.send`

`{id, text, clear_box?: bool, options?: {model?, effort?}, via?}` → `{"outboxId", "session", "state": "queued", "position": n}` where `position` is 1 for the head.

Refused (nothing saved): acting off (`forbidden`, `ACTING_OFF`, *B14*); empty text; text over 100 KB; no such session; a target that takes no replies ("Reply in its terminal."); a menu command (`control::slash::opens_menu`: `bad_request` "That command opens a menu; open the session's terminal."); 200 rows already; an Antigravity session another `agy` holds. A draft id (`codex-new:`, `agy-new:`) is refused here; drafts use `codex.send`/`agy.send`.

### 11.2 `outbox.*`

| Op | Args | Reply |
| --- | --- | --- |
| `outbox.list` | `{session?}` | `[row…]` in order, all sessions or one; not behind acting (read only) |
| `outbox.edit` | `{id, text}` | `{"edited": true}`; `failed` "It's being sent." on a `sending` or `unsure` row; `failed` "It's in the session's queue." on an `in_queue` row; not found is `bad_request` |
| `outbox.remove` | `{id}` | `{"removed": true}`; `failed` "It's being sent." on `sending`; `failed` "It's in the session's queue." on `in_queue` |
| `outbox.resend` | `{id, clear_box?: bool, resume?: bool}` | `{"state": "queued"}`: Send Again on `unsure`, Clear Box and Send on `box` (`clear_box: true`), Resume and Send on `stopped` (`resume: true`), Try Now on `waiting`/`retry` (clears `next_ms`) |
| `outbox.discard` | `{session}` | `{"discarded": n}`: removes the session's rows that may be removed (every row but a `sending` one; an `in_queue` row too, since Discard is an explicit action) |

A row on the wire: `{id, session, tool, kind, text, state, why?, box?, position, queuedMs, updatedMs, via, error?}` (`box` only for `box` rows, from memory; `error` only for Codex and Antigravity `retry` rows).

**Topic `outbox`**, app tier (its `OpGroup::topic_tier` says `Tier::App`; the `tools` topic that carried `session.held` falls through to agent tier today, which let agents read held text): `outbox.changed {row}` on every insert or change; `outbox.removed {id, session, why: "sent" | "removed" | "discarded" | "session_removed"}`.

### 11.3 `session.reply` kept

*Builder's call B16.* `session.reply {id, text, clear_box?}` inserts a row as `session.send` would, then waits up to `REPLY_WAIT_MS` (50 s, under the app's 60 s `SESSION_CALL_TIMEOUT`) for it to settle:

- Sent → `{"sent": true}`.
- In Claude Code's queue (`in_queue`) → at once, without waiting for the deadline, `{"sent": true, "state": "queued", "waiting": <position>, "outboxId"}` (today's queued shape); the row stays and its postman keeps watching it.
- Still queued, waiting or sending at the deadline → `{"sent": true, "state": "queued", "waiting": <position>, "outboxId"}` (today's held shape without `held`); the row stays.
- `box` → `failed` with today's message and `data {why: "not_cleared", box}` (`box` is the row's in-memory note, `OutboxOps::note`, carried by `Settled::NeedsUser(Row, Option<String>)`, R19); `unsure` → `failed` with `NotSent`'s message; `stopped`, `gone` → `failed` "The session is stopped." / "No such session."; in these four cases the row is removed first, because the caller puts the text back in its reply box as it does today.

Callers after the change: cluster forwarding of remote sessions (*B21*) and any older app; Chat, the notch and the phone move to `session.send`. The wake loop's nudge keeps its direct typing (`ActingOps::nudge`) under the per-session lock; it is not an Outbox row.

`session.held.discard {id}` stays and does `outbox.discard {session: id}` (`{"discarded": true}` when it removed any). `session.held` is no longer published (*B18*).

### 11.4 The phone

`DaemonActing::send_message` (`bridge_backend.rs`) calls `session.send` with `via: "phone"` through the daemon's `Ops` (today it calls `ActingOps` only) and maps any reply to `{"sent": true}`; refusals map to today's `BridgeFailure`s (an Antigravity session another `agy` holds is `failed` with its message). *B25:* `row_of` stops filtering to Claude Code jobs: a `codex:`/`agy:` `Control::Job` id is passed as the row id, so the phone's reply box on those sessions, which answers "Session not found." today, works. The phone protocol is unchanged (*B19*). The phone's `session.sendNow` (Esc so the queued message runs next) is unchanged. The 7-day notice reaches the phone through the Inbox, and Send and Discard work there (bridge on, acting on, as every `inbox.answer`).

### 11.5 Events and audit

*Builder's call B23.*

- `session.reply` (exists): one per try's end, from the postman, `via` the row's `via`. Its `waited_ms` becomes the time from queue to try.
- `session.outbox` (new, `crates/events/src/kinds/agents.rs`, Activity and DIAG, `Durability::Push`, 30 days): `session: Ref<Session>`, `then: OutboxThen` (`queued`, `in_queue`, `sent`, `unsure`, `box`, `stopped`, `gone`, `edited`, `removed`, `discarded`, `session_removed`, `restored_unsure`, `noticed`; `in_queue` added 2026-11-15, emitted when a row reaches Claude Code's queue, after a try or at daemon start), `chars: Count`, `via: ReplyVia`. No text. The golden and `docs/log-kinds.md` are regenerated.
- `session.reply_held` stays registered (old rows name it) and is no longer emitted.
- `op_kinds.rs` and `quiet_ops`: every new op is classified; none is quiet (they act, or `outbox.list` is asked on connect, not on a timer).
- A test sends a text holding a planted token and asserts no event, audit line, `reply-screen.txt` or inbox item contains it.

### 11.6 Cluster

*Builder's call B21.* `engine/src/cluster/policy.rs` forwards `session.reply` to a qualified session's owner today (`client_app("session.reply")`), and the owner runs it on its own Outbox: the mailbox stays on the machine that owns the session, as the source says. Chat keeps using `session.reply` for a row with a `node` (cluster b's `Agent::node`). *Built (Task 19):* `session.send` is `client_app` by `id`; `outbox.list`, `outbox.edit`, `outbox.remove`, `outbox.resend` and `outbox.discard` are `client_app_by(op, "session")`, since a row's own id names no node: the app adds the row's qualified `session` to edit, remove and resend, and the forwarding swaps it for the bare one. Chat now sends remote sessions with `session.send` too. A node tells its peers `outbox.changed` and `outbox.removed {id, session, why}` with no text, and a client node whose app follows the `outbox` topic asks the owner for its list and republishes the rows with `session` qualified (`docs/local-api-ops.md`, `outbox_ops.rs`).

## 12. Migration from held text

*Builder's call B18.*

- Held text was memory only, so there is nothing on disk to carry over. The daemon that is replaced at the update loses any held text it had, as every restart did before. The changes fragment says so: "Anything shown as Queued just before the update may need sending again."
- First start of the new daemon: `migrate` creates the table (schema 4); `OutboxOps::load` finds it empty.
- Code removed: `notchlingd/src/held_ops.rs` and its tests, `engine/src/reply/held.rs` and `held_tests.rs`, the `notchling-held` thread (`spawn_held_loop`/`stop_held_loop` in `Wired::start`/`close`). `held_guard.rs` becomes `turn_gate.rs`. The incident shapes in `held_race_tests.rs` and `held_kept_tests.rs` are ported to the safe-rule tests before the files go, so their cases keep holding.
- App: `ToolLive::held`, `held_note`, `ChatView::discard_held`, the held bubble, `NOTCHLING_RENDER_HELD`, `chat/box_holds.rs` and `composer.rs::box_holds_row` are replaced by Outbox bubbles (`NOTCHLING_RENDER_OUTBOX=<state>`).

## 13. The 7-day notice

*Builder's call B17.* `notchlingd/src/outbox_inbox.rs`. A sweep at start (after `InboxOps::load`) and every hour:

- For each session with rows whose `queued_ms` is 7 days old and `noticed_ms` is null (`in_queue` rows included): one Inbox item, id `outbox:<session>:<ms>` (fresh per raise), kind `outbox_unsent` (new `inbox::Kind`; the phone treats an unknown kind as a notice), title "Messages Not Sent", detail "3 messages to «session name» haven't gone out for 7 days." (the session's name from `sessions.list`, never message text: the `inbox` table is class Cluster), source `{session}`, actions **Send** (primary) and **Discard** (new `ActionId::Send`, `ActionId::Discard`; the phone shows any action by its label), no expiry. Those rows get `noticed_ms`.
- The responder only hands the answer to the Outbox's channel and returns true (responders run under the inbox lock and never block).
- **Send**: for each of those rows still there, its own action: `unsure` → Send Again, `box` → Clear Box and Send, `stopped` → Resume and Send, `waiting`/`retry` → Try Now; `queued`, `waiting` for a dialog, turn or queue, `in_queue` and `gone` stay as they are. **Discard**: removes those rows (not a `sending` one; an `in_queue` one too). Either resolves the item, `answered_by` `mac` or `phone`.
- When every noticed row of the session has gone (sent or removed elsewhere), the item is resolved Withdrawn.
- **Once**: a row with `noticed_ms` is never noticed again. After a restart the open item loads Withdrawn (the Inbox's rule) and is not raised again; Chat still shows the rows with their actions.

## 14. What the user sees

### 14.1 Chat

- Send returns at once; the reply box empties and the bubble shows Queued, then Sending, then the transcript's message (or Queued in the session until the model takes it). `ChatTools::sending` no longer spans a delivery.
- Outbox rows are drawn after the conversation's items, in order, as user bubbles with a status line and buttons, from `ConversationInput::outbox` (the rows of the selected session; part of the list's tail key, so a state change measures the row again). Words come from `locales/en/mac/chat.ftl`.

| Row | Status line | Buttons |
| --- | --- | --- |
| `queued`; `waiting` `queue` or `retry` | Queued (`retry`: "Queued · couldn't reach the terminal, trying again") | Edit, Remove |
| `waiting` `dialog` | Waiting: dialog open | Edit, Remove |
| `waiting` `turn` | Waits for the current turn | Edit, Remove |
| `sending` | Sending | none |
| `in_queue` | Queued in the session (`chat-outbox-in-queue = Queued in the session`) | none |
| `unsure` | May already be in the session | Send Again, Drop |
| `box` | Box has other text: «text», one line, elided | Clear Box and Send, Edit, Remove |
| `stopped` | Session stopped | Resume and Send, Edit, Remove |
| `gone` | Session not found | Remove |

- **Sent**: on `outbox.removed {why: "sent"}` the bubble stays as Sent until a load shows the message (`chat/outgoing.rs::Outgoing::arrived`), at most `GRACE_MS`, so it never blinks out.
- **Edit** puts the row's text in the reply box with an "Editing a queued message · Cancel" line; Return sends `outbox.edit`. If the row was taken meanwhile the edit fails, the line says "Already sent", and the text stays as a draft.
- Claude Code's own queue (`Conversation::queued`, text typed into the session but not read by the model) keeps its existing Queued bubbles and take-back. An `in_queue` row's text is also in that list (its `enqueue` line), so Chat draws the Outbox row and hides one matching entry of `Conversation::queued` (the same whitespace-squeezed text, the first match), and the message shows as one bubble. Entries the person typed keep their bubbles.
- A merged prompt's user row has the caption "Sent together with another message" ([10.7](#107-merged-prompts)).
- Chat sends Claude Code, Codex and Antigravity messages to existing sessions with `session.send`; Codex's and Antigravity's draft first messages keep `codex.send`/`agy.send`. A row with a cluster `node` keeps `session.reply`.
- On (re)connect the app's session source asks `outbox.list` and follows the `outbox` topic.

### 14.2 Notch

*Builder's call B19.* The Rust notch has no reply box, so nothing is sent from it. A session row whose head needs the person (`unsure`, `box`, `stopped`, `gone`) shows "Message needs you"; a click opens Chat on it. An `in_queue` head does not: the model will read it at its next step.

### 14.3 Phone

*Builder's call B19.* The phone sends as today and gets `{"sent": true}` as soon as the row is saved. It shows no Outbox state in v1; its message appears when the transcript has it (Claude Code's queue shows as `queued: true` messages as today). Stuck rows reach the phone as the 7-day Inbox notice. Showing Outbox rows on the phone is a later protocol addition.

## 15. Tests, rule by rule

Engine tests use temp databases, transcript fixtures with real shapes and synthetic values, and `reply::fake::FakePrompt`; daemon tests use `acting_ops_tests::setup`'s fake terminal and a fake courier; none attaches to a real session.

**Rule 1, saved first**
- `session.send` answers before the fake attach is entered (a blocking fake; the reply within 100 ms) and the row is in the table.
- Rows survive a restart: open, insert, drop, open again, list.
- A `sending` row at load: taken in the transcript fixture → deleted as Sent; only enqueued → `in_queue`; neither, or `None` → `unsure`/`restart`; never `queued`. An `in_queue` row at load stays `in_queue` and its postman watches it.
- Schema 4 on a version-3 file keeps every other table's rows; the class test passes with `outbox`.

**Rule 2, deliver like the terminal**
- A session whose fresh row is Working, `busy`, no tasks (a real turn) gets the message typed at once.
- A dialog on screen: nothing typed, `waiting`/`dialog`; the dialog gone: typed.
- `queue_state` over fixtures per Claude Code version (`engine/tests/fixtures/queue/<version>/`, at least 2.1.322 and 2.1.325): an `enqueue` with no `dequeue` waits; `dequeue`+user line, `queued_command`, `remove`, `popAll`, a later `turn_duration`, and a later typed prompt (lost queue) each free it; a `<task-notification>` entry never waits.
- An earlier message of ours in the queue holds the next; a message the person typed holds it too.
- No queue line within 5 s → safe mode → the next message waits for the turn end; a later line within 5 s → safe mode off.
- A tail with an unknown `queue-operation` (`QueueState::unknown > 0`) → that session's rows wait for the turn end; another session's don't.
- A `/model` row waits for the turn end and is confirmed by a `local_command` fixture line; `/clear` by a successor file.
- A menu command is refused at Send.
- The held incident shapes from `held_race_tests.rs` (a new turn after a held-looking row, the `shell` status, busy with tasks) hold for the safe rule.

**Rule 3, one postman per session**
- Two sessions, one with a fake attach that blocks: the other's message is typed meanwhile.
- One session, three rows: typed in order, through one attach (the fake attacher counts one attach), and the attach is closed 10 s after the last.
- Take against Edit and Remove on two threads, a thousand runs: a take always gets a consistent text and an edit never changes text already taken (an edit that lands first returns the row to `queued` and the take then takes the edited text; one that lands second is refused); a lost take types nothing (R22).
- An answer and a reply to the same session never interleave keys (the fake records writes per lock holder).

**Rule 4, never twice**
- Two "ok" rows: the second is Sent only by a second new taken line (counting from the try).
- `count_since` matches a long paste, a picture message (`[Image #N]` and image blocks), a `!` command (`enqueue` plain, user line `<bash-input>`); counts `enqueue` lines as `enqueued` and user lines, `queued_command` attachments and a command's `local_command` line as `taken`; a `dequeue` only through its user line; a merged user line (two `dequeue`s, one line of their texts joined with `"\n"`) as a take for each joined entry (R12); a `popAll` or `remove` line in neither.
- An `enqueue` line alone → `in_queue`, never Sent; a taken line later → Sent; the entry removed, popped (`popAll`) or lost to a Claude Code restart (the lost-queue rule, or the job's process changed) with no take within 30 s → `unsure`/`dropped`, decided at 30 s even with no file change; a vanished job's row only after 60 s missing plus the 30 s; still listed in an unchanged, live job → stays `in_queue` (R13, R21).
- A Codex or Antigravity row whose runner started the turn is Sent at once, with no transcript count (R14).
- A `gone` head is back to `queued` within 10 s of its session reappearing (R15).
- `NotCleared` → `box` with the box text on the topic and nowhere else; Clear Box and Send clears and types.
- `NotSent` after Return, nothing in the fixture after 30 s → `unsure`; a line at 20 s → Sent.
- `NotTaken`, `NoInput`, attach failure → `waiting`/`retry` with the back-off, never `unsure`.
- Send Again on an `unsure` row whose text is still in the box clears it and types it once.

**Retention and the notice**
- Sent, Remove, Drop, `outbox.discard`, `session.remove` and `codex.delete` delete; a vanished job goes `gone` after 60 s and is not deleted.
- Edit and Remove on an `in_queue` row fail "It's in the session's queue."; `outbox.discard` removes it.
- The 201st row is refused; text over 100 KB is refused.
- The notice: one item per session, no message text (planted token), Send and Discard act and resolve it, raised once, not again after a restart; it counts `in_queue` rows, Send leaves them as they are and Discard removes them.

**Codex and Antigravity**
- A busy Codex session waits as `turn`; idle → `send_if_idle` starts it → deleted; a race to busy → `waiting`/`turn`, never queued in `CodexRuns`.
- Antigravity likewise with its runner's `is_working` and `prompt`.

**Compatibility**
- `session.reply` returns `{"sent": true}` for a delivered row, the queued shape at once for a row that reaches `in_queue` (the row stays) and after 50 s for one still waiting, `failed` with `data {why: "not_cleared", box}` for a box (and the row removed).
- `session.held.discard` discards the session's rows.
- The bridge's `session.message` returns `{"sent": true}` at once with a row saved, `via: phone`.

**No text anywhere**: one planted-token test over events, the Audit, Activity rows, `reply-screen.txt` and inbox items.

**App** (pure models tested everywhere, renders on a Mac): the bubble model maps every state and `why` to words and buttons (`in_queue`: Queued in the session, none); an `in_queue` row hides one matching `Conversation::queued` entry, so the message is one bubble; `outbox.changed`/`removed` apply in order and a reconnect reseeds from `outbox.list`; Edit mode; `--render chat` with `NOTCHLING_RENDER_OUTBOX` for each state; `bench_chat` counts unchanged for a conversation with three Outbox rows.

**Live check** (maintainer only, plan's last task): a real background job with a long `until` shell, subagents running, a dialog open, leftover text in the box, a Codex session mid-turn, a daemon restart mid-send, a Claude Code restart while a message is Queued in the session.

## 16. Risks

- **A Claude Code restart while the daemon is down.** A row stays `in_queue` until the model takes it, and a Claude Code restart that drops its queue is caught by the lost-queue rule, the job's state or a change of the job's process ([10.6](#106-verifying)). The process the row was queued in is kept in memory only, so a Claude Code restart while the daemon was down that writes nothing to the transcript (no typed prompt after it, the job not stopped) leaves the row `in_queue` until the job's process is seen to change while the daemon runs, or the 7-day notice. (Resolved: question 1 is answered, and a row is no longer deleted at its `enqueue` line.)
- **A long turn after the queue line.** A turn that writes more than 1 MB after an `in_queue` row's `enqueue` line pushes it out of both reads: `queue_state` no longer lists it and `count_since` is `None`. The row then reads as dropped and comes back as May already be in after 30 s, the safe side.
- **A person typing the same words.** Identical text typed in the terminal during a verification counts as ours. Rare; the result is one missing copy, never a double.
- **A dialog that stays open** stops that session's mailbox, by design; the bubble says so and the Inbox notice comes at 7 days.
- **Format changes in Claude Code** (queue lines, the input box): an unknown queue line puts its session on the safe rule, and a queue line that stops appearing does too (the 5 s test); the input box rules are unchanged and fail closed (nothing typed).
- **An attach held open 10 s** keeps a `claude attach` process alive meanwhile; it types nothing outside a plan.
- **The per-session lock refactor** touches every acting op; a deadlock would hang a session's ops. Tests hold the order (one lock at a time, never across a socket call).
- **`notchling.db` contention** with the app's drafts and settings writes: single statements, 5 s busy timeout; a failed insert refuses the send (`failed` "Couldn't save the message.") so nothing is lost silently.
- **Respawn spends limits** only on the person's click.
- **Merges with the cluster lanes** on shared files (plan, merge notes).

## 17. Where the code goes

New:

- `crates/engine/src/outbox/` (`mod.rs`, `model.rs`, `store.rs`, `gate.rs`, `outcome.rs`, `verdict.rs`, and `*_tests.rs`)
- `crates/engine/src/transcript/queue_state.rs`, `transcript/message.rs` (a message's text, pictures, `!` command or slash command, moved from the app's `split_images`), `sent.rs::count_since` (+ tests), fixtures under `crates/engine/tests/fixtures/queue/<version>/` and `crates/engine/tests/fixtures/outbox/`
- `crates/engine/src/reply/plan.rs::reply_in`
- `crates/notchlingd/src/outbox_ops.rs`, `outbox_rows.rs`, `outbox_postman.rs`, `outbox_courier.rs`, `outbox_runners.rs`, `outbox_verify.rs`, `outbox_inbox.rs`, `typing_locks.rs`, `turn_gate.rs` (from `held_guard.rs`), and their `*_tests.rs`
- `crates/app/src/chat/outbox.rs` (pure bubble model), `crates/app/src/chat/view/outbox_rows.rs`
- `docs/decisions/XXXX-outbox.md`, `changes/outbox.md`

Changed: `engine/src/store.rs`, `store_class.rs`, `inbox/model.rs` (`Kind::OutboxUnsent`, `ActionId::Send`, `ActionId::Discard`), `transcript/conversation/` (merged flag), `sessions/agent.rs` (`outbox_key`), `tools/codex/runs.rs` (`send_if_idle`), `sessions/fresh.rs` (`job_state`, with the job's process id or start time when its files record one, for the `in_queue` watch); `crates/events/src/kinds/agents.rs`; `crates/local-api/src/wire.rs`; `notchlingd/src/acting_ops.rs`, `daemon_wiring.rs`, `lib.rs`, `op_kinds.rs`, `codex_ops.rs`, `agy_ops.rs`, `bridge_backend.rs`, `control_ops.rs`, `inbox_events.rs`, `acting_events.rs`; `app/src/actions.rs`, `chat/drops.rs`, `acting.rs`, `session_source.rs`, `tools/live.rs`, `chat/view/{tools,conversation,composer}.rs`, `app/src/inbox/` (words for the new actions), `notch/`; `locales/en/mac/chat.ftl`; `docs/local-api-ops.md`, `crates/AGENTS.md`, the crates' `AGENTS.md`, `docs/feature-map.md`.

Removed: `notchlingd/src/held_ops.rs`, `held_ops_tests.rs`, `held_race_tests.rs`, `held_kept_tests.rs` (cases ported first), `engine/src/reply/held.rs`, `held_tests.rs`, `app/src/chat/box_holds.rs`.

## For the maintainer

The builder check (Antigravity CLI, Gemini 4 Hydrogen, high effort, 2026-11-15) raised four questions. Three were settled here (*B11* scope, *B24*, *B25*). One changed a decision and was the maintainer's:

1. **Is a message Sent when Claude Code has queued it, or only when the model has read it?** The first draft counted a queue line naming the message as received (problem 10) and deleted a Sent row at once (retention). Claude Code's queue is in memory: if Claude Code restarts before the model reads the message, Claude Code drops it and Notchling would have no row left.

   **Answered 2026-11-15, on the safe side: a message is not Sent when it only shows in Claude Code's queue.** What the design now builds (the orchestrator's rulings R1 to R11 in `docs/superpowers/plans/2026-11-15-outbox.ledger.md`):
   - A new persisted state `in_queue`, bubble "Queued in the session", no buttons, not a "needs you" state ([9](#9-states-and-what-moves-them), [14](#14-what-the-user-sees)).
   - `count_since` returns two counts, `Seen { enqueued, taken }`, and the take stores both baselines (`seen_before`, new column `queued_before`) ([8](#8-data-model), [10.6](#106-verifying)). A `popAll` or `remove` line is never a take: the maintainer's brief listed "dequeue/popAll naming it" as taken, and the orchestrator ruled that `popAll` leaves the queue untaken, since it is what the Up key writes when it pulls the queue back into the box; that is the safe side. A `dequeue` counts through the user line after it.
   - After Return: taken → Sent; only enqueued → `in_queue` at once; neither by 30 s → May already be in ([10.6](#106-verifying)).
   - An `in_queue` head blocks the session's later rows and is watched: a taken line → Sent; Claude Code's queue no longer holding it (a `remove`, a `popAll`, the lost-queue rule, a later turn end with no take, the job stopped, gone or its process changed) with no take within 30 s → May already be in (`dropped`) ([10.6](#106-verifying)).
   - Edit and Remove refuse an `in_queue` row ("It's in the session's queue."); Discard removes it; the 7-day notice counts it and its Send leaves it ([11.2](#112-outbox), [13](#13-the-7-day-notice)).
   - At daemon start a `sending` row whose copy is only enqueued becomes `in_queue`, and an `in_queue` row keeps being watched ([10.6](#106-verifying)); the gap that leaves is in [16](#16-risks).
   - `session.reply` answers a row that reaches `in_queue` at once with the queued shape ([11.3](#113-sessionreply-kept)); Chat shows one bubble for it ([14.1](#141-chat)); Codex and Antigravity are unchanged ([10.8](#108-codex-and-antigravity)).
