# Reply receipts: the tool says what it got, the daemon alone decides

- **Date:** 2026-11-19
- **Status:** Approved by the maintainer on 2026-11-19 with every recommended answer to the open questions. Phase 1 is built on `agent/reply-phase1`; phases 2 to 4 are not built. Base: `origin/agent-run-oss` 88b868b9. Pre-clean (question 6) happens when the postman types the text, not when the row is saved, so the saved row keeps the person's words; Claude Code 2.1.328 keeps some joiners, selectors, tags and direction marks in context, so the fingerprint removes the whole table from both sides.
- **Amends:** the Outbox decision (`docs/decisions/XXXX-outbox.md`) and its design (`docs/superpowers/specs/2026-11-15-outbox-design.md`): the "Never twice" rule, the unsure head, and where matching lives. The Outbox table, the postman, the input-box rules and "never type over the person's text" stay as they are.
- **Supersedes two open branches:** `agent/fix-invisible-chars-reply` (folded into phase 1) and `agent/fix-image-echo-stays` (deleted by phase 2).

## In short

1. **The problem is guessing by text.** Notchling decides "was it delivered?" by comparing the text it typed with the screen, the transcript and the queue lines, in three places (the box reader, the daemon's `count_since`, the app's own matcher). Each Claude Code change to the text (picture chips, folded pastes, stripped characters) breaks one of them, and a failed guess leaves a row unsure and the session's mailbox stopped.
2. **One fingerprint.** One normalisation in the engine, used by every comparison that stays. It fixes the U+200B and `.heic` cases now, with or without the hook.
3. **The daemon is the only judge.** The app, the notch and the phone render daemon state. The daemon sends ids (the transcript line that took the message, the queue line that holds it) and never asks the app to match text. `outgoing.rs` and the matching in `chat/outbox.rs` go.
4. **A receipt from Claude Code.** Notchling installs its own `UserPromptSubmit` hook, async, so it never delays Claude Code. Claude Code calls it with the text it accepted, after its own changes. The hook sends the daemon a fingerprint (no text). The daemon ties it to the row it just pressed Return on, then follows that fingerprint in the transcript: Claude Code's text compared with Claude Code's text.
5. **Unsure never stops the queue for good.** An unsure row holds the rows behind it for 2 minutes, while a late receipt or transcript line can still settle it, then steps aside with **Retry** and **Mark as Sent**. A second unsure row in a row holds, since that means something systematic.
6. **A contract check per Claude Code version:** recorded receipts and transcript lines per version, one test that they give one fingerprint, and a drift event when production disagrees.
7. **The same for every tool.** States, the one judge, the app that only renders, the fingerprint and the unsure that steps aside hold for Codex and Antigravity too. Each tool has its own receipt (Codex: `turn.started` and the rollout's user message; ACP: the first update after `session/prompt`; the Antigravity app: the sidecar's post and its `SYSTEM_MESSAGE` step), declared as a new contract capability, `turn.receipt`. Today Codex and Antigravity count Sent the moment the message is handed over.
8. **Several messages sent fast** wait with a reason on each, stay editable, and go together as one message when their turn comes (question 11, option B), instead of one model step apart.
9. **The send queue is drawn like Chat** (section 16): every state with its exact words and buttons, at the bottom of Chat above the reply box, a count on the notch, the phone later.

## 1. The problem

### 1.1 What broke

Every incident in `docs/notes/replies.md`, plus the two open branches and one latent bug, sorted by cause. 55 commits touched the reply, verify and match files since 2026-10-01 (the brief counts 69 commits and 12 fixes over the whole area).

| When | What happened | Cause | Fixed by |
| --- | --- | --- | --- |
| 11-14 | Reply said ok, Claude Code never got it: Enter came before the picture paste landed | Typing blind | Input-box gating (`reply/exec.rs`), stays |
| 11-14 22:13 | `not_taken`: the attach's "Session is starting" screen read as an empty box | Screen reading | `await_prompt`, stays |
| 11-15 05:57 | Held reply typed into a new turn beside the person's text; the put-back typed it again | Turn guess, then a false failure | Outbox (no held text), stays |
| 11-15 | Held for good: `shell` status, then `busy` with subagents | Turn guess | Outbox, stays |
| 11-15 10:11 | Every reply `not_cleared` on a leftover, no way out | Leftover box | `reply_attempts`, Clear Box and Send, stays |
| 11-15 13:25 | Picture reply failed `Mixed` though the box held it (`[Image #48]` chip); the person pressed Return, nearly a double | **Text match**: box vs message | Chip rule in `input_box.rs` |
| 11-17 21:45 | "Already in the box": a look landed mid-frame | Screen reading | `settled` looks, stays |
| 11-19 (branch) | A picture message's Sent bubble stayed: the daemon said Sent, the app's own matcher didn't see the chip version | **Text match**: app vs transcript, a second matcher | The branch adds a third matcher (`Turns`, `Wanted`, `Pictured`) |
| 11-19 (branch) | U+200B in a message: Claude Code removed it, the transcript line no longer matched, the row went `unsure`/`not_found` and stopped the session's mailbox | **Text match**: daemon vs transcript, then the head blocks | Being built |
| latent | `transcript/message.rs` counts `.heic`, `.tiff`, `.bmp` as pictures; `input_box.rs` and Claude Code 2.1.328 read only png, jpg, jpeg, gif, webp. A dropped `.heic` stays as text in the transcript, so `count_since` never matches it | **Text match**: two picture lists | Nothing yet |

The typing and turn-guess classes are fixed and their rules stay. What keeps breaking is the last class: **matching our text against what Claude Code made of it**, and the head that then blocks.

### 1.2 Why it keeps breaking

- Claude Code changes the text between the box and the transcript, and the list grows: picture paths become `[Image #N]` chips (only 5 extensions), long pastes fold to `[Pasted text #N +M lines]` and come back expanded inside `<pasted_content id="…">` lines, invisible characters are removed (2.1.328 has a whole classifier: zero-width, bidi, tags, variation selectors, some kept between emoji), `!cmd` becomes `<bash-input>`, a skill becomes `<command-name>`.
- There are five normalisers today, each with its own idea: `sent::plain` (squeeze, cut chips, drop paste tags), `input_box::letters` (letters and digits), `outgoing::comparable` (trim and `!`), `outbox::squeeze`, and the branch's `Pictured::is_shown_as`. And two picture-extension lists.
- The app re-decides what the daemon already decided, so a fix in one place is a bug in the other.
- A failed match is not harmless: the row goes unsure, the head stops the session's mailbox (B3), and a person who presses Return or Send Again can send twice.
- There is no nonce: Notchling can't add hidden text to a message. Even a zero-width marker would not survive: Claude Code 2.1.328 strips U+200B, U+200E, U+200F and U+2800 before anything else sees the prompt.

## 2. What the `UserPromptSubmit` hook gives

Read from the Claude Code docs (hooks reference, agent view) and the 2.1.328 binary (`strings` on `~/.local/share/claude/versions/2.1.328`), and checked against about 400 transcripts on this Mac that recorded a project's own `UserPromptSubmit` hook (versions 2.1.312 to 2.1.325; structure and timestamps only, no text read).

### 2.1 What it receives

| Field | Notes |
| --- | --- |
| `prompt` | The submitted text. A folded paste arrives expanded; in some sessions between a `<pasted_content id="…">` line and a `</pasted_content id="…">` line (docs). Invisible characters are already removed (the stripper runs in the input pipeline, before submit). A skill or custom command arrives as typed (`/deploy staging`), not expanded. Pictures: expected as `[Image #N]` chip text, as in the transcript's user line; **not confirmed** (phase 3 records it). |
| `session_id`, `transcript_path`, `cwd`, `permission_mode` | Common fields. `transcript_path` is empty in some remote modes. |
| `prompt_id` | UUID of the prompt being processed. Transcript user lines carry `promptId`; `queued_command` attachments don't. That the hook's id equals the user line's for the same prompt is **not confirmed**. |
| `agent_id`, `agent_type` | Only inside a subagent or with `--agent`. A receipt with `agent_id` is never ours. |
| `session_title` | When the session has a custom title. |
| `source` | In the schema (`user`, `sdk`, `system`, `loop_wakeup`, `schedule_wakeup`, `poll_event`), "may be omitted while the field rolls out"; 2.1.328 doesn't send it (the spread is compiled to `...!1`). Used when it appears. |

