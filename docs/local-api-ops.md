# Local API ops: wire shapes

What each `notchling daemon` op takes and returns, tier by tier. Moved from the module docs of `crates/notchlingd/src/*_ops.rs` when the code lost its comments; the serde types next to each module are the final word. The socket, framing and tiers are in [docs/notes/daemon.md](notes/daemon.md) and [decision 0007](decisions/0007-local-api-tiers.md).

## `accounts_ops.rs`

Milestone 6's Claude Code accounts and lanes (E8a) as daemon ops, over
`notchling_engine::accounts::claude` and `notchling_engine::lanes`. Built and switched off:
while `accounts` is off every op but the two list reads answers `forbidden` with
[`ACCOUNTS_OFF`], and nothing reads the Keychain, a token or the network, or writes a file.
The two reads then use the daemon's own data only (the saved accounts list, a document in
`notchling.db` or, until a save moves it in, `<data>/daemon/accounts.json`; and `lanes.json`),
and write nothing, not even a database.
Besides the check at the top of [`AccountsOps::call`], the store's Keychain runner and HTTP
client are gated on the switch too ([`GatedSecurity`], [`GatedHttp`]), so a path that forgot
the check still reaches nothing.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `accounts.list` | app | `{sync?: bool}` | `{on, active, accounts: [{id, email, plan, needsLogin, lane}]}`; `sync` reads the live login first (switch on) |
| `accounts.switch` | app | `{id}` | as `accounts.list` |
| `accounts.add.begin` | app | `{}` | `{login, url}`: open `url` in the browser |
| `accounts.add.finish` | app | `{login, cancel?: bool}` | `{login, state: "waiting" \| "added" \| "failed" \| "cancelled", account?, error?}`, after up to 4 s |
| `accounts.remove` | app | `{id}` | as `accounts.list` |
| `accounts.move_all` | app | `{target: id \| null, dryRun?: bool}` | dry run `{plan: {jobs, closeLane, switchMain, signedOut}}`; else `{closedLane, switchedTo, moved, errors}` |
| `accounts.result` | app | `{ticket}` | the op's own reply, after up to 4 s; `{state: "working", ticket}` while it still runs |
| `lanes.list` | app | `{}` | `{lanes: [{name, path, accountID, email, folder, startCommand}]}` |
| `lanes.create` | app | `{account}` | `{lane}` |
| `lanes.remove` | app | `{name}` | `{removed: bool}` |
| `lanes.switch` | app | `{name, account}` | `{lane}` |

Every op that reads the Keychain or the network (`accounts.list {sync}`, switch, remove, Move
All, the lane ops) runs on its own thread ([`crate::accounts_late`]): one that takes more than
4.5 s answers `{state: "working", ticket, op}` (a sync answers the list as last read, with
`syncing: true`), and its reply comes from `accounts.result` and as `accounts.done`.

A refusal by the store (an account in a lane, no saved login, a Keychain error) is `failed` with
words for the user. After a change the list goes out on `accounts` as `accounts.changed`; a
sign-in's end as `accounts.add.ended`. Move All stops and resumes lane sessions too, so with any
to move it needs `acting` on as well.

## `acting_ops.rs`

