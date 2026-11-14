# XXXX: One typed event stream, with the daemon as its only writer

## Context

Notchling writes its history in several places, each in its own shape:

- `audit.jsonl` (the local API's refusals, failures and app-tier calls), `remote-log.jsonl` (what phones did) and `account-log.json` (account switches).
- `notchlingd.log` and `eprintln!` lines in every crate, with no levels and no owner.
- `crash.log`, the notch and Chat's own counters, and the Activity tab, which reads those files and guesses a row from each line.

Three things are wrong with that:

- What an agent did to your Mac, and who allowed it, is not in one place, and a line can be edited or cut without anyone knowing.
- A line can hold anything. Today's audit line puts `key=value` of a setting into `reason`, cut to 64 characters: an agent could put what it likes there. A file path in an error, a token in a URL or a message's text can end up in a log.
- A new feature adds a new file or a new `eprintln!`, so nothing says what Notchling records, why, or for how long.

Facts that bound the change:

- The daemon already owns every resource that acts or lives long ([0004](0004-notchlingd-engine-service.md), [0011](0011-one-owner-per-resource.md)), and tells its callers apart by executable ([0007](0007-local-api-tiers.md)). An agent with a shell can write any file, so a file at rest is not a boundary.
- Settings and the app's own data live in SQLite ([0014](0014-app-data-in-sqlite.md)); agents change settings by class ([0020](0020-agents-change-settings-by-class.md)).
- [Goals](../goals.md): agents never loosen their own guard rails, by any path. The log of what they did is one of them.

The design is [docs/superpowers/specs/2026-11-13-logging-design.md](../superpowers/specs/2026-11-13-logging-design.md); this record is its decision, and the plans are `docs/superpowers/plans/2026-11-13-logging-p1-registry.md` to `p7`.

## Decision

- **One typed event stream** (approach A). Everything worth recording is an event of a registered kind. Kinds live in one registry, the leaf crate `crates/events` (`notchling-events`), the way ops live in `wire.rs`. A new feature adds a kind, never a log file. Each kind has a title, what it records and why that matters, a lock (`Always` or `Toggle`), the sinks it goes to, and a version.
- **A closed list of field types, and no text.** Fields are choices, flags, counts, sizes, durations, times, money, percentages, versions, op and error names, executable and tool names, a place in our own source, a seal, a hex id, a kind name, and refs (identifiers). Only these implement a sealed trait, so a `String` field does not compile; `LINT-LOG-NO-TEXT` stops a second implementation, and a canary test (Phase 2 on) plants marker text everywhere and fails if any file holds it. An external error goes in as a choice, never as its message.
- **The daemon is the only writer.** The app, the hook and the command line send events to it over the local API; each process writes only its own Diagnostics. `LINT-LOG-ONE-WRITER` keeps `eprintln!`, `tracing` and `log` out of every crate but this one, with today's uses in the lint baseline, which only shrinks.
- **Four sinks.** Audit (JSON lines in monthly files, each line sealed with a keyed hash over the previous line's seal; the key is a Keychain item for the real data folder, a file beside a fixture root; when it can't be read the lines are written unsealed and the app says so, and a new chain starts only when the user asks), Activity (a table in `daemon/events.db`, read through `activity.list`), Diagnostics (one file per process per day in `~/Library/Logs/Notchling/`, kept 7 days) and Metrics (rolled-up counts and timings). Panics still go to the crash file.
- **Commit before the effect.** When an agent's request releases a secret, changes a setting it was allowed to or acts on a session, its Audit line is written and flushed before the effect, and the request is refused if that fails. Everything else is queued. `Emit` has two forms: `push` and `commit`.
- **Security kinds are always on.** Turning another kind off is itself recorded, and agents can never change logging settings.
- **The registry is data the build checks.** A golden (`crates/events/tests/golden/log-kinds.json`) holds every kind's name, version and fields: a field change without a version bump changes it, which is a protected test edit. `docs/log-kinds.md` is generated from the registry and compared. A kind is never deleted, only marked retired. The byte order the writers use is the seal's input from Phase 2 on and never changes.
- **Seven phases**, each shippable: 1 the registry (this record's phase: no behavior change), 2 the bus, Audit and Activity, 3 Settings > Logs, 4 Diagnostics and `op_id`, 5 the app, the hook and Metrics, 6 the bug-report bundle and the `notchling log` command, 7 later (phone checkpoints and the rest).

This touches [0004](0004-notchlingd-engine-service.md) (the daemon gains the log), [0007](0007-local-api-tiers.md) (new ops, and a tier rule for who may emit which kind), [0011](0011-one-owner-per-resource.md) (the log is a resource with one owner), [0014](0014-app-data-in-sqlite.md) (`daemon/events.db`) and [0020](0020-agents-change-settings-by-class.md) (`settings.agent` is an audited kind that commits first). From Phase 2, `remote-log.jsonl` and `account-log.json` stop growing and are imported, and `audit.jsonl` moves to `audit/legacy/`.

## Consequences

- One place answers what an agent did, who allowed it, and when, and a cut or edited Audit shows as broken. It is tamper-evident, not tamper-proof: an agent with a shell and the Keychain item could still forge a chain, and the Keychain checkpoint and a phone's copy (Phase 7) narrow that.
- A kind's fields never hold a path, a message, a secret value or a token. What helps a bug report is a count, a choice, a version or a hash.
- Adding a kind means editing the registry, the golden and `docs/log-kinds.md` together, and the maintainer sees the golden change.
- Phase 1 adds a crate, two lint rules and 72 baseline entries (`LINT-LOG-ONE-WRITER`), and nothing writes an event yet: no crate depends on `notchling-events`.
- A hot path pays for an event only as a push into a queue; only the commit kinds wait for the disk, and none of them runs on the hook's 2 ms path.
- Seals make the Audit costlier to write than a plain line, and an Audit that can't be written refuses what an agent asked for. That is the intended cost.

## Status

Proposed. The maintainer settles it by merging Phase 1, which brings the registry, and again by each later phase.

## Date

2026-11-14