### 2.2 When it fires, and when it doesn't

- **At submit, before Claude Code queues or runs the prompt.** For a message typed mid-turn the transcripts show it: the `queued_command` attachment is stamped at Enter (T), the hook's output at T+0.1 to 1.0 s, the `enqueue` line right after the hook ended (T+0.1 to 1.0 s), and the attachment with the hook output is written to the file only when the model takes the message, seconds or minutes later. So **a receipt means "Claude Code accepted it", not "the model read it"**.
- **Sometimes at dispatch instead.** One recorded queued message (2.1.319) got its hook output only when the model took it, 26 s after Enter; 2.1.328 has a "screened at dispatch" path (`wasUnscreenedAtDispatch`, and a re-screen when hooks or the permission mode changed). So a receipt can come at Enter or at the take; its absence in the first seconds proves nothing for a busy session.
- **It also fires for prompts that are not the person's:** background subagent reports, task notifications, messages from another session, scheduled tasks and `/loop` wake-ups (docs; about 1,700 such attachments were followed by hook output in the sample). Receipts must be filtered by matching, never trusted by session alone.
- **It doesn't fire** for built-in local commands (`/clear`, `/model`, `/compact`) or bash mode (`!cmd`): "the prompt did not go through prompt.submit (a command, bash mode, …)" in the binary; 127 of 128 `<command-name>` lines and all 35 `<bash-input>` lines in the sample had no hook output. Those keep their transcript proof (local command line, successor file for `/clear`, `<bash-input>` line).
- **Background jobs** (`claude --bg`, a `bg-spare` woken by `claude attach`): a job reads the same settings as `claude` in its folder, and Notchling's `PermissionRequest` and Artifact hooks already run there. Not in `--safe-mode` jobs, under `disableAllHooks`, or when managed policy allows managed hooks only.
- **`claude -p`**: fires, with `source: sdk` once rolled out. Notchling never types into those; `notchling hook` already skips headless sessions.

### 2.3 Timing against the transcript

- Claude Code starts all hooks of the event together. A synchronous hook holds the prompt (default 30 s for `UserPromptSubmit`; "a stuck hook stalls the session"); the user line or `enqueue` line is written after the synchronous hooks end. The transcript writer is asynchronous and "may lag the in-memory conversation" (docs).
- An **async** hook (`"async": true`) is started and not waited for; its timeout is not enforced; its JSON output would be given to the model on the next turn, so ours must print nothing.
- So the receipt usually reaches the daemon **before** the transcript line (tens of ms after Enter, against the transcript's write after any synchronous hooks: the sample's project hook took 0.1 to 1.0 s, and a slow hook can take up to 30 s). Not guaranteed: the design takes both orders.
- Side finding: a person's slow synchronous `UserPromptSubmit` hook delays the `enqueue` line, which can trip today's 5 s "no queue line" test (`QUEUE_LINE_MS`) and put a session in safe mode for no reason. A receipt explains the gap.

### 2.4 Limits

- **Accepted is not read.** Claude Code's queue is memory and a restart drops it; the person's own synchronous hook can block the prompt after our async hook already fired (the transcript then has the block message). The 2026-11-15 rule (a message only in Claude Code's queue is not Sent) still needs the transcript.
- **No tie but the event.** The receipt carries no id of ours. The tie is: the first fitting receipt for that session after our own Return, on a box we checked held only our message.
- **Forgeable inside the session.** A process in the session can run `notchling hook` with a made-up event; the daemon's peer check (the caller descends from that session's Claude Code process) limits it to its own session. It could make one of that session's rows look accepted, but it can't see Outbox text before delivery (the topic is app tier). Low risk; noted.
- **Not installed everywhere.** Existing users must click Install Hook again; some sessions run without hooks. The fallback is the same fingerprint over our own text.

## 3. Today's flow

```
Chat / notch / phone
  │ session.send {text}
  ▼
daemon: Outbox row "queued" ── postman gate (dialog? our earlier row in Claude Code's queue? turn end for /cmds?)
  │ take → "sending"; baseline = count_since(transcript, OUR text)        ◄── match 1: our text vs transcript
  ▼
claude attach → session's own prompt? → box empty? → paste → box holds our text? → Enter → box let go?
  │                                                                       ◄── match 2: our text vs screen
  ▼
Claude Code: strips invisible chars, folds pastes, turns picture paths into [Image #N], wraps !cmd and /skills
  │ (after any synchronous UserPromptSubmit hooks) writes: user line | enqueue | later queued_command
  ▼
daemon verify: count_since(transcript, OUR text) for 10 s, once more at 30 s   ◄── match 1 again
  ├─ taken    → row deleted (Sent)
  ├─ enqueued → "in_queue" → watch until taken, or dropped → unsure
  └─ neither  → "unsure" / not_found ── the head stops the session's mailbox until a person acts
               (look again every 5 s for 10 min)

app: outbox topic → bubbles, PLUS its own matcher                       ◄── match 3: our text vs parsed transcript
     Outgoing::arrived, dated_since, shown_by_transcript, hidden_queued,
     input_box::holds_message (box rows); the image-echo branch adds Turns, Wanted, Pictured
```

## 4. Proposed flow

```
Chat / notch / phone ── session.send {text} ──►  daemon (the only judge)
        ▲                                          │ types it; input-box rules unchanged
        │ outbox topic: rows, states, ids          ▼
        │ (take line uuid, queuedAt, boxIsOurs)   Claude Code (bg job through claude attach)
        │                                          │ Enter
        │                ┌─────────────────────────┤
        │                ▼                         ▼
        │   UserPromptSubmit, async          transcript: user line | enqueue | queued_command
        │   notchling hook --receipt               │
        │   fingerprint only, ≤ 1 s, no output     │
        │                │ hook.receipt            │ file watch
        │                ▼                         ▼
        └──────────── daemon ◄─────────────────────┘
                      1. receipt after our Return, fingerprint fits the row  → accepted; keep ITS fingerprint
                      2. transcript take line with that fingerprint          → Sent (+ the line's uuid)
                         enqueue line with that fingerprint                  → in_queue (+ the line's time)
                      3. no receipt (hook missing, unhealthy, late)          → same steps with OUR text's fingerprint
                      4. nothing by 30 s                                     → unsure; holds 2 min; steps aside
                      5. a late receipt or line within 10 min                → settles it
```

The typing part (attach, prompt, box checks, paste, Enter, box let go) does not change. What changes is what happens after Return, and who decides.

## 5. States

Persisted `state` values stay the same (old apps parse them). New columns: `entered_ms` (Return went out), `receipt` (JSON: fingerprint, `at_ms`, `prompt_id`), `aside_ms`. New `why` values: `accepted` (in_queue on a receipt alone), `no_receipt`, `blocked`. An older app maps an unknown `why` to the plain bubble.

| State | Means | Moves to | When |
| --- | --- | --- | --- |
| `queued` | In the mailbox | `waiting`, `sending`, `box`, `stopped`, `gone` | The gate (unchanged) |
| `waiting` | `dialog`, `turn`, `queue`, `retry` | `queued` / `sending` | Unchanged |
| `sending` (no `entered_ms`) | Keys going out | `box`, `waiting`/`retry`, `sending` with `entered_ms` | Unchanged (B6: nothing submitted before Return) |
| `sending` + `entered_ms` | Return pressed, waiting for proof | **Sent** (row deleted) | A take line with the receipt's fingerprint, or our text's when there is no receipt |
| | | `in_queue` | An `enqueue` line with that fingerprint; or a receipt and no line at all by 30 s (`why: accepted`) |
| | | `waiting`/`retry` | Healthy hook, no receipt by 3 s, and the box still carries our text: not submitted; cleared as our leftover and typed again |
| | | `unsure` | 30 s with no receipt and no line (`why: no_receipt` when the hook is healthy for this session, else `not_found`); a receipt then the session's block message (`blocked`) |
| `in_queue` | Claude Code holds it; the model hasn't taken it | **Sent** / `unsure` (`dropped`) | Unchanged: take line; queue lost (remove, popAll, lost-queue rule, stopped, gone, process changed) and 30 s grace |
| `unsure` | Return pressed, no proof | **Sent** | A fitting receipt or take line within 10 min |
| | | stays `unsure`, `aside_ms` set | 2 min (`UNSURE_HOLD_MS`) with no proof: rows behind it go on; unless the session's previous settled row was also unsure (then it holds) |
| | | `queued` | **Retry** (`outbox.resend`), our leftover cleared first (`reply_attempts`) |
| | | deleted | **Mark as Sent** (`outbox.remove {sent: true}`) |
| `box`, `stopped`, `gone` | Unchanged | Unchanged | Unchanged |

