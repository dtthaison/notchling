# XXXX: Delivery receipts: each tool's receipt confirms a message, the daemon alone decides delivery, and an unsure message never holds the queue for good

## Context

Messages to a Claude Code session are typed into its terminal by the Outbox's postman ([XXXX-outbox](XXXX-outbox.md)); there is no API for interactive sessions. After Return the daemon decides delivery by comparing the text it typed with the transcript (`transcript::sent::count_since`), and the app compares it again with its own matcher (`app/src/chat/outgoing.rs`, `chat/outbox.rs`).

Claude Code changes the text between the box and the transcript, and each change broke a comparison ([replies.md](../notes/replies.md), [the design](../superpowers/specs/2026-11-19-reply-receipts-design.md), §1):

- Picture paths become `[Image #N]` chips: a reply failed `Mixed` though the box held it (2026-11-15 13:25); a Sent bubble stayed because the app's matcher disagreed with the daemon's (branch `agent/fix-image-echo-stays`, 2026-11-19).
- Invisible characters are removed: a message with U+200B went `unsure`/`not_found` and stopped its session's mailbox (branch `agent/fix-invisible-chars-reply`, 2026-11-19).
- The transcript matcher counts `.heic`, `.tiff`, `.bmp` as pictures; Claude Code 2.1.328 reads only png, jpg, jpeg, gif and webp, so such a message is never matched.
- An unsure head blocks every later row of its session until the person acts (Outbox design, B3).

There are five text normalisers and two picture lists today. A hidden marker can't be added: Claude Code strips zero-width characters before anything sees the prompt.

Claude Code calls a `UserPromptSubmit` hook with the text it accepted, after its own changes (pastes expanded, invisible characters removed), with `session_id`, `transcript_path` and `prompt_id`; at submit, before it queues or runs the prompt (sometimes only at dispatch); not for built-in commands or bash mode; also for notifications, agent messages and scheduled prompts. A hook with `async: true` is not waited for. Notchling doesn't use the event today.

Codex and Antigravity count a message Sent when it is handed over: Codex when `codex exec resume` spawns (a turn that fails at once is lost from the Outbox), the Antigravity CLI when `session/prompt` is written, the Antigravity app's sidecar when the conversation's step count grows, which the agent's own steps also do while it works. Codex sees only Notchling's own runs as busy, so a thread open in the Codex app or TUI gets a second turn; an Antigravity conversation another `agy` holds is refused before the message is saved.

Several messages sent fast to a working Claude Code session go in one model step apart, each waiting until the one before is taken, and show a bare "Queued" with no reason.

## Decision

