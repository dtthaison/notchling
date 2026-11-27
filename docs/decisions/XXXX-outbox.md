# XXXX: The Outbox, a saved mailbox per session

## Context

Pressing Send in Chat, the notch or the phone makes Notchling type the text into the session's terminal. On 2026-11-15 that failed four ways ([the design](../superpowers/specs/2026-11-15-outbox-design.md), §1):

- Held text sat as Queued for hours: Notchling guessed "a turn is running" from `status`, `state` and `inFlight.tasks`, which mean other things and lag (job be50d36a; `crates/AGENTS.md`, Things that bit before).
- Every reply failed `not_cleared` on leftover text in the session's box, with no way out (job 3dc1c4be).
- Held text lived in the daemon's memory only (`held_ops.rs`), so a restart lost it.
- `session.reply` returned only after the box let go of the text, about 4 s and up to 15 s, and Chat waited with it.

The terminal never has these problems: Claude Code's input box takes text at any time, and a message typed mid-turn waits in Claude Code's own queue until the model's next step. That queue is memory: a Claude Code restart drops it.

The maintainer approved the design on 2026-11-15 and answered its open question the same day: a message only in Claude Code's queue is not delivered.

## Decision

- Messages to a session are saved in a Node-class `outbox` table in `notchling.db` before anything is typed; Send returns the row id at once.
- One postman per session delivers in order; only it types a message into a session; acting ops lock per session, not globally.
- Deliver like the terminal: type mid-turn; wait only for a dialog or an earlier prompt unread in Claude Code's queue; slash commands, Codex and Antigravity wait for the turn end; the safe rule (turn end) when the queue lines can't be read.
- Never twice: Sent only when the transcript shows the model took a new copy since the try began (a user line or `queued_command`); a copy only in Claude Code's queue keeps the row as In Claude Code's queue; otherwise May already be in, decided by the person.
- The Outbox is a mailbox: a row is deleted when settled; unsent rows are never deleted silently and are raised once in the Inbox after 7 days.
- `session.send`, `outbox.*` and the app-tier `outbox` topic are additive; `session.reply` runs on the Outbox; held text and `session.held` go.

## Consequences

- It amends the held-text notes in `crates/AGENTS.md` (Things that bit before): the guess "is a turn running?" survives only in the safe rule.
- `held_ops.rs` and `reply/held.rs` are replaced; `reply_attempts` and `Clearing` (the leftover-box fix) are kept and used by every try.
- The maintainer chose (2026-11-15) not to count Claude Code's queue as delivered, so a dropped queue comes back as May already be in. The remaining gap: the Claude Code process a row was queued in is kept in memory only, so a Claude Code restart while the daemon is down that writes nothing to the transcript leaves the row In Claude Code's queue until the job's process is seen to change, or the 7-day notice (design §16).
- The cluster keeps each mailbox on the node that owns the session: `session.send` and the `outbox.*` ops are forwarded to the owner, and a client node follows the owner's rows (`session.reply` is forwarded too, for older apps).
- Only character counts reach the Audit and Activity; message text lives only in the Outbox row and on the app-tier topic.
- Built on branch `agent/outbox`. The plan is [2026-11-15-outbox](../superpowers/plans/2026-11-15-outbox.md).

## Status

Proposed. The maintainer approved the design and answered its question on 2026-11-15; the maintainer settles the record by merging the change that builds it. The number is a placeholder, assigned at merge.

## Date

2026-11-15