**Head rule (B3) amended:** the head blocks the rows behind it, except an `unsure` row with `aside_ms`. Retry on an aside row puts it back in line by its own `n`, so it is the head again and goes next.

**If question 1 is Yes (the recommendation): a receipt is Sent.** The table above changes in two places. A fitting receipt settles the row as Sent (`then: accepted`), so it never goes `unsure` and needs no buttons. It stays on screen with its place until the take line: "Accepted" while Claude Code is idle, "In Claude Code's queue" while an `enqueue` line holds it (section 16). The gate is unchanged: the next row still waits until Claude Code's queue has none of ours unread. **Safety net:** the daemon keeps following the row until its take line. If Claude Code's queue is lost first (restart, `popAll`, the lost-queue rule), the row comes back as `unsure`/`dropped` with Retry, instead of being lost the way a typed message is lost in the terminal. `in_queue` without a receipt (hook missing) keeps today's rule.

## 6. Receipt matching rules

A receipt is `{session_id, transcript_path, prompt_id?, agent_id?, source?, fp, fired_ms}` with `fp = {v, strict, loose, chips, chars, kind}` (section 7). The daemon stamps `at_ms` when it reads it.

1. **Not ours, dropped at once:** `agent_id` present; `source` present and not `user`; `kind` other than `prompt` or `skill`; the session has no Outbox row that is `sending` with `entered_ms`, `in_queue`, or `unsure`.
2. **Session:** `session_id` maps to the row's key (the job's short id, B24) through the job files (`sessionId`, `resumeSessionId`) or `sessions/<pid>.json`. The daemon's peer check is the one `hook.request` uses.
3. **After our Return:** `at_ms >= entered_ms`. A receipt before Return is never ours (it is the person's own prompt or a notification).
4. **One for one:** a receipt settles at most one row and a row takes at most one receipt. Only one row per session is ever `sending` (one postman), so two identical rows ("ok", "ok") never compete for the same receipt.
5. **Text as a check, not as the key:** the receipt fits the row when, for some subset of the row's picture paths taken out, `strict` is equal, or `loose` is equal, and `chips` is at most the number taken out. Strict is the normal case; loose only (letters and digits) also fits, and logs a `claude.contract_drift` with the version, since Claude Code then changed the text in a way the fingerprint doesn't know yet. Neither: not ours.
6. **Earliest wins:** if several receipts fit, the first after Return.
7. **Then follow Claude Code's text:** from now on the row is followed in the transcript by the **receipt's** fingerprint, counting from the receipt's `at_ms − 2 s` (the B5 baselines, unchanged): a typed user line, a `queued_command` attachment or a merged line holding it is the take; an `enqueue` line is Claude Code's queue.
8. **Late receipts:** a receipt that fits an `unsure` row within 10 min (`UNSURE_LOOK_FOR_MS`) settles it the same way. A receipt for a row already settled does nothing.
9. **Health:** a session's hook is healthy when the daemon got any receipt from that session's current Claude Code process (ours or not). Only a healthy hook makes "no receipt" mean something (`no_receipt`, the not-submitted retry).

The person typing the same text:

- In the same box: can't mix; the box rules refuse, or clear only our own text, before Return, and Return goes only on a box holding only ours.
- Their own submit just before our Return: before `entered_ms`, ignored (rule 3).
- Their own identical text just after ours: two receipts, the first is ours (rule 6); the transcript part counts from the receipt, as B5 does today. Worst case one of two identical texts is credited to the other sender: the session still got the text once from each, never a double from Notchling.

## 7. One normalisation

One module in the engine, `transcript/fingerprint.rs`, absorbing `transcript/message.rs`, `sent::plain` and `names`, the letters rule of `input_box.rs`, and the picture rule. Used by `count_since`, `queue_state`, the input box, the daemon's receipt match, and `notchling hook --receipt`. Same binary, so the hook and the daemon agree; `v` changes when the rules do, and a receipt with another `v` is treated as missing.

`Fingerprint::of(text)` for a row, a receipt, a transcript line (string or text blocks), a queue entry:

1. **Unwrap Claude Code's wrappers:** `<bash-input>x</bash-input>` → kind `bash`, `x`; `<command-name>/n</command-name>` with `<command-args>a</command-args>` → kind `command`, `/n a`; drop `<pasted_content …>` and `</pasted_content …>` marker lines.
2. **Pictures:** in a row, a picture path is a token on a line of nothing but path tokens (Chat's `drops::with_attachments` form, the phone's `"…"` form), with a directory, ending in one of **one** list of extensions: png, jpg, jpeg, gif, webp (Claude Code 2.1.328's list; `.heic`, `.tiff`, `.bmp` are text). Paths are taken out and counted as `pictures`, each subset a candidate. On Claude Code's side, `[Image #N]` chips are cut out and counted as `chips`; image blocks as `images`.
3. **Unseen characters out,** on both sides: `cluster::text::unseen` (controls but `\n` and `\t`, U+00AD, U+034F, U+061C, U+180E, U+200B to U+200F, U+2028, U+2029, U+202A to U+202E, U+2060 to U+206F, U+3164, U+FE00 to U+FE0F, U+FEFF, tags, variation selectors) plus U+2800. Taking them out of both sides makes it indifferent to which ones Claude Code kept.
4. **Whitespace squeezed** to single spaces and trimmed; CRLF read as LF.
5. `strict = sha256(v ‖ kind ‖ text)`, `loose = sha256(v ‖ kind ‖ letters and digits of text)`, `chars` the length.

NFC normalisation is left out (no crate for it in the tree today; `unicode-normalization` is MIT/Apache if a recorded case needs it).

**Pre-clean before typing.** The Outbox removes the always-stripped characters (step 3) from a row's text when it is saved. Claude Code 2.1.328 holds a prompt in the box ("Removed N invisible characters · review and press Enter to send") when it stripped bidi controls, or when the text starts with `/` or `!`: our Return then does nothing, and today that ends `NotSent`, then unsure. Visible text is unchanged; nobody types those characters on purpose.

## 8. Unsure never blocks forever

**Today:** an unsure head stops its session's mailbox until a person presses Send Again or Drop (B3). One unmatched message stops every later one, from Chat and from the phone, which has no buttons for it.

**Proposed:**

- **Resolve it if the facts come.** A late receipt (rule 8) or a late take line (today's look-again, every 5 s for 10 min) deletes it as Sent.
- **Time out the hold, not the row.** After `UNSURE_HOLD_MS` (2 min) it steps aside: rows behind it go on; it stays visible with its buttons and the notch's "needs you". Nothing is typed again by itself.
- **A second unsure in a row holds.** If the session's last settled row was also unsure, the new one doesn't step aside: two in a row means something systematic (a Claude Code change, a broken terminal), and sending more into it is worse than waiting.
- **The person decides, with better words** (the `why` comes from the daemon; the app only maps it):
  - `no_receipt` (hook healthy, nothing came): "Claude Code didn't confirm it." **Retry** is likely safe.
  - `not_found` (no receipt evidence): "It may already be in the session." Check the conversation first.
  - `dropped`: "Claude Code's queue lost it (it restarted?)."
  - `blocked`: "A hook in the session blocked it." Retry would be blocked again.
  - `restart`: "Notchling restarted while sending it."
- **Buttons:** **Retry** (today's Send Again: clears our own leftover from the box first, `reply_attempts`) and **Mark as Sent** (today's Drop, logged as the person's confirmation).

**The trade-off with double-sending.** Three ways out of not knowing: retype by ourselves (a double when the first one went in), drop by ourselves (a loss when it didn't), or ask the person (a stuck queue). The Outbox chose "ask" and paid with a stuck queue. This keeps "ask" and removes the stuck queue: stepping aside types nothing twice and loses nothing; its cost is order, since a later message can arrive before an earlier unsure one if the person retries it. That cost is visible (the aside row sits above the later messages with its buttons). Receipts shrink the unsure set to the cases where Claude Code really didn't say it got the message, and they let Retry say honestly how risky it is.

## 9. What the protocol needs

All additive.

**Local API (app and daemon, `wire.rs`, `docs/local-api-ops.md`):**

| Change | Why |
| --- | --- |
| New op `hook.receipt` (agent tier with the peer check of `hook.request`, not behind `acting`, never logged on success) | The hook's receipt: `{event, sessionId, transcriptPath, promptId?, agentId?, source?, fp, firedMs}`. Answers `{}` at once. No text field |
| `outbox.remove {id, sent?: true}` | Mark as Sent; event `then: marked_sent` |
| Row fields `enteredMs?`, `accepted? {atMs, promptId?}`, `queuedAt?`, `boxIsOurs?`, `asideMs?` | The app draws them; it never compares text |
| `outbox.removed {why: "sent", take? {uuid, atMs}}` | The transcript line that took it; the app keeps the Sent bubble until its conversation has that line |
| New `why` words `accepted`, `no_receipt`, `blocked` | Bubble words from the daemon's reason |
| Event `session.outbox` gains `then` values `accepted`, `aside`, `marked_sent`; new kind `claude.contract_drift {version, rule}` | Counts and names only, no text (`LINT-LOG-NO-TEXT`); golden and `docs/log-kinds.md` regenerated |
| `why` words `held` (section 14.4) and, behind an unsure row, `unsure_ahead`; `outbox.removed {why: "joined", into}` (option B) | The waiting reasons and the join, drawn by the app |
| New op `outbox.hold {id, on}` (app tier, 10-minute lease) | Edit holds the row (section 16.2) |
| `take {id, atMs}` for every tool (`uuid` kept for Claude Code) | The record's item id: a Codex `UserMessage` item, an Antigravity step |
| `agy.sidecar.report` gains `result: "accepted"` | `agentapi` answered, no `SYSTEM_MESSAGE` step yet |

**Engine model (not wire):** `ItemKind::User` keeps the transcript line's `uuid` (from a user line or a `queued_command` attachment); `Conversation::queued` entries keep their `enqueue` line's timestamp. The app matches by those, exactly.

**`agentconsole.v1`: no change in phases 1 to 3.** The phone keeps `session.message` → `{"sent": true}` and shows no Outbox states (B19); it gains from the bounded unsure (its later messages no longer wait behind an unsure head). Showing Outbox states on the phone later is an optional field on `session.message`'s answer (`outboxId?`) and a new optional `outbox` event, by the `add-protocol-field` skill.

**Older peers:** an older app ignores the new row fields and keeps its own matcher; it works as today. A newer app with an older daemon (a cluster owner node not updated yet) gets no `take`: the Sent bubble leaves on a timer (`GRACE_MS`), no text compare. Cluster forwarding carries rows without text as today; the new fields pass through.

## 10. The hook entry, and never hanging

- **The entry.** A new installer category, Receipt, in `engine/src/install/hooks.rs`: event `UserPromptSubmit`, **no matcher** (the event ignores matchers; `Slot.matcher` becomes optional), `{"type": "command", "command": "'<notchling>' hook --receipt", "async": true, "timeout": 10}`. The `keep` check compares `async` too. Only Notchling's own entries are touched (`classify` on the command, as today); the person's own `UserPromptSubmit` hooks stay; a backup is kept. Installed only by the app's Install Hook (Welcome, Settings) through `install.apply` behind the `install` switch; never by an agent; tests only on temp copies.
- **`notchling hook --receipt`** prints nothing in every path (an async hook's stdout would reach the model next turn) and exits 0. It reads stdin (cap as today), returns at once for `claude -p` sessions (`headless`), an `agent_id`, or a `source` other than `user`, computes the fingerprint, connects with a 250 ms timeout, writes one line, waits at most 250 ms for the answer, and exits. Under 1 s whatever the daemon does, a few ms of CPU. No daemon, no socket, a panic: exit 0, nothing printed.
- **Async means Claude Code never waits for it,** so even a hung daemon costs the person nothing. If a future Claude Code ignored `async`, the 1 s self-limit and the 10 s timeout still bound it.
- **Cost:** one short `notchling` process per submitted prompt in every Claude Code session on the Mac (task notifications included). It touches no file but stdin and the socket.

## 11. Failure modes

| Case | What happens |
| --- | --- |
| Hook not installed | The fingerprint fallback (phase 1): our text's fingerprint against the transcript; unsure after 30 s; the hold and aside rules. Today's behaviour minus the matching bugs |
| Hook installed, not running in this session (`--safe-mode`, `disableAllHooks`, managed-only, a job started before install that didn't reload) | No receipt from that process, so not healthy: the fallback, and `no_receipt` is never claimed |
| Hook late (after the transcript line) | The transcript settles first by our text's fingerprint; the receipt then finds the row settled and does nothing |
| Hook late (after 30 s, row unsure) | Settles it within 10 min |
| Receipt lost (daemon restarting, socket busy) | The fallback for that row; a lost receipt never makes a row worse than today |
| Receipt only at the take (screened at dispatch) | The `enqueue` line by our fingerprint gives `in_queue`; the receipt then marks the take, followed by the take line |
| Two identical messages | One row `sending` per session; the receipt goes to the row whose Return came first; the transcript part counts from the receipt, as B5 |
| The person types the same text | Section 6. Never a double from Notchling |
| The person's own `UserPromptSubmit` hook blocks it | Receipt, then the session's block message in the transcript: `unsure`/`blocked` |
| Claude Code restarts with ours in its queue | Unchanged: `dropped`, unsure, and now it steps aside after 2 min |
| Daemon restarts mid-send | Unchanged: a `sending` row is counted once at load; receipts were memory only, so the fallback decides |
| A slash command | Built-ins: no receipt, today's proof (local command line, `/clear` successor). Skills: a receipt with the line as typed |
| `!cmd` | No receipt; the `<bash-input>` line by the fingerprint's `bash` kind |
| A picture path Claude Code can't read | It stays as text; the subset candidates cover it |
| Claude Code changes the text again | Strict misses, loose fits: still settled, `claude.contract_drift` logged with the version. Both miss: the fallback, then unsure; the version's contract fixtures fail before the Mac's checked version moves |
| Phone | Sends through `session.send` as today; no states shown; no longer stuck behind an unsure head after 2 min |
| Older Macs and cluster nodes | Section 9: old apps keep their matcher; new apps never compare text |
| A process in the session forges a receipt | Limited to its own session by the peer check; it can't know Outbox text before delivery |

## 12. Structured input for sessions Notchling starts (later, optional)

How Notchling starts sessions today: `claude --bg` (`control/session.rs::plan_new_session`), so Claude Code's supervisor owns the process and its terminal; Notchling reaches it by `claude attach <job>` and typing; `claude respawn` restarts it; the person can attach from any terminal or agent view.

`claude -p --input-format stream-json --output-format stream-json` would let Notchling write each message as a JSON user message (pictures as blocks, no box, no chips, no stripping surprises) and read Claude Code's echo. It is possible, but it is a different kind of session:

- Notchling owns the process (as the Codex runner does): it dies with the daemon or must be adopted after a restart; it is not a `jobs/` job, so it is not in agent view, `claude attach`, `claude respawn` or Notchling's job list.
- The person can't type into it from a terminal; Chat is its only door.
- It runs as `sdk-cli`: `notchling hook` skips it as headless, so permission prompts need `--permission-prompt-tool` or a hook change.
- Interactive-only features (menus, most built-in slash commands) don't work there.

**Recommendation:** don't build it now. Typing into bg jobs plus receipts covers delivery without giving up `claude attach`. The real fix would be Claude Code's own "send to a background session": agent view's peek panel already sends a reply to a bg session without attaching (docs, agent view), through the supervisor. If Anthropic documents that, it replaces typing for bg jobs entirely. Question 9.

## 13. A contract check when Claude Code updates

- **Fixtures per version:** `crates/engine/tests/fixtures/claude-contract/<version>/`, synthetic text, made by a maintainer-run recorder over a dev profile (`docs/dev-profile.md`) with a throwaway session. For each case (plain, multi-line, folded paste, two pictures, one unreadable picture path, U+200B, a bidi control, `!cmd`, a skill, a built-in command, a message typed mid-turn, two identical messages, a merge): the hook's stdin (`receipt-<case>.json`), the transcript lines (`transcript-<case>.jsonl`), the row text. A blocking recording hook captures receipts without spending a model turn; the transcript cases use a cheap model, as the existing terminal recordings do.
- **One test:** for every case, the row's fingerprint fits the receipt's, and the receipt's equals the transcript take or `enqueue` line's. A new version's folder must pass before it becomes the Mac's checked version (the tool-versions process).
- **In production:** the daemon counts, per Claude Code version, receipts that fit by strict, by loose only, or whose take line it never found by the receipt's fingerprint. Anything but strict raises `claude.contract_drift {version, rule}` once per version and a quiet note in Settings ("Claude Code 2.1.330 records prompts in a new way; Notchling checks replies the slow way"). The versions soak can read the same counts.

## 14. Codex and Antigravity

Read from `docs/provider-matrix.md` (`turn.send`: Codex is `codex exec resume`, the Antigravity CLI is ACP `session/prompt`), `docs/notes/codex-antigravity.md`, the Antigravity sidecar decision (`XXXX-antigravity-app-full-through-sidecar-and-hooks.md`), `notchlingd/src/{outbox_runners.rs, codex_slow.rs, agy_ops.rs, agy_sidecar_ops.rs}` and `engine/src/tools/{codex, agy, acp, agy_sidecar}`; plus the line types (no text) of a Codex 0.166.0 rollout and the open files of a running Codex on this Mac.

### 14.1 What is the same for every tool

The redesign is about the Outbox, not about Claude Code. Each part holds for every tool:

- **States.** The same rows, states and words. `sending` with `entered_ms` becomes "handed over": the moment after which the tool may have the message. Claude Code: Return pressed. Codex: `codex exec resume` started with the prompt. ACP: `session/prompt` written to `agy acp`. The Antigravity app: the sidecar took the job. Before that moment nothing can be in the session, and a failure is a plain retry (B6, unchanged).
- **The daemon alone decides.** Each courier hands the message over and reports what the tool said. One judge in the daemon (the postman) turns that into states for every tool. The app renders rows, `why` and `take` for every tool, and compares no text for Codex and Antigravity either (today the same matcher runs for every tool).
- **Receipt, then record.** Every tool gives two kinds of proof. A **receipt** is the tool saying it accepted this message. A **record** is the message in the tool's own conversation file. With question 1 answered Yes, the receipt is Sent. The record names the take: `take.id`, the conversation item's id in the engine's model for that tool (it generalises `take.uuid`).
- **One fingerprint.** The same `Fingerprint::of`, with one unwrap step per tool: Codex's `UserMessage` item (its `text` parts); Antigravity's `USER_INPUT` step; the app's `SYSTEM_MESSAGE` step, from `content=` inside `[Message] …` (the part `elsewhere::system_message` already cuts out).
- **Unsure steps aside** after `UNSURE_HOLD_MS`, with the second-in-a-row breaker, Retry and Mark as Sent, the same for every tool.

### 14.2 What each tool's receipt is

| Tool (channel) | Handed over | Receipt (accepted) | Record (in its file) | Tie to our row | Today |
| --- | --- | --- | --- | --- | --- |
| Claude Code (typed into a bg job) | Return on a box holding only ours | `UserPromptSubmit` hook, async, fingerprint only | Transcript user line, `queued_command`, `enqueue` | The first fitting receipt after our Return; the text is a check | Text guess, in three places |
| Codex (`codex exec resume <id> <prompt> --json`) | The process started | `turn.started` on that process's stdout | The rollout: `task_started` (`turn_id`), then `event_msg` `item_completed` with a `UserMessage` item (0.166.0) | The process: one prompt per process | Sent at spawn (`outbox_runners.rs::STARTED`, `verifies() == false`) |
| Antigravity CLI (ACP `session/prompt`) | The request written to `agy acp` | The first `session/update` after it; the request's answer (`stopReason`) at the turn's end | The `USER_INPUT` step in `brain/<id>/.system_generated/logs/transcript.jsonl` | The JSON-RPC request id; one turn per connection (`is_working`) | Sent when the pipe write succeeds (`live.rs::begin_prompt`) |
| Antigravity app (sidecar, `agentapi send-message`) | The sidecar took the job (`<row>-<try>`) | `agentapi`'s `response.sendMessage`, with the conversation's database present before the send | A `SYSTEM_MESSAGE` step, `sender=system`, with our fingerprint, after the step count before | The job id, then the fingerprint | `delivered` when the step count grows within 20 s (`agy_sidecar/deliver.rs::send`) |

How reliable each one is:

- **Codex: strong.** Notchling owns the process and gives it one prompt, so its `turn.started` can only be ours. A process that exits before it with an error never ran the turn, so a retry is safe. The rollout check covers a crash after `turn.started`: the `UserMessage` item is there or it isn't. **Gap today:** the Outbox counts Sent when the process spawns. A turn that fails at once (an expired login, a thread Codex can't find, a refused model) has already left the Outbox as Sent. Only a `codex.turn {state: failed}` event says otherwise, and the message is gone. Not confirmed: that every Codex version Notchling supports writes `item_completed` with `UserMessage`. 0.166.0 does; the record reader also takes `response_item` with `role: user`, which the same rollouts carry.
- **Antigravity CLI: strong.** The request id ties the answer to our prompt, and one turn per connection means the first `session/update` after the write is ours. ACP answers `session/prompt` only at the turn's end, so the answer is a late receipt; the first update is the early one. An error answer with no update before it means the prompt never ran, so a retry is safe. If `agy acp` dies after the write with no update, the log's `USER_INPUT` step decides; with neither, the row is unsure.
- **Antigravity app: medium.** `agentapi` says "sent" for a wrong id too; the sidecar guards this by checking that the conversation's database exists first. The step count is not proof: while the agent works, its own steps grow the count. So a timed-out `agentapi` call (60 s) during a working turn reads `delivered` though the message may not be in. And a message sent while the agent works waits in the app until the turn ends (the decision's live test). The fix: count only a `SYSTEM_MESSAGE` step with our fingerprint after the count before. The sidecar then reports `delivered` on that step, `accepted` on `agentapi`'s answer alone (new, additive), and `unconfirmed` on neither (question 16).
- **Claude Code: medium-high with the hook, medium without** (sections 2 and 6). It is the only tool where the tie is a guess bounded by time and text, because the message goes in through a shared terminal.

### 14.3 A contract capability: `turn.receipt`

A 40th capability in `engine/src/contract/`, added with the `add-provider` skill (each tool's `support.rs`, `gaps.json`, the generated matrix):

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `turn.receipt` | N: `UserPromptSubmit` hook, then the transcript's take line · unwired | N: `turn.started` on `codex exec --json`, then the rollout's `UserMessage` item · unwired | N: the first `session/update` after `session/prompt`, then the log's `USER_INPUT` step · unwired |

- The sidecar channel declares it Fallback in its `gap_notes()`: "`agentapi`'s answer, then the `SYSTEM_MESSAGE` step; while the agent works, only the answer". The `app` and `remote` channels send nothing, so it is Not applicable for them.
- The honest values today, before phase 4: Claude Code F (text over the transcript), Codex and Antigravity Gap (Sent when handed over). They go in `gaps.json` until phase 4 closes them.
- The postman asks the registry (`support(tool, TurnReceipt)`) instead of `Courier::verifies()`. Native or Fallback: follow the receipt, then the record. Gap: Sent when handed over, as today.

### 14.4 Busy sessions: wait, refuse or queue

Four rules, the same for every tool:

1. **Queue** whenever the session exists and the channel can send. The Outbox takes the message and shows why it waits. **Refuse** only what can never go: no such session, a channel that only drafts (Antigravity `app`, `remote`), acting off, the 200-row cap.
2. **Wait** when handing over now would cut into or fork someone else's work. The row says why and keeps Edit and Remove.
3. **Type into the tool's own queue** only when the tool has one that the person's own typing uses too (Claude Code's box; the Antigravity app through the sidecar), and with only one of ours unread at a time.
4. **Never** start a second process on a session another process holds. A wait never ends in a send by itself: it ends when the facts change, or when the person removes the message.

| Case | Today | Proposed |
| --- | --- | --- |
| Claude Code mid-turn | Typed into Claude Code's queue; the next row waits (`queue`), shown as plain "Queued" | The same, with the reason shown; the rows behind go together (section 15) |
| Claude Code with a permission prompt or dialog | Waits (`dialog`) | The same |
| Codex, Notchling's own turn running | Waits for the turn's end (`turn`; `send_if_idle` answers Busy) | The same; the rows behind go together (today they already join when saved) |
| Codex thread with a turn open that isn't ours (the Codex app, the TUI, another `codex exec`) | Not seen: `is_busy` knows only Notchling's runs, so a second turn starts on the same thread | Wait (`why: held`) until the rollout has `task_complete` or `turn_aborted`. Detected from the rollout's tail (`turn_open`, already read for the notch) when no run of ours is on it |
| Codex thread open but idle in the Codex app or TUI | Not seen: `codex exec resume` runs; the other window doesn't show the turn | Question 15. Detected with `lsof`, as for `agy`: on this Mac the Codex app's `codex app-server` (0.170.0, a managed daemon) keeps the rollout it holds open for reading and writing |
| Antigravity CLI, Notchling's own `agy acp` working | Waits (`turn`) | The same |
| Antigravity conversation held by another `agy` (a delegation, a terminal) | Refused when sent (`check_session`, the holder's note), before anything is saved; a row saved before the hold waits as busy, shown as "Waits for the current turn" | Saved, and waits (`why: held`, with the holder's note). A second `agy acp` on it would stop the other's work, so it never starts one. Question 14 |
| Antigravity app, agent working | Sent at once; the app holds it until its turn ends | The same: it is the app's own queue. "Accepted" until the `SYSTEM_MESSAGE` step |

New `why` value `held`; an older app shows it as plain "Queued".

## 15. Sending several messages fast

### 15.1 Today

Three messages sent from Chat about 2 s apart while a Claude Code bg job works (mid-turn):

| | Message 1 | Message 2 | Message 3 |
| --- | --- | --- | --- |
| Within 5 s | Sending, typed, put in Claude Code's queue (`enqueue` line): "Queued in the session", no buttons | Saved as its own row, because the last row is `in_queue` and `session.send` joins only a `queued` row or one waiting for `turn` or `dialog`. The gate sees ours unread in Claude Code's queue (`queue_waiting > 0`): waits with `why: queue`, shown as plain "Queued", with Edit and Remove | Its own row, because the last row waits for `queue`, which `combine_except` doesn't join: "Queued", Edit, Remove |
| Claude takes message 1 at its next step | Sent | Typed now, in Claude Code's queue | Still waiting |
| The step after | | Sent | Typed |
| The step after that | | | Sent |

The model sees three messages at three steps, each after it already acted on the one before, and message 3 can come minutes later. Typed in the terminal, all three would sit in Claude Code's queue and go in together at its next step: Claude Code joins queued prompts into one user line, and Chat already says "Sent together with another message" (`chat-merged`).

If message 1 goes unsure (a U+200B, a picture chip the matcher misses), it reads "May already be in the session" with Send Again and Drop. Messages 2 and 3 read "Queued" and wait for good (B3), with nothing saying why. The phone has no buttons for it.

Codex and Antigravity today: rows wait for the turn's end (`turn`), and `session.send` joins a new message into the last row while that row waits for `turn` or `dialog` (`OutboxStore::combine_except`, a blank line between, the answer says `combined: true`). So the same three messages to a working Codex session already become one message. Rows saved through `codex.send` and `agy.send` are kept apart (the `apart` set).

### 15.2 Redesigned

- **Accepted within about a second.** Message 1's receipt comes tens of ms to 1 s after Return. The bubble reads "In Claude Code's queue" (accepted, so Sent under question 1), then it joins the conversation when the model takes it.
- **A reason on every waiting row.** `queue`: "Waiting for Claude to read the message before". `turn`: "Waiting for the turn to end" (slash commands, Codex, Antigravity). `dialog`: "Waiting for a dialog". `held`: "Waiting: the session is open in another window". Behind an unsure row: "Waiting for the message before".
- **Edit and Remove on every row not handed over yet**, as today, and never on one that is.
- **Unsure steps aside after 2 minutes** with the countdown on the unsure row ("Next messages go in 1:42"), Retry and Mark as Sent.
- **The rows behind go together** when their turn comes (question 11, option B, below): messages 2 and 3 go in one step after message 1, as one message, the way the terminal would deliver them.

### 15.3 Question 11: separate, joined, or all into Claude Code's queue

- **A. Keep them separate (today).** Each row is its own message, typed only after the one before was taken. Each arrives a model step later, and the model acts on each alone. N messages mean N receipts and N chances of an unsure.
- **B. Join them when their turn comes.** When the head's turn comes, the rows right behind it that are still untouched go with it as one message, a blank line between them. Until then each row stays its own bubble with Edit and Remove.
- **C. Type them all into Claude Code's queue, like the terminal.** Fastest, and Claude Code joins them itself at the take. But they then live only in Claude Code's memory and are lost on a restart (the 2026-11-15 rule). They can no longer be edited or removed from Chat, several of ours unread in one queue bring back the "which one is which" problem, and Codex and ACP have no such queue, so it is Claude Code only.

**Recommendation: B.**

- It gives the session what the terminal gives it: the burst arrives together, one step after message 1, not one step apart.
- Notchling keeps the messages until they go: editable, removable, and safe across a Claude Code or daemon restart.
- One typed message means one receipt and one fingerprint, so a burst has one chance of going unsure, not three.
- It is one rule for every tool. Codex and ACP take one message per turn, so joining is how a burst reaches them at all, and they already join today, at save time.
- The cost is small: the model sees one message with blank lines instead of three. That is what the terminal's own join gives it too.

B's rules:

- When the postman takes the head (the gate says Go), it joins the rows right behind it that are untouched (`queued`, or `waiting` for `queue`, `turn` or `dialog`), in order, a blank line between, up to `MAX_TEXT`. It stops at the first row it can't join: a slash command or `!cmd` (`Kind` other than Prompt), Clear Box and Send, other options (model, effort), a row being edited (section 16), a row put back by Retry.
- The head keeps its id and place. The joined rows leave with `outbox.removed {why: "joined", into: <head id>}` (a new `why` word; an older app just drops the bubble). Chat shows one bubble from then on, labelled "2 messages".
- Pictures: each row's picture paths sit on their own lines (Chat's attachment form), so they still become chips; the fingerprint's subset rule covers them.
- The join moves from save time to take time for every tool, so save-time `combine_except` and the `apart` set go: one rule, and rows stay separate and editable until they go.

## 16. The send queue in Chat, the notch and the phone

The page `index.html` draws all of this. This section is the reference for the words and the behaviour.

### 16.1 Every state, with its words and buttons

Status words are short and plain; buttons are Title Case text links, as Edit and Remove are today (Ghost buttons; the main one in the accent colour). A bubble not yet in the conversation is drawn at 60% (as today); its status line has an icon (today every line has the clock).

| State (`state`/`why`) | Icon | Status line | Buttons | `.ftl` id |
| --- | --- | --- | --- | --- |
| Queued, first in line | Clock | Queued | Edit · Remove | `chat-outbox-queued` (kept) |
| Waiting, `queue` | Clock | Waiting for Claude to read the message before | Edit · Remove | `chat-outbox-wait-queue` (new; the tool's name: `{ $tool }`) |
| Waiting, `turn` | Clock | Waiting for the turn to end | Edit · Remove | `chat-outbox-turn` (reworded from "Waits for the current turn") |
| Waiting, `dialog` | Clock | Waiting for a dialog | Edit · Remove | `chat-outbox-dialog` (reworded from "Waiting: dialog open") |
| Waiting, `held` | Window | Waiting: the session is open in another window | Edit · Remove | `chat-outbox-held` (new) |
| Waiting behind an unsure row | Clock | Waiting for the message before | Edit · Remove | `chat-outbox-wait-unsure` (new) |
| Waiting, `retry` | Clock | Couldn't reach the session, trying again | Edit · Remove | `chat-outbox-retry` (reworded) |
| Editing (held by Edit) | Pencil | Editing | none on the bubble; the reply box has Cancel and Save | `chat-outbox-editing-row` (new) |
| Sending | Spinner | Sending… | none | `chat-outbox-sending` (kept, with the ellipsis) |
| Accepted (receipt, Claude Code idle) | Check | Accepted | none | `chat-outbox-accepted` (new) |
| In the tool's queue (receipt and an `enqueue` line) | Tray | In Claude Code's queue | none | `chat-outbox-in-queue` (reworded: "In { $tool }'s queue") |
| Sent | none | none: the bubble joins the conversation at full strength | Copy, as for every sent message | |
| Sent with others by the tool | none | Sent together with another message | | `chat-merged` (kept) |
| Joined by Notchling (option B) | Merge | 2 messages, then the state's own line | as for the state | `chat-outbox-joined` (new, `{ $count }`) |
| Unsure, holding | Alert (orange) | May already be in the session · Next messages go in 1:42 | Retry · Mark as Sent | `chat-outbox-maybe` (kept), `chat-outbox-hold-left` (new, `{ $time }`) |
| Unsure, aside | Alert (orange) | May already be in the session | Retry · Mark as Sent | `chat-outbox-maybe` |
| Failed or refused | Cross (red) | Couldn't send: the plain reason ("Codex isn't signed in", "The Notchling sidecar isn't running in Antigravity") | Retry · Remove | `chat-outbox-failed` (new, `{ $reason }`) |
| Box has other text (unchanged) | Alert | Box has other text: … | Clear Box and Send · Edit · Remove | `chat-outbox-box` |
| Session stopped (unchanged) | Stop | Session stopped | Resume and Send · Edit · Remove | `chat-outbox-stopped` |
| Session not found (unchanged) | Cross | Session not found | Remove | `chat-outbox-gone` |

Under the unsure line, the daemon's `why` picks a second line when it knows more: `no_receipt` "Claude Code didn't confirm it", `dropped` "Claude Code's queue lost it", `blocked` "A hook in the session blocked it", `restart` "Notchling restarted while sending it" (new ids `chat-outbox-why-*`). Button renames: Send Again becomes **Retry**, Drop becomes **Mark as Sent** (question 3).

### 16.2 Flows

- **Edit.** Edit opens the message in the reply box, with a bar above it, "Editing a queued message" (today's `chat-outbox-editing`), and Cancel. The bubble stays in its place, reads "Editing", and is **held**: a new `outbox.hold {id}` (additive, app tier, a 10-minute lease) makes the postman skip it, and it is not joined. The rows behind it wait ("Waiting for the message before"), so the order holds. **Save** (Return) sends `outbox.edit`, which puts the new text back in the same place and releases the hold; Cancel or Esc releases it unchanged. Today nothing holds the row: the postman can type it while the person edits, and Save then answers "Already sent" (question 17).
- **Remove.** Remove takes a row that has not been handed over out at once, with no confirmation (nothing was sent). The rows behind move up, and the new first row's line changes ("Queued" or its own reason). An unsure row has no Remove; Mark as Sent is its way out, logged as the person's call.
- **Send Now: not in v1.** It is only safe in one state: waiting for Claude to read the message before. There it would type the message into Claude Code's queue (option C for one message). With option B that message already goes one step later, joined, so the gain is small. In every other state it does harm: during a dialog it types into the dialog; during a turn Codex and ACP can't take it, and a slash command mid-turn is what the turn rule exists for; for a session open in another window it forks the conversation. If it is wanted later, it fits only the `queue` wait, for Claude Code.
- **Reorder: no drag in v1.** The queue is usually one to three messages and lives for seconds to minutes. With option B the waiting rows go together anyway, so their order only sets the order of paragraphs in one message. A drag races the postman, which can take the first row under the finger. The notch and the phone have no drag. Remove and Edit give the same result. If it is asked for, a later "Move Up" in the row's menu is simpler than drag.

### 16.3 Where the queue lives

- **Chat.** The queue's bubbles stay at the bottom of the conversation, above the reply box, in order: the first in line on top, the newest just above the box. With more than 3 messages, a compact bar, "5 waiting", collapses them. The first in line and any unsure message always stay in view, and the bar opens the rest. The bar is one line, right-aligned like the bubbles.
- **The notch.** The session's row shows a small count badge with the clock, "3", in the faint colour. An unsure message turns it into the "needs you" orange with "!", as a permission prompt does, since only an unsure message asks something of the person.
- **The phone: later** (question 8). The phone keeps `session.message` → `{"sent": true}` and shows no queue states; it gains from the bounded unsure.

## 17. Migration: four phases, failing tests first

Each phase builds and ships alone. The tests listed first fail on `origin/agent-run-oss` today.

### Phase 1: one fingerprint, and unsure steps aside (engine and daemon)

Failing tests first:

1. `count_since` takes a row whose text has U+200B against a user line without it (the open branch's case; today `unsure`/`not_found`).
2. A row with a `.heic` path against a user line that kept the path as text: taken (today never).
3. A row with a bidi control, or a leading `/` or `!` plus U+200B, is saved pre-cleaned, so `plan::reply` pastes it without them (today pasted as is, then held for review).
4. Postman: an `unsure` head and a `queued` row behind it: after `UNSURE_HOLD_MS` the row behind is taken; the unsure row keeps its buttons (today blocked for good).
5. Postman: a second unsure in a row holds.
6. Parity over the 2.1.328 fixtures: the row, the transcript user line, the `enqueue` content and the box text give one fingerprint.
7. (Option B) Three rows to a session whose Claude Code queue holds our earlier prompt: when that prompt is taken, rows 2 and 3 are typed once, as `two\n\nthree`, and row 3 leaves as `joined` into row 2 (today: two typings, one step apart).
8. (Option B) A `/cmd` row, or a row with other options, between them stops the join; a row saved while the head waits for `turn` stays its own row until the take (today it is joined when saved).

Then build: `transcript/fingerprint.rs`; `count_since`, `queue_state` and `input_box` on it; one picture list; pre-clean in `OutboxStore::insert`; the aside rule in `outbox/gate.rs` and the postman's head; `outbox.remove {sent: true}`; the join at take time in place of `combine_except`; bubble words for `aside`, the reworded unsure and each waiting reason (in `chat.ftl`). Close `agent/fix-invisible-chars-reply` into it.

### Phase 2: the app renders daemon state only

Failing tests first:

1. A picture message's Sent bubble leaves when the conversation holds the line whose `uuid` the daemon named in `take` (the image-echo case; today the app matches text and misses the chips).
2. An `in_queue` row hides the Claude Code queue entry with the same `queuedAt`, whatever its text says (today `hidden_queued` compares text).
3. A `box` row with `boxIsOurs: true` reads "The box holds this message" without calling `input_box` (today the app calls `holds_message`).
4. With no `take` (older daemon), the Sent bubble leaves after `GRACE_MS` on the view's tick.
5. A check that `crates/app/src/chat` compares no message text: `outgoing.rs`'s text helpers and `Pictured` are gone (a unit test over the module's public items, or a guard rule).
6. Edit holds the row: with `outbox.hold` on the head, the postman doesn't take it and the row behind waits; Save puts the new text in the same place (today the head can be typed while it is edited, and Save answers "Already sent").
7. With 5 rows, Chat shows the "5 waiting" bar, the first in line and any unsure row; the notch's session row has the count badge, orange with "!" when one is unsure.

Then build: the daemon reports the take line's `uuid` and the `enqueue` line's time from the same read that settles the row; the parser keeps `uuid` on user items and the time on queue entries; the app keys sending echoes on `outboxId`, Sent bubbles on `take.uuid`, in-queue bubbles on `queuedAt`; `outbox.hold`; the status icons, the collapsed bar and the notch badge (section 16). Don't merge `agent/fix-image-echo-stays`'s matcher (its expiry timer moves here).

### Phase 3: receipts

Failing tests first:

1. `notchling hook --receipt` with a fixture event: prints nothing, exits 0, returns in under 1 s with no daemon and with a daemon that never answers, and no planted token from the prompt is in the bytes it writes.
2. Installer over temp copies: Receipt=Install adds the async `UserPromptSubmit` entry with no matcher, keeps the person's own `UserPromptSubmit` hooks, is idempotent; Remove removes only ours.
3. A `sending` row with `entered_ms` and a fitting receipt goes `accepted`, then Sent on the take line whose fingerprint is the receipt's, in a fixture where our text and the transcript differ in a way phase 1 doesn't read (a made-up chip form): without receipts it would be unsure.
4. Receipt before the line, and after it: settled once.
5. A receipt before Return, one with `agent_id`, one that doesn't fit (a task notification): ignored.
6. A late receipt settles an `unsure` row within 10 min.
7. Healthy hook, no receipt in 3 s, box still carrying our text: `waiting`/`retry` with the leftover cleared, not unsure.
8. A loose-only fit raises one `claude.contract_drift` per version.

Then build: the Receipt category, `hook.receipt`, the receipt book (memory, per session, last 32, 10 min), health per process, the matching rules, the drift counts, the recorder and the first version folder, the Install Hook words. A live check by the maintainer: a real bg job, a reply idle, a reply mid-turn, a picture, a U+200B, a daemon restart mid-send.

### Phase 4: Codex and Antigravity receipts, and busy sessions

Failing tests first (fakes only: the fake `codex`, the fake ACP agent, fake `Steps` and `lsof`; never the real `~/.codex` or `~/.gemini`):

1. Codex: a fake `codex` that exits 1 with an error before `turn.started`: the row stays, with Codex's words and Retry (today it is Sent at spawn and gone).
2. Codex: `turn.started`, then a rollout `UserMessage` item with the row's fingerprint: Sent, with `take.id`.
3. Codex: a rollout with a turn open that no run of ours holds: the row waits (`held`) and no process starts (today one starts).
4. ACP: the fake agent answers `session/prompt` with an error and no update: the row stays, Retry (today Sent at the pipe write).
5. ACP: a conversation another `agy` holds (fake `lsof`): `session.send` saves the row and it waits (`held`) (today refused before saving).
6. Sidecar: `agentapi` times out while the agent's own `PLANNER_RESPONSE` steps grow the count: `unconfirmed`, not `delivered` (today `delivered`).
7. Sidecar: a `SYSTEM_MESSAGE` step with our fingerprint after the count before: `delivered`; `agentapi`'s answer alone: `accepted`.
8. Contract: `turn.receipt` declared by all three tools; the matrix regenerated; `gaps.json` loses the Codex and Antigravity cells this phase wires.

Then build: `turn.receipt` in the contract; each runner courier reports its receipt and record; `Courier::verifies()` replaced by the registry; the Codex holder check (`turn_open` not ours, `lsof` for a `codex` that isn't ours); `held` in the gate; the sidecar's `SYSTEM_MESSAGE` match and its `accepted` report. A live check by the maintainer: a Codex reply with an expired login, a Codex thread open in the Codex app, an `agy` delegation holding a conversation, a sidecar reply mid-turn.

## 18. What gets deleted

| Phase | Gone |
| --- | --- |
| 1 | `transcript/sent.rs::sent_since` (no caller) and `marks`; `sent::plain`, `names`, `unpasted` and the picture code of `message.rs` folded into `fingerprint.rs`; `message.rs::IMAGE_KINDS` (8 extensions); the separate letters rule in `input_box.rs`; with option B, `OutboxStore::combine`/`combine_except` and the `apart` set (the join moves to take time) |
| 2 | `app/src/chat/outgoing.rs` matching (`Outgoing::arrived`, `copies`, `dated_since`, `comparable`, `CLOCK_SLACK_MS`); in `app/src/chat/outbox.rs`: `shown_by_transcript`, the text compare in `hidden_queued` and `unbubbled`, `deliver`/`drop_boxed`/`forget` by text, `squeeze`, the `holds_message` call. The image-echo branch's `Turns`, `Wanted` and `input_box::Pictured` never land |
| 3 | The 5 s safe-mode trip for sessions with a healthy hook (a receipt explains a late queue line); `outbox_verify.rs`'s look-again loop shrinks to the late-settle check shared with receipts |
| 4 | `Courier::verifies()`; `outbox_runners.rs::STARTED` (Sent at hand-over); the sidecar's count-only `delivered`; `agy_ops::check_session`'s refusal of a held conversation on the send path |

## 19. Open questions for the maintainer

Each question has a recommendation, in bold.

1. **Accepted or Sent?** A receipt proves the tool accepted the message, not that the model read it. Should "accepted" count as Sent, instead of the 2026-11-15 answer (only in Claude Code's queue is not Sent)? **Yes: a receipt is Sent.** It is simpler, faster, and the same for every tool. Keep the safety net from section 5: the daemon follows the row until its take line, and a lost Claude Code queue brings it back as unsure with Retry.
2. **The hold:** 2 minutes, then step aside? Or step aside at once, or never? And the "second unsure in a row holds" breaker? **Yes: 2 minutes, with the breaker.**
3. **Button words:** rename Send Again and Drop to Retry and Mark as Sent? **Yes.**
4. **Install:** add the receipt hook to the one-click Install Hook (default on), or a separate choice in Settings? It runs a short Notchling process on every prompt in every Claude Code session on the Mac. **Yes: in Install Hook, on by default.**
5. **Fingerprint only on the socket:** the hook never sends prompt text to the daemon, so drift is studied from the recorded fixtures, not from production. Fine? **Yes.**
6. **Pre-clean:** the Outbox removes invisible characters from a message before typing it (Claude Code would remove them anyway, or hold the prompt for review). Fine? **Yes.**
7. **The open branches:** close `agent/fix-invisible-chars-reply` into phase 1 and drop `agent/fix-image-echo-stays` (phase 2 deletes its matcher)? Or merge the image-echo fix now as a stopgap? **Yes: fold the first into phase 1, drop the second.**
8. **Phone:** show Outbox states on the phone (additive `outbox` event)? **Later.**
9. **Structured input:** skip `stream-json` sessions and wait for a supported way to send to a bg job (agent view's peek panel does it through the supervisor), or plan a Notchling-run Claude session kind like the Codex runner? **Yes: skip it.**
10. **Auto-retry:** never (today's G5), or once when the hook is healthy, no receipt came, and the transcript shows nothing after 60 s? **No: never by itself.**
11. **Several messages sent fast:** A, keep them separate (today); B, join the rows still waiting into one message when their turn comes; C, type them all into Claude Code's queue like the terminal (section 15.3)? **B.** The burst arrives together one step after the first, stays editable and safe across restarts until it goes, gets one receipt instead of three, and is one rule for every tool.
12. **`turn.receipt`:** add it as the 40th contract capability, declared by each tool (Claude Code, Codex and Antigravity Native; the sidecar channel Fallback), and let the postman ask the registry instead of `Courier::verifies()`? **Yes.**
13. **Codex and Antigravity Sent moment:** Sent on the tool's receipt (Codex `turn.started` or the rollout's `UserMessage`; ACP the first update or the `USER_INPUT` step; the sidecar's answer, then the `SYSTEM_MESSAGE` step) instead of when handed over, so a turn that fails at once keeps its row with Retry? **Yes.** Today such a Codex message is lost from the Outbox.
14. **An Antigravity conversation another `agy` holds:** save the message and wait (`held`, with the holder's note), instead of refusing it before saving? **Wait.** Never start a second `agy acp` on it, since that stops the other's work.
15. **A Codex thread open in the Codex app or the TUI:** when a turn that isn't ours is open, wait. When it is idle: wait until it closes, or send with `codex exec resume` as today, though the other window may not show the turn until it reopens the thread? **Wait while a turn is open; when idle, send as today with a note on the row ("Codex may not show this in its other window"). First, a probe on a dev profile: does `codex app-server` re-read a rollout another process appended to?** Delivering through the holder (the app-server's own turn start) is **later**.
16. **The sidecar's proof:** report `delivered` only on a `SYSTEM_MESSAGE` step with our fingerprint, and `accepted` on `agentapi`'s answer alone, instead of on any growth of the step count? **Yes.** While the agent works, its own steps grow the count, so today a timed-out send can read `delivered` though the message isn't in.
17. **Edit holds the row:** a new `outbox.hold` while a message is open in the reply box, so it isn't sent or joined mid-edit, and the rows behind it wait? **Yes.** Today Save can answer "Already sent" after the person rewrote it.
