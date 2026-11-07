# Run Watcher

Branch `feat/run-watcher`. Approved in chat 2026-11-07.

## Goal

Show the long runs that sd (>= 1.21.1) starts, tell the session that started a run when it ends, and keep the Mac awake while one runs. No polling for discovery: FSEvents tells us when `runs.jsonl` or a run's log changes.

## Data

`<git common dir>/sd/runs.jsonl`, appended by sd:

- start: `{"event":"start","key","log","cwd","pid","session","limit"(seconds|null),"started_at"(ISO UTC)}`
- finish (no `event`): `{"key","cmd","seconds","rc","head","clean","log","at"}`, matched to its start by `log`.

A run is active while it has a start, no later finish for its log, and its pid is alive. Unknown keys and bad lines are ignored. Older lines have no start records.

## Pieces (`Sources/Notchling/Runs/`)

- `RunRecords.swift`: `RunStart`, `RunFinish`, `RunLine.parse`, `GitCommonDir.resolve(cwd:)` (reads `.git` file or dir, `commondir`), `RunFormat` (elapsed, limit, exit text, short name), `RunLog.lastLine`.
- `RunLedger.swift`: `JSONLTail` (incremental read: byte offset, partial last line kept, truncation resets, first read capped at 1 MB) and `RunLedger` (apply records, `active`, `finished`).
- `RunWaker.swift`: pure decision: for each new finish, send to the session's background job, queue while that agent is working or needs input, else notify. Dedupe set; skips finishes older than Notchling's start; setting off does nothing.
- `RunWatcher.swift`: glue on the main thread. Repos = cwds of Claude sessions + `machines.json` `localPath`s. One `FileEvents` over the existing `<common>/sd/` folders (re-made when the set changes), one over the parent folders of active logs (latency 1 s = the throttle). Publishes `[RunItem]` to `NotchModel.runs`. Injected: send, notify, clock, pid check, dispatch.
- UI: `NotchModel.runs`, "Runs" group in the Working tab, finished runs (10 min) in Completed (`RunRow` in `NotchRows.swift`, styled like `AgentRow`).
- `AppDelegate`: build the watcher, call `tick(agents:)` in `refresh()`, pass `runs.hasActive` to `awake.apply`, setting `wakeOnRunFinish` (default true), menu items and Settings toggles in the Keep Mac Awake section (the existing `continueAfterSleep` toggle is relabelled "Continue after the Mac wakes" and gets its menu item through `toggleResume`).

## Decisions

- Dedupe key is `log` + finish time, so a reused log path still wakes for its next run.
- A session whose agent is `needsInput` waits too (typing into a prompt could answer it).
- "Wake agents when runs finish" off: no message and no notification; the Completed tab still shows the run.
- Not in scope: remote machines' runs, lid-close behaviour, nightly JSON files (they sit in the watched folder but nothing reads them).

## Tests (`Tests/NotchlingTests`)

- `RunLedgerTests`: parse start/finish/bad/unknown keys, match by log, reuse of a log, active vs finished, pid dead, `JSONLTail` (append, partial line, truncation, cap), the real-file shape.
- `RunWakerTests`: send, queue while working, queue then send, no job -> notify, older than start, dedupe, setting off, agent vanished while queued.
- `RunFormatTests`: elapsed/limit text, exit text, short name from sd keys, last log line (ANSI, `\r`, 80 chars), `GitCommonDir` for dir, worktree file, no repo.
- `RunWatcherTests`: end to end on a temp repo with injected clock/sender: start shows active, finish sends once, working agent queues.
