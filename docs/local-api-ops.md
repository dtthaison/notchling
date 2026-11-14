# Local API ops: wire shapes

What each `notchling daemon` op takes and returns, tier by tier. Moved from the module docs of `crates/notchlingd/src/*_ops.rs` when the code lost its comments; the serde types next to each module are the final word. The socket, framing and tiers are in [crates/AGENTS.md](../crates/AGENTS.md) and [decision 0007](decisions/0007-local-api-tiers.md).

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

Milestone 3's ops: acting on sessions (E3b, E4), the `acting` switch, the held prompts and the
hook's request. Built and switched off: while [`Acting`] is off every acting op answers
`forbidden` with [`ACTING_OFF`], nothing is typed, and `hook.request` passes.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `session.reply` | app | `{id, text}` | `{"sent": true}`, or `{"sent": true, "state": "queued", "waiting": 1, "held": <text>}` when the session is working or just got a message (below) |
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

## `activity_ops.rs`

What Settings > Activity shows: `activity.list`, over `notchling_engine::activity`. App tier, read only,
no switch, `OpSpec::APP_QUIET` (a success is not written to `audit.jsonl`; the page asks only on open
and on Refresh). It reads the last 2 MB of `audit.jsonl` and of `audit.jsonl.1`, the last 1 MB of the
bridge's `remote-log.jsonl` and `account-log.json`, and answers normalized rows, newest first.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `activity.list` | app | `{kinds?, since?, limit?}` | `{rows: [{time_ms, source, kind, op, who, result, detail?}], truncated}` |

- `kinds` is a list of `refused`, `failed`, `acted`, `setting`, `inbox` and `account_switch` (none or
  empty means all; any other name is `bad_request`). `since` is a time in milliseconds: only rows after it.
  `limit` is 200 by default and 1 to 1000.
