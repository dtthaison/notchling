# Run Watcher

Branch `feat/run-watcher`. Approved in chat 2026-11-07.

## Goal

Show the long runs that sd (>= 1.21.1) starts, tell the session that started a run when it ends, and keep the Mac awake while one runs. No polling for discovery: FSEvents tells us when `runs.jsonl` or a run's log changes.

## Data

`<git common dir>/sd/runs.jsonl`, appended by sd:

- start: `{"event":"start","key","log","cwd","pid","session","limit"(seconds|null),"started_at"(ISO UTC)}`
- finish (no `event`): `{"key","cmd","seconds","rc","head","clean","log","at"}`, matched to its start by `log`.

A run is active while it has a start, no later finish for its log, its pid is alive and is still that run (see "Decisions"), and, with a limit, it is not more than 10 minutes past it. Unknown keys and bad lines are ignored. Older lines have no start records, and sd writes start-less finishes of its own (`{"key":"precheck: …","log":"<log>.precheck"}` before each main run).

## Pieces (`Sources/Notchling/Runs/`)

- `RunRecords.swift`: `RunStart`, `RunFinish`, `RunLine.parse`, `GitCommonDir.resolve(cwd:)` (reads `.git` file or dir, `commondir`), `RunFormat` (elapsed, limit, exit text, short name), `RunLog.lastLine`.
- `RunLedger.swift`: `JSONLTail` (incremental read: byte offset, partial last line kept, truncation resets, first read capped at 1 MB) and `RunLedger` (apply records, `active`, `finished`).
- `RunWaker.swift`: pure decision: for each new finish with a start, send to the session's background job (see "Decisions" for when), else notify. Several ends due for one job in one plan are one `.send` with a line per run. Dedupe set; skips finishes older than Notchling's start and finishes with no start; setting off does nothing.
- `RunWatcher.swift`: glue on the main thread. Repos = cwds of Claude sessions + `machines.json` `localPath`s (read with `MachineRegistry.peek`, which never writes). One `FileEvents` over the existing `<common>/sd/` folders (re-made when the set changes; every repo is read again right after it listens), one over the parent folders of active logs (latency 1 s = the throttle; event paths and active logs are compared after `FileEvents.realPath`, on the main thread, against the runs active then). Sends go through one serial queue, never two in flight to one job (what piles up meanwhile is one message). Publishes `[RunItem]` to `NotchModel.runs`. Injected: send, notify, clock, pid check, dispatch.
- UI: `NotchModel.runs`, "Runs" group in the Working tab, finished runs (10 min) in Completed (`RunRow` in `NotchRows.swift`, styled like `AgentRow`).
- `AppDelegate`: build the watcher, call `tick(agents:)` in `refresh()`, pass `runs.hasActive` to `awake.apply`, setting `wakeOnRunFinish` (default true), menu items and Settings toggles in the Keep Mac Awake section (the existing `continueAfterSleep` toggle is relabelled "Continue after the Mac wakes" and gets its menu item through `toggleResume`).

## Decisions

- Dedupe key is `log` + finish time, so a reused log path still wakes for its next run.
- sd sessions normally wait for a run in a way that wakes them on their own (Monitor, a background command, `sd.py wait`), so a typed message would be a second wake. It is sent only 60 s after the finish's `at`, and only if the owner agent is idle (not working, not needing input) and has been idle since before the finish: `Agent.since` (job `updatedAt` / live `statusUpdatedAt`), or, when the state files give no time, when the watcher first saw it idle. An owner that worked at or after the finish is marked handled with no send and no notification.
- An owner whose agent is `needsInput` waits (typing into a prompt could answer it); after 6 h still waiting on a prompt it is notified instead. An owner that moved on meanwhile counts as having worked after the finish.
- A finish with no start record never wakes or notifies. Keys starting with `precheck: ` get no Completed row either.
- Alive means `proc_pidinfo(PROC_PIDTBSDINFO)` finds the pid, it is not a zombie, and its start time is <= `started_at` + 2 s. `kill(pid, 0)` and EPERM no longer count, so a reused pid is not a run.
- A run seen alive in this process that then loses its pid (or fails that check) is not announced as lost at once: the watcher reads that repo's `runs.jsonl` again, and if its log still has no finish it makes up an end with no exit code, "ended without a result; last line: …", with its own dedupe id (`lost:<log>@<start>`). It shows in Completed and goes through the waker like any end (grace, idle rule, notification for a non-job owner). A finish line that turns up later replaces it in Completed. A run past its limit + 10 minutes with a live process is only hidden, not declared ended.
- "Wake agents when runs finish" off: no message and no notification; the Completed tab still shows the run.
- Not in scope: remote machines' runs, lid-close behaviour, nightly JSON files (they sit in the watched folder but nothing reads them).

## Tests (`Tests/NotchlingTests`)

- `RunLedgerTests`: parse start/finish/bad/unknown keys, match by log, reuse of a log, active vs finished, pid dead, past the limit, runs that vanish (`disappeared`/`markLost`), `JSONLTail` (append, partial line, truncation, cap), the real-file shape.
- `RunWakerTests`: send, 60 s grace, idle-since rule (own timestamp and first-seen fallback), a working owner is left alone, a prompt waits then notifies, no job -> notify, older than start, start-less finishes, dedupe, setting off, agent vanished, several ends for one job as one message.
- `RunFormatTests`: elapsed/limit text, exit text, short name from sd keys, last log line (ANSI, `\r`, 80 chars), `GitCommonDir` for dir, worktree file, no repo.
- `RunProcessTests`: the start-time identity check (own process, reused pid, EPERM process, ended process).
- `RunRepositoriesTests`: `machines.json` v1/v2 read without any write.
- `RunWatcherTests`: end to end on a temp repo with injected clock/sender: start shows active, finish sends once after the grace, a session that worked after the finish is left alone, one message per job and one send in flight, lost runs, precheck records, log paths through `/private`, real FSEvents for runs.jsonl and logs, an append between the first read and the stream.

## Known limits

- Relaunching Notchling forgets what it had queued: a finish that arrived while it was quit is shown (for 10 minutes) but not announced, since the waker skips finishes older than its own start; and a run lost while it was quit is never seen alive in the new process, so it is not made into an "ended without a result".
- The send goes through `claude attach` in a hidden terminal, like any reply from Chat. If the user has that same session open in a terminal with a message half typed, the reply lands in the same input box: Notchling empties the box first (a draft left there would join the reply), so the draft can be lost or mixed with the run message. The idle-since rule makes this rare, since a session the user is typing into has recent activity, but it is not ruled out.