Milestone 3's ops: acting on sessions (E3b, E4), the `acting` switch, the prompts a hook holds and the
hook's request. Built and switched off: while [`Acting`] is off every acting op answers
`forbidden` with [`ACTING_OFF`], nothing is typed, and `hook.request` passes.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `session.reply` | app | `{id, text, clear_box?: bool}` | `{"sent": true}`, or the queued shape `{"sent": true, "state": "queued", "waiting": <position>, "outboxId"}`; it saves an Outbox row and waits up to 50 s (below) |
| `session.send`, `outbox.*` | app | see [`outbox_ops.rs`](#outbox_opsrs) | |
| `session.held.discard` | app | `{id}` | `{"discarded": true}` when it removed any message waiting in the session's Outbox, `{"discarded": false}` when none waited; kept for older clients, it is `outbox.discard` of that session |
| `session.answer` | app | `{id, answers: [[n...] or "text"], question?: {items}}` | `{"answered": "hook" or "terminal", "promptId"?}` |
| `session.permission` | app | `{id, decision: "allow"/"deny"/"always", message?}` | `{"decided": decision, "promptId"}` |
| `session.permission` (no hook) | app | `{id: <row id>, decision, expect: {tool, summary}}` | `{"decided": decision, "answered": "terminal"}` |
| `session.interrupt` | app | `{id}` | `{"sent": true}` |
| `session.take_back` | app | `{id}` | `{"taken": bool}` |
| `session.stop`, `.remove`, `.new`, `.move`, `.command`, `folder.trust` | app | see [`crate::control_ops`] | |
| `settings.set_acting` | app | `{on: bool}` | `{"acting": bool}` |
| `prompts.list` | app | `{}` | `[prompt...]` |
| `settings.get_acting` | agent | `{}` | `{"acting": bool}` |
| `session.question` | agent (a held prompt), app (the screen) | `{id}` | `{"promptId", "items"}` for a held question; else, for the app with acting on, the job's screen read (`{"id": "screen:…", "items"}`, `{"items": []}` when no question shows) |
| `session.shell_output` | app (acting on) | `{id, command}` | `{"lines": [string]}`: what the running shell command `command` (a Claude Code `!` row) has drawn under its echo on the job's terminal, read through an attach that types nothing; `[]` when the job isn't working (judged from its own files, not the cached list), the command isn't on the screen or it printed nothing yet; `"busy": true` is added when a reply or interrupt holds the job's typing lock and the read was skipped. A quiet read (not in the audit) |
| `session.chrome` | app | `{id, ack?}` | `{offered, task, detected}`: whether Chrome can be turned on for this Claude Code session (a job on this Mac, no shell row), the task `session.command` `/chrome` started (`phase`: `waiting`, `working`, `done`, `needs_you` or `cancelled`, with `why`, `step`, `result` or `need`, plus `on` and `since_ms`) or `null`, and what the job's own files say (`detected.on`, `basis`: `flag`, `default` or `unknown`). `ack: true` clears a finished task after it was read. A quiet read (not in the audit) |
| `session.reload` | agent (only its own session, and always after the turn), app | `{id, afterTurn?, cancel?}`; `id` may be `self` for an agent | `{offered, kind, rev, task}` (as `session.reload.state`); starts a reload of a Claude Code job on this Mac: the daemon waits for the turn to end (an app call without `afterTurn` is refused with `need: busy` while a turn runs), for an open question and for background tasks (`inFlight.tasks > 0`), then restarts the job with `claude respawn`, the same start flags, folder, model and account, the permission mode as the session has it now, and queued replies still in the outbox. One task per session, one worker per task: a call while a task is `waiting` or `working` answers that task and starts nothing. It never waits for the restart: a call from the session itself would block its own turn. `cancel: true` asks a `waiting` task to stop (`cancelling: true`; the worker then writes `cancelled`), is refused with "Already restarting" once the task is `working`, and answers a finished or missing task as it is. A terminal session the user owns (`kind: terminal`) is refused with the command to run there; a session on another machine is refused. Behind the acting switch |
| `session.reload.state` | app | `{id, ack?, rev?}` or `{all: true}` | `{offered, kind, rev, task}`: `kind` is `job`, `terminal`, `remote` or `other`, `offered` is true for a job, `task` is `null` or `{phase: waiting (by: {why: turn, dialog or tasks, count?}), working (step), done (result, session? when Claude Code gave the conversation a new id), needs_you (need: {need: busy, by} or tasks_unknown or failed, text}), cancelled; after_turn, since_ms, cancelling?}`. Only the task's worker writes `done`, `needs_you` and `cancelled` (a worker that dies writes `needs_you`); a finished task is kept until `ack: true` with the `rev` it was read at, or a new start. `rev` grows with every change, across daemon restarts too. `{all: true}` answers `{rev, tasks: [{id, rev, task}]}`. Every change is pushed as `reload.changed {id, rev, task}` (topic `prompts`, app tier; `task: null` once acked). A quiet read (not in the audit) |
| `hook.request` | agent, `client: "hook"` only, from a process a Claude Code session started | `{event, waitMs?}` | a decision (`prompts`) |

An `id` is a row id from `sessions.list`. A held prompt is answered by its own id from
`prompt.pending` only (`session.permission` and `session.answer` when the id is a prompt's,
else they type into the row's dialog), never found from a row: a prompt is the one the app showed.
A typing failure (a dialog open, the question gone) is code `failed`.

`session.permission` with a row id and `expect` answers a background job's permission dialog
that no hook holds (the app read it from the job's state, "approve Bash: ls"): it attaches,
reads the dialog off the screen and types nothing unless it is a permission dialog asking about
`expect` (the same tool, the summary's text in it, compared without whitespace and with an
elided summary matched piece by piece) with an option for the decision; then it presses that
option's digit only (Allow "Yes", Always the "don't ask again" one, Deny the "No" one) and
waits for the dialog to go. Never Enter, Esc or Ctrl+C. A deny message isn't taken there.

The hook is who its process is, never what it says: see [`crate::hook_ops`].

### `session.reply` on the Outbox

`session.reply` is kept for the cluster (it forwards a remote session's reply to the owner node), older apps and agents. It does what `session.send` does (below), then waits up to 50 s (`REPLY_WAIT_MS`, under the app's 60 s call timeout) for the saved message to settle, and answers:

- `{"sent": true}` once the message is Sent (the transcript shows the model took it, or for Codex and Antigravity the turn went out).
- `{"sent": true, "state": "queued", "waiting": <position>, "outboxId": "<id>"}` at once when the message reaches Claude Code's own queue (`in_queue`), and at 50 s when it is still queued, waiting or sending. The message stays in the Outbox and goes in when its turn comes; nothing is typed twice.
- `failed` when the message needs the person. The saved message is removed first, because the caller puts the text back in its reply box. A message that found other text in the session's input box (`box`) is `failed` "The session's input box holds other text, so the message wasn't sent." with `err.data` `{"why": "not_cleared", "box"?: "<the text>"}` (`box` is what the input box holds, cut at 1000 characters, left out when it can't be read; it goes to the caller only, never to the audit log, an event or `reply-screen.txt`). A message that may already be in the session (`unsure`) is `failed` "It may already be in the session. Check the session before sending it again." (it does not say the message wasn't sent, because sending it again could type it twice) A stopped session is `failed` "The session is stopped." and one that is gone "No such session."
- `failed` "The message was removed." when something else removed the message while the call waited (Remove, Drop, Discard or the session being removed). It is never answered `sent`.

A message saved by `session.reply` is never joined into another row and never carries another (the caller waits for that row's own outcome), so it is typed as its own message.

Whatever `session.send` refuses, `session.reply` refuses the same way, before anything is saved. `clear_box` is `true`, `false` or absent; anything else is `bad_request`. An agent's reply also writes its `session.reply` event before the message is saved.

What may be cleared. A box that holds the whole message, or the whole or a start or an end (at least 10 letters and digits) of a text this daemon tried to put in that session and did not see go out (the last five per session, 12 hours, in memory), is cleared with the reply's usual keys (Ctrl+E, Ctrl+U, Backspace per line; never Ctrl+K, Esc or Ctrl+C) and the new message goes on. Any other text is never typed over. `clear_box: true` (on `session.send` or `session.reply`, or `outbox.resend {clear_box: true}` for a message already saved) clears the box with those keys whatever it holds, checks that it is empty (a box that does not empty, or that a person typed into meanwhile, is `box` again, with nothing pasted and no Enter) and then types and sends as usual. A folded paste (`[Pasted text #N +M lines]`) is only ever cleared that way. The error's `data` is a general, additive part of the wire: `{"code", "message", "data"?}`.

## `activity_ops.rs`

What Settings > Activity shows: `activity.list`, over `notchling_engine::activity`. App tier, read only,
no switch, `OpSpec::APP_QUIET` (a success is not recorded; the page asks only on open and on Refresh). It
reads the Activity store (`daemon/events.db`), where the daemon wrote every event, and answers rows, newest
first. The first start after the upgrade imports the old `audit.jsonl`, `remote-log.jsonl` and
`account-log.json` once (rows marked `legacy`).

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `activity.list` | app | `{kinds?, groups?, event_kinds?, session?, before_seq?, since?, limit?}` | `{rows: [{time_ms, source, kind, op, who, result, detail?, event, group, title, outcome, refs, seq, spooled, audit_n?, op_id?}], truncated}` |
| `log.verify` | app | `{n?}` | the Audit's state; with `n`, also `{line: {n, sealed, seal_ok, kind, outcome, ts, f}}` for that line |

- `kinds` is a list of `refused`, `failed`, `acted`, `setting`, `inbox` and `account_switch` (none or
  empty means all; any other name is `bad_request`). `groups` and `event_kinds` narrow to registry groups
  and event kinds, `session` to one session's rows, `before_seq` pages back, `since` is a time in
  milliseconds: only rows after it. `limit` is 200 by default and 1 to 1000.
- `source` is `audit` (the event is also a sealed Audit line, `audit_n` is its number), `legacy` (imported
  from the old files) or `activity` (the store only). `event` is the registry kind, `group` its group, `title`
  its title, `outcome` one of the registry's outcomes, `refs` the session, folder, account and device ids the
  event names, `seq` its place in the store and `spooled` is true for an event a client recorded while the
  daemon was down (the column is there; the app's spool arrives in Phase 5). `kind`, `op`, `who` and `result` keep the shape Activity had before, so an old app still
  reads them.
- `who` is `app` for an app-tier row, else the peer's executable name (`curl`, `claude`, `unknown`), or the
  phone's name for a device id the app knows. `result` is the event's `result` or `action` field, else its
  outcome. `detail` is the secret names of a vault use, a setting's key, a switch's name or an
  `api.refused` reason: names only, cut to 120 characters.
- `log.verify {n}` reads the one Audit line `n`, checks its seal and returns its `kind`, `outcome`, `ts` and
  fields. The page's Check Against the Audit Log compares `event`, `outcome` and `time_ms` of the row with
  them and needs `seal_ok`.
- Never in a row: message text, a typed text, an address, a free-text error, an inbox item id or a secret's
  value (a secret's name is shown). The reply stays under 900 KB: rows are dropped from the old end and
  `truncated` says so.

### Quiet ops

An app-tier call is recorded as an event (Audit and Activity) unless its spec is `OpSpec::APP_QUIET` and it succeeded.
These read-only polls are quiet: `awake.state`, `bridge.status`, `bridge.devices`, `bridge.tunnel.status`,
`accounts.list`, `lanes.list`, `providers.list`, `providers.usage`, `limits.status`, `limits.usage`,
`git.status`, `git.diff`, `git.branch_diff_stat`, `git.branch_diff_file`, `git.branches`, `git.worktrees`, `git.branch_list`, `activity.list`, `catalog.roles`, `codex.children` and `versions.workers`. A call to `accounts.list` or `providers.list` with
`sync: true`, or to `providers.usage` or `limits.usage` with `refresh: true`, reads Keychain and the network,
so it is logged. Failures and refusals of a quiet op are logged too. No op that acts, writes or touches the
vault is quiet.

## `agy_ops.rs`

Milestone 5's Antigravity CLI ops (E6b): `agy acp` sessions run by the daemon over
[`notchling_engine::tools::agy`]. Built and switched off: while [`Acting`] is off every op that
starts `agy`, types into it or answers it answers `forbidden` with [`ACTING_OFF`]; the reads
work.

| Op | Tier | Acting | Args | Reply |
| --- | --- | --- | --- | --- |
| `agy.sessions` | agent | no | `{}` | `[{id: "agy:<id>", name, project, cwd, sinceMs, working}]` (the summaries copy, plus sessions started here the copy doesn't list yet) |
| `agy.state` | app | no | `{id}` | `{id, open, loading, loadError, working, title, folder, config, commands, permission, elsewhere}` |
| `agy.conversation` | app | no | `{id, peek?}` | `{id, open, loading, loadError, working, conversation, truncated, omitted}`: the conversation the open runner holds (`transcript::model::Conversation` as JSON); not open, an empty one with `open: false`; one another `agy` holds (`elsewhere`), read from Antigravity's log with `elsewhere` set to what to say. A long one is its newest items within about 512 KB (`truncated: true`, `omitted` = how many older items were left out), since a reply is one 1 MiB line. `peek: true` doesn't count as use, so a list read every few seconds never keeps an idle process alive |
| `agy.open` | app | yes | `{id}` | `{id, loading: true}`: starts `agy acp` in the session's own folder and loads it; while another `agy` holds the session (a delegation) it starts nothing and answers `{id, loading: false, elsewhere}` ([`crate::agy_elsewhere`]) |
| `agy.send` | app | yes | `{id, text}` or `{draft_folder, text}` | with `id` (an existing session): `{id, sent: true, outboxId}` at once, the message saved in the Outbox; with `draft_folder`: `{id, sent: true}` after the session was started |
| `agy.cancel` | app | yes | `{id}` | `{cancelled: true}` |
| `agy.set_option` | app | yes | `{id, option, value}` | `{requested: true}`: the session's own options only |
| `agy.permission` | app | yes | `{id, rpc_id, option_id?}` | `{outcome}`; no `option_id` cancels |
| `agy.sidecar.take` | agent | no | `{waitMs?}` | `{jobs: [{id, kind: "reply"\|"start", session?, model?, title?, text}]}`; waits at most 25 s; only `notchling agy-sidecar` (client `hook`) with Antigravity above it in the process tree; also marks the sidecar as running |
| `agy.sidecar.report` | agent | no | `{id, result: "delivered"\|"unconfirmed"\|"failed", session?, error?}` | `{accepted}`; true only for a job that was taken and not yet reported; same caller rule as take |
| `agy.hook` | agent | no | `{event: "pre-tool"\|"pre-invocation", input, waitMs?}` | `{output}`: the JSON `notchling agy-hook` prints, or null for nothing; same caller rule; null unless acting is on and the sidecar channel is chosen |
| `agy.sidecar.status` | app | no | `{}` | `{state: "no_app"\|"off"\|"partial"\|"on"\|"blocked", app, chosen, enabled, plugin, pluginCurrent, hooks, running, waiting}` |
| `agy.sidecar.setup` | app | no | `{action: "turn_on"\|"turn_off", confirm?}` | the status plus `{backups}`; `turn_on` needs `confirm: true` and the install switch, and writes Antigravity's `config.json` (one key), `hooks.json` and Notchling's own plugin folder; `turn_off` removes the hooks, the plugin folder and the one `enabled` key (with a backup) |

While the `sidecar` channel is chosen (setting `antigravityChannel`), `agy.send` (with `id`, through the Outbox, or with `draft_folder`, which also takes optional `model` and `title`) and the Outbox's delivery to an `agy:` row queue a job for the sidecar instead of running `agy acp`, and `agy.cancel` asks for a soft stop (`{cancelled: true, soft: true}`) that the next `PreInvocation` hook turns into a terminate. A job waits for the sidecar's report: a sidecar that is not polling (or whose process is gone) fails the send at once with words that say so, a job nobody takes in 100 s is withdrawn and fails, and a taken job without a report in 100 s is shown as unsure. `agy.sidecar.take` takes optional `max` (jobs per take; the sidecar asks for one). A job's id is the Outbox row's id and its try number, so a repeat of an attempt that was delivered or is unsure never sends twice, while Send Again is a new attempt. `agy.send` for a new conversation can take up to 200 s and its caller should wait longer. The folder of `draft_folder` is checked and then unused: a new conversation has no workspace folder. Hook prompts are ordinary prompts (`prompt.pending`, answered by `session.permission`) with the session id `agy:<conversation>`, so Chat, the notch and the phone show them like any other.

`id` is a row id, `agy:<session>`. A session's folder comes from the summaries copy or from the
session this daemon started, never from the caller; a draft's `draft_folder` is an absolute
folder that exists. One turn runs per session. `agy.send {id}` saves the message in the Outbox (like `session.send`) and answers at once; while a turn runs it waits its turn, in order, instead of failing "busy". It counts Sent only when `session/prompt` was written to the agent (not while the prompt waits behind a session load); a load that fails keeps it waiting with the error as its note, and a turn cut after it was written is `unsure`. While another `agy` holds the session (a delegation) the call is `failed` with what Chat says, before anything is saved. A draft's first message and `draft_folder` still start the session directly.

Events ([`crate::agy_sink`]): `session.changed {id, session, row, change, error?}` on `tools`
(as Codex's; chunks at most every 250 ms), and on `prompts` (app tier) `agy.permission.pending`
/ `agy.permission.resolved`, each also as `prompt.pending` / `prompt.resolved`, so the notch
shares one answered state with Chat. A pending permission is a `prompts.list` row too. Its id is
`agy-perm:<row id>:<rpc id>`. Nothing is answered by the daemon on its own; turning acting off
closes every session, which cancels what was waiting.

## `ask_ops.rs`

Ask ([design](superpowers/specs/2026-11-16-ask-design.md), [decision](decisions/0022-ask.md), [note](notes/ask.md)): an agent saves a question in the daemon's `asks` table with `notchling ask`, the person answers on a card, and the answer goes to the command that waits. Served by `AskOps` (`ask_ops.rs`, `ask_main.rs`, `ask_waiters.rs`). `ask.open`, `ask.answer` and `ask.dismiss` are refused with `forbidden` and [`ACTING_OFF`] while the `acting` switch is off; `ask.wait`, `ask.mine` and `ask.list` are not, and open questions keep waiting while it is off.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `ask.open` | agent | `{question, key, choices?: [text, ...], mainMayAnswer?: bool, wait?: bool, deadlineMs?}` | `{"id", "state": "open"}`; the id is `q` and 32 hex characters |
| `ask.wait` | agent, the asking process only | `{id, waitMs?}` | `{"state": "open"}`, `{"state": "answered", "answer", "choice"?, "by"?}`, or `{"state": "dismissed"}`, `{"state": "expired"}`, `{"state": "gone"}` |
| `ask.mine` | agent | `{waitMs?}` | `[row, ...]`: the open questions the caller's session's main session may answer, without their answers |
| `ask.answer` | agent and app | `{id, choice?: n, text?}`, one of the two | `{"answered": true}`, or `{"answered": false, "state", "by"?}` when the question is no longer open or another answer was taken first |
| `ask.dismiss` | app | `{id}` | `{"dismissed": true}`, or `{"dismissed": false, "state", "by"?}` |
| `ask.list` | app | `{session?}` | `[row, ...]` in the order they were asked, open and closed (a closed one stays 24 hours); `session` is a row id from `sessions.list` or a session key |

`ask.open`:

- The session comes from the kernel, never from an argument: the peer's parents are walked (at most 16) to the Claude Code process whose `sessions/<pid>.json` names a session (`caller_session.rs`). The folder, the branch (`.git/HEAD`, worktrees included) and the process's start time come from the kernel too.
- `question` is at most 2,000 characters. `choices` is 2 to 6 texts of at most 120 characters, none empty. `wait: true` is a blocking ask (`mode` `wait`), else `background`; `deadlineMs` (ms since 1970) belongs to a blocking ask and is when the row expires. `key` is 1 to 64 characters, made by the `ask` command for itself; with the caller's pid it makes a retried open return the same question: `{id, state}` with the row's current state, nothing saved, nothing emitted.
- 20 open questions per session and 100 in all.
- The reply comes at once. The finder then looks for which agent asked, off the op thread (below), and the Inbox item is raised when it settles.

`ask.wait` is served only to the asking process: the caller's pid and start time must equal the row's. `waitMs` is 0 to 50,000 (default and cap 50,000). It replies at once when the row is `answered`, `delivered`, `dismissed`, `expired` or `gone`, else when an answer or an end comes, else `{"state": "open"}` at the end of `waitMs`. A reply that carries an answer moves an `answered` row to `delivered`. The same process asking again gets the same answer (a `delivered` row still hands it over), so a reply lost on the way loses nothing. A closed socket ends the wait; the 1 s tick then decides whether the asker is gone.

`ask.mine` needs a session as `ask.open` does. It lists the rows that are `open`, have `mainMayAnswer`, whose asker is `subagent`, `codex` or `antigravity` (never `main` or `unknown`), and whose session is the caller's. Without `waitMs` it answers at once; with it (at most 50,000) it waits until the list is not empty or the time ends. It waits on one slot every publish wakes.

`ask.answer`, the person (app tier: Chat, the notch; the Inbox's answer comes through `inbox.answer`): any `open` question. `choice` is the 1-based number of a choice; `text` is up to 10 KB. An agent (agent tier) is taken only when all of these hold: the question is `open` with `mainMayAnswer`, its asker is `subagent`, `codex` or `antigravity`, the caller's kernel session is the question's session, and the caller is the main session: the finder must find the caller's own `notchling answer` command (command match, up to 5 s) in the session's own transcript and in no subagent's. The answer is recorded `by: main`. A caller that fails any of these gets `forbidden` `Only the person answers this question.`; that includes an agent in no session, another session, a sibling subagent, and any question from `main` or `unknown`. A question that is no longer `open` is not an error: it replies `{"answered": false, "state", "by"?}`.

First answer wins: the daemon marks the question in memory under one lock, then one conditional write (`WHERE state = 'open'`) saves it. A second answer or a dismiss that finds the mark replies with the state and the winner's `by` (`mac`, `phone`, `main`) and writes nothing. A mark whose write fails is dropped, so a daemon that stops between the mark and the write finds the question `open` again.

`ask.dismiss` refuses an agent with `forbidden` `Only the person answers this question.` and otherwise takes the same mark. The next `ask.wait` replies `{"state": "dismissed"}`.

Refusals, exact words. `bad_request` unless noted:

- `ask.open`: `Ask a question first.`; `That question is over 2,000 characters.`; `Give 2 to 6 choices.`; `A choice is empty or over 120 characters.`; `On a Worker, give choices (--choice): the answer comes from your Mac's Inbox.` (only on a node that is a Worker in its cluster); `key is needed`; `forbidden` `Notchling can't tell which session is asking.` (no Claude Code session in the parents, or no pid); `failed` `20 questions from this session are already waiting.`; `failed` `100 questions are already waiting.`; `failed` `Couldn't save the question.`. A refusal after the session is known saves nothing and records one `session.ask` event with `then: refused` and counts only; a refusal before it records the session as `unknown`.
- `ask.wait`: `id is needed`; `No such question.`; `forbidden` `Only the command that asked can wait for this answer.`.
- `ask.mine`: `forbidden` `Notchling can't tell which session is asking.`; `failed` `Couldn't read the questions.`.
- `ask.answer`: `id is needed`; `No such question.`; `Answer with a choice or text.` (neither, both, empty text, or a `choice` of 0); `That answer is over 10 KB.`; `No such choice.` (a number past the choices, or choices none); `forbidden` `Only the person answers this question.`; `failed` `Couldn't save the answer.`.
- `ask.dismiss`: `id is needed`; `No such question.`; `forbidden` `Only the person answers this question.`; `failed` `Couldn't save the answer.`.
- `ask.list`: `failed` `Couldn't save the answer.` when the store can't be read (the words are shared with the writes).
- Every gated op: `forbidden` [`ACTING_OFF`].

A row on the wire: `{id, session, sessionId?, asker, agentId?, label?, agentType?, model?, folder?, branch?, question, choices?, mainMayAnswer, mode, state, waiting, answer?, choice?, by?, createdMs, updatedMs, closedMs?}`. `asker` is `main`, `subagent`, `codex`, `antigravity` or `unknown`; `mode` is `background` or `wait`; `state` is `open`, `answered` (taken, not yet handed to the asker), `delivered` (the asker got it), `dismissed`, `expired` or `gone`. `session` is the session key (`Agent::outbox_key()`: the job's short id, else the row id; until a scan lists the session it is the Claude Code `sessionId`, and the next scan re-keys the rows). `waiting` says a waiter is connected. `answer` is on `ask.list` and the topic only (app tier), never on `ask.mine`. `label`, `agentType` and `model` are the agent's, from its transcript, and say who asked; they prove nothing.

Who asked (the finder, off the op thread, again on each file event in the session's transcript folder, for 10 s from the open; without a watcher every 500 ms): a delegated Codex or Antigravity job is a process on the walk whose executable is this `notchling` and whose command line is `notchling delegate codex` or `agy`; the row is `codex` or `antigravity`, labelled `Codex job · delegated by «name»` (the delegator found the same way) or just `Codex job`. Otherwise, over the session's transcript and its subagents' (written in the last 10 minutes, at most 64 files, each read from its last 256 KB): if the asking process's stdout is a regular file named `….output` and a transcript holds a Bash tool result whose background-launch path is that file (compared canonical), that transcript's agent asked; else the Bash `tool_use` whose command, split into simple commands at `&&`, `||`, `;`, `|` and newlines and into shell words, holds one whose words after the program equal the process's command line after its program (the program matched by the file name `notchling`; a `$`, a backtick or `<<` outside single quotes never matches; `tool_use` ids another open question holds are skipped; at most 10 minutes older than the open). One transcript matches: the main one makes the asker `main`, a subagent's makes it `subagent`. None, or more than one: `unknown` (after the 10 s). A found asker is saved and published as `ask.changed`; the `tool_use_id` is kept on the row, and for a delegated job it is empty.

Topic `asks` (app tier, so an agent never reads a question or an answer): `ask.changed {row}` after every insert or change (the row as above, with `answer`), and `ask.removed {id, session}` when the hourly sweep deletes a closed row (24 hours after `closedMs`, with its answer). A client reads `ask.list` once and follows the topic.

Timing. A 1 s tick checks every `open` or `answered` row: a blocking ask (`mode` `wait`) past its `deadlineMs` is `expired`; a row with no waiter connected whose asking process has ended (no such pid, or a start time that differs; with no start time saved, no pid and no parent either) is `gone`. A connected waiter is never checked. Restart: `AskOps::load` runs between bind and serving, after the Inbox's load; it records `restored` for every `open` or `answered` row and raises each open one in the Inbox again under a fresh id (the old copy is dropped), so the `ask` command's reconnect with the same id finds its question and an answer given meanwhile is handed over at its next `ask.wait`.

The Inbox. One item per open question, raised once the finder settles: kind `question`, id `ask:<question id>:<ms>`, title `<label> asks · <branch>` (the label is the agent's name, `This session` for the main session, `This session or one of its subagents` for `unknown`; cut to 80 characters), detail the question cut to 400 characters, no expiry. Actions in order: one per choice (`choice_1` to `choice_6`, the label cut to 60 characters, none primary), Answer in Chat (`open`, primary when there are no choices, holds `{ask, session}`), Dismiss (`cancel`). The item's responder only takes the mark and queues the write for the loop, so the Inbox never shows an answer that was not taken; it refuses while `acting` is off and `open` is refused from a phone, as for every item. An answer resolves the item Answered with the choice's action (none for typed text) and `answered_by`; a dismiss resolves it Answered with Dismiss; `expired` and `gone` withdraw it. The answer text is never in the item.

Events, audit and Activity carry `session.ask`: `session`, `then` (`asked`, `found`, `answered`, `delivered`, `dismissed`, `expired`, `gone`, `refused`, `restored`), `asker`, `by` (`mac`, `phone`, `main`, `none`), `mode`, and the counts `chars`, `choices`, `answer_chars` and `waited` (ms), never the question or the answer. `ask.wait`, `ask.mine` and `ask.list` are not quiet ops.

Command line (`ask_cli.rs`, `ask_cli_args.rs`, `ask_cli_run.rs`):

```
notchling ask [--choice <text>]... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] [--socket <path>] -- <question>
notchling asks [--wait] [--json] [--socket <path>]
notchling answer <id> (--choice <n> | -- <text>) [--socket <path>]
```

- `ask` makes a random `key`, retries `ask.open` for 60 s while the daemon is down, prints `Asked (<id>). Waiting for the answer.` on stderr, then calls `ask.wait` with `waitMs` 50,000 in a loop (a call times out at 60 s). If the daemon goes away it reconnects after 1, 2, 5 and 10 s, then every 10 s, and asks again with the same id. `--wait` sends `wait: true` and a `deadlineMs`: `--timeout` seconds (default 900) from now, and never later than `NOTCHLING_DELEGATE_DEADLINE_MS` minus 60 s when that variable is set (the Codex and Antigravity delegates set it); without `--wait` it waits without end. Answered: stdout `Answer: <text>` and `From: you (Mac)`, `From: you (phone)` or `From: the main session`, exit 0. `--json` prints `{"id","state","answer","choice","by"}` on stdout instead, for every closed state. Dismissed: stderr `No answer: the question was dismissed.`, exit 2. Expired or the deadline: `No answer in time.`, exit 124. `gone`: `The question is no longer waiting.`, exit 1. A refusal or no daemon (stderr: `Notchling isn't running. Ask the user to open it, then try again.`): exit 3. A usage error or anything else: exit 1.
- `asks` calls `ask.mine`; `--wait` loops on `waitMs` 50,000 until a question is listed. It prints each as `<id>  «label» (<asker>) · <branch> · <waited>` (`12s`, `3m`, `1h 5m`), the question, and `  <n>. <choice>` lines; `No questions waiting.` when none; `--json` prints the rows. Exit 3 with no daemon or on a refusal.
- `answer` prints `Answered.` (exit 0), `Already answered · Mac.`, `· Phone.` or `· Main session.` (exit 2), or `Already <state>.` when the question closed some other way (exit 2); a refusal or no daemon is exit 3.

## `awake_ops.rs`

What the menu bar cup shows: whether the daemon holds the Mac's power assertion now. App tier, read only,
no switch, `OpSpec::APP_QUIET` (a success is not written to `audit.jsonl`; the app asks once on each connect).
Additive: an older daemon answers it as an unknown op, and the app then shows the cup empty.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `awake.state` | app | `{}` | `{holding: "system" \| "display" \| null}` |

The daemon holds the assertion only while an app is attached (`awake_loop.rs`), so the reply is `null` with no
app, and for the first tick (2 s at most) after one attaches. Each change of the hold is an event on topic
`settings` (which agent-tier clients may follow too, like the other settings events): `awake.changed {holding}`
(taken, swapped between `system` and `display`, let go), sent only when the hold changes and never for a refused one. `engine::awake::state_json` and `holding_in` are the one
form of the reply and the event.

`wake_loop.rs` (continue agents after sleep, setting `continueAfterSleep`) has no op: it types into a cut-off
job through the same path as `session.reply` and so only while `acting` is on and an app is attached.

## `bridge_ops.rs`

Milestone 4's phone bridge in the daemon: the app-tier `bridge.*` ops over the engine's
[`Bridge`] (E5, TLS on the LAN port, the tunnel origin on loopback, the router).

The bridge is off at every start, whatever a file says: it binds no port and writes nothing
under the data folder (`AgentConsole`) until the app's `bridge.set {"on": true}` in this run
(decision 0007's note: files at rest are not a tier boundary). While the Swift app still runs
its own bridge it holds both ports, so turning this one on then fails with a busy port and says
so (decision 0011: one owner).

## `catalog_ops.rs`

The model catalog in the daemon (decision 0016, phase 1): one [`CatalogService`] owns the
engine's `Catalog` (the data file with the user's `catalog.json` laid over it) and the live
lists, and serves `catalog.get`. Agent tier, read only, not switched: it reads Codex's cache
file and the daemon's own cache, and the one thing it writes is that cache.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `catalog.get` | agent | `{tool: "claude" \| "codex" \| "antigravity"}` | see below |
| `catalog.roles` | app (quiet) | `{}` | `{roles, override}`, see below |

| Event | Topic | Body | When |
| --- | --- | --- | --- |
| `catalog.changed` | `tools` | `{tool}` | a fetch of Claude's list lands, or a loaded `agy acp` session reports options that differ from the cached ones |

```json
{"tool": "claude", "fetchedAt": 1794150245.0,
 "models": [{"id": "claude-opus-5-5", "name": "Opus 5.5", "tier": "current",
             "contextWindow": 400000, "efforts": ["low", "medium", "high", "xhigh", "max"],
             "defaultEffort": null, "price": {"input": 4.0, "output": 20.0, "cacheRead": 0.2},
             "borrowed": false}]}
```

- A `borrowed` model also has `borrowedFrom`, the known model whose price and window it uses.
- An unknown or missing `tool` is `bad_request`.
- Where each list comes from (the spec's table): Claude's is Anthropic's `GET /v1/models`, read
  with the main login's token and cached for a day; while none has ever been read (no token,
  or every fetch failed), Claude Code's own `additionalModelOptionsCache` in its main
  `~/.claude.json` (read only, re-read when its mtime changes); else the data file's. Codex's
  is the `models_cache.json` Codex keeps (re-read when its mtime changes); Antigravity's is the
  Model and Thinking Level options the last loaded `agy acp` session reported, cached.
  `fetchedAt` is when that list was fetched or seen (the file's mtime for Codex's and Claude
  Code's), `null` for the data file's list.
- A call for Claude when its list is due (a day old, or none) starts a fetch on a thread (one at
  a time) and answers from what it has; the fetch ends as `catalog.changed {tool}` on `tools`.
  It runs only while `accounts` is on, with the main login's access token read from Keychain by
  [`ClaudeAccounts::live_token`], which only reads the live login, never renews anything and
  answers nothing for an expired one. A failed fetch keeps the old list, and a try that found
  no token or failed is not repeated for [`RETRY_SECS`]. After the endpoint refused a login
  (400, 403 or 404; a 401 or 429 passes with time) the same token is not sent again for
  [`REFUSED_RETRY_SECS`] (a day), told by its SHA-256 fingerprint, never kept itself; a new
  live token is sent at the next try. The first failure of a run is logged
  to standard error by its status alone, never its body or the token. Chat asks at start, every hour and when
  a Claude picker menu opens, so the list is fetched about once a day while Chat runs.
- The user's `catalog.json` (beside the socket) fixes prices, windows and names, entry by
  entry; it never adds a model to what a session may start on. It is read once, when the
  daemon starts, so an edit applies at the next start (and the cost cache then reprices the
  models whose price changed). Its shape is the data file's, per tool (`claude`, `codex`,
  `antigravity`): `prices` (a list of `{"match": "fable-5-3", "input": 9.0, "output": 45.0,
  "cache_read": 0.3}` in dollars per million tokens; a Claude key may be bare or the full
  `claude-` id, another tool's is its whole id), `windows` and `names` (objects keyed by the
  full model id). An entry that doesn't parse, or a negative price, is skipped alone.
- Codex and Antigravity models also carry roles for `notchling delegate-models --for <role>`: `picks` (`{"model": "gemini-4-hydrogen", "effort": "high", "roles": ["review"], "use": "one line"}`) and `not_for` (`{"match": "flash", "roles": ["review"]}` or `{"effort": "ultra", "roles": ["review"]}`). In the override file a pick replaces the compiled-in ones for the same model id (empty `roles` removes them) and may name a model only the live list reports; a `not_for` entry replaces the one with the same `match` and `effort` (empty `roles` removes it). A role never lets a session start on a model the allowlist refuses; it only names one. The command line (`delegate-models`, no daemon) reads the same file; `--for` reads the data file, then the override, then the live list: a marked model that is gone gives way to the provider's own replacement (Codex's `upgrade`) or the newest live model of its family (the codename word, the number compared numerically: `gemini-4-hydrogen` to `gemini-4.1-hydrogen`), said in a `note:` line; a newer family model while the marked one is live is a note only. A `not_for` entry may name a `match`, an `effort` or both; a model or effort it excludes is never printed by `--for`, not even as a provider hint or family successor. `--for` takes `--for <role>` or `--for=<role>` and nothing else (an empty role or an extra argument exits 2); with `--json` it prints `{"role", "picks": [{"tool", "model", "effort"}], "notes", "error"?}`. A pick with empty `roles` in the override needs no `effort`.
- Codex and Antigravity models also carry roles for `notchling delegate-models --for <role>`: `picks` (`{"model": "gemini-4-hydrogen", "effort": "high", "roles": ["review"], "use": "one line"}`) and `not_for` (`{"match": "flash", "roles": ["review"]}` or `{"effort": "ultra", "roles": ["review"]}`). In the override file a pick replaces the compiled-in ones for the same model id (empty `roles` removes them) and may name a model only the live list reports; a `not_for` entry replaces the one with the same `match` and `effort` (empty `roles` removes it). A role never lets a session start on a model the allowlist refuses; it only names one. The command line (`delegate-models`, no daemon) reads the same file; `--for` reads the data file, then the override, then the live list: a marked model that is gone gives way to the provider's own replacement (Codex's `upgrade`) or the newest live model of its family (the codename word, the number compared numerically: `gemini-4-hydrogen` to `gemini-4.1-hydrogen`), said in a `note:` line; a newer family model while the marked one is live is a note only.
- `catalog.roles` is what Settings > Skills & Models shows under Models by Job: the choice `notchling
  delegate-models --for <role>` makes for each role, with where it came from. It reads the user's
  `catalog.json` afresh on every call (so an edit shows at once, unlike prices and windows, which
  are read at daemon start), the data file, Codex's `models_cache.json` (with its `upgrade` hints) and
  Antigravity's cached list. Read only, app tier, quiet.

  ```json
  {"roles": [{"role": "review", "choices": [
      {"tool": "antigravity", "marked": "gemini-4-hydrogen", "model": "gemini-4.1-hydrogen",
       "effort": "high", "source": "live", "note": "gemini-4-hydrogen is gone; using gemini-4.1-hydrogen (same family, newer)",
       "advice": "Review after each task. ...", "checked": true}]}],
   "override": {"path": "/…/Notchling/catalog.json", "state": "missing" | "found" | "unreadable"}}
  ```

  `roles` lists `review`, `review-final`, `review-secrets` and `spec-check` first, then any other
  role the catalog marks; a role with no pick has `choices: []`. `source` is `override` when the pick
  is the user's own entry, `live` when the marked model is gone and its successor stands in, else
  `built_in`. `model` is `null` when nothing live is in the marked model's family. `checked` is
  false while the tool's live list is unknown (Antigravity's, until a session reported it): `model`
  is then the marked one, not verified, or `null` with a `note` when the catalog says that model (or effort) is not for the role. `note` is English text from the engine, the same as the
  command line's `note:` line. `override.state` is `unreadable` for a file the loaders skip (not
  JSON, or not an object).
- The same catalog is read elsewhere in the daemon: `cost.summary`'s prices (`cost_ops`), the
  `session.new --model` check (`control_ops`, through [`CatalogService::is_allowed`]: the
  built-in models plus the live lists, a Claude id in the shape `[a-z][a-z0-9.-]*` with an
  optional `[1m]`), and the phone's `models` list and `modelId` (`bridge_backend`). A `/model`
  sent through `session.command` isn't checked against it yet.

## `models_ops.rs`

`notchling models` and Settings > Skills & Models > Refresh Models: read each tool's own model list, report what changed, and (for `models.update`) write Notchling's copy.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `models.check` | agent | `{skills?, force?}` | the report, nothing of Notchling's written |
| `models.update` | agent | `{skills?, force?}` | the report, after writing |

- Sources, read only, no login and no token refresh: Claude Code's list from Anthropic's `GET /v1/models` with the live login's access token (only while `accounts` is on; asked at most once a minute and through the same refused-login gate as the background fetch, so an agent can't loop api.anthropic.com; an empty answer is an error, "Anthropic listed no models"; otherwise the report says it was not read and the lists Notchling has stay); Codex's from `codex debug models` (Codex's own catalog; it may refresh Codex's own `models_cache.json` and may renew Codex's login, so it is not run while a Codex session is live, whose rollout changed in the last 30 minutes or has an open turn, and the report says so); Antigravity's from `agy models`, whose id-shaped lines (`[a-z0-9][a-z0-9._-]*`) are `<model>-<level>`, so a model's efforts are the levels listed for it (`xhigh` only where listed); a model listed at a single level is keyed by that whole id. The tools are found again on each call. Output is read through a process group with a deadline and a 4 MB cap. A tool that can't be run, lists nothing, or lists less than half of the models Notchling had (3 or more) keeps its old list and the report says why; `force` accepts the shrink.
- `update` writes: Claude's and Antigravity's lists into `live-models.json` (`catalog.changed {tool}` when they differ; a loaded `agy acp` session keeps the per-model efforts the listing gave), and Codex's, when it differs, as a trimmed `codex-models.json` beside it (same shape as `models_cache.json`; whichever of the two files is newer is Codex's list for `catalog.get`, `delegate-models` and the delegate checks). It never writes `~/.codex`, `~/.gemini` or `~/.claude`, the override file or a price, and never changes a role pick.
- `skills` (default true): after `update` from an app-tier caller, the `notchling-delegate` and `notchling-models` skills are rewritten through `install.apply {what}` (so only while the `install` switch is on and only skills Notchling wrote); an agent-tier caller gets "skills not rewritten (needs the app)"; `check` and an off switch only say what would change. A bad `skills` or `force` is `bad_request`.
- Reply: `{applied, changed, tools: [{tool, source, read, note, total, first, added, removed, efforts: [{model, added, removed}], unpriced, written}], suggestions: [text], skills: [text], priceHelp, text}`. `suggestions` are role picks to review (a marked model that is gone or has a newer one in its family, or no longer offers the marked effort) and are never applied; `unpriced` are models with no price in the data file or the override; `text` is the report as the command line prints it.
- `notchling models [check] [--json]` and `notchling models update [--json] [--no-skills]` call these; `--force` passes `force`; exit 0 report made, 1 refused or failed, 2 not running or a wrong command. A list read for the first time (`first`) has no diff to show; it says so. The app waits up to 120 s.

## `cost_profile_ops.rs`

The Cost Profile's Set Up ([decision XXXX](decisions/XXXX-cost-profile.md)): one table of models and efforts, planned, applied, undone and checked per tool. Never `cost.report`; that name and `crates/engine/src/cost/` belong to the spend report.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `cost.profile.get` | agent | none | `{profile}` |
| `cost.profile.set` | app | `{preset?, picks?, habits?, tools?, guards?, reset?}` | `{profile}` after saving |
| `cost.setup.plan` | agent | `{consent?, wait?}` | `{on, plan}`; writes nothing |
| `cost.setup.apply` | app | `{consent?, wait?}` | `{on, applied, failed, results, plan}`; with `install` off, the plan and `message` |
| `cost.setup.undo` | app | `{tool}` | `{on, tool, results, conflicts}` |
| `cost.setup.check` | agent | `{resolve?: {id, choice: keep or restore}}` | `{on, rows, drift}`; `resolve` is app tier and needs `install` on |

- `profile`: `{preset, custom, tools, picks, habits, table, presets, floors}`; an app-tier caller also gets `guards`, `imports` (one `{source, kept, report}` per rule file read, the lines taken from it, each with where it went) and `imported` (the first of them, or `null`). Each hand-written rule file has its own action id, `claude.rules.replaced.<file>`, and a pointer file its path under `~/.claude`, `claude.pointer.<path>` (`claude.pointer.agents/CLAUDE.md`). `table` is `{role: {tool: {model, effort, own, notes} or null}}` for the seven roles and `claude`, `codex`, `agy`; `notes` say a floor raised a pick, a family moved, an effort clamped or a list was unchecked. `presets` is the same table for each preset.
- `set`: `picks` is `{"role.tool": "model effort" or null}`, `habits` `{name: number or null}` (each name has a range; outside it is refused), `tools` `{claude, codex, agy: bool}`, `guards` `{name: level or null}`, `reset` clears picks and habit edits. Everything else is refused, and the preset must be one of `saver`, `balanced`, `quality`.
- `consent` names parts of files the person owns that Set Up may change: `rules` (replace a hand-written model rule), `pstack`, `orchestrator`, `orchestrator_new`, `codex_agents`, `codex_config`. A part without its consent shows as `offer` with its diff and is not written. `wait` turns on the rule's `notchling wait` lines (off until that command ships).
- A plan action is `{id, tool, path, part: file or keys or block, kind: create, update, remove, unchanged, skip or offer, state, needs, reason, diff, keys, notes}`. To an agent, the `diff` of a `keys` action (`settings.json`, Codex's `config.toml`) is empty: it gets the changed keys and their before and after only.
- `apply` writes the record of a change to `daemon/cost-setup.json` before it makes the change (and a record it cannot save stops everything unchanged), makes a backup of every file it changes (`<file>.notchling-setup-<stamp>`, never replacing an existing file; a backup the record names is never pruned) and refuses a path with a link in it; the old rule files are removed only when their replacement was written; a damaged record is an error, not an empty one; a failure part way stops, keeps what was done and answers `failed` with the results as error data. `undo` per tool puts back each file, key and block from the manifest and reports a `conflict` for anything changed since, leaving it.
- `check` rows: `{id, tool, path, state: in_step, outdated, edited, missing, changed_by_tool, returned or skipped, detail}`.
- `notchling cost setup [--apply] [--consent a,b] [--wait] [--json]`, `notchling cost check [--json]` and `notchling cost undo <claude|codex|agy> [--json]` call these. Setup is a dry run without `--apply`. The command line is agent tier, so `--apply` and `undo` are refused with "Only the Notchling app writes these files"; exit 0 done, 1 refused or failed, 2 not running or a wrong command.

## `cluster_ops.rs`

Cluster plan B ([plan](superpowers/plans/2026-11-13-cluster-v1-b-nodes.md)): this machine as a node of one
cluster over pinned TLS 1.3 on port 47823 (Tailscale addresses only). The `cluster` switch is off at every
start: no cluster port is bound and nothing is dialed until `settings.set_switch {"switch": "cluster", "on": true}`
from the app in this run, or `cluster.join` (below), or a restart of a headless member (below).

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `cluster.status` | agent | `{}` | `{cluster: {id, name, term, home, vault} \| null, joining: {since, cluster, primary, answered} \| null, me, members: [Member], jobs: [Job], main: [session id], kept: [session id], claims: [Claim], sideCopies: [SideCopy], waiting: [{session, node, name, title}], link, listening, tailscale, error}`; `jobs` is the 50 newest dispatched jobs, `[]` with no cluster, each `{id, node, tool, cwd, state, session, qualified, detail, createdMs}` |
| `cluster.create` | app | `{name}` | `{cluster: <id>}`; this node is its home (primary) and reference, and `cluster.status` has the rest |
| `cluster.join.token` | app | `{}` | `{token: "nlj1.…"}`, one use, 10 minutes, at most 16 open at once (the oldest is dropped); a non-home member asks its home, which records that member as the maker. Only the app may ask (maintainer 2026-11-16): a terminal or agent is refused, so there is no `notchling cluster token`, and the home answers a forwarded call only from a client member at app origin |
| `cluster.invite` | app | `{node, name, canBeMaster?, acting?}` | `{token: "nlj1.…", expiresMs}`: a join token bound to one machine. `node` is the machine's 16 character id, `name` is checked like a machine name (`forms::name_problem`), `canBeMaster` and `acting` default to false. One use, 10 minutes, and it shares the join token caps (4 open per maker, 16 in all; the oldest is dropped). A node whose id differs from `node` that presents it is refused ("this token was made for another machine"). The machine that presents it joins with no Add click: the primary approves it, sets `canBeMaster` and `desired.acting` from the invite, and raises the inbox notice `invite-joined:<node>:<ms>` ("<name> joined the cluster", detail "Added over SSH from <maker> · <os> <arch> · <node>"); a node already Pending from an ordinary token stays Pending and still needs Add. Only the app may ask: a terminal or agent is refused (`forbidden`, "only the Notchling app makes invites"), so there is no CLI for it. A non-primary Mac forwards the call to the primary with app origin; the primary answers a forwarded call only from a client member at app origin, which names who asked as `minter` (the maker the invite is counted against), and refuses a non-member `minter` ("only a member makes invites") and any call that reaches it as a non-primary ("only the primary makes invites"). Other refusals: `bad_request` for a node that isn't 16 hex characters, a bad name or a non-boolean flag, `failed` ("this node is in no cluster") outside a cluster. The token carries the primary's addresses, port and certificate fingerprint like `cluster.join.token`, and the secret is in the reply only |
| `cluster.join` | agent | `{token}` | `{state: "pending" \| "joined"}` |
| `cluster.member.set` | app | `{node, canBeMaster?, client?, reference?, acceptsDelegation?, headless?, desired?}` | `{}`; `cluster.status` shows the member as stored |
| `cluster.remove` | app | `{node}` | `{}`; the member shows `state: "removed"` in `cluster.status` |
| `cluster.failover.set` | app | `{value: "off" \| "30m" \| "1h"}` | `{value}`; journaled on the primary, a client member's call is forwarded there |
| `cluster.main.set` | app | `{id, main}` | `{}`; `main: false` is "Keep on <machine>", `true` undoes it; the primary applies it (a client forwards it), the id is at most 256 bytes with no control characters |
| `cluster.move_home` | app | `{to}` | `{started: true, moving: [session id], staying: [[session id, reason]]}` once the check passes; the primary runs the handover (a client forwards it) and refuses with the reason when `to` isn't a ready, online Can Be Master member that passes the promote check, or while the resume proof isn't recorded (`"Moving the primary needs the resume proof first"`). The other refusals: `"A move is already running"`, `"This machine is in no cluster"`, `"<name> is primary already"`, `"That machine isn't in this cluster"`, `"<name> can't be primary: Can Be Master is off"`, `"<name> is offline"`, `"<name> isn't ready: <first failing check>"`, the promote check's own text, and `"Couldn't start the move"`. The args must be exactly `{to}`. The rest runs in the background and ends in an inbox `notice` (`"<to> is primary now"` or `"Couldn't move the primary: <reason>"`) |
| `cluster.move.now` | app | `{}` | `{ok: true}`; ends the wait for turn ends: the working sessions that are moving are stopped mid-turn and named in `cluster.status` `move.cut`; refused (`"No move is running"`) when no handover runs |
| `cluster.promote` | app (the phone with acting and the bridge on; never an agent) | `{node, confirmed?}` | Makes the named Standby the primary when the old primary can't be reached; forwarded to `node` (an App-tier origin travels with it) and run there. `{relayed: true, text}` when the primary, or a peer, still reaches the old primary (nothing changes), `{needsConfirm: <risk text>}` when it can't be reached and `confirmed` is not true (nothing changes), `{step}` once the promote runs (it goes on in the background; `cluster.status` `promote` carries the rest), or an error. |
| `cluster.take_back` | app (the phone with acting and the bridge on; never an agent) | `{}` | Takes the primary back to this machine, which was the primary and was replaced. Each of this machine's side copies that lies outside `<data>/cluster/copies/<session>/<set>/` is moved there first (never overwritten, never deleted; the old path is journaled as the copy's new path), then this machine asks the primary to move home here (`cluster.move_home {to}` forwarded with an App-tier origin) and answers with that answer. Refused with `"This machine is already the primary"`, `"This machine is in no cluster"` or `"<path> already holds a copy, so <path> was not moved"`; any failure stops the take back and the files already moved stay moved |
| `cluster.continue_here` | app (the phone with acting and the bridge on; never an agent) | `{id}` | `{id, claimed: true}`: this machine claims a session that waits for another machine (a new claim, `n + 1`, journaled at the primary), its Waiting gate goes and its `session_waits` item closes. `"That session isn't waiting"` when `waiting` doesn't list it; a plain error (`"Couldn't claim <id> on this machine: <reason>"`) when the primary can't be reached, and the session keeps waiting. The app and the phone confirm before they call it |
| `cluster.side_copy.hide` | app | `{session, node}` | `{hidden: true}`; hides the side copy from lists (recorded at the primary with an App-tier origin). Nothing on disk changes |
| `cluster.follow` | app | `{id: "<node>/<session>"}` | `{following: true, id, until}` (`until` in ms; see Remote sessions) |
| `cluster.dispatch` | agent (the app for anything but a delegate; see Dispatch) | `{node, key, tool: "claude" \| "codex" \| "agy" \| "shell", cwd, model?, effort?, text?, command?, code?: "folder" \| "send", secrets?}` or `{node, key, delegate: [args]}` | `{job, state, session, qualified, warning}`; a delegate adds `{exit, out, err}` |
| `shell.stop` | app | `{id: "shell:<job>"}`, or `"<node>/shell:<job>"` for a job on another node | `{stopped: bool}` |
| `cluster.return` | app | `{job}` | `{job, branch: "notchling/<node name slug>/<job>", node: <the node's name>}` |
| `cluster.allow.set` | app | `{node, names?, any?}` | `{}`; `names` replaces the vault secrets a job on that machine may carry, `any` allows every name; a field left out stays as it was; relayed to the home from a client |

- `cluster.status` carries the record of sessions that moved (`cluster.record` writes it at the primary; it rides in every `Members` push, so a Standby and a client list it too): `claims` is `[{session, node, n, term, base_set, base_len, touched}]`, one per moved session, the machine that owns it now and its counter; `sideCopies` is `[{session, node, base_set, extra_bytes, repo_changed, path, at_ms, hidden}]`, work a machine did after the set its successor resumed from, never deleted or merged (a hidden copy stays in the list with `hidden: true`; `cluster.side_copy.hide` is the only way to set it and nothing unhides it); `waiting` is `[{session, node, name, title}]`, this machine's sessions that wait for the machine `node` (named `name`). Nothing writes `waiting` in production yet: the return-with-the-primary-off detection of Task 7 isn't built. Claims and side copies are never pruned.
- `cluster.status` `failover.takesOverAt` is always `null`: automatic failover isn't in v1 (the maintainer decided on 2026-11-16), so there is no lease and no timed takeover. The app offers no choice for it and never sends `cluster.failover.set`; the op and the `clusterFailover` value remain and act on nothing.
- The phone's bridge ops `cluster.machines {}` and `cluster.promote {node, confirmed?}` (protocol `agentconsole.v1`, see `protocol/README.md`) aren't local API ops: the bridge answers them through the daemon (`bridge_machines.rs`). `cluster.machines` maps `cluster.status` to `{machines: [{id, name, online, canBeMaster, ready, setAt?, canResume}]}` (every active member, online or not; `setAt` only where the machine it reached knows it); `cluster.promote` needs the phone's `acting` switch, is audited (`cluster.promote` and `cluster.promote:refused:<reason>` rows), is never held or relayed, and returns the daemon's `cluster.promote` answer as `{step?, relayed?, text?, needsConfirm?}`.
- `cluster.status` carries `promote`, the running or last promote's state (`{node, step, text?, error?}`, `null` when none ran since the daemon started; steps include `relayed`, `confirm`, `failed` and the engine's promote steps) and `failover.takesOverAt` (`null` until the timed takeover exists; always `null` today). Items: on a Standby, `primary-unreachable:<term>:<since>` (60 s without the primary or a peer reaching it; `<since>` is the outage's start on the boot clock in ms, so a second outage in the same term raises a new item; `Move to <name> Now`, then `primary-unreachable:<term>:<since>:confirm` with `Move Anyway` when the risk text is needed), on a client that can't be master `no-primary:<term>:<since>` (2 min), `failover:<term>` after a promote, `paused-step:<term>:<session>:<k>` (Continue or Done) for a session whose step was cut by the primary's loss, and `paused-step-failed:<term>:<session>` when Continue couldn't be sent to the session. A watcher withdraws its own open items of an earlier outage, also after a daemon restart. Answering Move, Move Anyway or Continue needs acting on.
- `cluster.status` carries `move`, the last handover's state (kept in `move-saga.json`, so it stays after the move ends and after a restart, until the next move starts; `null` when there was none): `{to, step, moving, staying: [[id, reason]], cut, stopped, failed: [[id, why]], unfenced, reason}` (`unfenced` is `null`, or why the old primary couldn't pause the moved sessions after the switch: they may then run on both machines). `cluster.changed` events carry no `move`: read `cluster.status`, as the app does every second while a move runs. `step` is `prepare`, `catch_up`, `freeze`, `final_snapshot`, `stop_old`, `switch`, `resume`, `announce`, `done` or `undone` (then `reason` says why). From `freeze` until the move ends the primary refuses `session.reply`, `session.answer`, `session.permission` and `codex.send` for a moving session with `"That session is moving to <machine>"`. From `final_snapshot` until it ends the primary is fenced: `session.new`, `session.move`, `cluster.dispatch`, `cluster.main.set` and `cluster.return` answer `"The primary is moving to <machine>"`, and no snapshot but the final one is taken (a turn end, the timer or a newly ready Standby waits until the move ends). Inbox items raised on the old primary in that window stay there and are named in the announce notice's detail.
- Node-only ops of the handover (Node tier, sent by the primary, answered by the machine that hosts or takes the sessions; none is an app or agent op): `move.freeze {sessions, to}`, `move.stop {sessions}` (`claude stop` or `codex.stop`; a successful `claude stop` counts as stopped, the session list isn't read again, R110), `move.switch {id, name, term, vault, members, sessions, set}` (the new primary checks the cluster, the term and its own membership, that it holds the final set `set` complete and that Claude Code trusts each moving session's folder, all before anything changes; then restores into a staging folder and takes term `term`, swapping the database in right before the takeover persists; any refusal answers code `refused`, a handover already running there answers `"A handover is in progress here"`, and a repeat for a term it already holds answers Ok), `move.resume {sessions: [id]}` (only from the machine that handed over, only while the receiver still holds that term; starts Claude Code sessions with `claude --bg --resume` and their recorded flags, skips ones already running or already started by an earlier try, and a repeat that arrives while one still runs waits for it; sends no message), `move.undo {sessions}` (unfreezes and restarts the stopped sessions).

- `joining` (additive) is `null` unless this node's own `cluster.join` is in progress, then `{since, cluster,
  primary, answered}`: `since` is the wall-clock ms when the join began, `cluster` the id from the token,
  `primary` the node id of the home the token names, `answered` `false` until that home says the join waits for
  approval, then `true`. `cluster` is `null` meanwhile; `joining` is back to `null` when the home approves,
  denies or doesn't answer. The token itself is never in it. `cluster.changed` carries the same body, so it
  fires when a join starts and when the home answers.
- `Member` is `{id, name, role, canBeMaster, online, client, headless, reference, state, versions, load, desired,
  acceptsDelegation}`; `role` is `home`, `standby` or `worker`, `state` `pending`, `active` or `removed`,
  `load` `{cpu, memUsedMb, memTotalMb}` or `null`, `desired` `{acting, bridge, install}`.
- `Member.versions` may carry `host` `{encrypted: bool \| null, home, user, free_mb}`, the machine's own report of
  its disk (FileVault or LUKS, read-only, at most every 10 minutes by a background thread, free space rounded down to
  256 MB); a node that hasn't reported yet, or reports an empty `home` or `user`, has no usable `host` and is not ready.
- `Member.versions` may also carry `bridge` `{port, fingerprint?}`, the node's own phone bridge: its LAN port (the
  configured port while the bridge is off) and the SHA-256 fingerprint of the certificate it serves (left out while
  the bridge is off or failed). The bridge's `hello` builds `cluster.nodes` from it (see `protocol/README.md`); a node
  with no `bridge` is listed with no addresses and an empty fingerprint, and one with no `fingerprint` with an empty
  fingerprint; a phone dials neither.
- `cluster.status` adds `ready` `{ok, reason, checks: [{id, ok, text}]}` to every active Can Be Master member (`id` is
  `encrypted`, `home`, `space` or `tools`; `reason` is the first failing check's text, or `null`). `cluster.changed`
  events carry the status without `ready`: read `cluster.status` for it. The space check needs twice the newest set
  and 5 GB, and the set's size comes from the node that answers: on the primary the newest set it staged (before
  any, the largest total of transcript lengths a Standby acknowledged, else 0); on a Standby the size of its own
  current set from the primary (the sum of the set's part lengths, 0 before one). So a Standby's own status and the
  primary's can differ on `space` while the sets are changing; the primary's figure is the one that decides whether
  a set is sent.
- `cluster.status` adds `replication` (additive; `[]` on a machine that isn't the primary and before the daemon
  has a sender): one `{node, setAgeS, sizeMb, workedAgeS}` per active Can Be Master member other than the primary.
  `setAgeS` is the seconds, at the reply, since the set that machine last acknowledged was made, `sizeMb` the size
  of what was sent for it (its tails, rounded up to MB), both `null` before any set arrived; `workedAgeS` is the
  seconds since the primary last saw a main session working (`null` if never since the daemon started). The
  app marks a machine behind when `setAgeS` is 900 or more and `workedAgeS` is smaller (a main session worked
  after that set). `cluster.changed` events carry the status without it: read `cluster.status`.
- `cluster.join` refuses `"this node is already in a cluster"`. With the switch off it turns the switch on
  when the app asks, or an agent while no app is attached (a headless machine), recorded
  as a `switch.changed` event (switch `cluster`, cause `user` or `cluster_join`); an agent's join while an app is
  attached is refused with the off message. An agent's join while no app is attached is a headless join: the
  joiner says so in its `Join`, the home records the member `headless` and the inbox item's detail says
  "Headless".
- At daemon start, a node whose `cluster.db` says it is an active, headless, non-client member of a cluster
  whose home is another active member turns the `cluster` switch on by itself (no app is attached then) and
  dials its home, recorded as a `switch.changed` event (cause `cluster_rejoin`) and a `cluster.step` event
  (`rejoined`, with the home as `peer`), and published as `settings.switch {switch: "cluster", on: true}`.
  A node in no cluster, the home itself, a member joined from the app, a client, or one with an app already
  attached stays off. Its other switches still come only from the home's `Members`/`HelloOk`; `vault` and
  `accounts` never follow. The home's app clears the mark with `cluster.member.set {node, headless: false}`.
- A join on the home raises the inbox item `node-join:<node>:<ms>`, kind `node_join`, actions Add (`allow`)
  and Deny; only `mac` or `phone` may allow it. Unanswered for 10 minutes it expires and the joiner is denied.
  The detail names who made the token ("Token made by <name>"), and Can Be Master defaults on for a Mac only
  when the maker is the primary or a member that has Can Be Master on.
- `notchling cluster join --token-stdin` reads the token from stdin only; a token among the arguments is
  refused.
- `cluster.status` on the primary (only there) adds `invites`: `[{node, name, expiresMs}]`, the open invites, oldest first, with no token and no maker. It is absent on any other member. `cluster.changed` carries it too. The audit has `cluster.step` events `invited` (`peer` is the maker: the primary for its own app, else the forwarding client) and `invite_joined` (`peer` is the machine that joined); neither holds a token (S8).
- `cluster.status` members carry `addrs` (the member's Tailscale addresses, as strings) and `port` (its cluster port), additive. They are not secrets (every join token carries them), so every caller of `cluster.status`, agents included, and every `cluster.changed` event sees them. The app reads the primary's address and port from them for the Reaches the Primary check. `versions.build` (a member's `versions`) is the build id of that node's binary (7 to 40 hex characters, with `-dirty` when the tree was), or absent when the binary was built without `NOTCHLING_BUILD`; `notchling --version` prints it as `notchling <version> (<build>)`. The app offers Update N Machines when two builds are known and differ.
- `notchling cluster reach <addr> <port>` (a new machine's check that it reaches the primary) opens one TCP connection with a 3 s timeout and says nothing else: `addr` must be an IPv4 address in 100.64.0.0/10 (loopback too only for a cargo-target build with `NOTCHLING_CLUSTER_LOOPBACK`), `port` 1 to 65535. Exit 0 and `reached <addr>:<port>` on stdout when it connects; exit 1 and a line on stderr (`refused`, `no answer in 3 s` or `no connection: <why>`) when it doesn't; exit 2 for a usage error or an address that isn't a Tailscale one. It never talks to the daemon, and `--json` and `--socket` are refused with the usage text.
- Join tokens and invites come only from the app. `cluster.join.token` and `cluster.invite` are app tier, and the CLI has no command for either (`notchling cluster token` was removed); the Add Machine flow's join Fix sends the token to the new machine on the stdin of one ssh call, never in an argument, an environment variable, a log or the audit.
- Status carries `failover: {value, armed, target}`: the primary's journaled value, whether the target has granted a lease at the
  current term, and the target's node id (or `null`). The settings key `clusterFailover` (class Ask) mirrors `value`: the journal on
  the primary wins, so an allowed agent `settings.set` is journaled first and fails (key unchanged) when the primary can't be reached.
- The failover value is set by `cluster.failover.set` (above; the app doesn't send it in v1) or an allowed `settings.set` of `clusterFailover`. The primary
  refuses the call from a member that isn't marked client (`client_home`).
- Topic `cluster` (agent tier): `cluster.changed` with the status body after a change other than load, and
  `cluster.refused {from, op, reason}` (also a `cluster.step` event, `refused`).
- Calls from other nodes run through the same ops as `ClientKind::Node` with the socket's checks, recorded
  as a `cluster.step` event (`call`, the op and the origin node as `peer`). `node.switch.set {switch, on}` from the home sets `acting`,
  `bridge` or `install` on a node with no app attached, recorded as a `switch.changed` event (cause `cluster_node`).
- Prompts on other nodes. A client node that has an app attached (`Room::can_answer`) sends every online linked
  member `node.follow {topic: "prompts"}` (a daemon const, not a wire.rs op; policy `client_plain`) when the link
  comes up and then every 30 s; the owner leases it for 60 s (`FOLLOW_LEASE_MS`) and answers
  `{following: true, topic, leaseMs}`. It stops when the app detaches or the node stops being a client, so the
  leases lapse within 60 s; a Worker never sends it. While a node has an active lease, it also sends each
  hook-prompt `prompt.pending` and `prompt.resolved` of its own (topic `prompts`, the bodies below) to those
  nodes only (and only those that are still active client members when the cluster driver sends it), as peer events
  (`Msg::Event`); a `prompt.pending` is first trimmed with the rule `prompts.list` uses (a row over 128 KB gets
  its `input` cut to `{}` and its text to 4000 characters and `trimmed: true`; one still over 128 KB is not
  sent); agy prompts (they carry `tool`) and prompts that are themselves
  remote (their `id` has a `/`) are never sent on. The receiving core publishes a `prompts` peer event only
  when this node is a client and the sender an active member, with `from` set to the real sender.
- The client republishes them on its own local `prompts` topic (app tier) with qualified ids (`<node>/<id>`,
  decision 0012). `prompt.pending` is `{id: "<node>/<prompt>", sessionId: "<node>/<session>" or "", kind:
  "permission" | "question", toolName, summary, input, canAlwaysAllow: false, question, cwd, transcriptPath,
  createdMs, node: "<node id>", trimmed?: true, rowId?: "<node>/<row id>"}`: `rowId` (additive, left out when
  unknown) is the qualified id of the owner's session row, from the remote rows this client holds, whose
  transcript path equals the prompt's owner path or whose id or session id equals the prompt's `sessionId`
  (the app ties a prompt to its row by it; it works for rows that aren't followed); `trimmed` says the
  owner or this client cut `input`; `transcriptPath` is the mirror file of the session whose owner path the
  prompt named when a mirror exists, else `null`, never the owner's path; `canAlwaysAllow` is always `false`
  (below). `prompt.resolved` is `{id, sessionId, outcome, by, node, rowId?}` (`rowId` as the pending row had it). Everything from the peer is validated: ids
  are bare (no `/`, no control characters, at most 128 bytes), `kind` is `permission` or `question`, text is cut
  at 4096 characters, an `input` over 64 KB becomes `{}` (with `trimmed: true`) and a `question` over 128 KB, the owner's own row
  limit, drops the event, unknown
  fields are dropped, at most 50 prompts per node and 200 in all. The shaped row then gets the whole-row rule of
  `prompts.list` (over 128 KB: trimmed as above; still over: not published), and the client holds remote rows
  only up to 384 KiB in all (a new row past that is not published); the app's `prompts.list` fits local rows
  into 768 KiB minus what is held and then lists every held remote row, so every published row is listable.
  Each `prompt.pending` the client publishes is followed by its `prompt.resolved`, never the other way round. Each renewal (every 30 s, and the
  first one when the link comes up) the client also reads the owner's `prompts.list` (the same op, called as a
  node, so it never carries remote prompts), adds what it lacks and resolves (`by: "unlisted"`) what it held
  before the call that the owner no longer lists, so a lost `prompt.resolved` is repaired (skipped when the
  owner's rows sum to over 640 KiB, since its list may then have been cut at 768 KiB and a missing row may still be pending); a follow that
  succeeds after the 60 s lease has lapsed first drops that node's prompts (`by: "unfollowed"`). Each peer is
  asked on a thread of its own, one at a time, so a dead peer never delays the others' renewals. A
  node going offline, leaving, a lapsed follow, or the app detaching publishes `prompt.resolved {outcome:
  "pass", by: "offline" | "unfollowed"}` for each of that node's prompts and drops them.
- `prompts.list` on a client also carries the remote pending prompts (qualified, the shape above), after its
  own, for an app caller and never for a `ClientKind::Node` caller; the merged rows (its own first) go through the
  same size fit as the local list, so the reply stays within the socket's line cap.
- Answer one with `session.permission {id: "<node>/<prompt>", decision: "allow" | "deny", message?}` or
  `session.answer {id: "<node>/<prompt>", answers}`: the `id` field is routed to the owner with the bare id
  (app tier only, as every forwarded op), where the usual checks apply and it arrives as a `ClientKind::Node`
  app-origin call. `decision: "always"` from another node is refused with `bad_request` ("always can't be
  granted from another node; allow once, or answer on this machine"): it would write a permanent rule on a
  machine whose owner isn't at the keyboard.
- `prompt.resolved.by` (topic `prompts`, app tier) says who or what ended a prompt: `app` (an answer), `stopping`,
  `acting_off`, `timeout`, `no_app` or `hook_gone` on the node that owns it (a hook whose wait ended because its channel closed answers the hook with a pass and publishes no `prompt.resolved`); on a client's republished
  copy also `offline` (the node went offline or left), `unfollowed` (the app detached, the node stopped being a
  client or the 60 s follow lapsed) and `unlisted` (the owner's list no longer holds it). The `outcome` of
  those last three is `pass`.

### Remote sessions

- `sessions.list` on a client node (its own member record has `client: true`) also holds every linked member's
  rows, after its own. A remote row is the owner's `Agent` row with `id: "<node>/<id>"` (decision 0012), `host`
  the node id, `node: {id, name, online}` (additive; absent on a local row) and `transcriptPath` the mirror file
  on this machine once `cluster.follow` has made one, else `null`, never the owner's path. An offline node keeps
  its rows with `online: false`; a node that left the member list loses them. A Worker's list never carries
  another node's rows. A remote row must read as `Agent` (unknown fields are dropped), and its node comes only
  from the link that proved it, never from the row. Claude Code rows come in the default list and in
  `sessions.changed`; Codex, Antigravity and shell rows only with `tools: "all"`. `sessions.list {remote: "shell"}` (additive; anything else but `null` is `bad_request`) adds the linked members' shell rows to the default list and nothing else, for a client that shows them; the daemon then also sends `sessions.remote.shell` on topic `sessions` whenever that set changes (data: the whole current list of remote shell rows, `[]` once the last is gone; never sent while there are none and there never were). `remote` also takes `"codex"` (the linked members' Codex rows, `<node>/codex:<id>`, whose `transcriptPath` is the mirror like any remote row) or a list of the two, `["shell", "codex"]`; `sessions.remote.codex` is the same kind of event for the Codex rows. `sessions.changed` is unchanged.
- A shell job on this node is a row with `id: "shell:<job>"`, `tool: "shell"`, `host: "Shell"`,
  `target: {process: <pid>}`, `command` the command line, `name` its first line cut to 60 characters, `label`
  `Running`, `Done`, `Stopped` or `Failed` (`group` `working` or `completed`, `detail` the reason when there is
  one) and `transcriptPath` the job's log (0600 under `<data>/cluster/jobs/`, stdout and stderr together). Rows
  come with `tools: "all"` only; 50 are kept, finished ones dropped first. A shell row has no reply path.
- `cluster.follow {id}` (app tier) leases a mirror of one remote session's transcript for 10 minutes (`until` is
  now plus 600000 ms; at most 16 at once, and a 17th drops the lease renewed longest ago instead of being refused), renewed by calling it again. It is `failed` for a session this client
  doesn't list or one with no transcript, and `bad_request` for an id that isn't on another node. The mirror
  file grows by `node.tail` pulls (every 2 s, and when the owner announces a change) and stops at 256 MiB. Chat
  calls it while a remote session is open (a refusal is tried again after 5 s; it needs no acting switch, since it only copies a transcript here), the phone's `chat.get` at most once a minute per session and for its four newest chats.
- An op that takes a session id (`session.reply`, `.answer`, `.permission`, `.interrupt`, `.stop`, `codex.send`,
  `codex.stop`, `agy.send`, `agy.cancel`, `agy.permission`, `agy.conversation`, `shell.stop`) goes to the owner
  when the id is `<node>/<id>`, as the app tier only, never relayed twice; an unknown or offline node is an
  error, never a local fallback. The phone's bridge calls the daemon at the app tier, so its replies on a remote
  row go the same way; with no cluster attached the call is refused "`<node>` is not reachable from the phone
  yet". The phone's list holds remote Claude Code rows only, with `node: {id, name, online}` (additive).
  With this node's acting switch off, `shell.stop` (a local id too) and an app-tier `cluster.dispatch` that isn't a
  `delegate` answer `forbidden` with [`ACTING_OFF`], like the session ops above; `cluster.follow` and
  `cluster.return` don't, since they only copy into this Mac's own mirror and repository.

### Dispatch

`cluster.dispatch {node, key, ...}` starts a job on a node. Only the home dispatches: another member's call is
forwarded to it, and a non-home asked directly answers `forbidden` "`<name>` is primary now". `key` (at most 128
bytes) names the request: a second call with the same key starts no second job and answers the first one's
`{job, state, session, warning}` (the core's `job_reply`: no `qualified`; `warning` is the job's saved detail; a
delegate's repeat has no `exit`, `out` or `err`). With `code: "send"` the daemon sends the code (bundle,
`node.put`, `workspace.prepare`, the note under `<data>/cluster/workspaces/<job>.json`) before the core looks at the
key, so a repeat sends it again; the prepare finds the job's worktree and reuses it untouched.

- `tool: "claude"` takes `cwd` (absolute) and `model?` (becomes `--model`); `tool: "agy"` takes `cwd` and `text`
  (the first message, at most 64 KB); `tool: "codex"` takes `cwd`, `text` (the first message, at most 64 KB) and
  `model?`, `effort?` (the Worker checks them against its own Codex model list) and starts `codex.send
  {draft_folder, text, job}` on the node, which runs Codex under that machine's own login, sandbox and network
  setting; `tool: "shell"` takes `cwd` and `command` (at most 64 KB). A field another
  tool owns is `bad_request`. `session` in the reply is the session's id on that node and `qualified` its
  `<node>/<id>`; for a shell job it is `shell:<job>`. `state` is `dispatching`, `running`, `done`, `failed`,
  `interrupted` or `lost`.
- `code: "folder"` (default) uses `cwd` as it is on the node. `code: "send"` (app tier only; refused for a
  delegate, and a dispatch to this machine sends nothing) bundles the repository `cwd` is in, sends the unpushed
  commits to the node as `node.put` chunks, has it clone `origin` once and add a worktree on branch
  `notchling/<job>`, and starts the tool there (`cwd` becomes the worktree). Refused before anything is sent: a
  folder outside a repository, a repository with no commits or no `origin`, a detached HEAD, more than 256 MB of
  commits. `cluster.return {job}` later fetches the job's commits into `refs/heads/notchling/<node name slug>/<job>`
  of the local repository (the slug is the node's name lowercased, other characters turned to `-`; the reply's
  `node` is the name), never merging or forcing; it is `bad_request` for a job whose code wasn't sent and `failed`
  when that branch already holds other commits.
- `delegate: [args]` runs `notchling delegate` on the node (`delegate.run`, depth 1; `--write`, `--out`,
  `--prompt-file`, Codex and a timeout over 2100 s are refused; `--folder` is required). It is the only form an
  agent may send; anything else is `forbidden` "only the Notchling app starts that on another node". The node
  needs `acceptsDelegation` (`cluster.member.set`), else "`<name>` doesn't take delegated jobs". The call waits
  for the job (up to 36 minutes) and adds `{exit, out, err}` (each output at most 256 KB) to the body; the job
  is `done` on exit 0 and `failed` otherwise.
- `warning` is `null` or the one text `cluster::versions::compat` gives when the node's Notchling or tool
  version differs from the reference's; it never blocks. Refusals come back as errors with a text for the user:
  `forbidden` (not the primary, the app-only rule above), `bad_request` (arguments) and `failed` for the rest:
  "`<name>` is not a member", "was removed", "is not approved yet", "is offline", "hasn't reported its
  versions", a protocol or schema mismatch, a tool that isn't installed, and Claude Code on Linux ("isn't proven
  yet") or Windows.
- A Codex job with a non-empty `secrets` list is refused ("Codex jobs on another machine take no secrets") in the
  daemon and again in the core: a Worker can't hand a grant to a Codex job, since `codex.send` is not one of the
  start ops that note a job's grants. A Codex delegate (`delegate: ["codex", ...]`) is still refused ("Codex
  delegated jobs on another machine come in a later version"). A node without Codex, or where `codex login
  status` says signed out, is refused by `compat` ("Codex isn't installed on `<name>`", "Codex isn't signed in on
  `<name>`").
- Allow `cluster.dispatch` and `cluster.return` 300 s, `cluster.follow` and `shell.stop` 60 s (the app's
  `call_timeout`).
- A job's end reaches the home as a `jobs` peer event (`job.state {job, state, detail}`: done, failed or
  interrupted) from its node. `jobs` is not a local API topic. A Worker that restarts forgets its shell jobs and
  the home keeps them `running`; nothing marks a job `lost` yet.

### Node-only ops

These travel between nodes as calls and have no local API name: `ops::*` has no constant for them and the socket
answers them as an unknown op. The receiver checks the sender against `engine::cluster::policy::RULES` (the
sender's role, the origin's tier, the origin node equal to the sender; a pending or removed sender is refused)
and answers them before `Ops` (`cluster_switches`, `cluster_sessions`, `cluster_dispatch`, `cluster_blobs`,
`cluster_workspace`). A forwarded session op (above) is the same kind of call with the bare id.

| Op | Sent by (origin tier) | Args | Reply |
| --- | --- | --- | --- |
| `node.sessions` | a client or the home (node) | `{}` | the receiver's own `sessions.list {tools: "all"}` rows, never remote ones; a Worker never asks |
| `node.tail` | a client or the home (app) | `{id, from, gen?}` | `{data (base64), next, size, reset, gen}`; at most 512 KB a call, only a transcript a row of this node names |
| `node.follow` | a client or the home (app) | `{topic: "prompts"}` | `{following: true, topic, leaseMs: 60000}` |
| `node.switch.set` | the home (node or app) | `{switch: "acting" \| "bridge" \| "install", on}` | `{switch, on, applied}`; only with no app attached |
| `node.put` | the home (node), a client or the home (app) | `{transfer, offset, data (base64)}` | `{next}` |
| `node.put.end` | as `node.put` | `{transfer, sha256}` | `{size}`; the file is renamed in only when the hash matches |
| `workspace.prepare` | as `node.put` | `{job, facts, transfer}` | `{cwd, head}` |
| `workspace.return` | as `node.put` | `{job, from, nonce?}` | `{data (base64), next, size, done, nonce}`, plus `sha256` on the last chunk |
| `shell.run` | the home (node or app) | `{job, cwd, command}` | `{id: "shell:<job>", session, job, pid}`; the same job twice answers the first |
| `delegate.run` | the home (agent) | `{args, job}` | `{exit, out, err}` |
| `node.versions` | any member (node) | the sender's versions | `{}` |
| `node.load` | any member (node) | `{}` | `{cpu, memUsedMb, memTotalMb}` or `null` |
| `node.reach` | any member (node) | `{node}` | `{online: bool}`; whether this node's own link to `node` is up now. A Standby asks the others before it treats the primary as unreachable. |
| `cluster.record` | any member, to the home (node; app, from a client or the home, for a hide) | `{command}`: a `claim`, `touched`, `side_copy` or `hide_side_copy` command | `{changed}`; anything else is `bad_args`; relayed to the home from a member |
| `snap.offer` | the home (node), to a Can Be Master member | `{manifest}` | `{missing: [part], lengths: {path: length}}`: the parts the receiver still lacks and the transcript lengths it holds; refused from anyone but the primary, on the primary itself and on a machine that isn't Can Be Master |
| `snap.part` | as `snap.offer` | `{set, part, transfer}`, or `{set, part, files: [{rel, transfer}]}` for a folder | `{ok: true}`; the bytes came first by `node.put`, and the part is checked against the manifest's hash |
| `snap.commit` | as `snap.offer` | `{set}` | `{set, lengths}`; the set becomes current only when every part is there and checks, the receiver keeps the two newest, then sends `snap.ack` to the home |
| `snap.ack` | a Can Be Master member (node), to the home | `{set, lengths}` | `{ok: true}`; recorded only on the primary and only from an active Can Be Master Standby |
| `move.freeze`, `move.stop`, `move.switch`, `move.undo` | the home (node) | see the handover under `cluster_ops.rs` | as there |
| `move.resume` | the machine that handed over (node), to the new home | `{sessions: [id]}` | as there |

The session ops that are forwarded (`session.new`, `codex.send`, `agy.open`, `agy.send`, `session.stop`,
`codex.stop`, `agy.cancel`, `shell.stop`) are accepted from the home at node or app origin; `session.reply`,
`.answer`, `.permission`, `.interrupt`, `.stop`, `codex.send`, `.stop`, `agy.send`, `.cancel`, `.permission`,
`.conversation`, `prompts.list` and `shell.stop` from a client or the home at app origin. `cluster.dispatch`,
`cluster.member.set`, `cluster.remove` and `cluster.join.token` reach the home from a client at app origin (a Standby
or Worker without the app is refused a join token), and a delegate `cluster.dispatch` from any member to the home.

### Cluster secrets, logins and inbox mirrors

Cluster plan C ([decision XXXX](decisions/XXXX-cluster-secrets-and-logins.md)). `ClusterState.vault` is the vault node, the Mac whose Keychain holds the vault.
Only it, while it is the primary, releases a value. None of the ops below is served on the local socket
except where a tier is shown; the node-only ones come from another node's link and are checked by
`policy::RULES` first.

| Op | Sent by | Tier | Args | Reply |
| --- | --- | --- | --- | --- |
| `cluster.dispatch` | the app, or an agent | app, agent | adds `secrets: [name, …]` (at most 16, lowercased, no delegate job) | a job whose names the node doesn't allow is refused before it starts; one whose vault node isn't primary is refused with "`<vault node>` holds the vault; dispatch again when it is primary", and no job is made |
| `grant.attach` | this node's own core | node, local only | `{job, member, vaultName, vaultPrimary}` | `{delivered: [names]}` or `{waiting: reason}`; no other node can send it. The core fails the dispatch on `waiting` ("`<reason>`; dispatch again when it is primary") and revokes what was sent |
| `grant.revoke_job` | this node's own core | node, local only | `{job, node}` | best effort; asked at every end of a job; the vault node then sends `grant.revoke` |
| `grant.request` | a Worker's `vault.use`, for a running job | agent origin, to the vault node | `{job, names, command, folder}` | `{delivered: [names]}` or `{waiting: reason}`; the job must be on the sender and the sender must allow each name |
| `grant.deliver` | the vault node, while it is home | node | `{job, name, value, expiresMs}` | `{stored: name}`; `value` is the entry's environment as JSON (variable to string), at most 32 KiB |
| `grant.revoke` | the home or the vault node | node | `{job, names?}` | `{}`; no `names` removes every grant of the job |
| `login.deliver` | the vault node, while it is home | node | `{tool: "claude", value, expiresMs}` | `{stored: "claude"}`, or a refusal while the login is off in this build |
| `login.revoke` | the home or the vault node | node | `{}` | `{}`; deletes the Worker's login file |
| `node.inbox` | any member to the home and Can Be Master members | node | `{event: "changed", item}`, `{event: "removed", id}` or `{event: "sync", items}` | `{ok: true}` |
| `inbox.list` | a client (app) | app | `{}` | `{items: [Item], unread}`; on a client that isn't the home, its own items plus the home's, with the home's ids qualified `<node>/<id>` |
| `inbox.answer` | a client (app) | app | `{id, action, by?}` | an id of another node is forwarded to its owner, which takes the first answer only. `by` (`mac` or `phone`) is read only from a node-link caller; from anyone else it is ignored and the answer counts as `mac` |
| `vault.list`, `vault.save`, `vault.remove` | a client (app) | app | the same args plus `node` | with `node` set to the vault node's id, the call goes to that machine's vault; any other value is a bad request |

- A grant's value never appears in a reply, an event, the audit log or a `Debug` print. `grant.*` and
  `login.*` arguments print as `Args(<hidden>)`.
- `grant.deliver` does not wait for a job record. A Worker knows its jobs from its own daemon: a shell job
  from spawn to reap, a Claude or Antigravity job from its start op (`session.new` or `agy.send`).
  - A job the cluster state lists elsewhere or finished, or one that ended here, is refused.
  - A job that is live here is stored as live.
  - Any other job is accepted as pending. Its grant is stored and held for 8.5 minutes (`PENDING_MS`,
    the attach wait plus one call) from the first deliver, so the grant can arrive before the start op.
  - The start op makes a pending grant the job's own. A whole-job `grant.revoke` drops the hold. The
    janitor deletes a pending grant when its window has passed.
  - `expiresMs` is clamped to 12 hours from now, and a value it could not mask is refused.
- `grant.request` and a dispatch that names secrets wait up to 8 minutes (`ATTACH_CALL_MS`) for an
  Ask me first approval. The approval card is the inbox kind `secret_approval`, with Allow Once,
  Always Allow and Deny. The vault node's audit log records a release as `vault.secret_used` with the
  names, before the first send.
- A `vault.list`, `vault.save` or `vault.remove` with `node` never falls back to this machine's vault:
  with no cluster it says "this machine is in no cluster, so it has no other vault", with the vault
  node offline "The vault is on `<name>`, which is offline". Without `node`, or with `null`, the local
  vault answers. An agent-tier or node-tier caller is refused when it names `node`.
- `vault.use` on a Worker (a node in a cluster that is not the vault node) answers from the grants of
  the job that the caller's process belongs to. A caller outside every job's process tree gets
  "secret isn't available here: this command isn't part of a cluster job", and a machine whose cluster
  state hasn't loaded yet gets "secret isn't available yet: this machine's cluster is still loading".
  A process root is recorded for shell jobs only, so `vault.use` inside a Claude or Antigravity job on a
  Worker is refused. A request that has to wait for the vault node answers "secret isn't available yet:
  waiting for `<vault node>`, which holds the vault". A vault node that is not the primary usually answers "no job" instead, since its state has none of the Worker's jobs unless it dispatched the job itself. A dispatch that names secrets while another machine is primary is refused with "`<vault node>` holds the vault; dispatch again when it is primary".
- `cluster.status` members carry `secrets: {names, any}` and `versions.logins` (a map of tool to signed
  in, absent when the Worker could not tell). `node.versions` reports the logins: `codex` (from
  `codex login status`), `agy` (whether `~/.gemini/jetski-standalone-oauth-token` exists and is not empty, read as signed
  out on Linux and as unknown on a Mac, where the login may be in the Keychain, which is never read)
  and `claude` only while the cluster login is on, which it is not in this build (it means the cluster's
  Worker login, never the machine's own). The machine's own Claude Code login is `versions.own_logins`
  (additive, a map of tool to `true`, omitted when empty; older nodes ignore it): `claude` is `true` when
  `~/.claude.json` has a non-empty `oauthAccount` object, and the entry is left out otherwise, since a
  machine can run Claude Code without one. Only the profile is read, never a token or the Keychain, and it
  never affects dispatch. The probes
  run off the driver thread, at most every 10 minutes, with a 5 s limit per command, and never refresh a
  token. A tool known to be signed out (`false`) is refused at dispatch with "`<Tool>` isn't signed in on
  `<node>`".
- Inbox mirrors live in memory, are refilled by the owner's `sync` and are never written to the
  receiver's inbox file. A mirrored item keeps its owner's qualified id, drops its `open` form ticket
  and is cut to fixed lengths. A call with an id the sender doesn't own refuses the whole call.

## `codex_ops.rs`

Milestone 5's Codex ops (E6a): `codex.send`, `codex.stop` and `codex.delete` over the engine's
[`CodexRuns`]. App tier, recorded by the server like every app-tier op, and refused with
[`ACTING_OFF`] while the `acting` switch is off: every one of them starts `codex` or sends it a
signal.

| Op | Args | Reply |
| --- | --- | --- |
| `codex.send` | `{id \| draft_folder, text, model?, effort?, draft?}` | with `id` (an existing session): `{"id": "codex:<session>", "session", "state": "queued", "waiting": <position>, "outboxId"}` at once, the message saved in the Outbox; with `draft_folder` (a new session): `{"id": "codex:<session>", "session", "state": "started"}`, or `{"state": "starting", "draft"}` for a session Codex hasn't named within 4.5 s |
| `codex.stop` | `{id}` | `{"stopped": bool}`: SIGINT to the running turn; false when none runs |
| `codex.delete` | `{id}` | `{"deleted": true}`: stops the turn, drops what waits, runs `codex delete --force`; `{"deleted": false, "state": "deleting"}` when that takes over 4.5 s |

`codex.send {id}` saves the message in the Outbox like `session.send` and answers at once. The session's postman starts the turn when the session is idle (`Waits for the current turn` otherwise, in order) and counts the message Sent only when the process was started with the prompt; one cut after it started, by acting going off, is `unsure` ("May already be in"), and one that could not start waits. Its options (`model`, `effort`) are checked again when it is delivered. A draft's first message and `draft_folder` still start the session directly and are not an Outbox row; later messages are. `codex.send` is refused with `forbidden` while `acting` is off, as before; messages already saved wait.

`id` is a row id (`codex:<session>`). `draft_folder` starts a new session in that folder and
the reply comes once `thread.started` names it, or `starting` if that takes over 4.5 s (a call
is given up at 5 s); `draft` (`codex-new:<uuid>`) is the app's row for it, echoed in the late
events below. Events, on topic `tools`: `codex.turn {id, session, state: started | queued |
ended | failed, waiting?, error?}` and `session.changed {id, session, row}` ([`topics`]); the
ends of the jobs that outlast a reply are in `codex_slow.rs`.

What a caller may name is narrow, since whoever holds the app tier for a moment shouldn't be
able to point a sandboxed-but-writable agent anywhere or pass Codex options:

- a session must have a rollout under the Codex home (`delete` and `send` to a session that
  isn't running), and runs in the folder the rollout records, which must still exist;
- `draft_folder` is an absolute path to an existing folder that is neither `/` nor the home
  folder (the workspace-write sandbox makes the folder writable);
- `model` is a slug (letters, digits, `.`, `_`, `-`), `effort` a word, and an effort the
  model's cache entry doesn't list is refused; nothing else reaches Codex's command line, and a
  prompt that starts with `-` follows a `--`;
- a prompt is at most 100 KB, and at most eight turns run at once.

## `control_ops.rs`

Session control (E11) as daemon ops: `engine::control`'s planners, run through the daemon's
[`notchling_engine::control::Launcher`]. App tier and behind the acting switch like every acting
op ([`crate::acting_ops`] checks both before these run).

| Op | Args | Reply |
| --- | --- | --- |
| `session.stop` | `{id}` | `{"stopped": true}` |
| `session.remove` | `{id}` | `{"removed": true}` |
| `session.new` | `{folder, config_dir?, flags?: ["--model", <an allowed model>]}` | `{"jobId": "6ce4c19c" or null}` |
| `folder.trust` | `{folder, config_dir?}` | `{"trusted": true}` |
| `session.move` | `{id, to_config_dir}` | `{"jobId": ...}` |
| `session.command` | `{id, command, force?}` | `{"sent": true}`; for `/chrome` and `/chrome off` nothing is typed: `{"sent": false, "chrome": <the `session.chrome` reply>}` and the work runs in the daemon (`force: true` restarts past running background tasks, and past work it can't count) |

`config_dir` and `to_config_dir` name a lane by its folder (a `lanes.json` entry), or the main
login by `~/.claude` (or `null`, or left out); any other folder is refused. `session.new`
refused by Claude Code for an untrusted folder fails with Claude Code's words ("Workspace not
trusted"); `folder.trust` then a second `session.new` is the way on. `session.command` types a
slash command like a reply; one that opens a menu (`/plugin`, `/mcp`…) needs a visible terminal,
so it is refused here. `/chrome` and `/chrome off` (alone, with no words after) are caught first: any other text starting with `/chrome` is an ordinary message. A second `/chrome` with the other direction while one waits cancels the first. Only the app calls it (agents and the command line are refused). The daemon turns Chrome on or off for the job (it edits the job's `respawnFlags`, runs `claude respawn`, and checks `/chrome`'s own screen through an attach that types nothing over the box), so it never reaches the terminal as a reply.

What a caller may change is narrow, since whoever holds the app tier for a moment shouldn't be
able to widen what a session may do:

- `flags` is nothing, or exactly `--model <id>` with a model the catalog allows: the built-in
  ones plus what the tool lists live (`Catalog::is_allowed`, which sees through a `[1m]` or a
  date suffix). The user's `catalog.json` renames and prices models but never adds one.
  Anything else is refused, naming the allowed ids.
- `folder.trust` takes an absolute path to a folder that exists, isn't `/` or the home folder,
  and that `session.new` was refused as untrusted within the last [`TRUST_WINDOW`] (compared
  canonical). It trusts only what the user just tried to start a session in.

## `cost_ops.rs`

E8c in the daemon: `cost.summary`, the token cost over the transcripts. Agent tier, read only,
not switched: it reads transcripts and nothing else (and the account log, for the CLI's
account), and writes only the daemon's own cache.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `cost.summary` | agent | `{range?: "7" or "30" or "all"}` (30 by default; the numbers 7 and 30 too) | see below |

```json
{"range": "30", "scanned": 1794150245.0, "scanning": false,
 "summary": {"days": [{"day": "2026-11-08", "cost": 1.5}], "today": 1.5, "week": 9.0, "month": 31.0,
            "models": [{"name": "Opus 5.5", "cost": 30.0}],
            "model_detail": [{"name": "Opus 5.5", "cost": 30.0, "percent": 96.8, "input": 1.0,
                              "output": 2.0, "cache_read": 3.0, "cache_write": 4.0}],
            "projects": [{"name": "notchling", "cost": 20.0}],
            "accounts": [{"id": "uuid:uuid", "cost": 25.0}],
            "channels": [{"name": "Claude Code", "cost": 30.0}],
            "channel_tokens": {"Claude Code": 123456.0},
            "unpriced": ["claude-new-9"], "borrowed": ["claude-fable-5-3"]},
 "by_account": {"uuid:uuid": {...same as summary...}}}
```

- Costs are dollars at API list prices. `week` and `today` are the same in every range;
  `month` is the range's total.
- The reply is what the last scan published. A scan starts on a thread when a request finds the
  last one 20 s old or none; `scanned` is when the last finished one began (`null` before the
  first) and `scanning` says one is running, so a client asks again a moment later.
- Prices come from the daemon's one catalog ([`crate::catalog_ops::CatalogService`]: the
  built-in data, with the override file `catalog.json` beside the socket laid over it).
  `unpriced` lists the models in the range that have no price: their tokens are counted and
  their dollars are 0, and the daemon logs each such id once per run ([`UnpricedLog`]).
  `borrowed` lists those newer than any the catalog knows, priced as the newest of their
  family. Both are sorted ids, once each.
- The lists are cut to 50 rows, and `by_account` is left out if the reply would pass 700 KB
  (a line is at most 1 MiB).

## `cost_report_ops.rs`

`cost.report`: the Cost Check over the usage ledger (`<data>/daemon/usage.db`, [`notchling_engine::cost::ledger`]). Agent tier, read only, not switched. It reads the ledger and writes only the ledger's own rows: it first reads any transcript bytes added since the last pass, for at most 20 s.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `cost.report` | agent | `{days?: 1 to 30, plan?: "api" or "subscription" or "auto"}` (14 days, `auto` by default) | see below |

```json
{"complete": true, "scanned": 1795176000.0, "ledger_calls": 41207,
 "report": {"days": 14, "from": 1793966400.0, "to": 1795176000.0, "plan": "unknown", "money": "api_equivalent",
            "totals": {"calls": 127307, "tokens": 25100000000, "cost": 10800.0, "unpriced_calls": 0},
            "by_role": [{"key": "sub", "calls": 103000, "tokens": 1, "cost": 8155.0, "cost_share": 0.76, "token_share": 0.7}],
            "by_model": [{"key": "claude-opus-5-5", "calls": 52000, "tokens": 1, "cost": 6036.0, "cost_share": 0.56, "token_share": 0.4}],
            "cache": {"main": {"five_min": 0, "one_hour": 9, "mixed": 0, "unknown": 1}, "sub": {"five_min": 1500, "one_hour": 0, "mixed": 0, "unknown": 2}},
            "findings": [{"id": "premium_subagents", "saving": 850.0, "saving_share": 0.079, "tokens_avoided": null,
                          "evidence": [{"name": "runs", "value": 282.0}], "detail": {"target": "claude-sonnet-5-5"}}],
            "not_worth": [{"id": "main_effort", "saving": null, "saving_share": null, "tokens_avoided": null, "evidence": [], "detail": {}}],
            "unknown": [{"id": "cache_ttl", "count": 120}], "saving": 1500.0, "saving_share": 0.14}}
```

- `findings` are ranked by `saving`, biggest first; a finding whose price is missing has `saving: null` and comes last. `not_worth` holds the levers under the bar (1% of spend or $5) and the ones already low. `saving` over all findings counts each call once: a call a finding claimed is skipped by the ones after it.
- Dollar amounts are API list prices. `money` is `"estimate"` for `plan: "api"` and `"api_equivalent"` otherwise; for a subscription a client leads with `saving_share` and `tokens_avoided`.
- A `null` is unknown, never zero: `totals.cost` is `null` when no call had a priced model; `unknown` lists what the transcripts could not tell (`price`, `cache_ttl`, `agent_type`, `effort`), each with a call count.
- `cache` counts main sessions and helper runs by the time their own `cache_creation` numbers show: `5m` or `1h` is read from `ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens`, `mixed` has both, `unknown` has no split to read. The check never assumes an hour.
- `complete` is `false` when the first reading of the transcripts hit the 20 s budget; ask again for the rest.

## `daemon_wiring.rs` and `every_session.rs`

`sessions.list` is the one op `Ops` answers itself (agent tier, read only, no switch).

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `sessions.list` | agent | `{tools?: "claude" \| "all"}` | a bare list of rows (`Agent`, camelCase; `atPrompt: true` is present only on a Claude Code row whose turn may be over though its group says working or needs input, because background tasks or subagents run or it asked in text, and means the job file alone cannot say a turn is running: the daemon decides from the transcript before it types a reply). No argument, `{}` and `"claude"` give Claude Code's rows exactly as before. `"all"` adds Codex and Antigravity rows after them and puts `tool` (`claude`, `codex`, `antigravity`) on every row; any other `tools` is `bad_request`. `partial: true` beside `ok` still means the first read had not landed |

On a cluster client node the list also holds other nodes' rows (`<node>/<id>`, `host`, `node {id, name, online}`) and, with `tools: "all"`, shell job rows (`shell:<job>`, `tool: "shell"`); their shapes are under "Remote sessions" in `cluster_ops.rs`.

A row has `worktreePath` (the job state's) only when the session ran EnterWorktree; `git.*` ops and Chat's Details Panel read there instead of `cwd`, and the folder is a known one. The phone's bridge leaves it out.

Codex rows are the 30 newest top-level rollouts under `<codex>/sessions` of the last 14 days (every age under a fixture root), without child rollouts (a first `session_meta` with `parent_thread_id` or `thread_source: "subagent"`); ids `codex:<thread>`. Antigravity rows are the summaries copy's top-level conversations of the last 14 days; ids `agy:<conversation>`. Both are cached 2 s, read only and need neither the bridge switch nor an app. `notchling sessions` and the phone's bridge (`Sources::foreign_rows`) ask for them; the app's session source and every other client keep the default. The `sessions` topic's events still carry Claude Code's rows only. The phone keeps `agy` as `Session.tool` on the wire. The phone's Antigravity rows take their working flag and chat read only: from the session open here, one another `agy` holds, or its own log (newest 512 KB, as `agy.conversation`; the phone protocol has no `truncated` field).

## `git_ops.rs`

What Chat's Details Panel, Changes panel and Branches panel show of a session's repository: `git.status`, `git.diff`, the three branch ops and `git.worktrees` and `git.branch_list`, over
`notchling_engine::git` and the engine's `control::Launcher`. App tier, read only, no switch:
the app never runs a process, so the daemon runs `git` for it (decision 0004). Every call is
refused (`forbidden`) unless `cwd` is absolute, has no `..`, and (after symlinks) is a known
session's folder or below one; the known folders are the session list's `cwd`s and `worktreePath`s, Antigravity's
rows and the last 14 days' (30 newest) Codex rollouts, read again only once the last read is 5 s
old (a refused `cwd` does not read them again). `git` runs in the canonical path that was
checked. A `path` is refused if it is empty, absolute, has a `..` or a NUL, goes through a link
that leaves the repository, or is not exactly one of the current changed files (no `.`, no
directory, not a file that is clean).

All seven ops are `OpSpec::APP_QUIET` (and `Home::Quiet`): a success is not recorded (the app asks every
5 s or 15 s, which would fill the log); a refusal or failure still is.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `git.status` | app | `{cwd}` | `{repo: false}` when `cwd` is in no repository; else `{repo: true, branch, head, detached, initial, upstream, ahead, behind, changes: [{path, status, from, added, removed}], changed_at?, truncated}` |
| `git.diff` | app | `{cwd, path}` | `{path, text, truncated, untracked}` |
| `git.branch_diff_stat` | app | `{cwd, base?, worktree?, ref?, pick?}` | `{repo: false}` when `cwd` is in no repository; else `{repo: true, includes_uncommitted: true, branch, head, detached, base, merge_base, files: [{path, status, from, added, removed, uncommitted}], added, removed, file_count, uncommitted_files, changed_at?, truncated}` |
| `git.branch_diff_file` | app | `{cwd, base, path, from?, worktree?, ref?}` | `{path, text, truncated, untracked}` |
| `git.branches` | app | `{cwd, worktree?}` | `{repo: false}`, or `{repo: true, current, default, remote_default, local: [name], truncated}` |
| `git.worktrees` | app | `{cwd, fresh?: [path]}` | `{repo: false}`, or `{repo: true, name, default, integration, fetched_at, no_worktree, truncated, worktrees: [{path, real, main, current, branch, head, detached, locked, missing, upstream, upstream_ahead, upstream_behind, upstream_gone, base, ahead, behind, unrelated, committed_at, work}]}` |
| `git.branch_list` | app | `{cwd}` | `{repo: false}`, or `{repo: true, name, default, integration, fetched_at, truncated, pending, branches: [{name, ref, local, remote, remote_only, on_github, is_base, worktree, head, detached, committed_at, subject, upstream, upstream_ahead, upstream_behind, upstream_gone, base, ahead, behind, unrelated, merged_into, lines, pending}]}` |

- `status` is one of `M`, `A`, `D`, `R`, `U` (unmerged) or `?` (untracked). `path` is relative to the
  repository's root, as `git` prints it, not to `cwd`; `from` is a rename's old path. `added` and
  `removed` are `null` for an untracked or binary file. `branch` is `null` while `detached`
  (then `head` is the short commit); `upstream`, `ahead` and `behind` are only set with an upstream.
  `initial` is a repository with no commit yet.
- `git.branch_diff_stat` compares the working tree with the merge base of `base` and `HEAD`, so it
  covers the branch's commits (`merge-base..HEAD`) plus uncommitted work: staged and unstaged
  changes to tracked files and untracked files (`includes_uncommitted` is always `true`; each file's
  `uncommitted` says whether it differs from `HEAD` or is untracked, and `uncommitted_files` counts
  them). `base` is optional: without it the base is `origin/HEAD`'s target, else `main`, else
  `master`, the first that shares history with `HEAD`; `base` and `merge_base` (7 characters) are
  `null`, with no files, for a repository with no commit or none of those. A given `base` must be a
  plain branch name (letters, digits and `._/+@-`, no leading `-`, no `..`, at most 200 characters)
  or the call is `forbidden`; one that is not a branch or has no common history is `failed` (`no
  such branch`, `no common history with that branch`). `status` is `M`, `A`, `D`, `R`, `U` or `?`
  (untracked); `from` is a rename's old path; `added` and `removed` are `null` for a binary file or
  an untracked one over 2 MB (untracked files are counted by reading them, at most 8 MB in all).
  Files are sorted by path, cut at 2,000 (`truncated`); `added`, `removed`, `file_count` and
  `uncommitted_files` count every file, cut or not. Renames are found (`-M`), so a renamed file is
  one `R` row.
- `git.branch_diff_file` is that file's patch against the merge base, with the `git.diff` limits
  (400 lines or 128 KB, `truncated`; an untracked file's patch is the whole file as added lines). A
  rename needs `from` (the row's old path) to show as a rename; without it the file shows as new.
  `base` is required and checked as above; `path` and `from` follow `git.diff`'s rules, and a
  `path` with no change against the base (and not untracked) is `forbidden` (`not a changed
  file`).
- `git.branches` lists the local branches (newest commit first, at most 200, `truncated`), the one
  checked out (`current`), `remote_default` (`origin/HEAD`'s target, only if it resolves) and
  `default`, the base the stat uses without a `base` (`remote_default`, else `main`, else
  `master`, only among the local branches).
- `status.changed_at` (ms, left out when nothing changed) is the newest modification time of the changed files that exist (one `symlink_metadata` each under the repository root, at most the 500 listed; the root is one `rev-parse --show-toplevel`, run only when there are changes). `git.branch_diff_stat` fills the same field.
- `git.branch_diff_stat` takes three more optional arguments, and without them nothing changes. `worktree` reads that worktree instead of `cwd`'s. `ref` reads one branch's committed work only: no `status`, no untracked file, `diff --name-status|--numstat -z -M <merge base> <ref> --`; the reply has `includes_uncommitted: false`, `branch` is the ref's name without `refs/heads/` or `refs/remotes/`, `detached` is `false`, `head` is its 7-character commit, and a `ref` with no base picked has no base and no files. `pick: "nearest"` (any other value is `bad_request`) uses the nearest base when no `base` is given, and the reply's `base` names it. `worktree` and `ref` together are `bad_request`. A stat with `pick: "nearest"` that used a nearest base (for `cwd`'s worktree or for `worktree`) stores its result as that worktree's `work`, so while the app asks every 5 s the session's own worktree is not read a second time by `git.worktrees` or `git.branch_list`. When `pick: "nearest"` finds no base, a branch with no candidate at all (it is the default or the integration branch) and one whose pick is still pending keep today's rule (`origin/HEAD`'s target, else `main`, else `master`), but a branch whose candidates are all unrelated has `base: null` and no files.
- `git.branch_diff_file` takes `worktree` or `ref` as `git.branch_diff_stat` does. With `ref`, `path` must be one of the files the stat lists for that ref and base (`not a changed file` otherwise), the patch is `diff <merge base> <ref> -- :(literal)<path>` and no untracked-file path exists. `git.branches` with `worktree` lists that worktree's branches.
- `git.worktrees` is for the Details Panel. `name` is the main checkout's folder name (the repository's folder for a bare one), `default` and `integration` are revs (`origin/main`, `agent-run-oss`; `null` when none), `fetched_at` is the modification time (ms) of `<common dir>/FETCH_HEAD`, `null` when there is none, and `no_worktree` is the count the "›" row shows: local branches and remote-tracking refs (a remote ref whose name, less its remote, is a local branch is dropped; `origin/HEAD` is skipped) that no worktree has checked out, without the default's own row. Each worktree is a non-bare entry of `git worktree list`, at most 50 (`truncated` also says refs were cut at 300): `path` as git lists it, `real` canonical, `main` the first entry, `current` the worktree that contains `cwd` (the longest path wins), `missing` that git calls it prunable (then no numbers), `head` 7 characters, `committed_at` the head commit's time (ms). `base`, `ahead` and `behind` are against the nearest base (a rev); `unrelated: true` with `base: null` is a branch that shares no history with any candidate. `work` is `{added, removed, files, uncommitted_files, changed_at, read_at}` as the stat counts the worktree (uncommitted included) from the daemon's cache, `null` until it was read; the current worktree's is always `null` (the app has it from `git.status` and the stat). A call reads working trees by this rule: worktrees with no `work` yet first (up to 4), else the stalest one older than 15 s, plus up to 2 of the `fresh` paths (an unknown path is ignored, not refused); no read starts more than 1.5 s after the call began, and the call waits for its reads at most that long, so a read that is still running finishes into the cache and the next call returns it.
- `git.branch_list` is for the Branches panel. One row per local branch, per remote-tracking ref that is no local branch's name, and per detached worktree (`name` `null`, `detached`, `head`). `name` has no remote (`agent/ask`), `remote` is the remote's name or `null`, `ref` is the full ref, `on_github` says the remote's host is `github.com`. `worktree` is `{path, real, main, current, missing, work}` or `null`, and the branch's missing `work` is read within the call's budget. `is_base` marks the default and the integration branch (the branch the main checkout is on, when it is not the default's short name); a base has no `base`, `merged_into` or `lines`. `merged_into` is the first of the integration branch and the default (as revs) that the branch is 0 ahead of, so a branch is never merged into itself; `lines` is `{added, removed, files}` of its committed work against `base`, `null` for a merged branch. A row with no candidate base (it carries the default's or the integration branch's own name) has `base: null` and neither `pending` nor `unrelated`; a row whose candidates all share no history with it has `base: null` and `unrelated: true`. `pending` on a row says its numbers are not computed yet; the reply's `pending` counts such rows. Line counts are computed for 1 s per call at most (the rest come back pending and the app asks again after 2 s), and counts that no snapshot fill covers (a branch whose upstream is another candidate, a prunable detached head) are counted within the same budget.
- The nearest base is one of the default, the integration branch and the branch's own upstream (when it has another name than the branch), never a rev with the branch's own short name; the one with the fewest commits ahead wins, then the fewest behind, then that order. `%(ahead-behind:<base commit>)` (one `for-each-ref` per base for every ref at once; `rev-list --left-right --count <base>...<head>` per branch when git is older than 2.41 and refuses the atom) prints counts even for histories with no common ancestor, so a pick that is not 0 ahead is confirmed before it is used: `diff --shortstat` exits 128 for no merge base in `git.branch_list`, and `merge-base <base> <head>` exits 1 before a worktree's work is read or a `pick: "nearest"` stat. An unrelated pair is remembered for good and the row is picked again without that candidate.
- Merged is read from the counts: a branch is merged into a base exactly when it is 0 ahead of it. The daemon runs neither `for-each-ref --merged` nor `merge-base --is-ancestor`.
- The daemon keeps in memory, per repository (keyed by its canonical common dir, dropped when not asked for 10 minutes): the worktree list with the remotes, the default and `fetched_at` (5 s), the numbers between two commits (`ahead`, `behind`, `lines`, keyed by the base's and the head's commit, so an entry is right forever; 2,000 entries, least recently used), each worktree's `work` with when it was read, and a detached head's commit facts. A second `git.branch_list` with unchanged heads runs no `diff` and no `rev-list`; a second `git.worktrees` inside 15 s runs no `status`. The lock covers the maps only, never a `git` run, and a read in flight is marked, so Details and Branches asking together do not read the same worktree twice.
- New arguments are checked against what `git` itself lists for `cwd`'s repository. `worktree` must be absolute, with no `..` or NUL, and canonicalize to exactly one of the non-bare, non-prunable paths of `git worktree list` (`forbidden`, `not a worktree of this repository`); `git` then runs in that canonical path. `ref` must pass `clean_ref`, start with `refs/heads/` or `refs/remotes/`, and be one of the refs `for-each-ref` lists (`forbidden`, `not a branch of this repository`); it is only ever passed as a revision before `--`, to `merge-base`, `rev-parse` and `diff`. A `base` and every rev handed to `git` is a 40-character commit from git's own output or a name that passed `clean_ref`.
- A remote's URL is parsed for its host (`github.com` or not) inside the engine's `Reader::remotes` and dropped there: it is in no reply, error, event or Audit line.
- `changes` is cut at 500 files and `truncated` says so. `diff.text` is cut at 400 lines or
  128 KB (a single line is cut on a character boundary), and an untracked file over 2 MB gives
  an empty `text` with `truncated: true`. An untracked file's diff is the whole file as added
  lines (`git diff --no-index /dev/null <path>`).
- Every read runs under `/usr/bin/sandbox-exec` with a profile that allows all but `process-exec*`
  and then allows exec of git's own binaries (the `xcrun --find git` target and the Command Line
  Tools' git, with their `libexec/git-core`, each only if it and every parent folder is owned by
  root and not writable by group or others). A filter, hook, fsmonitor or
  textconv program therefore cannot start whatever the repository's config says. Without
  `sandbox-exec`, without such a git, while the first resolution is still running (after 1 s), or if
  `sandbox-exec` refuses the profile, a call fails with `can't read this repo safely`
  and never runs unsandboxed; off macOS both ops answer `unimplemented`. An LFS file whose
  filter can't start may show as modified with a binary diff.
- Reads run no program of the repository: every command is `git --no-optional-locks -c
  core.fsmonitor=false -c core.hooksPath=/dev/null -c diff.external=` plus, for each filter
  driver `git config -z --get-regexp '^filter\.'` names (a read that runs nothing), `-c
  filter.<name>.clean=`, `smudge=`, `process=` and `required=false`. Then `status
  --porcelain=v2 --branch -z --untracked-files=all --ignore-submodules=all`, then `diff
  --numstat -z HEAD` (or `--cached` with no commit); the branch ops add `merge-base`, `symbolic-ref
  --quiet --short refs/remotes/origin/HEAD`, `for-each-ref`, `rev-parse --verify` and `diff
  --name-status|--numstat -z -M <merge base> --`, all of them reads; a diff is `diff --no-color --no-ext-diff
  --no-textconv --ignore-submodules=all HEAD -- :(literal)<path>`. Each command has a 3 s
  timeout, `LC_ALL=C`, `GIT_TERMINAL_PROMPT=0`, `GIT_PAGER=cat`, and every other `GIT_*`
  variable of the daemon's environment is cleared. The launcher stops a command that writes
  more than 4 MB (a list) or 128 KB + 1 (a diff). Not a repository is told by `rev-parse
  --is-inside-work-tree` after a 128, never by the message. A timeout or any other failure
  answers `failed`.
- The `git` subcommands the seven ops may run are a fixed list: `config` (only `-z --get-regexp`), `status`, `diff`, `rev-parse`, `merge-base`, `symbolic-ref`, `for-each-ref`, `worktree` (only `list --porcelain -z`), `rev-list` and `log` (`-1`, for a detached head). `fetch`, `pull`, `push`, `remote`, `ls-remote`, `gc` and `worktree add|remove|prune|repair` never run: the view is as fresh as the last fetch you or an agent ran, and `fetched_at` says when that was. `git_ops_allowlist_tests.rs` runs every form of every op and fails on any other subcommand. Every diff carries `--no-color --no-ext-diff --no-textconv --ignore-submodules=all -M` and a trailing `--`.

## `hook_ops.rs`

The hook's side of [`ActingOps`]: `hook.request`, and who may make it.

The hook is who its process is. Its pid comes from the socket's peer credentials
([`Caller::pid`]), never from its arguments, and that process must descend from the Claude Code
process whose `sessions/<pid>.json` (in the main config folder or a lane's) holds the event's
`session_id`: the parent pids are walked with `proc_pidinfo` on macOS and `/proc/<pid>/stat` on
Linux ([`crate::hook::parent_pid`]). Anything else is `forbidden`, and `notchling hook` passes
on it. An agent inside that same session still descends from it; what this stops is a process
answering for a session it doesn't belong to, and a pid it names for itself.

`notchling hook --fence` is a second form with no op behind it: it asks no daemon and opens no socket. It reads the session
id from the event, reads `<data dir>/cluster/fence.json` and passes (prints nothing, exit 0), waits up to 20 s, or prints a
`PreToolUse` deny with the reason. No file passes; a file that exists but can't be read, or a session whose lease ran out
on this boot, waits and then denies. It is the sixth hook slot `install.apply` writes. Nothing writes `fence.json` yet, so it
always passes today.

## `install_ops.rs`

E13 in the daemon (Milestone 6): installing the hook entries into Claude Code's
`settings.json` and the `notchling-*` skills into Claude Code's, Codex's and Antigravity's
skills folders. Built and switched off, like `acting`: the `install` switch is off at every
start, and until the maintainer turns it on the Swift app owns both (decision 0011).

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `install.plan` | app | `{action?: "install" \| "remove", what?, with_swift_hooks?, node?}` | the envelope, `applied: false`; reads only, works with the switch off |
| `install.apply` | app | `{what?, with_swift_hooks?, node?}` | the envelope |
| `install.remove` | app | `{what?}` | the envelope |

`what` names the parts to act on; left out it is all of them. The names are `prompt_hooks`
(`PreToolUse` `AskUserQuestion` and `PermissionRequest` `*`, and the fence hook: `PreToolUse` `*` with the command
`notchling hook --fence`, timeout 30 s, which stops a cluster session's tool calls on a machine it moved away from;
it installs on every Mac, and outside a cluster it always passes), `page_hooks` (`PreToolUse`,
`PostToolUse` and `PostToolUseFailure` matching `Artifact`) and the skills' folder names
(`notchling-delegate`, `notchling-pages`, `notchling-settings` (Claude Code and Codex), and `notchling-vault` once the vault supplies it).

`node` (a node id) runs the call on that machine instead, with its own folders and its own `install` switch. Only the
primary forwards it, for the app tier, to an Active member; a `node` that is not a well-formed id is a bad request, never
a local run, and the node's own id runs locally. A node whose switch is off answers `applied: false`, `on: false` and
writes nothing. `install.remove` has no `node`.

Settings > Skills & Models lists the skills with `install.plan` (no `what`, so it also plans the
hooks it ignores) and its Reinstall Skills button sends `install.apply {what: [<every skill name the
plan listed>]}`, which leaves the hook entries alone. With the switch off the reply says so and nothing is
written. A skill's `action` reads as: `unchanged` installed, `update` out of date, `write` not installed,
`skip` with `tool_missing: true` the tool is missing, any other `skip` a folder
Notchling doesn't own, left alone.

The envelope:

```json
{"on": false, "applied": false,
 "message": "install is off: the Swift app owns hooks and skills until the maintainer turns this on",
 "backup": "/…/settings.json.notchling-backup-1794150245123",
 "plan": {"action": "install", "changed": true, "blocked": null,
   "hooks": {"file": "/…/settings.json", "state": "present", "read_only": false,
     "refused": null, "changed": true,
     "changes": ["add PreToolUse (AskUserQuestion): '/…/notchling' hook (timeout 300 s)"],
     "notes": ["the Swift app's hook is in PermissionRequest (*); it was not changed"],
     "present": [{"event": "PermissionRequest", "matcher": "*", "origin": "swift",
                  "command": "'/…/Notchling' --hook", "managed": false}]},
   "skills": [{"name": "notchling-pages", "tool": "claude", "path": "/…/skills/notchling-pages",
               "action": "write", "reason": null, "tool_missing": false, "error": null}]}}
```

- `message` is there while the switch is off (and `applied` is false, and nothing was written).
- `state` is `missing`, `empty`, `present`, or `refused` (with `refused` saying why: broken
  JSON, not an object, a dangling link, …). Planning a refused file is not an error.
- `blocked` is the reason an `install.apply` would be refused: the Swift app's `--hook` is
  still in the file, and two hooks must not both answer. `with_swift_hooks: true` goes ahead.
  Retirement rewrites the Swift entries (decision 0011), not this.
- `present` lists every `notchling hook` and Swift `--hook` entry in the file; `managed` is
  true for the ones in our places. Other hooks are never listed or changed.
- A failure (a refused file, a file that changed meanwhile, a write error) is code `failed`
  with a message; the file is then as it was. Nothing from the file reaches a message.

## `limits_ops.rs`

E8c in the daemon (Milestone 6): the limit watchers and their ops. Built and switched off.

While the `accounts` switch is off the watcher loop does nothing at all: it calls no source (so
no Keychain read, no usage request, no switch), runs no check and says nothing. The switch is
the daemon's [`SwitchFlag`], in memory only and off at every start; turning it off also drops
what the watchers last saw, so the first round after it comes back on syncs afresh. Everything
that reads or writes a login sits behind the traits in
[`notchling_engine::limits::sources`]; the daemon's implementations over the real stores are
[`crate::limits_sources`].

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `limits.status` | app | `{}` | see below |
| `limits.set_mode` | app | `{provider, mode?, switch_at?}` | `{"provider", "mode", "switch_at"}` |
| `limits.usage` | app | `{refresh?}` | each Claude Code account's limits, see [`crate::limits_usage`]; `renewals: {id: "renewing" \| "failed"}` (absent when empty) names a login in use that a Refresh asked Claude Code to renew |
| `cost.summary` | agent | `{range?}` | see [`crate::cost_ops`] |
| `cost.report` | agent | `{days?, plan?}` | see [`crate::cost_report_ops`] |

```json
{"accounts_on": false, "watching": false, "switch_at": 95,
 "providers": [{"id": "claude", "label": "Claude Code", "mode": "notifyAndSwitch",
                "modes": ["off", "notify", "notifyAndSwitch"], "default": "notifyAndSwitch",
                "registered": false, "active": "me@example.com" | null, "active_id": "acct-id" | null,
                "windows": [{"name": "5-hour", "percent": 41.0, "resets_at": 1794150245.0}],
                "last_switch": 1794150245.0 | null, "checked": 1794150245.0 | null}],
 "notices": [{"at": 1794150245.0, "text": "me@example.com is at 91% of its 5-hour limit."}]}
```

- `providers` are `claude`, `codex` and `antigravity`. `registered` says a store feeds that
  watcher; `active`, `active_id`, `windows`, `last_switch` and `checked` are what its last check saw (`null`
  or empty before one). `active_id` is the id of the account the numbers belong to (the same id as
  `accounts.list`'s `active`); a client shows `windows` only while it equals the account it calls in
  use, since a check runs every tick and an account can change in between. A Claude check that has
  nothing to read (the mode is `off`, no account is in use, the account in use is not in the saved
  list, or it just switched away) clears `active`, `active_id`, `windows` and `checked`;
  `last_switch` stays. `notices` are the last 20 things the watchers said, newest last.
- `limits.status` and `limits.set_mode` work while `accounts` is off: they read and write only
  the daemon's own memory and its own `daemon/limits` document in `<data>/notchling.db` (the modes and the switch
  point, under the Swift keys), never a login.
- `limits.set_mode` takes a `provider` and a `mode` (`off`, `notify`, `notifyAndSwitch`) or a
  `switch_at` (80 to 99, refused outside it) or both. Antigravity refuses `notifyAndSwitch`.
- Every message the watchers say goes out as `limits.notice {"at", "text"}` on the `limits`
  topic (app tier), for the app to show.

## `organizations_ops.rs`

What Settings > Organizations shows and changes, over `notchling_engine::organizations` (the `organizations`
document in `notchling.db`). Every op is app tier, so an agent can't list folders, create, change or delete an
organization (the design's reduced agent-tier `organizations.list` waits for the phase that needs it). No
switch. The two reads are `OpSpec::APP_QUIET` (the page asks on open, after each edit and while a folder is
typed); the five edits are `OpSpec::APP` and recorded as `op.called`. The registry is opened read only here, so
no op migrates a machines file.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `organizations.list` | app, quiet | `{}` | `{organizations: [{id, name, githubOwner, color, folders, personal, counts: {accounts, secrets, machines}}], colors, onlyPersonal, unreadable}` |
| `organizations.create` | app | `{name, color?, githubOwner?}` | `{id}`; with only Personal it first marks every account and secret as Personal's (machines without a mark already count as Personal) |
| `organizations.rename` | app | `{id, name}` | `{}` |
| `organizations.color.set` | app | `{id, color}` | `{}` |
| `organizations.folders.set` | app | `{id, folders: [path]}` | `{folders}`, the folders as stored |
| `organizations.folders.preview` | app, quiet | `{id, folders: [path]}` | `{folders, problem, problemText, overlaps: [{folder, organization, other, relation}], moved: [{folder, from, to}]}`; writes nothing |
| `organizations.delete` | app | `{id}` | `{}` |

- Personal first, then the others in their `order`. `colors` is the closed list a colour must come from
  (`organizations::COLORS`: grey, orange, yellow, lime, teal, blue, purple, pink, red); a colour already stored
  that isn't in it still reads, and only a new choice is checked. `unreadable` is `null`, or why the document
  can't be read: the list then shows Personal alone and every edit answers `failed` with `data.kind` `unreadable`
  and writes nothing.
- A refused edit is `bad_request` with a plain `message` and `data: {problems: [{kind, ...}]}`; `kind` is one of
  `name_empty`, `name_too_long`, `name_taken {name}`, `id_unknown {id}`, `color_invalid {color}`,
  `folder_invalid {folder}`, `folder_taken {folder, by}`, `personal_not_deletable` and
  `not_empty {counts: {accounts, secrets, machines}}`. The app words these from its own messages, never from
  `message`.
- `folders` are full paths or start with `~/`; they are normalized (`~/work/./x/` is `~/work/x`), a folder can
  belong to one organization only (same real folder, case folded where the disk folds it), and a folder inside
  another organization's folder is allowed: the longest match wins. The preview's `overlaps` say `inside` (the
  folder lies in the other's) or `contains`; `moved` lists the sessions the daemon knows whose organization
  would change, with the organization ids before and after.
- Delete is refused for Personal and for an organization that still holds an account, a secret or a machine,
  counted inside one transaction with the removal.

## `outbox_ops.rs`

The Outbox ([design](superpowers/specs/2026-11-15-outbox-design.md), [decision](decisions/XXXX-outbox.md)): a message to a session is saved in the daemon's `outbox` table first and a postman per session types or starts it as soon as the session can take it, in order. App tier, served by `OutboxOps`. Every op but `outbox.list` is refused with `forbidden` and [`ACTING_OFF`] while the `acting` switch is off (messages already saved wait); `outbox.list` only reads.

| Op | Args | Reply |
| --- | --- | --- |
| `session.send` | `{id, text, clear_box?: bool, options?: {model?, effort?}, via?}` | `{"outboxId", "session", "state": "queued", "position": n}`; `position` is 1 for the head of the session's mailbox. Rows are never joined at save time: each send is its own row, and rows still `queued` right behind a head when it is taken go with it as one message (see below)` |
| `outbox.list` | `{session?}` | `[row...]` in order, every session or one; on a client node whose app follows the `outbox` topic, a list of every session (or of a qualified remote `session`, when it is not forwarded) also holds the remote rows the daemon already knows, after the local ones, each with `session` qualified `<node>/<session>` and in the owner's queue order |
| `outbox.edit` | `{id, text}` | `{"edited": true}` |
| `outbox.remove` | `{id, sent?: bool}` | `{"removed": true}`; `sent: true` is Mark as Sent and is only for an `unsure` row |
| `outbox.hold` | `{id, on: bool}` | `{"held": bool}`; `on: true` keeps a `queued` or `waiting` row from being typed for 10 minutes (Edit holds a row while it is open), `on: false` lets it go |
| `outbox.resend` | `{id, clear_box?: bool, resume?: bool}` | `{"state": "queued"}` |
| `outbox.discard` | `{session}` | `{"discarded": n}`: how many rows it removed |

`id` of `session.send` is a row id from `sessions.list` (a Claude Code job, `codex:<session>` or `agy:<session>`). `session` of `outbox.list` and `outbox.discard` is such a row id too (a bare session key is accepted); the rows carry the session's key (the job's short id, `codex:<id>`, `agy:<id>`), which survives `/clear`, a respawn and a move. `id` of the other ops is a row's own id (`outbox.list`'s `id`, the `outboxId` of a send). `options` (Codex `model` and `effort`) is read only when it is an object and is checked again at delivery with `codex.send`'s rules. `via` is `chat`, `notch`, `phone` or `agent`; left out it comes from the caller.

`session.send` refuses before anything is saved (all `bad_request` unless noted):

- `id is needed`, `text is needed` when an argument is missing or not a string.
- `Type a message first.` for empty text; `That message is over 100 KB.` for text over 100 KB.
- `No such session.` for an id the daemon doesn't know, and for a Codex or Antigravity session with no rollout or summary.
- `Reply in its terminal.` for a session that takes no replies (another terminal app, an `exec` command job).
- `That command opens a menu; open the session's terminal.` for a Claude Code slash command that opens a menu (`/plugin`, `/mcp`, ...).
- `Start the session with codex.send or agy.send.` for a draft id (`codex-new:`, `agy-new:`).
- `failed`: an Antigravity session another `agy` holds (what Chat says), `200 messages are already waiting for this session.`, `Couldn't save the message.`

`outbox.edit`, `.remove` and `.resend` refuse with:

- `bad_request` `No such message.` when the row is gone.
- `failed` `It's being sent.` on a row that `outbox.remove` finds `sending`, or that `outbox.edit` finds `sending` or `unsure` (an `unsure` message may already be in the session, so it can only be sent again or removed); `failed` `It's in the session's queue.` on an `in_queue` row (edit and remove). Discard removes an `in_queue` row, since it is an explicit act.
- `outbox.edit` also refuses empty text, text over 100 KB and a menu command (the `session.send` words). A successful edit puts the row back to `queued` (from `queued`, `waiting`, `box`, `stopped` or `gone`), keeps its place and clears its back-off and Clear Box choice.
- `outbox.resend` is Send Again on `unsure` (the person decided it may go again), Clear Box and Send on `box` (`clear_box: true`, else `bad_request` `The box has other text; clear it to send.`), Resume and Send on `stopped` (`resume: true`, else `bad_request` `Resume the session to send.`; `failed` `Resume isn't available.` without a resumer) and Try Now on a `waiting` row whose `why` is `retry`. Any other row is `bad_request` `There's nothing to send again.`; a row that changed meanwhile is `failed` `The message changed; try again.`

`outbox.discard` removes every row of the session except one that is `sending`, `in_queue` ones included.

Joining at the take. When the head of a session is taken to be typed, the rows right behind it that are still `queued` (or `waiting` for the queue, the turn or a dialog), never tried and not held by `outbox.hold`, go with it as one message, a blank line between them, and are deleted (`outbox.removed` with `why` `joined` and `into` the head's id; `session.outbox` `then` is `joined`). It stops at the first row that is a command, a shell line (`!...`, judged after hidden characters are removed, as the text is typed), `clear_box`, of another kind or with other options, one that was put back by Retry or that a `session.reply` call is waiting on, and at 100 KB. A head that a `session.reply` call is waiting on takes nothing. The head keeps its id and its row says how many messages it carries (`joined`).

An unsure head holds its session's mailbox for at most two minutes (`asideAtMs` says when). Then it steps aside (`asideMs`, `session.outbox` `then` `aside`): the rows behind it go on, nothing is typed again by itself, its buttons stay (Retry is `outbox.resend`, Mark as Sent is `outbox.remove {sent: true}`) and the daemon still looks for it in the transcript for 10 minutes. An `unsure` head with rows queued behind it steps aside before the two minutes once its session has stayed idle at its prompt for 3 seconds (job status `idle`, no dialog, nothing in Claude Code's queue, not in safe mode) and it has been unsure for 3 seconds, and only while a row behind it can go (not held, not backing off); it is looked at again every second until then, and a row that turns out to have landed is settled as Sent first. A second `unsure` row in a row in the same session holds the mailbox with no timer, and no early step-aside applies to it; a message that is Sent or found in between ends the run, and a restart, a Remove or a Discard does not. Retry on a row that stepped aside puts it back in line by its place, but it waits while a later row is being sent, is in Claude Code's queue or holds as `unsure`.

A row on the wire: `{id, session, tool, kind, text, state, position, queuedMs, updatedMs, via, why?, attemptMs?, asideAtMs?, asideMs?, heldUntilMs?, joined?, box?, error?}`.

- `tool` is `claude`, `codex` or `antigravity`; `kind` is `prompt` or `command` (a Claude Code message that starts with `/`); `via` is as above; `position` counts from 1 within its session.
- `state` is `queued`, `waiting`, `sending`, `in_queue` (in Claude Code's own queue; the model has not read it yet), `unsure` (may already be in; the person decides), `box` (the session's input box holds other text), `stopped` or `gone`.
- `why` is a code for `waiting` (`dialog`, `queue`, `turn`, `retry`), `unsure` (`not_found`, `restart`, `dropped`), `box` (`not_cleared`, `mixed`); there is none for `in_queue`.
- `asideAtMs` (an `unsure` head that holds the mailbox) is when it steps aside; `asideMs` (an `unsure` row that has) is when it did; `heldUntilMs` is the end of an `outbox.hold` lease; `joined` is how many messages a row carries when more than one.
- `attemptMs` (an `unsure` row only) is when its last attempt began; Chat hides the row's bubble once the transcript holds the message dated at or after it. The daemon looks at an `unsure` row's transcript again every 5 s for 10 minutes and, when the message turns up, settles the row as sent (never sends it again).
- `box` (a `box` row only) is the text the input box holds and `error` (a Codex or Antigravity `retry` row only) is what went wrong. Both live in the daemon's memory, so a restart drops them, and neither is ever written to a log, an event or the audit.

Sent, removed, discarded and dropped rows are deleted, not listed. A row never expires by age; one that has waited 7 days is raised once in the Inbox (kind `outbox_unsent`, Send and Discard).

Topic `outbox` (app tier, so an agent never reads message text): `outbox.changed {row}` after every insert or change, and `outbox.removed {id, session, why}` with `why` `sent`, `removed`, `discarded`, `joined` (with `into`) or `session_removed`. A client reads the list once and follows the topic. Events, audit and Activity carry `session.outbox` with the row's change, its length and `via`, never its text.

Cluster. A session belongs to the node that runs it and its mailbox stays there. `session.send` with a qualified id (`<node>/<job>`), and `outbox.edit`, `.remove`, `.resend`, `.discard` and `.list` with a qualified `session` argument, are forwarded to that node, which runs them on its own Outbox (`policy.rs`: `session.send` by `id`, the others by `session`). The row id of `outbox.edit`, `.remove` and `.resend` names no node, so the app adds the row's qualified `session` beside it and the daemon ignores it on the node that owns the row. Only the app, from a client or home node, may forward; a Worker may not. A node tells its peers `outbox.changed` (with no data) and `outbox.removed {id, session, why}` and never message text; a client node whose app follows the topic asks the owner for its list and publishes the rows with `session` qualified as `<node>/<session>`, and only the difference from the last list, in the owner's queue order. That list is also what `outbox.list` adds to its answer (so an app that reconnects, or a second app, is seeded without waiting for a row to change). A list answer that was started before a peer event arrived is dropped and asked again at once, so a `sent` removal is never undone by an older list; a failed ask is tried again after 5 s, not 30.

## `pages_ops.rs`

E7 in the daemon: the pages library and the session index as agent-tier read ops (Milestone 5).
Reads always work, whatever the `acting` switch says; nothing here acts or writes the library
(`notchling hook` does, in its own process: `hook_pages.rs`).

| Op | Args | Reply |
| --- | --- | --- |
| `search.query` | `{text?: string, limit?: int}` | `{"pages": [Page], "sessions": [Session], "indexing": bool}` |
| `pages.list` | `{}` | `{"pages": [Page], "truncated": bool}` |
| `pages.find` | `{query: string}` | `{"pages": [Page + {"describe": string}], "truncated": bool}` |

`Page` (the fields of `pages.json`, plus two):

```json
{"id": "AbCdEfGhIjKlMnOpQrStUv", "title": "Cost Report", "url": "https://claude.ai/artifact/AbCd...",
 "project": "notchling", "account": "me@example.com", "updated": 1794150245.0,
 "version": "1793506664-65e3", "folder": "cost-report-1a2b3c4d", "file": "index.html",
 "source": "/w/report.html", "path": "/.../pages/cost-report-1a2b3c4d/index.html"}
```

- `id` is the last part of the claude.ai link, `null` for a page that never got a link.
- `url`, `source`, `project`, `account` and `version` are `""` when unknown.
- `updated` is seconds since 1970. Pages come newest first.
- `path` is the page's local copy, absolute; `null` when the stored folder or file name is not
  a plain name. It is what the pages window loads. `index.html` beside the folders is the
  library's own list page.

`Session`:

```json
{"key": "claude:ab12", "tool": "claude" | "codex" | "agy", "session_id": "ab12",
 "title": "Token cache work", "folder": "/w/app", "path": "/.../ab12.jsonl",
 "config_dir": "/Users/me/.claude-lanes/work" | null, "parent": null | "codex:0199a1b2",
 "modified": 1794150245.0,
 "snippet": "...the stale token cache..." | null, "highlights": [[4, 9]], "score": 61.5}
```

- `key` is Chat's row id (`claude:<id>`, `codex:<id>`, `agy:<id>`); Return on a session opens it
  with that.
- `parent` is `null` for a session. A Codex subagent's rollout is a hit of its own (its own task
  and words, none of the parent's history it was forked from) with `parent` set to the row id
  of the session that spawned it; it is not a Chat row, so a client opens `parent` instead. A
  subagent whose rollout does not name its parent is not indexed.
- `path` is `""` for Antigravity (found by title). `config_dir` is the lane's folder, `null` for
  `~/.claude`.
- `snippet` is a cut of the text around the match, `null` when only the title or folder
  matched or there was no query; `highlights` are byte ranges `[start, end)` into the snippet.

`search.query`:
- `text` is split on whitespace; a page needs every word in its title, project, account or
  link (a pasted link or id finds its page); a session needs every word in its title, folder
  or text. `text` empty or missing lists the newest of each.
- Pages: words in the title first, newest first within each; at most 20 (6 for an empty
  `text`), and no more than `limit`. Sessions: at most `limit` (default 30, 6 for an empty
  `text`; capped at 200).
- The reply is the index as it is: a refresh runs in the background when the last one is 8 s
  old or more, and `indexing` says one is running, so a client asks again a moment later. The
  very first query waits up to 2 s for the first refresh.

`pages.find`: the page with this link or id, else the pages holding every word (an empty query
is every page). `describe` is the text `notchling page` prints.

A reply is one line and a line is at most 1 MiB, so `pages.list` and `pages.find` stop at 1000
pages or 700 KB and say `"truncated": true`.

## `providers_ops.rs`

Milestone 6's provider ops (E8b): the accounts of Codex, Antigravity and Claude Desktop over
[`notchling_engine::providers`]. App tier (recorded by the server like every app-tier op) and
behind the `accounts` switch, which is off at every start.

While the switch is off every op answers `forbidden` with [`ACCOUNTS_OFF`], and nothing runs: no
Keychain, no process, no network, no file write, no read of a login. The one exception is
`providers.list`, which lists the saved account names from the daemon's own files (the plan:
"listing saved account names from the daemon's own files still works"); it says `accounts_on:
false`, names no active account, and does not even ask whether an app is installed.

| Op | Args | Reply |
| --- | --- | --- |
| `providers.list` | `{sync?: bool}` (default true) | `{"accounts_on", "providers": [{id, title, capabilities, available, state: {accounts: [{id, email, plan, needs_login, switchable}], active_id, busy, adding, sign_in_url, unlisted_active_title}}], "syncing"?: true}` |
| `providers.usage` | `{provider, refresh?: bool}` | `{"provider", "reports": {<account id>: {fetched, groups: [{name, bars: [{label, kind, percent, resets_at}]}]}}, "standing"?: {<account id>: {reached: "credits" \| "limit" \| null, credits: {unlimited, balance} \| null}}, "refreshing"?: true}` |
| `providers.switch` | `{provider, id, confirm?: bool}` | `{"state": "switched", provider, id}`, `{"state": "needs_confirmation", confirmation: {title, message, button}}` (pass `confirm: true` after asking), or `{"state": "switching"}` when it takes over 4.5 s; code `failed` with the reason otherwise |
| `providers.add.begin` | `{provider, confirm?: bool}` | `{"state": "waiting", provider, sign_in_url?}`, `needs_confirmation` as above, or `{"state": "starting"}` |
| `providers.add.cancel` | `{provider}` | `{"cancelled": true}`, or `{"state": "cancelling"}` |
| `providers.remove` | `{provider, id}` | `{"removed": true}`: forgets the saved login; the account stays signed in if it is the one in use |

`percent` in a bar is always the share used, whatever the tool's own page shows (ChatGPT's usage page shows what is left; the app turns it over for Codex). `standing` is only for an account whose last read said it is blocked (`reached`: `credits` when the workspace has no credits, `limit` for any other refusal) or has credits to show (`unlimited`, or a plain positive `balance` string); an account with neither is absent, and so is the whole key when none has one. Only Codex fills it today.

`provider` is `codex`, `antigravity` or `claudeDesktop`. Events on topic `providers` (app tier):
`providers.changed {provider, reason: "sync" | "usage" | "switch" | "add" | "remove", state?,
id?, error?, sign_in_url?}`. An add flow's progress (`started`, `waiting` with the sign-in page
once Codex prints it, then `done`, `cancelled` or `failed`) arrives as `reason: "add"` events; a
switch or refresh that outlasts its reply ends as one too. No token, cookie or login is in a
reply or an event; a failure's text is fixed words.

One switch or add flow per provider at a time. A flow that waits for the user stops being
polled the moment the switch turns off and is abandoned (`AccountProvider::abandon_add`: Codex's
`codex login` stopped and its scratch home removed; nothing written or restored, the event's
`note` says where a provider was left), and is cancelled when the daemon stops while it is on.

## `settings_ops.rs`

Agents read and change Notchling's settings ([decision 0020](decisions/0020-agents-change-settings-by-class.md)). Every key in `crates/engine/src/settings/keys.rs` carries a class for agents: `free` (an agent sets it at once), `ask` (the user allows or denies the change in the Inbox) or `never` (only the user, in Notchling). The five switches are listed as `switch.acting`, `switch.bridge`, `switch.accounts`, `switch.vault` and `switch.install` (a bare `acting`, `bridge`, `vault` or `install` finds them too; `accounts` is the saved-logins key), always `never`: `settings.set_acting`, `bridge.set` and `settings.set_switch` stay app tier.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `settings.list` | agent | `{}` | `{settings: [Entry]}`: the five switches first, then every key in `keys.rs` order |
| `settings.get` | agent | `{key}` | `Entry`, plus `pending: [{id, value}]` while asks for the key wait |
| `settings.set` | agent | `{key, value, wait?: seconds}` | `{key, value, state: "applied", changed}` for a `free` key, or any `free` or `ask` key from the app tier; `Ask` for an `ask` key from the agent tier |
| `settings.asked` | agent | `{id, wait?: seconds}` | `Ask` |

`Entry` is `{key, title, description, kind, class, why, allowed?, range?: [low, high], value?, default?, switch?: true}`, `kind` one of `bool`, `int`, `number`, `string`, `list`, `object`, `json`, and `class` one of `free`, `ask`, `never`. An agent-tier caller gets `value` and `default` only for `free` and `ask` keys and the switches (a switch's `value` is its state in this run, `null` when unknown); the app tier gets every value.

`value` is JSON of the key's type, or text: `"true"`, `"off"`, `"90"` and `"8MB"` become `true`, `false`, `90` and `"8mb"` (`KeySpec::read_value`). It is then checked against the key's type, `allowed` list and range (`KeySpec::check`); a bad one is `bad_request` saying what the key takes ("chatHistory takes one of 2mb, 4mb, 8mb, 16mb."). An unknown key is `bad_request`, naming up to three agent-settable keys whose name or title holds the word. A switch or a `never` key is `forbidden`, for the app tier too: the app writes its own records itself.

`Ask` is `{id, key, value, state, message}`, `state` one of `waiting`, `applying`, `applied`, `denied`, `expired`, `withdrawn` (the daemon stopped first) or `failed`. An ask raises an inbox item (`setting:<id>`, kind `setting_change`, actions `allow` and `deny`, expiring with the ask after [`ASK_WAIT`], 300 s) that the Mac or a paired phone answers; the first answer wins. Allow writes the value; Deny, no answer or a stop leave it as it was. The item's responder only marks the ask (it runs under the inbox lock); the ask thread in `settings_asks.rs` writes the value, closes expired asks and their items, and withdraws the asks still waiting at `close`. `wait` (0 to 60 s, default 0) holds the call until the ask ends; `settings.asked` polls it, and an ended ask is kept 10 minutes. An allowed ask is written even when nothing polls it any more. With no app following `inbox` an ask is refused at once with `needs_approval`; at most 8 wait at a time (`failed` beyond that).

A write goes to `notchling.db` (the limit keys `limitMode`, `codexLimitMode`, `antigravityLimitMode` and `limitSwitchAt` first to the daemon's `LimitConfig`, as `limits.set_mode` does), then out on `settings` as `settings.changed {key, value, by: "agent" | "app", ask?: id}`, set or not. The app reads the key back from the store on that event, so an older event never undoes a newer write. The Settings sheet writes the store itself and applies its own change in place; it doesn't go through this op, so it isn't on the topic. Every `settings.set` from the agent tier is a `settings.agent` event (committed to the Audit before the write) with result `set`, `asked`, `allowed`, `denied`, `expired`, `withdrawn`, `failed`, `refused`, `bad_value`, `unknown_key`, `no_app` or `full`, and the key (a hash for an unknown key), the value's length and its hash, never the value. Reads are not recorded.

## `tailscale_ops.rs`

The one op that changes which Tailscale network this Mac is on. It is app tier only: an agent or the `notchling` command line gets `forbidden`, and the group checks the caller's tier again itself. The app sends it only from the Switch Network button, after a confirm that says the other network drops. Nothing switches by itself.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `tailscale.switch` | app | `{tailnet}` | `{switched, tailnet}`: `switched` is false when that network is already the active one |

`tailnet` is the Tailnet or Account column of `tailscale switch --list`. The daemon reads that list, finds the network, and runs `tailscale switch <id>` (the CLI is the Tailscale app's own, or `tailscale` on PATH). A network that isn't in the list is `bad_request`; a refusal by the CLI is `failed` with its first line.

## `vault_ops.rs`

E9 in the daemon: the Secret Vault as a service of its own, behind the `vault` switch
(decision 0007: the vault is a separate service inside the daemon, with its own checks).

While the switch is off (every start, until the app's `settings.set_switch`), every op below
answers `forbidden` with [`VAULT_OFF`], nothing reads the Keychain item or the list, values
read earlier are dropped, and no socket beyond the daemon's own is bound.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `vault.find` | agent | `{query?}` | `{entries: [Entry], saved: ["github (api.github.com)", …]}` |
| `vault.use` | agent | `{names: [..], folder, command}` | `{env: {VAR: value}, hide: {value: "[secret:name]"}}` |
| `vault.request` | agent | `{name, kind?, url?, note?, how?, expires?, reason?}` | `{entry: Entry}` once saved |
| `vault.list` | app | `{}` | `{entries: [Entry]}` |
| `vault.save` | app | `{entry: Entry, value?: {username?, secret}, replacing?: name, request?: id}` | `{entry, answered}` |
| `vault.remove` | app | `{name}` | `{removed}` |
| `vault.admin` | app (the Notchling app only) | `{name, machine}` | `{secret}` |
| `vault.approval.answer` | app | `{id, decision: "deny"\|"once"\|"always"}` | `{answered}` |
| `vault.request.cancel` | app | `{id}` | `{answered}` |
| `vault.pending` | app | `{}` | `{approvals: [..], requests: [..]}`, the pending events below |

The list is the `daemon/vault` document in `notchling.db` (the daemon's own `vault.json`
moves in once; the Swift app's stays its own); `Entry` is its shape: `{name, kind, note, url?, howTo?, expires?, userVar,
secretVar, ask?, updated}`. No reply but `vault.use`'s and `vault.admin`'s carries a value, and no event ever does. `vault.admin` is the app's read of one entry's password for a machine's sudo step (Machines, Use From Vault): the same switch, Ask me first and `vault.secret_used` audit as `vault.use` (reason `sudo on <machine>`), refused to every client but the Notchling app.

**Approval.** `vault.use` of an entry marked ask-first publishes `vault.approval.pending {id,
names, entries, command, folder, verified, session?}` on the `vault` topic (`command` and `folder` as the
caller claims them, `verified` what the kernel says about it: see [`crate::vault_caller`]), which only the app tier may follow, and
waits up to [`APPROVAL_WAIT`] for `vault.approval.answer`. The id is 128 random bits. The
answer must come from an app-tier client that said hello as the app; an agent-tier client is
refused by the server before this module sees the call. With no app following `vault` the use
is denied at once, and a wait ends denied when the app leaves, the switch goes off or the
daemon stops. Always Allow turns ask-first off for those entries. A denial is `needs_approval`
("The user didn't allow it."), which `notchling secret` exits 3 on.

**Requests.** `vault.request` publishes `vault.request.pending {id, request, entry, existing,
changes, verified, session?}` (the form, filled in: an existing entry with the passed fields laid over it, and what
that changed) and waits up to [`REQUEST_WAIT`] for the app's `vault.save {…, request: id}` or
`vault.request.cancel {id}`. A cancel is `needs_approval` ("The user cancelled.").

`session` is the Claude Code session id of the asking process and is present only when the daemon can vouch for it: `sessions/<pid>.json` names a process on the caller's tree whose resolved executable is an installed Claude (`~/.local/bin/claude`, `/usr/local/bin/claude`, `/opt/homebrew/bin/claude`, anything under `~/.local/share/claude` or `~/.claude/local`), so a process merely named claude, a symlink or a file planted for a shell is ignored. The app puts a card in that session's Chat only when exactly one row carries the id and the caller's verified folder is inside that row's folder or worktree; any other ticket opens the sheet. `verified` is the same kernel lineage as on approvals and the app shows it on both. The remaining limit: an agent inside a real Claude session in the same folder can still ask as that session (and one in another session of the same folder as that one), because the daemon holds no launch record to tell them apart; a card is a convenience placement, never a grant, and Allow still needs a click.

`session` is the Claude Code session id the asking process runs under, found the way `ask` finds it; it is left out when no session is known (a job's grant, a machine's sudo step, a shell), and the app then shows the prompt as a sheet instead of a card in that session.

**Reconnecting.** `vault.pending` answers the `vault.approval.pending` and
`vault.request.pending` events of the waits still open, oldest first, as they were sent: an
app that (re)connects asks it after following `vault`, so nothing that waits for the user is
missed.

Errors carry names and fixed texts only: never a value, never a parse error that could quote
one. Agent-tier calls are recorded as events here (the op's kind, the peer's executable, the outcome and the
names used); the server records every app-tier call that is not quiet.

## `versions_ops.rs`

Version sync, phase 1 ([design](superpowers/specs/2026-11-13-version-sync-design.md)): what the Mac has
installed, read from files. The daemon starts no tool to read a version; the one exception is
`agy --version`, run at most every 10 minutes (and never from a `versions.get`), because no file names
agy's version.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `versions.get` | agent | `{tool?}` | `{tools: [{tool, installed, method, path, problems, running, reference, usable, failed, check}]}` |
| `versions.workers` | app (quiet) | `{}` | `{workers: {<machine>: {tools: {<tool>: ToolState}, atMs}}}` |
| `versions.check` | app | `{tool}` | `{started, running}` at once; the result comes as `versions.checked` |
| `versions.latest` | app (quiet) | `{tool}` | `{latest, at}` or `{latest: null, at, error: "couldn't reach <host>"}` |
| `versions.update_local` | app, `install` switch | `{tool, to?}` | `{started, running}` at once; the result comes as `versions.updated` |

- `tool` is `claude`, `codex`, `antigravity` or `notchling`; the name `agy` is read as `antigravity`
  and answered as `antigravity`. Without `tool` there is one row per tool, in that order.
- `installed` is `null` when the tool isn't found, else `{version, sha256}`: `version` is `null` when
  it can't be told (agy before its first answer, a damaged `package.json`), `sha256` is the binary the
  launcher resolves to. `method` is `native`, `standalone`, `npm`, `brew`, `apt`, `release` or
  `unknown` (`null` when not installed). `problems` holds `{kind: "two_installs", other: <method>}` and
  `{kind: "cant_tell"}`. Notchling's own row is the daemon's version and its own executable.
- `running` is `[]` until phase 4. `reference` is `null` or `{version, sha256, result, partly, at,
  unchecked}` (`result` is `partly` for every check built so far, `partly` is `result == partly`);
  `usable` is true only when the installed version and binary (SHA-256) are the ones the reference
  checked; `failed` is `[{version, part, at, why}]` (`part` is `version`, `doctor`, `exec_flags`,
  `transcript` or `self_test`; `why` is fixed text the daemon writes, never tool output); `check` is
  `null`, `{running: true}` or the last result as `versions.checked` carries it.
- `versions.check {tool}` runs the smoke check: files and flags only (`<tool> --version`,
  `claude doctor`, `codex exec --help`, then Notchling's readers in strict mode over the newest 20
  transcripts or rollouts, each at most 4 MB; Notchling's own check is in process). It starts no session,
  opens no pty, sends no turn and writes nothing under `~/.claude`, `~/.codex` or `~/.gemini`. A new
  version or binary is checked by itself after a rescan; a failed version is checked again only by this
  op. `{started: false, running: true}` when that tool's check is already running, `{started: false,
  running: false}` when checks are off (a fixture root) or the tool has no known version. A check that
  can't tell (a command didn't start or timed out) is tried once more after 2 minutes, then fails with
  part `version`. Every result is `partly`: nothing in the check sees screens, dialogs or turns.
- `versions.latest` reads the vendor's newest version with `curl --proto =https -fsSL --max-time 10` through
  the launcher: Claude Code's `releases/latest` file, npm's `@openai/codex/latest` and Antigravity's manifest
  for this Mac's platform. The answer is kept an hour (a failed read 5 minutes); `versions.get` rows carry
  the kept one as `latest`. Notchling has none (its check stays in the app's update check). The error names
  the host and nothing else.
- `versions.update_local` runs the method's own update on the Mac: `claude update`, `npm install -g
  <package>@latest`, `brew upgrade --cask <cask>`, Codex's installer, `agy update`. With the `install`
  switch off it answers `forbidden` with the switch's off message and starts nothing. `to` is Go Back: only
  the newest seen version below the installed one that hasn't failed, only where the method can install an
  exact version (`claude install <v>`, `npm install -g <package>@<v>`, Codex's installer with
  `--release <v>`); the same version is `goBack` in the `versions.get` row (`null` when there is none or the
  method can't pin). A second update of one tool while it runs answers `{started: false, running: true}`.
  `versions.updated {tool, from, to, ok}` follows, then a rescan checks the new version. The `versions.changed`
  fact records the cause `user_update` (or `rollback` for Go Back) when the daemon ran the update.
- `soak` is `null` or `{since, turns, unknown, line}`: every 10 minutes the daemon runs the strict pass over
  transcripts written since the reference and keeps the highest count it has seen of turns and unknown
  lines written by the reference's version (`line` is "Since 2.1.324: 14 turns read, nothing unknown").
  A new reference starts a new soak. It is shown and gates nothing: it never changes a reference or a result.
- `versions.workers` is what Machines saved in `notchling.db` (`versions/workers`), read back through the
  typed `WorkerTools`, so a field that isn't part of it never reaches a client. `ToolState` is
  `{state: "notInstalled"}` or `{state: "installed", version, method, path}`.
- Topic `versions` (agent tier): `versions.changed {tool, installed}` when a rescan finds a different
  version or binary; `versions.checked {tool, version, result, failedPart, why, unknownCount, unchecked,
  at}` when a check finishes, `versions.updated {tool, from, to, ok}` when `versions.update_local` ends (`result` is `partly`, `failed` or `cant_tell`). Rescans follow
  `platform::watch` on the install folders and run every 10 minutes.
- A version seen for the first time in a data folder is a `tool.version_seen` event (the tool and the
  version, never a path or free text). The other facts (a change, a check, a drift, a self-update, a failed
  load) are not recorded yet: the registry has no kind for them.

## `codex_children.rs`

The Codex child index: which rollouts are subagents of which thread. The daemon keeps `<data>/daemon/codex-children.db` (SQLite, one row per rollout: path, size, mtime, thread, parent, depth, nickname, role, start time) from a backfill of the newest 30 day folders and `platform::watch` on `<codex>/sessions`; a rollout whose size and time are unchanged is never read again, and a day's rows for files that are gone are swept when the day is read again. Deleting the file costs one backfill.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `codex.children` | app (quiet) | `{thread}` | `{children: [{path, thread, parent, depth, nickname, role, started}]}`, oldest first; an unknown or empty `thread`, or the index not open yet, answers `{children: []}` |

Read only, no switch. With no daemon (a fixture root, a `--render`) the app reads the parent's day folder and the next one itself.