- `source` is `audit` (the daemon's audit), `phone` (`remote-log.jsonl`) or `account` (`account-log.json`).
  `kind`: a refusal (`forbidden`, `needs_approval`, `peer_changed`, `peer_unknown`, `other_user`,
  `file_said_on_ignored`, a vault `denied`) is `refused`; any other result that is not `ok`, `on` or `off`
  is `failed`; a success is `setting` (`settings.*`), `inbox` (`inbox.answer`, `inbox.seen`,
  `vault.approval.answer`, `vault.request.cancel`), `account_switch` (`accounts.switch`, `accounts.move_all`,
  `lanes.switch`, `providers.switch`, and every row of `account-log.json`) or else `acted`. A phone's
  `inbox.answer:refused:<reason>` is `refused`, its `inbox.answer:<action>` is `inbox`, anything else it did
  is `acted`.
- `who` is `app` for an app-tier row, else the peer's executable name (`curl`, `claude`, `unknown`), or the
  phone's device name. `result` is the audit's result up to its first `:` (so a vault use shows `ok` or
  `denied`, never the secret's name) or, for a phone, `ok`, the inbox action or `refused: <reason>`.
  `detail` is a failed or refused call's error message when the audit kept one (cut to 120 characters),
  or the first 8 characters of an account id.
- Never in a row: message text, a phone's session id, address or message length, an inbox item id, a
  secret's name or value. The reply stays under 900 KB: rows are dropped from the old end and
  `truncated` says so.

### Quiet ops

An app-tier call is written to `audit.jsonl` unless its spec is `OpSpec::APP_QUIET` and it succeeded.
These read-only polls are quiet: `bridge.status`, `bridge.devices`, `bridge.tunnel.status`,
`accounts.list`, `lanes.list`, `providers.list`, `providers.usage`, `limits.status`, `limits.usage`,
`git.status`, `git.diff` and `activity.list`. A call to `accounts.list` or `providers.list` with
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
| `agy.send` | app | yes | `{id, text}` or `{draft_folder, text}` | `{id, sent: true}`; a draft starts the session first |
| `agy.cancel` | app | yes | `{id}` | `{cancelled: true}` |
| `agy.set_option` | app | yes | `{id, option, value}` | `{requested: true}`: the session's own options only |
| `agy.permission` | app | yes | `{id, rpc_id, option_id?}` | `{outcome}`; no `option_id` cancels |

`id` is a row id, `agy:<session>`. A session's folder comes from the summaries copy or from the
session this daemon started, never from the caller; a draft's `draft_folder` is an absolute
folder that exists. One turn runs per session: `agy.send` while one runs is `failed` ("busy"), and while another `agy`
holds the session it is `failed` with what Chat says.

Events ([`crate::agy_sink`]): `session.changed {id, session, row, change, error?}` on `tools`
(as Codex's; chunks at most every 250 ms), and on `prompts` (app tier) `agy.permission.pending`
/ `agy.permission.resolved`, each also as `prompt.pending` / `prompt.resolved`, so the notch
shares one answered state with Chat. A pending permission is a `prompts.list` row too. Its id is
`agy-perm:<row id>:<rpc id>`. Nothing is answered by the daemon on its own; turning acting off
closes every session, which cancels what was waiting.

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
- The same catalog is read elsewhere in the daemon: `cost.summary`'s prices (`cost_ops`), the
  `session.new --model` check (`control_ops`, through [`CatalogService::is_allowed`]: the
  built-in models plus the live lists, a Claude id in the shape `[a-z][a-z0-9.-]*` with an
  optional `[1m]`), and the phone's `models` list and `modelId` (`bridge_backend`). A `/model`
  sent through `session.command` isn't checked against it yet.

## `codex_ops.rs`

Milestone 5's Codex ops (E6a): `codex.send`, `codex.stop` and `codex.delete` over the engine's
[`CodexRuns`]. App tier, audit-logged by the server like every app-tier op, and refused with
[`ACTING_OFF`] while the `acting` switch is off: every one of them starts `codex` or sends it a
signal.

| Op | Args | Reply |
| --- | --- | --- |
| `codex.send` | `{id \| draft_folder, text, model?, effort?, draft?}` | `{"id": "codex:<session>", "session", "state": "started"}`, or `"state": "queued", "waiting": n` behind the session's running turn, or `{"state": "starting", "draft"}` for a new session Codex hasn't named within 4.5 s |
| `codex.stop` | `{id}` | `{"stopped": bool}`: SIGINT to the running turn; false when none runs |
| `codex.delete` | `{id}` | `{"deleted": true}`: stops the turn, drops what waits, runs `codex delete --force`; `{"deleted": false, "state": "deleting"}` when that takes over 4.5 s |

A reply to a Claude Code job that is working, or that got a message less than 20 s ago and
hasn't shown work yet, is held by the daemon and not typed (`held_ops.rs`, engine `reply::held`).
There is one held entry per session: every message sent meanwhile joins it, separated by a blank
line, and the reply carries the whole held text (`held`). When the row is `completed` and can take
replies, the daemon types the held text as one message (a 1 s tick, only while `acting` is on).
Each change is an event on topic `tools`: `session.held {id, text}`, with `text: null` when
nothing is held any more. The held text lives in the daemon's memory only, so a daemon restart
drops it; it is never in Claude Code's own queue.

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
| `session.command` | `{id, command}` | `{"sent": true}` |

`config_dir` and `to_config_dir` name a lane by its folder (a `lanes.json` entry), or the main
login by `~/.claude` (or `null`, or left out); any other folder is refused. `session.new`
refused by Claude Code for an untrusted folder fails with Claude Code's words ("Workspace not
trusted"); `folder.trust` then a second `session.new` is the way on. `session.command` types a
slash command like a reply; one that opens a menu (`/plugin`, `/mcp`…) needs a visible terminal,
so it is refused here.

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

## `daemon_wiring.rs` and `every_session.rs`

`sessions.list` is the one op `Ops` answers itself (agent tier, read only, no switch).

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `sessions.list` | agent | `{tools?: "claude" \| "all"}` | a bare list of rows (`Agent`, camelCase). No argument, `{}` and `"claude"` give Claude Code's rows exactly as before. `"all"` adds Codex and Antigravity rows after them and puts `tool` (`claude`, `codex`, `antigravity`) on every row; any other `tools` is `bad_request`. `partial: true` beside `ok` still means the first read had not landed |

Codex rows are the 30 newest top-level rollouts under `<codex>/sessions` of the last 14 days (every age under a fixture root), without child rollouts (a first `session_meta` with `parent_thread_id` or `thread_source: "subagent"`); ids `codex:<thread>`. Antigravity rows are the summaries copy's top-level conversations of the last 14 days; ids `agy:<conversation>`. Both are cached 2 s, read only and need neither the bridge switch nor an app. `notchling sessions` and the phone's bridge (`Sources::foreign_rows`) ask for them; the app's session source and every other client keep the default. The `sessions` topic's events still carry Claude Code's rows only. The phone keeps `agy` as `Session.tool` on the wire. The phone's Antigravity rows take their working flag and chat read only: from the session open here, one another `agy` holds, or its own log (newest 512 KB, as `agy.conversation`; the phone protocol has no `truncated` field).

## `git_ops.rs`

What Chat's Details Panel and Changes panel show of a session's folder: `git.status`, `git.diff` and the three branch ops, over
`notchling_engine::git` and the engine's `control::Launcher`. App tier, read only, no switch:
the app never runs a process, so the daemon runs `git` for it (decision 0004). Every call is
refused (`forbidden`) unless `cwd` is absolute, has no `..`, and (after symlinks) is a known
session's folder or below one; the known folders are the session list's `cwd`s, Antigravity's
rows and the last 14 days' (30 newest) Codex rollouts, read again only once the last read is 5 s
old (a refused `cwd` does not read them again). `git` runs in the canonical path that was
checked. A `path` is refused if it is empty, absolute, has a `..` or a NUL, goes through a link
that leaves the repository, or is not exactly one of the current changed files (no `.`, no
directory, not a file that is clean).

All five ops are `OpSpec::APP_QUIET`: a success is not written to `audit.jsonl` (the app asks every
5 s, which would fill the log and rotate out the refusals and the vault's entries); a refusal or
failure still is.

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `git.status` | app | `{cwd}` | `{repo: false}` when `cwd` is in no repository; else `{repo: true, branch, head, detached, initial, upstream, ahead, behind, changes: [{path, status, from, added, removed}], truncated}` |
| `git.diff` | app | `{cwd, path}` | `{path, text, truncated, untracked}` |
| `git.branch_diff_stat` | app | `{cwd, base?}` | `{repo: false}` when `cwd` is in no repository; else `{repo: true, includes_uncommitted: true, branch, head, detached, base, merge_base, files: [{path, status, from, added, removed, uncommitted}], added, removed, file_count, uncommitted_files, truncated}` |
| `git.branch_diff_file` | app | `{cwd, base, path, from?}` | `{path, text, truncated, untracked}` |
| `git.branches` | app | `{cwd}` | `{repo: false}`, or `{repo: true, current, default, remote_default, local: [name], truncated}` |

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

## `hook_ops.rs`

The hook's side of [`ActingOps`]: `hook.request`, and who may make it.

The hook is who its process is. Its pid comes from the socket's peer credentials
([`Caller::pid`]), never from its arguments, and that process must descend from the Claude Code
process whose `sessions/<pid>.json` (in the main config folder or a lane's) holds the event's
`session_id`: the parent pids are walked with `proc_pidinfo` on macOS and `/proc/<pid>/stat` on
Linux ([`crate::hook::parent_pid`]). Anything else is `forbidden`, and `notchling hook` passes
on it. An agent inside that same session still descends from it; what this stops is a process
answering for a session it doesn't belong to, and a pid it names for itself.

## `install_ops.rs`

E13 in the daemon (Milestone 6): installing the hook entries into Claude Code's
`settings.json` and the `notchling-*` skills into Claude Code's, Codex's and Antigravity's
skills folders. Built and switched off, like `acting`: the `install` switch is off at every
start, and until the maintainer turns it on the Swift app owns both (decision 0011).

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `install.plan` | app | `{action?: "install" \| "remove", what?, with_swift_hooks?}` | the envelope, `applied: false`; reads only, works with the switch off |
| `install.apply` | app | `{what?, with_swift_hooks?}` | the envelope |
| `install.remove` | app | `{what?}` | the envelope |

`what` names the parts to act on; left out it is all of them. The names are `prompt_hooks`
(`PreToolUse` `AskUserQuestion` and `PermissionRequest` `*`), `page_hooks` (`PreToolUse`,
`PostToolUse` and `PostToolUseFailure` matching `Artifact`) and the skills' folder names
(`notchling-delegate`, `notchling-pages`, `notchling-settings` (Claude Code and Codex), and `notchling-vault` once the vault supplies it).

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
               "action": "write", "reason": null, "error": null}]}}
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
| `limits.usage` | app | `{refresh?}` | each Claude Code account's limits, see [`crate::limits_usage`] |
| `cost.summary` | agent | `{range?}` | see [`crate::cost_ops`] |

```json
{"accounts_on": false, "watching": false, "switch_at": 95,
 "providers": [{"id": "claude", "label": "Claude Code", "mode": "notifyAndSwitch",
                "modes": ["off", "notify", "notifyAndSwitch"], "default": "notifyAndSwitch",
                "registered": false, "active": "me@example.com" | null,
                "windows": [{"name": "5-hour", "percent": 41.0, "resets_at": 1794150245.0}],
                "last_switch": 1794150245.0 | null, "checked": 1794150245.0 | null}],
 "notices": [{"at": 1794150245.0, "text": "me@example.com is at 91% of its 5-hour limit."}]}
```

- `providers` are `claude`, `codex` and `antigravity`. `registered` says a store feeds that
  watcher; `active`, `windows`, `last_switch` and `checked` are what its last check saw (`null`
  or empty before one). `notices` are the last 20 things the watchers said, newest last.
- `limits.status` and `limits.set_mode` work while `accounts` is off: they read and write only
  the daemon's own memory and its own `daemon/limits` document in `<data>/notchling.db` (the modes and the switch
  point, under the Swift keys), never a login.