- **One fingerprint.** One normalisation in the engine (`transcript/stored.rs` in phase 1, `fingerprint.rs` with the hashes in phase 3): each tool's wrappers unwrapped, picture paths (one list of extensions, Claude Code's) and `[Image #N]` chips counted, unseen characters removed on both sides, whitespace squeezed; a strict and a loose hash. Every comparison that remains uses it, for every tool. Claude Code 2.1.328 keeps some of those characters in context (a joiner between emoji, a selector after one, a zero-width non-joiner in Persian or Indic text, the tag flags of England, Scotland and Wales, direction marks in right-to-left text), so the table is the union of what it may remove, and both sides lose all of it.
- **Pre-clean before typing.** `plan::reply` removes from the text it types only what Claude Code really drops (the same table, checked against 2.1.328) and so would have held the prompt for ("Removed N invisible characters · review and press Enter to send"): bidirectional overrides and isolates, the soft hyphen, the word joiner, the byte order mark, control characters, stray tag characters and a zero-width space outside Thai, Khmer, Lao and Myanmar text, so its Return never sits on a held prompt. A lone carriage return, vertical tab, form feed, line or paragraph separator becomes a space, as Claude Code does. It never removes what Claude Code keeps and what changes how text looks: a joiner between emoji, a variation selector, a zero-width non-joiner, the tag flags of England, Scotland and Wales, left-to-right and right-to-left marks. The saved row keeps the person's text. Known gap: a text that holds a direction mark and no right-to-left character is held for review by Claude Code, so it can still end `unsure`; the person gets Retry and Mark as Sent.
- **The daemon is the only judge, for every tool.** The app, the notch and the phone render daemon state. The daemon names the record that took a message (`take.id`; `take.uuid` for Claude Code), the queue line that holds it (`queuedAt`) and whether a box holds our own text (`boxIsOurs`); the app compares no message text.
- **Receipts, per tool.** A new contract capability, `turn.receipt`: Claude Code's async `UserPromptSubmit` hook entry (`notchling hook --receipt`, no matcher, installed by the app's Install Hook only, touching no other hook, printing nothing, sending a fingerprint and never the text over `hook.receipt`, ending within 1 s); Codex's `turn.started` and the rollout's `UserMessage` item; ACP's first `session/update` after `session/prompt` and the log's `USER_INPUT` step; the sidecar's `agentapi` answer and a `SYSTEM_MESSAGE` step with our fingerprint (Fallback). The postman asks the registry instead of `Courier::verifies()`.
- **A receipt is Sent.** The first fitting receipt after the hand-over settles the row as Sent; it stays on screen ("Accepted", "In Claude Code's queue") until its record appears. The daemon follows it until then, and a lost tool queue (a Claude Code restart) brings it back as unsure with Retry. Without a receipt, the same steps run with our own text's fingerprint over the record. This replaces the 2026-11-15 answer that a message only in Claude Code's queue is not Sent.
- **Unsure never holds the queue for good.** An unsure row holds the rows behind it for 2 minutes (`UNSURE_HOLD_MS`), with the countdown on it, while a late receipt or record can settle it (10 minutes), then steps aside with Retry and Mark as Sent. A second unsure in a row holds. Nothing is typed twice by itself.
- **Busy sessions: queue, wait, never fork.** A message is saved whenever its session exists and its channel can send, and waits with its reason (`queue`, `turn`, `dialog`, the new `held`). Notchling never starts a second process on a session another process holds: a Codex thread with a turn open that isn't ours, an Antigravity conversation another `agy` holds.
- **Several messages go together.** When the first waiting row's turn comes, the untouched rows right behind it go with it as one message, a blank line between (the join moves from save time to take time, for every tool). It stops at a command, a shell line, a row with other options, Clear Box and Send, a row put back by Retry, a row held by Edit and 100 KB. The head keeps its id and its row says how many messages it carries ("2 messages"); the joined rows leave as `joined`. Until then each row is its own bubble with Edit and Remove.
- **The send queue in Chat.** Every state has its own short words and an icon; the queue stays at the bottom above the reply box, collapsing behind a "N waiting" bar past 3; the notch shows a count. Edit holds the row (`outbox.hold`, a 10-minute lease) until Save. No Send Now and no drag reorder in this version.
- **A contract per Claude Code version.** Recorded receipts and transcript lines per version must give one fingerprint before a version is the Mac's checked one; production disagreements raise `claude.contract_drift`.
- All API changes are additive. `agentconsole.v1` does not change.

## Consequences

- Amends [XXXX-outbox](XXXX-outbox.md): its "Never twice" rule now counts a receipt as Sent, with the record still followed; its head rule (B3) gets the unsure exception; its save-time combine becomes a take-time join; Drop and Send Again become Mark as Sent and Retry.
- Amends [XXXX-antigravity-app-full-through-sidecar-and-hooks](XXXX-antigravity-app-full-through-sidecar-and-hooks.md): `delivered` needs a `SYSTEM_MESSAGE` step with our fingerprint, and the sidecar may report `accepted`.
- Deleted: the app's text matching (`outgoing.rs` matching, `shown_by_transcript`, the text compares in `hidden_queued` and `unbubbled`, the `holds_message` call), `sent_since`, the second picture list, `combine_except` and the `apart` set, `Courier::verifies()`, Sent at hand-over for Codex and Antigravity. `agent/fix-image-echo-stays`'s matcher never lands; `agent/fix-invisible-chars-reply` is folded into phase 1.
- Every submitted prompt in every Claude Code session on the Mac starts a short `notchling` process. Prompt text never reaches the daemon through it; drift is studied from recorded fixtures.
- Without the hook (not installed, `--safe-mode`, `disableAllHooks`, managed-only) Claude Code delivery works as today with one matcher and the bounded unsure.
- A process inside a session can forge a receipt for its own session only; it can't know Outbox text before delivery.
- A message that stepped aside and is retried can arrive after later ones: order gives way, visibly, instead of the queue stopping. A burst reaches the model as one message with blank lines, as from the terminal.
- A Codex thread open but idle in the Codex app or TUI still gets `codex exec resume`, with a note, until a probe shows whether the app re-reads the thread; a held Antigravity conversation waits as long as the other `agy` holds it.
- Structured input (`claude -p --input-format stream-json`) for sessions Notchling starts is not adopted: it would give up `claude attach`, agent view and the bg job lifecycle. Typing stays until Claude Code offers a supported way to send to a background session.
- Built in four phases: one fingerprint, the bounded unsure and the join; the app renders daemon state only; Claude Code receipts; Codex and Antigravity receipts and busy sessions. The design is [2026-11-19-reply-receipts-design.md](../superpowers/specs/2026-11-19-reply-receipts-design.md).

## Status

Accepted by the maintainer on 2026-11-19, with every recommended answer to the design's open questions. Phase 1 is built on `agent/reply-phase1`; phases 2 to 4 follow. The number is a placeholder, assigned at merge.

## Date

2026-11-19
