# Logging: one typed event stream

- **Date:** 2026-11-13
- **Status:** Draft, for the maintainer. Nothing here is built. No open questions: the user decided the last three on 2026-11-13 ([section 19](#19-decided-by-the-user)).
- **Approach:** A, one typed event stream in the daemon, chosen by the maintainer on 2026-11-13.
- **Base:** branch `agent/logs-activity` (quiet polls in the audit, `notchlingd.log` rotation, the Activity page that reads log files). Building plans against the integration branch `merge/settings-batch`, which merges it with `agent/agent-settings-cli` (decision 0020).
- **Touches:** decisions 0004, 0007, 0011, 0014, and 0020 on `agent/agent-settings-cli`. It needs a decision record of its own (Phase 1).

## In short

1. Everything worth recording becomes an **event** of a registered **kind**. Kinds live in one registry, the way ops live in `wire.rs`. A new feature adds a kind, never a new log file.
2. No event has a free-text field. Field types come from a closed list: choices, yes/no, counts, sizes, durations, times, versions and identifiers. The compiler, two lint rules and a test that plants marker text hold it.
3. The daemon is the only writer. The app and the hook send their events to it over the local API. Each process writes its own diagnostics.
4. Each event goes to the **sinks** its kind names: **Audit**, **Activity**, **Diagnostics**, **Metrics**. Panics still go to **Crash**.
5. The Audit is JSON lines in monthly files. Each line is sealed with a keyed hash that covers the previous line's seal. The key is in a Keychain item only Notchling's daemon reads without a macOS prompt. When the key can't be read, lines are written unsealed and the app says so; a new key is made only when the user asks. It is tamper-evident, not tamper-proof.
6. When an agent's request releases a secret or acts, its audit line is written and flushed to disk before the effect; if it can't be, the request is refused. Everything else is queued.
7. Activity is a table in `daemon/events.db`. Settings > Activity reads it through `activity.list`: same op, same reply, more fields.
8. Diagnostics are one file per process per day in `~/Library/Logs/Notchling/`, kept 7 days. An `op_id` follows one action from the app through the daemon to the hook.
9. Settings > Logs lists every kind with what it records and why that matters. Security kinds are always on. Turning a kind off is itself recorded, and agents can never change these settings.
10. Seven phases, each shippable. Most of them change code outside the auto-merge zone, so they wait for the maintainer.

## Calls settled after review

A builder read the first draft and asked eight questions. The coordinator settled them; each is marked **coordinator's call** and folded into the sections named.

1. **Commit before the effect** (coordinator's call). An agent-tier event of an `Always` kind that releases something or acts (`vault.secret_used`, `vault.secret_requested`, `settings.agent` when it sets or is allowed, and any agent-tier `session.reply` or `session.control`) is written and flushed, and acknowledged, before the secret is released or the action runs. If that fails, the op is refused. Everything else is pushed into the queue. `emit` has two forms, `commit` and `push`. The hook's 2 ms budget covers push events only; no hook kind needs commit. See [7.2](#72-the-daemon), [8.1](#81-audit), [13](#13-performance-budget).
2. **A missing or unreadable key is never replaced silently** (coordinator's call). Daemon start never waits on the Keychain. A key that is missing, refused or still waiting on a prompt puts the Audit in **unsealed** mode (`seal: null` and a reason on each line), with the red banner and an Inbox item. Only the user, from the app, can choose **Start New Chain**, which makes a new key and writes a sealed `audit.chain_started`. The daemon, the hook and the command line share one executable; none has an op that returns the key. Each data folder has its own Keychain item. See [12](#12-tamper-evidence).
3. **Phases 2 and 3 of the first draft are one phase** (coordinator's call), so `activity.list` never reads a half-migrated format. The importer reads both the old `audit.jsonl` and the new months. Existing tests that read audit lines are edited, which needs the maintainer under `WEAK-TEST-EDIT`. The plan is made against `merge/settings-batch`. See [17](#17-phases).
4. **`op.called` for app-tier ops with no kind of their own** (coordinator's call): `{op, tier, result, ms}`, on by default; quiet ops are counted in Metrics only. A test maps every op in `wire.rs` to a kind, `op.called` or quiet. See [6.3](#63-kinds), [7.2](#72-the-daemon).
5. **The app's own start and stop are never sealed** (coordinator's call). Its `process.*` events, spooled or not, go to Activity and Diagnostics only. What the daemon itself sees is audited: `daemon.started`, `daemon.stopped`, `app.attached`, `app.detached` (a dropped connection stands in for an app crash). See [6.3](#63-kinds), [7.6](#76-when-the-daemon-is-down).
6. **`actor.session` is only for agent-tier callers** (coordinator's call), worked out once per connection at hello and kept. An app-tier event names the session it acts on as a field. See [6.1](#61-the-envelope).
7. **The hash salt is its own file** (coordinator's call), `<data>/daemon/log-salt`, not in `events.db`. A lost salt is replaced and `log.salt_rotated` recorded; hashes from before and after don't match. See [11](#11-privacy).
8. **Pruning leaves an anchor** (coordinator's call). Before an Audit month is deleted, a sealed `audit.pruned` line goes into the current month and the same anchor into the Keychain checkpoint; a check starts at the oldest kept month and accepts its first `prev` only if it equals that anchor. See [10](#10-retention), [12](#12-tamper-evidence).

## Contents

1. [Why](#1-why)
2. [Goals, non-goals, anti-goals](#2-goals-non-goals-anti-goals)
3. [Constraints](#3-constraints)
4. [Approaches](#4-approaches)
5. [The questions, and what answers each](#5-the-questions-and-what-answers-each)
6. [The event model](#6-the-event-model)
7. [Producers and the flow](#7-producers-and-the-flow)
8. [Sinks](#8-sinks)
9. [Settings > Logs](#9-settings--logs)
10. [Retention](#10-retention)
11. [Privacy](#11-privacy)
12. [Tamper evidence](#12-tamper-evidence)
13. [Performance budget](#13-performance-budget)
14. [Migration of today's logs](#14-migration-of-todays-logs)
15. [The bug-report bundle](#15-the-bug-report-bundle)
16. [Local API changes](#16-local-api-changes)
17. [Phases](#17-phases)
18. [Left to the builder](#18-left-to-the-builder)
19. [Decided by the user](#19-decided-by-the-user)

## 1. Why

What Notchling writes today, on `agent/logs-activity`:

| File | Writer | Shape | Kept | Problem |
| --- | --- | --- | --- | --- |
| `<data>/audit.jsonl` (+ `.1`) | `local-api` (`conn.rs`, `audit.rs`), `vault_ops.rs`, `switches.rs`, `daemon.rs`; `settings_ops.rs` on `agent/agent-settings-cli` | `{time_ms, op, tier, peer, result, reason?}` | 5 MB, one old copy | Data packed into strings (`ok:gh,reg`, `settings.set_switch vault`, `reason: "key=value"`). No chain: any line can be edited. A vault use is logged only for the agent tier and never says who allowed it. 10 MB in all can be a few weeks. |
| `AgentConsole/bridge/remote-log.jsonl` | `engine::bridge::net::audit` | `{time (ISO), device, op, session?, length?, address?, stoppedPid?}` | never rotated | Every append reads the whole file and writes it back. Another time format. |
| `<data>/account-log.json` | the Swift app only | JSON array `{id, at}`, seconds since 2001 | grows | No writer in `crates/` (checked 2026-11-13). Since the Swift app retired on 2026-11-11 no switch is recorded, so cost gives Claude Code's spend to the account the Swift app recorded last. Being fixed now on its own branch, `agent/account-log-fix`, which writes the file again in its existing format. |
| `<data>/notchlingd.log` (+ `.1`) | the daemon's stdout and stderr, opened by `ensure_daemon` or launchd | free text from 28 `eprintln!` | 5 MB, one old copy | No levels, no time on some lines, nothing ties a line to the action that caused it. |
| `<data>/logs/crash.log` | the app's panic hook | text with backtrace | 1 MB, then `.1` | The app only. A daemon or ptyhost panic reaches `notchlingd.log` or nowhere. |
| (the app's stderr) | 38 `eprintln!` in `crates/app` | free text | nowhere | Lost when the app starts from Finder. |

Questions people ask that no file answers: who allowed this secret, Mac or phone; why my message never left Queued; was this log edited; what the app did while the daemon was down; which version ran when this failed; which ops are slow. And each new feature that wants a record adds a file in its own format, which Activity must then learn to parse.

## 2. Goals, non-goals, anti-goals

**Goals**

1. Every question in [section 5](#5-the-questions-and-what-answers-each) has a named kind and sink.
2. A new feature adds a kind (one registry entry and its typed fields), never a log file or a format.
3. No log holds message text, prompts, tool input or output, file contents, diffs, command arguments, tokens, cookies or secret values. Types, lint and a test enforce it, not review.
4. The Audit shows when an agent running as the user changed it, and the app says so plainly.
5. Logging never slows the UI or holds up a reply or an approval from the user.
6. The user can see, keep, turn off (except security kinds), export and clear their logs, in plain words.

**Non-goals**

- Sending logs, metrics or crash reports anywhere. Opt-in crash reports are a later, separate design (privacy.md, Planned).
- Tamper-proof logging against root, against someone who allows macOS's Keychain prompt, or against a debugger attached to the daemon.
- Replacing transcripts. Content stays in each tool's own transcript; events point at sessions by id.
- Unified logging (`os_log`, Console.app) in this design. See [8.3](#83-diagnostics).
- The phone's own diagnostics.

**Anti-goals** (failures even if everything works)

- A log line holds content: a prompt, a message, tool output, a diff, a command's arguments, a token, a cookie, a vault value.
- Logs leave the Mac without the user pressing a button that says so. Logging turns into telemetry.
- An agent turns a log off, shortens how long it is kept, or edits the Audit, and nothing shows it.
- An agent gets a secret, or acts, and no record of it survives a crash.
- Logging makes Chat stutter, or a full disk or a stuck sink blocks a reply or an approval from the user.
- Polls, reloads and other noise reach the Activity page or push security lines out.
- A kind the user can't understand: each has a title, what it records and why it matters.

## 3. Constraints

- **Licenses** (decision 0008, `deny.toml`). Only MIT, Apache and BSD crates. `tracing` (MIT) is already in `Cargo.lock` through GPUI; `tracing-subscriber` (MIT) is new. `hmac` (MIT/Apache) is already locked at 0.12.1, and `sha2` is already an engine dependency. `tracing-oslog` is Zlib: `cargo deny` would pass it, since `deny.toml` lists Zlib, but 0008 and goals.md say MIT, Apache or BSD, so it is not taken. `tracing-appender` (MIT) is optional ([8.3](#83-diagnostics)).
- **Privacy** (`docs/policies/privacy.md`). It lists every log file. Each phase that adds, moves or removes one changes that page in the same pull request. Still no telemetry.
- **Zones and maintainer-gate.** `crates/app/**` and `crates/kit/**` are in the auto-merge zone; `crates/engine` (outside `transcript/` and `search/`), `crates/notchlingd`, `crates/local-api`, `crates/platform`, every `Cargo.toml`, `docs/rules.md`, `docs/decisions/`, `docs/policies/` and `docs/superpowers/` are not. New file writers trip `CAP-FILEWRITE`, the audit key trips `CAP-KEYCHAIN`, and showing a folder in Finder is `CAP-PROCESS`. Editing existing tests is `WEAK-TEST-EDIT` ([17](#17-phases) lists the ones expected).
- **The local API changes additively only.** New ops, new optional fields. `Request` has no `deny_unknown_fields`, so an old daemon ignores a new `op_id`, and clients ignore reply keys they don't know (`wire.rs::Reply`). A line is at most 1 MiB.
- **Files at rest are not a tier boundary** (0007, note of 2026-11-08). Log settings, the salt, spool files and old logs on disk are claims, never facts the daemon trusts.
- **Code rules.** No comments in code: the why goes in `crates/events/AGENTS.md`. Files under 1,000 lines (warning at 600), so the registry is split by group. `app` and `kit` deny `unwrap` and `expect` outside tests.
- **Tests** never touch the real Keychain, `~/Library/Logs`, `~/.claude` or the network. Every root is a parameter; the audit key has a memory fake, and a fixture root keeps it in a file.
- **Platforms.** Everything builds and is tested on macOS, Linux and Windows. Only the Keychain anchor is macOS-only; elsewhere the anchor is weaker ([12](#12-tamper-evidence)).

## 4. Approaches

| | **A. One typed event stream** (chosen) | **B. Keep five files, normalize, rotate** | **C. OpenTelemetry with a collector** |
| --- | --- | --- | --- |
| What it is | A registry of kinds; the daemon writes every event; sinks are views of one stream | What `agent/logs-activity` does: each writer keeps its file; Activity reads and normalizes their tails; rotation per file | OTel SDK in each process, a local collector process, exporters to files |
| New feature | Adds a kind | Adds a file, a format and an Activity parser | Adds spans and attributes, by convention |
| No content in logs | Enforced by types | By review of each writer | By convention; OTel's GenAI fields carry content when turned on |
| Tamper evidence | Keyed chain, Keychain anchor | None | None built in |
| Activity | SQL rows, indexed | Parses up to 6 MB of tails on every open | Query a trace store |
| Correlation | `op_id` across processes | None | Trace context, its best part |
| Cost | A new crate, a writer thread, a migration | Smallest: mostly done | Big dependency tree, one more process to keep alive, exporters that reach the network (`CAP-NETWORK`) and make telemetry one setting away |

**A was picked by the maintainer** on 2026-11-13: one place to add kinds, privacy enforced by types instead of review, an Audit that shows tampering, and Activity from a table rather than from parsing files, without running a collector. B stays the fallback that already works: Phase 2 turns its file readers into the one-time importer. C's field names can be borrowed for an export later, if one is ever wanted.

## 5. The questions, and what answers each

Kinds are named `area.what`; [6.3](#63-kinds) lists their fields. "Activity" means Settings > Activity; "Logs" means Settings > Logs.

**The user**

| Question | Kinds | Sink | Where they see it |
| --- | --- | --- | --- |
| What did agents do on my Mac today? | `session.reply`, `session.answered`, `session.control`, `vault.secret_used`, `settings.agent`, `tool.turn`, `delegate.job` | Activity (and Audit for most) | Activity, Today; `notchling log --since today` |
| Who answered this approval, Mac or phone? | `vault.secret_used.answered_by`, `inbox.closed.by`, `session.answered.by`, `settings.agent.answered_by` (with `device`) | Audit, Activity | The row says "Allowed Once · iPhone" |
| Why is my message stuck in Queued, or not sent? | `session.reply` (`result`, `why`, `waited_ms`), `session.reply_held`, `session.queue_stuck` (`guess`), `hook.decided` | Activity, Diagnostics | Activity, Agents; the `op_id` chain in Diagnostics |
| When and why did my account switch? | `account.switched` (`cause`, `window`, `used_pct`), `limits.crossed` | Activity | Activity, Accounts and Limits |
| How many tokens and how much money today? | Today: the cost engine's live totals. Past days: `usage.daily` | Activity | Accounts' cost view, as now; Activity for past days |

**The owner, for security**

| Question | Kinds | Sink | Where |
| --- | --- | --- | --- |
| Did any agent try something forbidden? | `api.refused`, `api.unknown_peer`, `settings.agent` (refused), `vault.secret_used` (denied) | Audit, Activity | Activity, Security; refusals stand out |
| Which secret was taken, by which session, and who allowed it? | `vault.secret_used` (committed before the value is released) | Audit, Activity | The Activity row and its details; the sealed Audit line |
| When was each switch turned on or off? | `switch.changed`, `daemon.started` (a restart turns every switch off) | Audit, Activity | Activity, Security |
| Which phone connected, and what did it do? | `phone.paired`, `phone.connected`, `phone.acted`, `phone.answered` | Audit, Activity | Activity, Phone |
| Was the log altered? | `audit.checked`, `audit.chain_started`, `audit.pruned` | Audit, Activity | Logs header: "Audit log intact · checked 2 minutes ago", or a red banner and an Inbox item |
| Does any log hold a prompt, a token or message text? | None, by construction: no field type can hold text ([6.2](#62-field-types)) | — | The Logs page says so; the canary test ([6.5](#65-registry-tests-and-lint)) proves it on every build |

**The maintainer**

| Question | Kinds | Sink | Where |
| --- | --- | --- | --- |
| When and where did an error happen, and what led to it? | `op.failed`, `op.called`, diagnostic points, all sharing an `op_id` | Diagnostics, Activity | `notchling log trace <op_id>` merges app, daemon and hook files |
| Did the daemon die or restart? Did the app? Panics with a stack? | `daemon.started` (`after`), `daemon.stopped`, `app.attached`, `app.detached` (`dropped`), `process.*` for the app and ptyhost | Audit (daemon-seen), Activity, Crash | Activity; `logs/crash.log` |
| Which ops are slow? Main-thread stalls? | `perf.op`, `perf.stall`, `perf.poll`, `perf.reload` | Metrics | Logs, Performance; the bundle |
| Which app, macOS and Claude Code versions ran then? | `daemon.started` and `app.attached` (versions), `process.started` (joined on `boot`), `tool.version_seen` | Audit, Activity | Each row's details; the bundle |
| A redacted bundle for a bug report? | All Diagnostics and Metrics in a window | Bundle | Logs, Export Bug Report… ([15](#15-the-bug-report-bundle)) |

**Later features**

| Need | How this design meets it |
| --- | --- |
| New features add a kind, not a log | The registry and its tests ([6.5](#65-registry-tests-and-lint)) |
| One stream feeds Activity, the phone, the Inbox and search | Sinks and the `log` topic read one stream; the Inbox raises an item on `audit.checked` failures; the phone and search read Activity later ([17](#17-phases), Phase 7) |
| `notchling log --json`, an agent reads only its own events | `log.read`: agent tier gets only events whose `actor.session` is its own, of kinds marked readable ([16](#16-local-api-changes)) |
| A machine id on every event | Envelope field `host` ([6.1](#61-the-envelope)) |
| Retention per kind in Settings | Logs page ([9](#9-settings--logs)) |

## 6. The event model

### 6.1 The envelope

Every event, in every sink, has the same outer fields. Audit and Diagnostics write it as one JSON line; Activity stores it as columns plus the kind's fields.

| Field | Type | Meaning |
| --- | --- | --- |
| `v` | 1 | Envelope version |
| `kind`, `kv` | registry name, kind version | What happened, and which version of its fields |
| `ts` | ms since 1970, UTC | When it happened, by the emitting process's clock |
| `ts_rx` | ms | When the daemon got it, only for events from another process |
| `seq` | u64 | The daemon's sequence number, never reused (kept in `events.db` and the checkpoint). Order by `seq`, not `ts` |
| `host` | 16 hex | The machine: decision 0012's host id (the first 16 hex of the bridge certificate's SHA-256). Until a certificate exists, a random id made once per data folder; when the certificate appears, `host.identity` records old and new |
| `proc`, `pid`, `boot` | `daemon`/`app`/`hook`/`cli`/`ptyhost`, u32, 8 hex | Who wrote it; `boot` is random per process run, and that run's start event holds its versions |
| `op_id` | 16 hex, optional | Ties every event of one user or agent action across processes ([7.5](#75-op_id)) |
| `actor` | object | Who caused it, filled by the daemon from the kernel, never from what a client says: `tier` (`app`, `agent`, `daemon`, `phone`), `via` (`app`, `cli`, `hook`, `bridge`; the hello's self-reported kind), `exe` (executable name), and `device` for phone events. **`session` only for an agent-tier caller** (coordinator's call): the Claude Code session whose process the caller descends from, worked out once per connection at hello the way `hook.request` does it (`sessions/<pid>.json` over the peer's process tree) and kept for the connection, since the peer can't change on it; the exec-race check still runs on every app-tier request. App-tier events have no actor session: the session they act on is a field (`f.session`) |
| `outcome` | `ok`, `refused`, `failed`, `waiting`, `info` | A coarse class the kind maps its result to, for colors and filters |
| `repeat` | count, optional | The same event this many times in its window ([13](#13-performance-budget)) |
| `spooled` | flag, optional | Sent by the app after the daemon was away ([7.6](#76-when-the-daemon-is-down)) |
| `f` | object | The kind's own fields |

An event is at most 4 KB as JSON. Lists in fields are capped (16 items).

### 6.2 Field types

This list is the whole allowlist. There is no text type.

| Type | Written as | Notes |
| --- | --- | --- |
| `Choice` (a kind's own enum) | `"allow_once"` | Values are declared in the registry; the Logs page and docs list them |
| `Flag` | `true` | |
| `Count`, `Bytes`, `Tokens` | integer | `Bytes` and `Count` stand in for text: a message is its character count |
| `Millis`, `Time` | integer | A duration; a time in ms since 1970 |
| `Money` | integer micro-dollars | No floats |
| `Percent` | 0 to 100 | |
| `Version` | `"2.1.322"` | `[0-9A-Za-z.+-]`, at most 32 |
| `Op` | `"session.reply"` | Must be a name in `wire.rs` (`ops`), else `unknown` |
| `ErrorCode` | `"forbidden"` | `wire::ErrorCode` |
| `Exe` | `"gh"` | The file name of an executable, never its folder or arguments; `[A-Za-z0-9._+-]`, at most 64 |
| `Tool` | `"Bash"`, `"mcp__github__create_issue"` | A tool or slash command name; `[A-Za-z0-9_:.-]`, at most 64 |
| `CodeLoc` | `"crates/app/src/chat/view/mod.rs:812"` | A place in Notchling's own source |
| `Seal` | base64, 44 characters | An Audit seal, only in `audit.*` kinds |
| `Ref<T>` | the id, or `"h:3f9a0c1b"` | An identifier of a session, folder, account, device, secret name, lane, inbox item, page, model or machine. At most 256 characters, no control characters. How it is written depends on the sink ([11](#11-privacy)) |

An external error (I/O, SQLite, JSON, HTTP) never goes in as its message, which can quote paths, input or tokens. It goes in as a `Choice`: `io_kind` (`not_found`, `permission_denied`, …), `sqlite` (`busy`, `corrupt`, …), or the area's own `why`.

### 6.3 Kinds

The first registry. **Lock** is `Always` (on, greyed in Settings, with the reason shown) or `Toggle`. **Sinks**: A Audit, Ac Activity, D Diagnostics, M Metrics. **Commit** marks the kinds an agent-tier op writes before its effect ([7.2](#72-the-daemon)). Every kind also has `records` and `why` texts, shown on the Logs page ([9](#9-settings--logs)); "Records" below is a short form.

**Security**

| Kind | Title | Lock | Sinks | Records (fields) |
| --- | --- | --- | --- | --- |
| `api.refused` | Refused Requests | Always | A Ac D | A program asked for what it may not do: `op`, `exe`, `tier`, `why` (`forbidden`, `peer_changed`, `needs_approval`, `switch_off`, `not_descendant`, `audit_unwritable`), `switch` |
| `api.unknown_peer` | Unknown Programs | Always | A Ac D | A connection the daemon couldn't identify, or from another user: `why` (`peer_unknown`, `other_user`), `exe` |
| `switch.changed` | Switches On and Off | Always | A Ac D | `switch` (`acting`, `bridge`, `accounts`, `vault`, `install`), `on`, `cause` (`user`, `restored_at_launch`, `daemon_start`, `file_said_on_ignored`, `restore_failed`) |
| `daemon.started` | Daemon Started | Always | A Ac D | `daemon` and `macos` versions, `data_root` (`real`, `fixture`), `after` (`clean_stop` when the Audit's last line is a `daemon.stopped`, else `unknown`), `sealing` (`sealed`, `unsealed`, `waiting`) |
| `daemon.stopped` | Daemon Stopped | Always | A D | `cause` (`quit`, `signal`, `replaced`, `update`), `uptime_ms` |
| `app.attached` | App Connected | Always | A Ac | The app connected with the app tier: `version` (from its hello), `exe`, `pid` |
| `app.detached` | App Disconnected | Always | A Ac | `cause` (`quit`: the app said it was quitting; `dropped`: the connection ended without that, which stands in for a crash or a kill), `attached_ms` |
| `audit.checked` | Audit Log Checks | Always | A Ac D | `result` (`intact`, `broken`, `gap`, `tail_missing`, `file_missing`, `unanchored`, `unsealed`, `resumed`), `from_n`, `to_n`, `first_bad_n` |
| `audit.chain_started` | Audit Log Started | Always | A Ac | A new key and chain: `reason` (`first_run`, `key_missing`, `key_denied`, `user`), `previous_head` (`n` and `seal` of the last sealed line found, if any) |
| `audit.pruned` | Old Audit Months Removed | Always | A Ac | Written, sealed, before a month is deleted: `through_month`, `last_seal` (`Seal`), `lines`, `reason` (`retention`, `size_cap`, `user`), `user_deleted` (every deletion the user ever made, carried forward: the newest 16 and a count) |
| `log.setting_changed` | Logging Settings Changed | Always | A Ac | `target` (a kind or a sink), `on`, `keep_days`, `detail`, `settings_hash` |
| `log.settings_restored` | Logging Settings Reset | Always | A Ac | Settings on disk didn't match the Audit's record, so defaults came back: `kinds_changed` (count) |
| `log.pruned` | Old Activity and Diagnostics Removed | Always | A Ac | `sink` (`activity`, `diagnostics`), `from`, `to`, `entries`, `reason` (`retention`, `size_cap`, `user`) |
| `log.dropped` | Entries Not Recorded | Always | A Ac D | `sink`, `count`, `reason` (`queue_full`, `rate_limited`, `write_failed`, `spool_full`, `invalid`) |
| `log.salt_rotated` | Diagnostics Hashes Changed | Always | A Ac | The salt file was missing or unreadable and a new one was made: `old_id`, `new_id` (8 hex of each salt's own hash) |

**Secrets**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `vault.secret_used` | Secrets Given to Commands | Always, **commit** | A Ac | `secrets` (`Ref<Secret>` list), `session`, `folder`, `command_exe` (`Exe`), `approval` (`always_allow`, `asked`), `answered_by` (`mac`, `phone`, `none`), `device`, `result` (`given`, `denied`, `expired`, `cancelled`, `no_app`, `failed`), `waited_ms` |
| `vault.secret_requested` | Secrets Asked For | Always, **commit** | A Ac | An agent asked you to type a new secret: `name`, `session`, `folder`, `result` (`saved`, `cancelled`, `expired`, `withdrawn`), `answered_by` |
| `vault.entry_changed` | Vault Entries Saved or Removed | Always | A Ac | `name`, `action` (`saved`, `removed`, `ask_first_on`, `ask_first_off`) |

**Agents**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `session.reply` | Messages Sent | Toggle, on; **commit** from the agent tier | A Ac D | `session`, `via` (`chat`, `notch`, `phone`, `held`, `agent`), `chars`, `images`, `result` (`sent`, `failed`), `why` (`no_terminal`, `not_idle`, `attach_failed`, `timeout`, `acting_off`, …), `waited_ms` |
| `session.reply_held` | Messages Held | Toggle, on | Ac D | A message kept until the turn ended: `session`, `then` (`sent_at_turn_end`, `replaced`, `cancelled`, `session_gone`) |
| `session.queue_stuck` | Messages Stuck in Queued | Toggle, on | Ac D | From the app, once per entry Queued over 2 minutes: `session`, `waited_ms`, `guess` (`no_dequeue_seen`, `claude_restarted`, `still_working`, `bash_input_unmatched`) |
| `session.answered` | Questions and Permissions Answered | Toggle, on | A Ac | `session`, `prompt` (`question`, `permission`, `plan`), `tool`, `answer` (`allow_once`, `allow_always`, `deny`, `option`, `typed`), `by` (`chat`, `notch`, `phone`), `device`, `waited_ms` |
| `session.control` | Sessions Started, Stopped or Moved | Toggle, on; **commit** from the agent tier | A Ac | `action` (`new`, `stop`, `remove`, `move`, `move_all`, `trust_folder`, `interrupt`, `take_back`, `slash_command`), `command` (`Tool`), `session`, `folder`, `account`, `result` |
| `hook.decided` | Prompts From Claude Code | Toggle, on | Ac D | Why a prompt was shown in Notchling or left to Claude Code's terminal: `session`, `event`, `tool`, `outcome` (`asked_app`, `passed_no_app`, `passed_acting_off`, `passed_too_big`, `passed_timeout`, `refused_not_descendant`), `waited_ms` |
| `tool.turn` | Codex and Antigravity Turns | Toggle, on | Ac D | `tool` (`codex`, `agy`), `session`, `model`, `result` (`done`, `failed`, `stopped`), `duration_ms`, `tokens_in`, `tokens_out` |
| `delegate.job` | Jobs Handed to Another Tool | Toggle, on | Ac D | Sent by `notchling delegate` itself, so it is the agent's own account: `tool`, `model`, `write`, `result`, `duration_ms` |
| `pages.saved` | Artifact Copies | Toggle, on | Ac | `page`, `action` (`published`, `read_redirected`), `account` |
| `tool.version_seen` | Tool Versions | Toggle, on | Ac D | The first time a Claude Code, Codex or Antigravity version is seen, read from what the daemon already reads, never by running the tool: `tool`, `version` |

Agent-tier `session.reply` and `session.control` don't exist today (those ops are app tier); the commit rule covers them when one is added.

**Inbox and Approvals**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `inbox.raised` | Items That Need You | Toggle, on | Ac | `item_kind` (`secret_approval`, `secret_request`, `limit_notice`, `setting_ask`, `log_warning`), `item` (always hashed: an id can hold a vault ticket), `session` |
| `inbox.closed` | Items Answered or Closed | Toggle, on | A Ac | `item_kind`, `item`, `state` (`answered`, `expired`, `withdrawn`), `action`, `by` (`mac`, `phone`), `device`, `waited_ms` |

**Accounts and Limits**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `account.login` | Logins Added or Removed | Always | A Ac | `provider` (`claude`, `claude_desktop`, `codex`, `agy`), `account`, `action` (`added`, `removed`, `renewed_saved`), `result` |
| `account.switched` | Account Switches | Toggle, on | A Ac | `provider`, `from`, `to`, `cause` (`user`, `limit_auto`, `move_all`, `lane`), `window` (`five_hour`, `weekly`, …), `used_pct`, `result` |
| `limits.crossed` | Limit Warnings | Toggle, on | Ac | `provider`, `account`, `window`, `used_pct` (50, 75, 90, 100), `resets_at` |
| `usage.daily` | Daily Use and Cost | Toggle, on | Ac | Once a day per account and model: `day`, `provider`, `account`, `model`, `tokens_in`, `tokens_out`, `cache_read`, `cache_write`, `cost` (`Money`), `priced` |
| `lane.changed` | Lanes | Toggle, on | A Ac | `lane`, `action` (`created`, `removed`, `switched`), `account` |

**Phone**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `phone.paired` | Phones Paired or Removed | Always | A Ac | `device`, `action` (`paired`, `revoked`, `refused`), `via` (`qr`, `code`) |
| `phone.connected` | Phone Connections | Always | A Ac | `device`, `via` (`lan`, `tunnel`), `action` (`connected`, `disconnected`), `duration_ms`, `ops`. Reconnects within 5 minutes fold into one row |
| `phone.acted` | What Phones Did | Always | A Ac | Replaces `remote-log.jsonl`: `device`, `op` (`message`, `send_now`, `answer`, `new_session`, `stop`, `image`, …), `session`, `chars` |
| `phone.answered` | Phone Answers | Always | A Ac | `device`, `item_kind`, `action`, `result` (`taken`, `acting_off`, `invalid`, `open`, `not_found`, `not_taken`) |
| `phone.viewed` | What Phones Read | Toggle, **off** | Ac | `device`, `view` (`sessions`, `conversation`, `terminal`, `page_image`), `session` |
| `bridge.tunnel` | Tunnel | Toggle, on | Ac D | `action` (`keys_fetched`, `token_refused`, `connected`), `result` |

**Settings**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `settings.agent` | Settings Changed by Agents | Always, **commit** when it sets or is allowed | A Ac | Decision 0020's `settings.set`: `key`, `class` (`free`, `ask`, `never`), `value`, `result` (`set`, `asked`, `allowed`, `denied`, `expired`, `refused`, `unknown_key`, `bad_value`, `failed`, `no_app`, `full`), `answered_by`, `device` |
| `settings.changed` | Your Settings Changes | Toggle, on | Ac | Changes made in the app: `key`, `value` |
| `install.hook` | Hook Installed or Removed | Always | A Ac | `target` (`claude`, `codex`, `agy`), `action` (`installed`, `removed`, `repaired`), `result` |

A setting's `key` is a `Choice` over `settings/keys.rs`, or `unknown` with `key_hash`. Its `value` is written as-is only when the key's type is a flag, a choice or a number; a text value is written as `value_chars` and `value_hash`. Today's line puts `key=value` in `reason`, cut to 64 and 120 characters: an agent could put anything in an unknown key's name or a text value.

**Diagnostics**

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `process.started` | App and Terminal Host Started | Always | Ac D | From the app and `notchling-ptyhost`, spooled when the daemon is away, **never sealed**: `proc`, versions |
| `process.stopped` | App and Terminal Host Stopped | Always | Ac D | `proc`, `cause`, `uptime_ms`; never sealed |
| `process.crashed` | Crashes | Always | Ac D | Any process, read from `crash.log` at its next start, so never sealed: `proc`, `at` (`CodeLoc`), `message_hash`, `thread`; the stack stays in `crash.log` |
| `op.called` | Other Requests From the App | Toggle, on | A Ac | An app-tier op with no kind of its own (coordinator's call): `op`, `tier`, `result` (`ok` or an `ErrorCode`), `ms`. Quiet ops never produce it; they are counted in `perf.poll` |
| `op.failed` | Failed Operations | Toggle, on | Ac D | `op`, `code` (`ErrorCode`), `why` (the area's `Choice`), `duration_ms` |
| `daemon.unreachable` | Daemon Not Reachable | Toggle, on | Ac D | From the app: `why` (`no_socket`, `refused`, `timeout`, `untrusted_server`, `version`), `count` |
| `host.identity` | Machine Id | Always | A Ac | `old`, `new` |

**Performance** (counts and timings only, rolled up; no rows in Activity)

| Kind | Title | Lock | Sinks | Records |
| --- | --- | --- | --- | --- |
| `perf.op` | Operation Times | Toggle, on | M | Per op and result: a histogram of milliseconds |
| `perf.poll` | Background Checks | Toggle, on | M | Per quiet op: a count of successes |
| `perf.stall` | Slow Moments in the App | Toggle, on | M D | Main-thread work over 50 ms: `part` (`chat_root`, `conversation`, `sidebar`, `details`, `notch`, `other`), a histogram |
| `perf.reload` | Chat Reloads | Toggle, on | M | `bytes_parsed`, `rows_built`, milliseconds |

**Every op has a home** (coordinator's call). `crates/notchlingd/src/op_kinds_tests.rs`, built like the branch's `quiet_ops_tests.rs` (which it replaces), lists every op name in `wire.rs` and the daemon's own op constants as one of: a named kind (`session.reply` → `session.reply`, `vault.use` → `vault.secret_used`, `hello` → `app.attached` for the app tier), `op.called`, or quiet (success counted in Metrics only: today's `QUIET_READS`, `git.*`, `activity.list`, and agent-tier reads such as `sessions.list` and `catalog.get`). An op missing from the list fails the test, and an op that acts, writes or touches the vault can't be quiet. Refusals and failures of any op are always `api.refused` and `op.failed`, whatever its class.

### 6.4 Versioning

- `v` (the envelope) changes only by adding optional fields. It stays 1.
- `kv` (a kind's version) goes up when a field is removed, renamed or changes meaning. Adding an optional field or a choice value doesn't change it; readers treat unknown values as `other`.
- A golden file, `tests/golden/log-kinds.json`, holds every kind's name, version and fields. A field change without a version bump changes the golden, which is a protected test edit (`WEAK-TEST-EDIT`), so the maintainer sees it.
- A kind is never deleted, only marked `retired`, so old Audit lines stay readable.

### 6.5 Registry, tests and lint

The registry is a new leaf crate, **`crates/events` (`notchling-events`)**, which `engine`, `local-api`, `notchlingd`, `ptyhost` and `app` depend on. It holds the envelope, the field types, the kinds (one file per group: `kinds/security.rs`, `kinds/agents.rs`, …), the `diag!` macro, the diagnostics writer and the sinks' pure parts. ("Event" here is a log event, not a local API topic event in `wire.rs`.)

A kind is declared once, and a `macro_rules!` macro makes both its typed struct and its entry:

```rust
kinds! {
    group: Group::Secrets,
    VaultSecretUsed = "vault.secret_used", kv 1 {
        title: "Secrets Given to Commands",
        records: "Each time a command an agent ran got a secret from the vault: which secrets, which session and folder, which program, and who allowed it.",
        why: "Shows which agent used which secret, and whether you, your phone or an Always Allow rule let it.",
        default: On,
        lock: Lock::Always("Every use of a secret is part of the audit trail."),
        sinks: AUDIT | ACTIVITY,
        durability: Durability::CommitFromAgent,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::Own,
        keep_days: 365,
        fields {
            secrets: List<Ref<Secret>>,
            session: Option<Ref<Session>>,
            folder: Option<Ref<Folder>>,
            command_exe: Option<Exe>,
            approval: Approval,
            answered_by: Option<Answerer>,
            device: Option<Ref<Device>>,
            result: SecretResult,
            waited_ms: Option<Millis>,
        }
    }
}
```

The macro accepts only field types that implement `LogField`, a sealed trait implemented in `field.rs` for the types in [6.2](#62-field-types). `String`, `&str` and `serde_json::Value` don't implement it, so a text field does not compile. `log_choice!` declares a kind's enums and gives the registry their values.

**Registry tests** (`crates/events/src/registry_tests.rs`):

- `every_kind_has_title_records_why_default_and_lock`: title in Title Case, at most 40 characters; `records` and `why` one or two sentences, at most 200 characters, ending in a period; `Always` means on by default with a reason; `Toggle` has no reason. (The struct has no optional title, so a missing one doesn't compile; the test catches empty and badly formed ones.)
- `names_are_unique_and_dotted`: `^[a-z]+(\.[a-z_]+)+$`, no two alike.
- `owner_kinds_are_always_on`: an explicit list of the kinds that answer the owner's questions must be `Always`. Unlocking one means editing this test, which needs the maintainer.
- `commit_kinds_are_audited_and_always`: every kind with a commit durability has the Audit sink; `vault.secret_used`, `vault.secret_requested` and `settings.agent` are on the list by name.
- `agents_cannot_emit_locked_kinds`: no `Always` kind lists `Proc::Hook` or `Proc::Cli` as an emitter, and no kind an agent may emit has the Audit sink.
- `app_lifecycle_is_never_sealed`: `process.*` kinds have no Audit sink.
- `field_names_and_choices_are_well_formed`: snake_case, at most 24 fields, no empty choice list.
- `kinds_match_the_golden` and `docs_list_every_kind`: `docs/log-kinds.md` is generated by `notchling log kinds --markdown` and compared.

**Lint rules** (new rows in `docs/rules.md`, checks in `tools/guard`, each with its bad example for `GUARD-RULES`):

- `LINT-LOG-NO-TEXT`: in `crates/events/src/kinds/**`, no `String`, `str`, `Value` or `Vec<u8>`; no `impl LogField` outside `crates/events/src/field*.rs`.
- `LINT-LOG-ONE-WRITER`: no `eprintln!`, `println!`, `tracing::` or `log::` macros in `crates/*/src` outside tests, `crates/events`, and the command-line output files (`clients.rs`, `pages_cli.rs`, `vault_cli.rs`, `delegate_cli.rs`, `settings_cli.rs`, `service.rs`). Today's uses (76 `eprintln!` outside tests on this branch, some in those output files) go into the lint baseline, which only shrinks.

**The canary test** (`crates/notchlingd/tests/log_canary_e2e.rs`) runs the daemon over a copied fixture with fakes and plants a marker in every place content could come from: a reply's text, a hook event's tool input, a vault command's arguments and the secret's value, a `settings.set` text value and an unknown key's name, an error that would quote a path, a phone message, an `op_id` that is not hex. It then reads every file under the data root and the logs folder, `events.db` included, and every file of a bug-report bundle, and fails if a marker is anywhere. The one exception is `crash.log`, which may hold a panic's message on this Mac; the bundle must not.

## 7. Producers and the flow

### 7.1 The flow

```
 app (GPUI)            hook                notchling CLI      phone
  push() ─┐             │                    │                 │ bridge (in daemon)
  queue   │ log.emit    │ hook.request       │ its ops,        │
  thread ─┴──────┐      │ (+ log.emit)       │ delegate.job    │
                 ▼      ▼                    ▼                 ▼
          ┌──────────────── daemon ──────────────────────────────────┐
          │ local API: tier from the kernel, actor filled in          │
          │ push() or commit(): check against registry, settings;     │
          │   stamp op_id, seq, host; bounded queues (Always/Toggle)  │
          │                    │                                      │
          │              writer thread (a commit flushes at once)     │
          │   ┌──────────┬─────┴─────┬───────────┬──────────┐         │
          │   ▼          ▼           ▼           ▼          ▼         │
          │ Audit     Activity   Diagnostics   Metrics    topic "log" │
          │ audit/    events.db  ~/Library/    events.db  (app tier)  │
          │ 2026-11   activity   Logs/…/       metrics                │
          │ .jsonl                daemon-…                            │
          │   │ seal checkpoint, prune anchor                         │
          │   ▼                                                       │
          │ Keychain: audit key, head (one item set per data folder)  │
          └───────────────────────────────────────────────────────────┘
 app and hook also write their own Diagnostics files; panics go to crash.log
```

Readers: Settings > Activity (`activity.list`), Settings > Logs (`log.settings.get`, `log.verify`, `log.chain.start`), the Inbox (an item on a broken or unsealed chain, or a failed Audit write), `notchling log` (`log.read`), and later the phone and search.

### 7.2 The daemon

- **Two forms of `emit`** (coordinator's call):
  - `push(event)` on any thread: validate against the registry (a debug build panics on a mismatch; release drops it and counts `log.dropped{invalid}`), drop it if the user turned the kind off (counted in Metrics), stamp `seq`, `host`, `boot` and the current `op_id`, and put it in one of two bounded queues. No I/O, no lock held across I/O.
  - `commit(event) -> Result<Committed, Unwritable>` for agent-tier events of commit kinds: the same checks, then it waits for the writer's acknowledgement that the line is written and flushed (`F_FULLFSYNC` on macOS, `fsync` elsewhere). Only then does the op release the secret, write the setting or act. No acknowledgement within 2 s, or a write or flush error, and the op answers `failed` ("the audit log can't be written"), is recorded as `api.refused{why: audit_unwritable}` once writing works again, and nothing is released. In unsealed mode ([12](#12-tamper-evidence)) a commit still succeeds: the line is on disk, only not sealed, and the banner is up.
  - Which events commit is data on the kind (`Durability::CommitFromAgent`): `vault.secret_used`, `vault.secret_requested`, `settings.agent` (set or allowed), and agent-tier `session.reply` and `session.control`. App-tier calls, the phone, and every other kind push.
- One **writer thread** owns every sink. It takes a batch (up to 250 ms or 500 events), appends Audit lines with one write, writes Activity rows in one transaction, hands Diagnostics lines to the daemon's file, adds to Metrics, and publishes Activity rows on the `log` topic. A commit ends the current batch at once; commits that arrive together share one flush.
- Existing audit call sites move to kinds: `conn.rs` (`api.refused`, `api.unknown_peer`, `app.attached`, `app.detached`, `op.called`, `op.failed`), `vault_ops.rs`, `switches.rs`, `daemon.rs` (`daemon.*`, `switch.changed{file_said_on_ignored}`), the bridge router (`phone.*`), `inbox_ops.rs`, `settings_ops.rs` (decision 0020). An app-tier op that succeeds is recorded as its own kind, else `op.called`; quiet ops become `perf.poll` counts.

### 7.3 The app, the hook and the command line

They send events with **`log.emit`** ([16](#16-local-api-changes)), which is always a push. The daemon checks each event's kind against its `emitters` and the caller's tier and fills `actor` itself.

- **App** (app tier): `process.*` for the app, `settings.changed` (the Settings sheet writes the store itself, decision 0020), `session.queue_stuck`, `daemon.unreachable`, `perf.stall`, `perf.reload`, app-side `op.failed`. `push` is a non-blocking put into a 2,048-event queue; a background thread sends batches every 500 ms or 64 events. UI threads never touch the socket. Before quitting, the app sends `process.stopped`, which is how the daemon tells `app.detached{quit}` from `{dropped}`.
- **Hook** (agent tier): the daemon already records `hook.decided` for every `hook.request`. The hook sends `log.emit` only for a pass it decided alone while the daemon is up (an event over the 1 MiB line: `passed_too_big`). It writes its own Diagnostics line, one `O_APPEND` write. Its 2 ms budget is for push only; no hook kind commits.
- **Command line** (agent tier): its effects are daemon ops, already recorded, except `notchling delegate`, which runs the other tool itself and sends `delegate.job`. An agent-tier event is the agent's own account, so it reaches Activity and Diagnostics, never the Audit.
- An agent-tier caller may emit only kinds that list it, never an `Always` kind (a refused `log.emit` is itself `api.refused`), at most 20 events a second and 1,000 an hour per connection.

### 7.4 The phone

The bridge runs inside the daemon, so phone events are emitted there, with `actor.tier = phone` and `actor.device` from the paired device. `remote-log.jsonl` stops growing ([14](#14-migration-of-todays-logs)).

### 7.5 `op_id`

- 64 random bits as 16 lowercase hex, made where an action starts: an app click, a hook event, a phone request, a command-line call.
- The local API request gets an optional top-level `op_id`; the reply echoes it as an extra field. Events published on topics carry it in their data where one exists (`prompt.pending` carries the hook's, so the app's answer reuses it). A malformed one is replaced and noted at `warn`.
- Inside a process it is the current context (a `tracing` span field, or a thread-local in the hook), so every diagnostic point and event picks it up without being passed it.
- Children the daemon starts (`claude`, `codex exec`, `agy`) get `NOTCHLING_OP_ID`, so a `notchling` command an agent runs in that job carries it back.
- An `op_id` is a hint for reading logs, never an identity or a permission: an agent can set the variable to anything.
- `notchling log trace <op_id>` merges the app's, daemon's and hook's Diagnostics files and the Activity rows for that id, in time order.

### 7.6 When the daemon is down

- **App:** events wait in its queue. `process.*` and `daemon.unreachable` also go to a spool file, `<data>/app/log-spool.jsonl` (0600, at most 1 MB, oldest dropped first and counted). On reconnect they are sent with `spooled: true` and go to **Activity and Diagnostics only, never sealed** (coordinator's call): a spool file is something an agent can write. Activity marks them "recorded by the app while the daemon was down".
- **What the daemon witnesses is sealed instead:** `daemon.stopped` and `daemon.started{after}` bound the time no daemon ran, and `app.attached` and `app.detached{dropped}` show the app's connection, a drop standing in for an app crash or kill.
- **Hook:** passes as today and writes only its Diagnostics line.
- Nothing can record what happened while no daemon ran.

## 8. Sinks

### 8.1 Audit

- **Where:** `<data>/audit/2026-11.jsonl`, one file a month, 0600 in a 0700 folder, opened `O_APPEND` and never through a symlink. Only the daemon's writer thread opens it.
- **What:** kinds with the `AUDIT` sink, emitted by the daemon. Never sampled. Identical refusals within a second fold into one line with `repeat`, so a flood is counted, not lost and not written a thousand times.
- **Line:** the envelope plus `n` (the line's number, never reused), `prev` (the previous sealed line's seal) and `seal` (this line's):

```json
{"n":4812,"v":1,"kind":"vault.secret_used","kv":1,"ts":1794560523117,"seq":918233,"host":"3f9a0c1b7d2e4a56","proc":"daemon","pid":812,"boot":"b7e1c2d4","op_id":"9f2c41d07ab3e512","actor":{"tier":"agent","via":"cli","exe":"notchling","session":"6a1fd0c2-…"},"outcome":"ok","f":{"secrets":["gh"],"folder":"/Users/me/src/app","command_exe":"gh","approval":"asked","answered_by":"phone","device":"d41c…","result":"given","waited_ms":8120},"prev":"Hk3…","seal":"q9Z…"}
```

- **An unsealed line** (the key can't be read, [12](#12-tamper-evidence)) has `"prev":null,"seal":null,"unsealed":"key_missing"` (or `key_denied`, `key_waiting`).
- **Durability:** a commit ([7.2](#72-the-daemon)) is flushed before its op goes on. Otherwise the writer flushes after a batch that holds an `Always` kind, and at least once a second, so a power loss can lose at most the last second of pushed lines.
- **When it can't be written** (disk full, permission changed, file gone): commits fail, so agent-tier ops that release or act are refused; reads still work, and an Inbox item says why. App-tier actions still work, since the user is at the Mac; the gap is recorded as `log.dropped{sink: audit, reason: write_failed}` when writing works again.
- Tamper evidence is [section 12](#12-tamper-evidence).

### 8.2 Activity

**Where: its own file, `<data>/daemon/events.db`**, not `notchling.db`. Decision 0014 puts the app's own data (settings, drafts, docs, the inbox) in `notchling.db`, which both the app and the daemon write, and it already gave the session index its own daemon-owned file, `daemon/search.db`, which is rebuilt when damaged. Activity is like the index, not like settings:

- It grows and is pruned all day (thousands of rows a day). In `notchling.db` it would bloat the file that holds settings and drafts.
- One writer, the daemon. The app reads it through `activity.list`, the same path the phone and agents get later, with the same filters.
- A damaged `events.db` is set aside as `events.db.damaged` and rebuilt from the Audit months for audited kinds, without touching settings.
- Clearing Activity can delete rows, or the whole file, safely.

It opens like `store.rs`: WAL, a busy timeout, 0600, a symlink in its place refused.

```sql
CREATE TABLE activity (
  seq INTEGER PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL, kv INTEGER NOT NULL,
  grp TEXT NOT NULL, outcome TEXT NOT NULL, actor_tier TEXT, actor_via TEXT, actor_exe TEXT,
  actor_session TEXT, session TEXT, folder TEXT, account TEXT, device TEXT, op_id TEXT,
  host TEXT NOT NULL, audit_n INTEGER, spooled INTEGER NOT NULL DEFAULT 0, f TEXT NOT NULL,
  expires INTEGER NOT NULL);
CREATE INDEX activity_ts ON activity(ts DESC);
CREATE INDEX activity_kind ON activity(kind, ts DESC);
CREATE INDEX activity_session ON activity(session, ts DESC);
CREATE INDEX activity_actor_session ON activity(actor_session, ts DESC);
CREATE INDEX activity_expires ON activity(expires);
CREATE TABLE metrics (start INTEGER NOT NULL, span TEXT NOT NULL, proc TEXT NOT NULL,
  name TEXT NOT NULL, labels TEXT NOT NULL, count INTEGER NOT NULL, sum INTEGER NOT NULL,
  buckets TEXT, PRIMARY KEY (start, span, proc, name, labels));
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

`activity.list` keeps its reply, `{rows: [{time_ms, source, kind, op, who, result, detail?}], truncated}`, so the Activity tab on `merge/settings-batch` works unchanged; rows gain `event` (the kind's name), `group`, `title`, `outcome`, `refs`, `op_id`, `seq`, `spooled`. Its `kind` filter keeps the six names the branch has (`refused`, `failed`, `acted`, `setting`, `inbox`, `account_switch`), mapped from groups and outcomes, until the page moves to groups. New optional arguments: `groups`, `event_kinds`, `session`, `before_seq` for paging.

Activity is for reading. It is not tamper-evident: an agent can edit `events.db`. A row that came from an audited event carries `audit_n`, and the page's details can check that row against the sealed line.

### 8.3 Diagnostics

- **Where:** `~/Library/Logs/Notchling/` (Console.app lists this folder): `app-2026-11-13.jsonl`, `daemon-…`, `hook-…`, `ptyhost-…`. Under `NOTCHLING_DATA_ROOT` or `--data-root` they go to `<root>/notchling/logs/`, never the real folder. Linux and Windows use `platform::paths` (`$XDG_STATE_HOME/notchling/logs`, `%LOCALAPPDATA%\Notchling\Logs`).
- **What:** every event of a kind with the `DIAG` sink, plus **diagnostic points**: `diag!(Level::Warn, "reload took long", ms = took, rows = n)`. A point's message is a string literal, its fields are `LogField` types, and it is not registered (developers add them freely; they never reach Audit or Activity and never leave their process's file). In Diagnostics every `Ref` is hashed ([11](#11-privacy)); each file's first line names the salt id it hashes with.
- **How:** `tracing` with `tracing-subscriber`'s registry and an env filter, under a JSON layer of our own that writes the envelope's shape and enforces the field rules. Files are written by our own non-blocking writer: one thread, a day per file, 20 MB a day per process at most, a janitor at start and hourly that deletes past 7 days and keeps the folder under 300 MB. `tracing-appender` rolls only by time, so it would still need the size cap; the builder may use it if that is simpler.
- **Levels:** `error` an operation failed, `warn` degraded but handled, `info` a change of state, `debug` off unless asked. No `trace` in release builds. Default: `info` for the app, daemon and ptyhost, `warn` for the hook (it runs on every tool call). Settings > Logs > Detail: Normal, or Detailed for 24 hours (then back to Normal by itself). `NOTCHLING_LOG=debug,notchling_engine::chat=trace` overrides it in development.
- **`notchlingd.log` stays**, as the catch file for what is printed before the logger starts (launchd's `StandardErrorPath` and `ensure_daemon`), with today's `log_rotate.rs`. The `dev.notchling.daemon` plist doesn't change. `ptyhost` gets the same.
- **Unified logging is not in this design.** A mirror to `os_log` would let `log stream` follow Notchling, but the one Rust crate for it is Zlib-licensed, outside 0008's list, and writing our own needs a C shim and a protected `build.rs`. The files cover the need; revisit if users ask.

### 8.4 Metrics

- Counters and histograms (fixed buckets: 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000 ms and more) kept in memory per process. Labels are `Choice`s and `Op`s only.
- The daemon writes a minute's rollup to `events.db.metrics` every 60 s; the app sends its own with `log.emit` every 60 s. Minute rows are kept 24 hours, then folded into hourly rows kept 7 days.
- Where they come from: `conn.rs` times every op (`perf.op`) and counts quiet ones (`perf.poll`); the app measures main-thread work (`perf.stall`: a watchdog stamps the main loop and notes any turn over 50 ms with the part being drawn) and reloads (`perf.reload`, which the benches already count).
- Read on Settings > Logs > Performance ("Slowest operations today", "Slow moments in Chat") and by `notchling log metrics --json`. Sent nowhere.

### 8.5 Crash

- `<data>/logs/crash.log` stays where it is, and keeps the **last 20** crashes (oldest dropped when a 21st is written; 1 MB a crash at most).
- The daemon and `notchling-ptyhost` get the same panic hook as the app; each entry names its process.
- A crashing process can't count on sending an event, so at its next start it reads the last entry it hasn't reported and emits `process.crashed` with the place and a hash of the message. `crash.log` is a file, so that event is never sealed; the daemon's own `daemon.started{after}` is the sealed side. The message itself stays in `crash.log` on this Mac.
- macOS's own crash reports (`~/Library/Logs/DiagnosticReports/*.ips`) are untouched; the bundle can include Notchling's.

## 9. Settings > Logs

A fourth Settings tab after General, Agents and Activity (`merge/settings-batch` made Activity a tab), built from the registry, so a new kind appears there on its own. Activity stays the place to read; Logs is where to choose what is kept.

```
Settings   General | Agents | Activity | Logs
--------------------------------------------------------------------------------
Logs stay on this Mac. They never hold your messages, prompts, files or secrets.

[ok] Audit log intact. Checked 2 minutes ago. 12,480 entries since Nov 1.  [Check Now]

Keep
  Audit log      [ 1 year  v ]  What agents and phones did, and what was refused.
  Activity       [ 30 days v ]  The default for each kind below.
  Diagnostics    [ 7 days  v ]  Detail: [ Normal v ]   For fixing bugs.

Security                                                         Always on
  (locked) Refused Requests        A program asked for something it may not do.
                                   Why: shows an agent trying to get past your settings.
  (locked) Switches On and Off     When Acting, the phone bridge, Accounts, the Vault
                                   or Install was turned on or off, and by what.
  (locked) App Connected / App Disconnected / Daemon Started / Audit Log Checks ...
Secrets                                                          Always on
  (locked) Secrets Given to Commands   Which secret, which session, who allowed it.
Agents
  [x] Messages Sent                When your message reached a session, or why not.  [ 30 days v ]
  [x] Messages Stuck in Queued     ...                                               [ 30 days v ]
  [x] Prompts From Claude Code     Why a prompt showed here or in the terminal.      [ 30 days v ]
Inbox and Approvals
  [x] Items Answered or Closed     ...                                               [ 30 days v ]
Accounts and Limits
  (locked) Logins Added or Removed
  [x] Account Switches             When and why an account changed, and the limit.   [ 90 days v ]
Phone
  (locked) What Phones Did         Messages, answers, new sessions. Never the text.
  [ ] What Phones Read             Off. Each time a phone opened a session.          [ 30 days v ]
Settings
  (locked) Settings Changed by Agents
  [x] Your Settings Changes                                                          [ 30 days v ]
Diagnostics
  [x] Other Requests From the App                                                    [ 30 days v ]
  [x] Failed Operations                                                              [ 30 days v ]
Performance                                    Counts and times only, kept 7 days.
  [x] Operation Times    [x] Slow Moments in the App    [x] Chat Reloads

[ Export Bug Report... ]   [ Show Logs in Finder ]   [ Clear Activity... ]
[ Delete Old Audit Months... ]   Months older than 30 days. The deletion is recorded for good.
```

When the key can't be read, the header is red and offers the one way out:

```
[!] The audit log isn't sealed. Notchling can't read its key in your Keychain
    (it is missing). New entries are written but can't be checked.
    [Details]  [Start New Chain...]
```

- A locked row is greyed and says why it can't be turned off (the kind's `lock` reason) when hovered or clicked. Its retention follows the Audit's.
- A row turned off says since when ("Off since Nov 13, 14:02"). Off means the kind is written to no sink; Metrics keep a count of how many were skipped.
- **Red header states:** `broken`, `gap`, `tail_missing`, `file_missing`, `unanchored` ("The audit log was changed outside Notchling. Entries 4,790 to 4,812 can't be trusted."); `unsealed` with the reason (missing, refused, or waiting for you to answer macOS's prompt). Each also goes to the Inbox and, once, to a notification. **Start New Chain…** asks for confirmation, says the old entries stay but can't be checked against the new key, and is recorded ([12](#12-tamper-evidence)).
- **Who can change it.** Only the app, through the daemon: `log.settings.set` and `log.chain.start` are app tier. Agents can't: these settings are not keys in `settings/keys.rs`, so decision 0020's `settings.set` can't name them, and if they ever become keys their class must be Never (a test holds it). An agent-tier `log.*` call that changes anything is refused and recorded as `api.refused`.
- **Every change is recorded** as `log.setting_changed` (`Always`), including a retention change.
- **Changes on disk don't count.** The daemon keeps the choices in its own `docs` row (`daemon/log-settings` in `notchling.db`) and puts the row's hash in each `log.setting_changed` and in the Keychain checkpoint. At start, a row whose hash doesn't match is ignored: defaults (everything on) come back, `log.settings_restored` is recorded and the Inbox says so. An agent that edits the database turns logging back on, never off.

## 10. Retention

| Sink | Default | Choices | Cap |
| --- | --- | --- | --- |
| Audit | 1 year, by whole months | 90 days, 1 year, 2 years | 500 MB; past it the oldest month goes first |
| Activity | Per kind: 30 days; `Always` kinds follow the Audit; `account.switched` 90 days; `usage.daily` 1 year | 7, 30, 90, 365 days, per kind | 200 MB; past it the oldest rows go first |
| Diagnostics | 7 days | 1, 3, 7, 14 days | 20 MB a day per process, 300 MB in all |
| Metrics | Minute rows 24 hours, hourly rows 7 days | Fixed | — |
| Crash | Last 20 crashes | Fixed | 1 MB a crash |
| App spool | Until sent | Fixed | 1 MB |

- **Removing an Audit month** (coordinator's call): first a sealed `audit.pruned{through_month, last_seal}` goes into the current month, where `last_seal` is the final seal of the month being removed; the same anchor goes into the Keychain checkpoint; then the file is deleted. A check starts at the oldest kept month and accepts that month's first `prev` only if it equals the anchor (else `unanchored`).
- **Deleting Audit history** (the user's decision 2): only whole months older than 30 days, never the current month, and only by the user, from Settings > Logs > Delete Old Audit Months… (`log.audit.delete`, app tier; an agent-tier call is refused and recorded as `api.refused`). It runs the same steps as retention, with `audit.pruned{reason: user}`. That record is permanent: every later `audit.pruned` line carries the user's deletions forward in `user_deleted` (the newest 16 as `{through_month, at}`, and a count of all), and so does the Keychain checkpoint, so the fact outlives the month that first held it.
- Activity and Diagnostics pruning runs at start and hourly in the writer thread and records `log.pruned`.
- **Clear Activity…** removes rows of chosen groups (locked kinds included only if the user ticks them) and records `log.pruned{reason: user}`. Diagnostics can be cleared the same way.

## 11. Privacy

**Never written, by any sink:** prompts and message text; tool input and output; file contents and diffs; terminal screens; command lines past the executable's name; environment variables; tokens, cookies, refresh tokens, Access client secrets, pairing codes, device tokens; vault values; Keychain contents; error messages from outside (only their kind); URLs. `permission-screen.txt` is not a log: it stays as it is, and no bundle includes it.

**Identifiers** (`Ref`) are written according to the sink:

| Sink | How a `Ref` is written | Why |
| --- | --- | --- |
| Audit, Activity | The id itself: a session id, a folder path, an account id, a device id, a secret's name | They stay on this Mac, 0600, and the user must be able to tell which session and which secret. The same ids are in files Notchling already reads |
| Diagnostics, Metrics labels | `h:` + 8 hex of SHA-256(salt, ref type, id) | So Diagnostics can be shared. The same id gives the same hash, so a chain of steps still reads |
| Bug-report bundle | Hashed, from every source | It leaves the Mac when the user sends it |
| `inbox.item` | Always hashed | An item id can hold a vault ticket |

**The salt** (coordinator's call) is 32 random bytes in its own file, `<data>/daemon/log-salt`, 0600, written without following a symlink, never in `events.db` and never in a bundle. Every process reads it at start. If it is missing, the first process to need it creates it (create-only, so two processes can't both write one); if it can't be read, a new one replaces it. The daemon keeps the salt's id (8 hex of its own hash) and records `log.salt_rotated{old_id, new_id}` when the file's id changes. **Hashes made before and after a new salt don't match**: the same session gets a different `h:` value, so a Diagnostics chain can't be followed across that point, and a bundle says which salt ids it spans. The salt doesn't hide ids from an agent on this Mac, which can read the same files; it hides them from whoever reads a bug report. `notchling log ref h:3f9a0c1b` and the Logs page turn a hash back into a name for ids this Mac still knows, under the current salt.

Names are resolved when shown, not when written: an Activity row stores an account id, and the page shows the account's current name. Account emails are never written.

**privacy.md** changes with each phase: the "Logs on this Mac" list (new files, new folder, `remote-log.jsonl` frozen), the statement that no log holds content and that a test checks it, and that a bug report leaves the Mac only when the user sends it.

## 12. Tamper evidence

**Threat:** an agent running as the user, with a shell. It can read, write and delete every file the user owns, run any program the user can (Notchling's own binaries included), and restart the daemon. It cannot click Allow on macOS's Keychain prompt without the user seeing it, and it cannot reach a paired phone's storage.

**The seal.** Each Audit line's `seal` is HMAC-SHA256 under a 32-byte key K, over the line's bytes up to `,"seal":` (byte-exact, so there is no canonical-JSON step), and the line holds the previous sealed line's seal as `prev`. Changing, adding, removing or reordering any sealed line breaks every seal after it, and redoing them needs K. A new month's first line carries the last month's final seal.

**Where to anchor it.** Evaluated:

| Anchor | Strength | Weakness |
| --- | --- | --- |
| A file | None: the agent rewrites it with the log | — |
| **Keychain item** holding K and the latest checkpoint | Reading K needs macOS's prompt for every program but the build that made the item, as with the vault's item. The agent can't forge seals without the user seeing a prompt | Deleting or replacing the item may not prompt (assume it doesn't). Ad hoc builds: each update asks once to read K, as the vault does, until Developer ID signing. The daemon, hook and command line share one executable (below). Linux and Windows stores have no per-app access control |
| **Paired phone** holding checkpoints | Fully out of the Mac's reach | Only when a phone is paired, the bridge is on and it connects; needs protocol fields and storage on the phone |

**Decision: the Keychain now, the phone later as a second anchor** (Phase 7; the user's decision 3). The Keychain is on every Mac with no pairing, and its prompt is the one barrier an agent with a shell can't pass silently. The phone is the only anchor fully off the Mac, but it is optional and intermittent, so it adds to the Keychain and doesn't replace it.

**The Keychain items** (coordinator's call). One set per data folder: service `dev.notchling.audit` (K) and `dev.notchling.audit.head` (the checkpoint), account = the first 16 hex of SHA-256 of the data folder's canonical path. So the installed app, a dev profile (its own `HOME`, so its own data folder) and a `build/next` app over another folder each have their own key; two builds over the same folder share one, and the second asks once. Under `NOTCHLING_DATA_ROOT` (fixtures, e2e tests) the key is a file, `<root>/notchling/daemon/audit-key`, and unit tests use a memory fake, so no test touches the Keychain.

**One executable, one trust** (coordinator's call, accepted). The daemon, `notchling hook` and the command line are the same `notchling` file, and the item's access list trusts that file, so an agent running `notchling <anything>` runs a trusted program. That is accepted: no mode or op of the binary returns K or seals a line on request; only the daemon's writer thread reads K and only to seal what the daemon itself writes. An agent that starts its own `notchling daemon` on the real folder (after stopping the real one) runs the same code, which records `daemon.stopped`, `daemon.started` and everything it does. A library injected into an ad hoc build without the hardened runtime could read K (decision 0007's Phase 6 note on signing applies here too).

**Reading K, never minting it silently** (coordinator's call):

- **Start never waits on the Keychain.** The daemon reads K on its own thread. Lines written in the first 2 s wait in memory; past that, or if the answer is no, the Audit runs **unsealed** and the daemon keeps serving.
- **Missing** (no item) or **refused** (macOS said no, or the read failed): every line is written with `prev: null`, `seal: null` and `unsealed` naming why. `audit.checked{unsealed}` is recorded, the Logs header is red, and an Inbox item offers Start New Chain.
- **Waiting** (macOS's prompt is up, typically after an update): lines are unsealed with `unsealed: key_waiting`. If the user allows it, sealing resumes on its own: the next sealed line's `prev` is the last sealed line's seal, and `audit.checked{resumed, from_n, to_n}` names the unsealed stretch, which stays marked as unchecked.
- **Start New Chain** (app tier only, `log.chain.start`, recorded): makes a new K, stores it, and writes a sealed `audit.chain_started{reason, previous_head}` with `prev: null`, where `previous_head` is the `n` and `seal` of the last sealed line found. Lines before it stay; they show as "sealed with a key that is gone" and can't be checked.
- **The first run** is the one case the daemon makes K by itself: no key item, no checkpoint item, no Audit month and no legacy marker. It writes `audit.chain_started{first_run}`, and the Logs page always says when the chain started, so a history that begins today on a Mac used for months is visible. An agent able to delete every one of those can make an old Mac look new; the phone anchor is what would catch that.

**Checkpoint and checks:**

- The checkpoint `{n, seal, month, settings_hash, prune_anchor, user_deleted, at}` is written after a batch with an `Always` kind (at most every 10 s), every 5 minutes, and at every prune.
- Checks run at daemon start (from the last checked line to the end), hourly, and on Check Now (`log.verify`; `full: true` reads every kept month, about 1 s for 100 MB). They start at the oldest kept month, whose first `prev` must equal the prune anchor.
- Results (`audit.checked`): `intact`; `broken` (a seal doesn't match: edited, inserted or reordered); `gap` (`n` skips: lines removed); `tail_missing` (the file ends before the checkpoint: cut, or an older copy put back); `file_missing` (a kept month is gone); `unanchored` (the oldest month's first `prev` isn't the prune anchor); `unsealed`; `resumed`. Anything but `intact` and `resumed` raises the Inbox item and the red header; the daemon keeps writing, so the warning is about the past.

**What it catches:** any edit, insert, delete or reorder of sealed lines; removal of whole months, the oldest included; the tail cut back past the last checkpoint; an old copy put back; the key deleted or refused (shown, never replaced quietly). **What it doesn't:** lines written after the last checkpoint can be cut (up to 10 s of security lines, 5 minutes of others); unsealed lines can be edited; someone who allowed the Keychain prompt has K; what happened while no daemon ran was never recorded. Deleting K and the files together can't be hidden, but can't be undone either.

**In plain words for the user:** "The audit log is sealed. If anything changes it outside Notchling, Notchling notices and tells you. It can't stop a program from deleting it."

## 13. Performance budget

| Where | Budget | How |
| --- | --- | --- |
| `push()` on any thread | under 5 µs at p99 | Build a small struct, put it in a bounded channel; no I/O, no lock across I/O, no formatting |
| `commit()` (agent-tier secret uses, settings, acts) | under 50 ms at p99 on an Apple SSD; 2 s gives up and refuses | Ends the writer's batch; commits waiting together share one `F_FULLFSYNC`. Rare by nature: one per secret use or setting change |
| App UI thread | no I/O, ever | Non-blocking push; sends happen on the log thread; `bench_chat` counts must not change |
| Daemon queues | `Toggle`: 16,384 events; `Always`: 4,096, then an in-memory overflow up to 8 MB | A full `Toggle` queue drops and counts (`log.dropped`, at most one a minute). `Always` events are never dropped: past the overflow, commits fail, so agent-tier ops that need them are refused until it drains |
| Writer thread | a batch in under 5 ms typical, plus the flush when there is one | One write for Audit lines, one SQLite transaction, prepared statements |
| Steady state | under 0.5% of one core, under 5 MB of disk a day | Polls are counters; info-level Diagnostics; refusals fold into `repeat` |
| Hook | under 2 ms added, for push only | One `O_APPEND` line; `log.emit` only for a rare pass; no hook kind commits |
| Verify | about 1 s per 100 MB, off the op threads | Runs on the writer's side thread |

`crates/notchlingd/tests/bench_log.rs` (ignored by default, release) sends 100,000 mixed events with commits among them: no `Always` event lost, p99 `push()` under 5 µs, p99 commit under 50 ms, the writer keeping up at 5,000 events a second. Diagnostics drop when their writer falls behind; the Audit never does.

## 14. Migration of today's logs

| Today | After |
| --- | --- |
| `<data>/audit.jsonl`, `.1` | Phase 2 moves both to `<data>/audit/legacy/` (0400), starts the chain with `audit.chain_started{first_run}` holding their SHA-256 and line counts (the legacy folder is the marker that this Mac had an audit before), and imports their lines into Activity once, as `source: legacy` |
| `AgentConsole/bridge/remote-log.jsonl` | Phase 2 stops appending (`phone.*` replace it). The file stays where it is, read-only, because it is the user's history; Phase 2 imports it once |
| `<data>/account-log.json` | `agent/account-log-fix` restores its writer first, in the existing format, so cost is right before any of this lands. Phase 2 then moves that writer to `account.switched`, imports the file once, and cost reads switches from `events.db`, keeping the file as a fallback |
| `<data>/notchlingd.log`, `.1` | Kept as the catch file for early output ([8.3](#83-diagnostics)) |
| `<data>/logs/crash.log` | Kept; last 20; daemon and ptyhost added |
| `activity.list` over files | Same op and reply over `events.db`, switched in the same phase as the Audit's format (coordinator's call), so it never reads half of each. The branch's `engine::activity::normalize` becomes the importer, which reads both the old `audit.jsonl` lines and the new months (the second for rebuilding a damaged `events.db`); its rule against a phone's session id becomes part of the import mapping; its rule hiding a secret's name after `ok:` is dropped, since names now show in Activity (the user's decision 1) |
| `audit.jsonl` in `machines/` (`notchling-machines`) | Unchanged until Machines moves into the daemon; then its kinds join the registry |
| `permission-screen.txt` | Unchanged; not a log |

An import marks every row `legacy`, keeps only fields the new kinds have, and never fails a start: a line it can't read is counted and skipped. It runs once (`meta.imported = 1`).

## 15. The bug-report bundle

**Export Bug Report…** on the Logs page (app tier, `log.bundle`). Nothing is built or sent until the user presses it, and nothing is sent at all: the user picks a folder.

1. The user picks a window (last hour, 24 hours, 7 days) and two options, both off by default: include Activity (what agents did, names hashed), include crash messages.
2. The daemon builds a folder in `<data>/app/reports/`:
   - `README.txt`: what is inside, what was removed and how many lines, and which salt ids the hashes span.
   - `system.json`: versions (app, daemon, ptyhost, macOS, Claude Code, Codex, Antigravity), the Mac's model, switch states, counts of accounts per provider, paired phones and kept pages; Free settings with values, other settings by name only.
   - `diagnostics/*.jsonl` from every process, `metrics.json`, `audit-check.json` (the chain's status, never its lines), `crashes.txt` (places and stacks; messages replaced unless chosen).
   - Never: Audit lines, transcripts, `notchling.db`, `events.db` itself, `log-salt`, anything from the Keychain, `permission-screen.txt`, the vault list, `devices.json`.
3. A last scrub runs over every file: a line with a token shape (`sk-ant-`, `ghp_`, `Bearer `, a JWT, 40+ hex characters, 60+ base64 characters), an email address or a home folder path is dropped, or has the home folder replaced with `~`, and counted in the README. It is a backstop; the types are the defense.
4. The app shows a preview: each file, its size, its first lines and what was removed, with Remove per file. Then Save moves it where the user chooses, or Cancel deletes it.

The folder format (a plain folder, or a zip, for which no crate is in the tree today and `ditto` would be a process) is left to the builder.

## 16. Local API changes

All additive. Wire shapes go into `docs/local-api-ops.md`.

| Op | Tier, spec | Args | Reply |
| --- | --- | --- | --- |
| `log.emit` | agent or app, `AGENT` (the kind's `emitters` and the tier are checked inside); always a push | `{events: [{kind, kv, ts, op_id?, f}]}`, at most 256 KB | `{accepted, dropped}` |
| `log.read` | agent: events whose `actor.session` is its own, kinds marked `agents_read`; app: everything; `AGENT` | `{since?, until?, kinds?, groups?, session?, limit?, before_seq?}` | `{events, more}` |
| `log.verify` | agent or app, `AGENT` (a status, no lines) | `{full?}` | `{result, sealing, from_n, to_n, first_bad_n?, checkpoint_n, chain_started, at}` |
| `log.chain.start` | app, `APP` | `{confirm: true}` | `{chain_started_n}` |
| `log.settings.get` | app, `APP_QUIET` | `{}` | `{kinds: [{kind, on, keep_days, locked, reason?}], sinks, detail}` |
| `log.settings.set` | app, `APP` | `{target, on?, keep_days?, detail?}` | the settings |
| `log.clear` | app, `APP` | `{sink: "activity" or "diagnostics", groups?}` | `{removed}` |
| `log.audit.delete` | app, `APP` | `{through_month}`: a month that ended more than 30 days ago, never the current one | `{months_removed, audit_pruned_n}` |
| `log.bundle` | app, `APP` | `{window, include_activity?, include_crash_messages?}` | `{folder, files: [{name, bytes, lines, removed}]}` |
| `activity.list` | app, `APP_QUIET` (as on the branch) | as today, plus `groups?`, `event_kinds?`, `session?`, `before_seq?` | as today, rows with more fields |

- **Topic `log`** (app tier): `log.event` (each Activity row as written) and `log.check` (a change in the chain's status or sealing).
- **Request:** optional top-level `op_id`; the reply echoes it. **Hello:** optional `boot` from the client.
- **An agent's own session** is `actor.session`, worked out at hello ([6.1](#61-the-envelope)), never taken from an argument. A caller with no session gets an empty list.
- **Command line:** `notchling log [--json] [--since 1h|today|<ms>] [--kind …] [--limit n]`, `notchling log trace <op_id>` (reads Diagnostics files directly), `notchling log verify`, `notchling log kinds [--markdown]`, `notchling log ref <hash>`, `notchling log metrics [--json]`. The command line is always agent tier, so it sees what an agent may see; the app sees everything.

## 17. Phases

Each phase ships on its own, keeps every test green, and updates privacy.md, README's Logs table, `crates/AGENTS.md` and `docs/local-api-ops.md` where it changes them. **Plan and build against `merge/settings-batch`** (coordinator's call): it already has the Activity tab, the quiet-ops test, decision 0020's `settings_ops.rs` and its audit lines, so nothing is built against stubs.

| Phase | Ships | Tests | Zone |
| --- | --- | --- | --- |
| **1. Registry** | `crates/events`: envelope, field types, kinds, macros, a test sink; `docs/log-kinds.md`; the decision record; `LINT-LOG-NO-TEXT`, `LINT-LOG-ONE-WRITER` with today's baseline. No behavior change | Registry tests ([6.5](#65-registry-tests-and-lint)), the golden, the lint rules' bad examples | Maintainer: new crate, `Cargo.toml`, `docs/rules.md`, `docs/decisions/`, `docs/feature-map.md` |
| **2. Bus, Audit and Activity** (the first draft's 2 and 3, merged: coordinator's call) | Daemon `push` and `commit`, queues, writer thread; Audit months with seals, unsealed mode, `log.chain.start`; the Keychain key and checkpoint behind a trait; legacy audit moved; every audit call site moved to kinds (`vault.secret_used` gains who allowed it and commits first); `op.called` and `op_kinds_tests.rs`; `daemon.*`, `app.attached`, `app.detached`; `log.verify`; checks at start and hourly; the Inbox item; `daemon/events.db`; the importer of the three old formats and the new months; `activity.list` over SQL (same reply); `account.switched` in place of the `account-log.json` writer that `agent/account-log-fix` restores, that file imported, cost reading switches from `events.db`; pruning with the anchor; Delete Old Audit Months (`log.audit.delete`); the salt file | Seal tests (edit, insert, delete, reorder, cut before and after the checkpoint, month removed with and without its anchor, key missing, refused, waiting then allowed, Start New Chain, first run); commit refuses when the write or flush fails and releases nothing; folding keeps counts; importer goldens over the old formats; the Activity tab's model reads the new reply; a damaged `events.db` rebuilt; e2e over a fixture; the canary over Audit and Activity | Maintainer: `local-api`, `notchlingd`, `engine`; `CAP-KEYCHAIN`, `CAP-FILEWRITE`; existing tests edited (below) |
| **3. Settings > Logs** | The Logs tab ([9](#9-settings--logs)); `log.settings.*`; `log.setting_changed`; the check of settings on disk; Check Now; Start New Chain | Pure model tests, `--render settings-logs`, no arguments turn off an `Always` kind, agent tier refused and recorded, an edited row brings defaults back | The tab is auto; the daemon ops wait for the maintainer |
| **4. Diagnostics and `op_id`** | `diag!`, per-process files, the janitor, levels and Detailed for 24 hours; `op_id` on requests, replies, `hook.request` and `prompt.pending`; the `eprintln!` baseline shrinks; panic hooks in the daemon and ptyhost; crash.log keeps 20; `process.*` | One `op_id` across a client, the daemon and the hook; caps and janitor; a lost salt makes `log.salt_rotated`; nothing written to `~/Library/Logs` under a data root | Maintainer: `Cargo.toml` (`tracing-subscriber`), `CAP-FILEWRITE` |
| **5. App, hook and Metrics** | `log.emit` with its tier rules and limits; the app's log thread and spool (Activity and Diagnostics only); `session.queue_stuck`; `delegate.job` from `notchling delegate`; `perf.*` with rollups; Performance on the Logs tab | Agents can't emit `Always` or audited kinds, spooled events never reach the Audit, limits hold, `bench_chat` counts unchanged, `bench_log` | Mixed: the app is auto, the daemon waits |
| **6. Bundle and command line** | `log.bundle` with the preview; the scrub; `notchling log` and its subcommands | The canary through a bundle; an agent's `notchling log --json` shows only its own session; no bundle holds Audit lines or the salt | Maintainer: `notchlingd`, `CAP-FILEWRITE` |
| **7. Later** | Phone checkpoints (additive protocol fields, `GUARD-PROTOCOL-GEN`), Activity on the phone, Activity as a corpus for search, cluster host ids | Their own | Maintainer |

**Existing tests Phase 2 edits** (`WEAK-TEST-EDIT`, for the maintainer's approval; the builder lists the final set in the pull request): in `crates/local-api`, the tests inside `audit.rs`, `server_audit_tests.rs`, `server_audit_reason_tests.rs` and the audit parts of `server_tests.rs`; in `crates/notchlingd`, `activity_ops_tests.rs`, `quiet_ops_tests.rs` (replaced by `op_kinds_tests.rs`), `vault_ops_tests.rs` (it asserts `"result":"ok:gh,reg"`), `settings_ops_tests.rs`, the tests inside `switches.rs`, `tests/vault_e2e.rs`, `tests/settings_e2e.rs`, `tests/install_e2e.rs` and `tests/hook_cli.rs`; in `crates/engine`, `activity/tests.rs`, the tests inside `bridge/net/audit.rs`, `bridge/router_tests.rs` and `cost/tracker_tests.rs`. Each edit keeps what the old test proved (a refusal is recorded, a secret's value never is) and only changes where and in what shape it is read.

Phases 3 and 5 can run beside 4 once 2 is in.

## 18. Left to the builder

- The exact choice values per kind. The lists here are the first cut; adding a value needs no version bump.
- Queue sizes, batch sizes and histogram buckets, inside the budget in [13](#13-performance-budget); whether `F_FULLFSYNC` meets the commit budget on the maintainer's Mac (else `fsync` plus a note in the decision record).
- Keychain access-list code (reuse `vault/keychain_mac.rs`), and how an update's one prompt is worded.
- Our own Diagnostics writer or `tracing-appender` with a size guard.
- The bundle's format, and how the saved folder is shown (showing it in Finder is `CAP-PROCESS`).
- Which level each of today's `eprintln!` lines becomes, and which become kinds.
- Log folders on Linux and Windows through `platform::paths`.
- The Logs tab's layout inside the kit's tokens, menus and rows.
- Whether the stall watchdog lives in `mac/shell.rs` or in each part.

## 19. Decided by the user

The first draft left three questions open. The user decided all three on 2026-11-13, each as recommended; nothing is open now.

1. **Secret names show in Activity and stay in the Audit.** They already show in the Vault window, in Inbox items and on a paired phone. Diagnostics, Metrics and bug reports hash them. A secret's value is never written anywhere. The branch's rule hiding names in Activity is dropped ([14](#14-migration-of-todays-logs)).
2. **Audit history can be deleted only as whole months older than 30 days**, never the current month, and only by the user from the app (`log.audit.delete`, app tier; agents never). The deletion is a sealed `audit.pruned{reason: user}` line that is never lost: later `audit.pruned` lines and the Keychain checkpoint carry it forward. Retention is 1 year by default, and older months are pruned on their own through the same `audit.pruned` anchor ([10](#10-retention)).
3. **The Keychain checkpoint now; the phone as a second checkpoint later**, in Phase 7 (Phase 8 in the first draft, before Phases 2 and 3 merged) ([12](#12-tamper-evidence)).

The account-switch record is being fixed separately, on `agent/account-log-fix`, which writes `account-log.json` in its existing format. Phase 2 (the first draft's Phase 3) later moves that writer to `account.switched` and imports the file ([14](#14-migration-of-todays-logs)).