- `limits.set_mode` takes a `provider` and a `mode` (`off`, `notify`, `notifyAndSwitch`) or a
  `switch_at` (80 to 99, refused outside it) or both. Antigravity refuses `notifyAndSwitch`.
- Every message the watchers say goes out as `limits.notice {"at", "text"}` on the `limits`
  topic (app tier), for the app to show.

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
 "config_dir": "/Users/me/.claude-lanes/work" | null, "modified": 1794150245.0,
 "snippet": "...the stale token cache..." | null, "highlights": [[4, 9]], "score": 61.5}
```

- `key` is Chat's row id (`claude:<id>`, `codex:<id>`, `agy:<id>`); Return on a session opens it
  with that.
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
[`notchling_engine::providers`]. App tier (audit-logged by the server like every app-tier op) and
behind the `accounts` switch, which is off at every start.

While the switch is off every op answers `forbidden` with [`ACCOUNTS_OFF`], and nothing runs: no
Keychain, no process, no network, no file write, no read of a login. The one exception is
`providers.list`, which lists the saved account names from the daemon's own files (the plan:
"listing saved account names from the daemon's own files still works"); it says `accounts_on:
false`, names no active account, and does not even ask whether an app is installed.

| Op | Args | Reply |
| --- | --- | --- |
| `providers.list` | `{sync?: bool}` (default true) | `{"accounts_on", "providers": [{id, title, capabilities, available, state: {accounts: [{id, email, plan, needs_login, switchable}], active_id, busy, adding, sign_in_url, unlisted_active_title}}], "syncing"?: true}` |
| `providers.usage` | `{provider, refresh?: bool}` | `{"provider", "reports": {<account id>: {fetched, groups: [{name, bars: [{label, kind, percent, resets_at}]}]}}, "refreshing"?: true}` |
| `providers.switch` | `{provider, id, confirm?: bool}` | `{"state": "switched", provider, id}`, `{"state": "needs_confirmation", confirmation: {title, message, button}}` (pass `confirm: true` after asking), or `{"state": "switching"}` when it takes over 4.5 s; code `failed` with the reason otherwise |
| `providers.add.begin` | `{provider, confirm?: bool}` | `{"state": "waiting", provider, sign_in_url?}`, `needs_confirmation` as above, or `{"state": "starting"}` |
| `providers.add.cancel` | `{provider}` | `{"cancelled": true}`, or `{"state": "cancelling"}` |
| `providers.remove` | `{provider, id}` | `{"removed": true}`: forgets the saved login; the account stays signed in if it is the one in use |

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

A write goes to `notchling.db` (the limit keys `limitMode`, `codexLimitMode`, `antigravityLimitMode` and `limitSwitchAt` first to the daemon's `LimitConfig`, as `limits.set_mode` does), then out on `settings` as `settings.changed {key, value, by: "agent" | "app", ask?: id}`, set or not. The app reads the key back from the store on that event, so an older event never undoes a newer write. The Settings sheet writes the store itself and applies its own change in place; it doesn't go through this op, so it isn't on the topic. Every `settings.set`, from either tier, is in the audit log as op `settings.set` with result `set`, `asked`, `allowed`, `denied`, `expired`, `withdrawn`, `failed`, `refused`, `bad_value`, `unknown_key`, `no_app` or `full`, and `reason` `<key>=<value as JSON>` (cut at 64 and 120 characters). Reads are not audited.

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
| `vault.approval.answer` | app | `{id, decision: "deny"\|"once"\|"always"}` | `{answered}` |
| `vault.request.cancel` | app | `{id}` | `{answered}` |
| `vault.pending` | app | `{}` | `{approvals: [..], requests: [..]}`, the pending events below |

The list is the `daemon/vault` document in `notchling.db` (the daemon's own `vault.json`
moves in once; the Swift app's stays its own); `Entry` is its shape: `{name, kind, note, url?, howTo?, expires?, userVar,
secretVar, ask?, updated}`. No reply but `vault.use`'s carries a value, and no event ever does.

**Approval.** `vault.use` of an entry marked ask-first publishes `vault.approval.pending {id,
names, entries, command, folder, verified}` on the `vault` topic (`command` and `folder` as the
caller claims them, `verified` what the kernel says about it: see [`crate::vault_caller`]), which only the app tier may follow, and
waits up to [`APPROVAL_WAIT`] for `vault.approval.answer`. The id is 128 random bits. The
answer must come from an app-tier client that said hello as the app; an agent-tier client is
refused by the server before this module sees the call. With no app following `vault` the use
is denied at once, and a wait ends denied when the app leaves, the switch goes off or the
daemon stops. Always Allow turns ask-first off for those entries. A denial is `needs_approval`
("The user didn't allow it."), which `notchling secret` exits 3 on.

**Requests.** `vault.request` publishes `vault.request.pending {id, request, entry, existing,
changes}` (the form, filled in: an existing entry with the passed fields laid over it, and what
that changed) and waits up to [`REQUEST_WAIT`] for the app's `vault.save {…, request: id}` or
`vault.request.cancel {id}`. A cancel is `needs_approval` ("The user cancelled.").

**Reconnecting.** `vault.pending` answers the `vault.approval.pending` and
`vault.request.pending` events of the waits still open, oldest first, as they were sent: an
app that (re)connects asks it after following `vault`, so nothing that waits for the user is
missed.

Errors carry names and fixed texts only: never a value, never a parse error that could quote
one. Agent-tier calls are written to the audit log here (op, tier, peer, outcome and the
names used); the server logs every app-tier call.
