# Ask: an agent asks you a question, and gets your answer back

- **Date:** 2026-11-16
- **Status:** Draft for the maintainer's review. Nothing here is built. The calls only the maintainer can make are in [Needs the maintainer](#needs-the-maintainer), each with a recommendation.
- **Source:** the design page *Outbox & Ask*, Part 2 (local copy `docs/local/2026-11-15-outbox-ask-design.html`, not in git). Its problem, its fix (a card per question, the answer straight back to the agent) and its three safety rules are decisions and are not reopened here. Its open question, how a waiting agent is woken, was answered by the wake test of 2026-11-16 ([8](#8-who-asks-and-how-each-one-waits)). Part 1, the Outbox, is built ([design](2026-11-15-outbox-design.md)).
- **Base:** `origin/agent-run-oss` 0a32aaca (the Outbox merged; cluster v1 lanes B to D merged, lane E's fencing and promote merging).
- **Touches:** decisions 0004 (the daemon owns what acts), 0007 (tiers, additive API, files at rest are not a tier boundary), 0011 (one owner per resource), 0014 (app data in SQLite), 0018 (Chat's parts), 0019 (agent instructions per tool), the typed event stream (XXXX), skills follow the app (XXXX), the Inbox (cluster v1 plan A), inbox mirrors (cluster v1 plan B). It needs a decision record of its own (plan Task 2).
- **Gaps** the source left open are settled here and marked **Builder's call**, collected in [Builder's calls](#builders-calls).

## In short

1. An agent runs `notchling ask "Keep the old API?"` (with `--choice` buttons if it has them). Notchling saves the question in `notchling.db` and shows a card that names the agent, its branch and how long it has waited: under the session in Chat, in the notch, and in the Inbox, which also puts it on the phone.
2. You answer on the card. The answer goes straight to the `ask` command that is waiting, which prints it and exits. The main session's model is not involved.
3. A Claude Code agent (a background subagent or the main session) starts `ask` in the background and ends its turn; the command's exit wakes it. Codex and Antigravity jobs started by `notchling delegate` run `ask --wait`, which blocks inside their turn, within the delegate's time limit.
4. **Notchling works out who asked.** The session comes from the kernel (the `ask` process's parents, as the hook's check). The agent inside that session comes from its transcript: the background command's output file, else the command line. No token.
5. **Only you answer by default.** The main session may answer a question from its own subagents only when the asker passed `--main-may-answer`; the card then says "Answered · main session". No agent can answer a question meant for you, and no agent of another session can answer at all.
6. **A restart doesn't lose questions.** The waiting `ask` reconnects with the same question id, and an answer given while the daemon was down, or before the command reconnected, waits in the table until it is handed over.
7. Agents on a Worker can ask too: the question reaches the home Mac's Inbox through the existing inbox mirror and is answered there with one of its choices.
8. Additive only: new `ask.*` ops and an app-tier `asks` topic; one new Inbox kind with new action ids that today's phones already show by label; no phone protocol change. Only counts reach the Audit and Activity, never question or answer text.

## Builder's calls

Each is marked *Builder's call* again in its section.

| # | Call | Section |
| --- | --- | --- |
| B1 | A table `asks` in `notchling.db` (schema 4 to 5), class Node; the daemon is its only reader and writer | [9](#9-data-model) |
| B2 | Six states: `open`, `answered`, `delivered`, `dismissed`, `expired`, `gone`. The first answer wins in memory (one lock), then one conditional `UPDATE` persists it | [10](#10-states) |
| B3 | `ask.open` answers at once with the id; the command then long-polls `ask.wait` (50 s a call) and reconnects with the same id after a daemon restart. A retried open with the same `key` from the same process returns the same question | [13](#13-waiting-delivery-and-restarts) |
| B4 | Delivered means an `ask.wait` reply carried the answer to the asking process; that process asking again gets the same answer, so a reply lost on the way loses nothing | [13](#13-waiting-delivery-and-restarts) |
| B5 | Who asked: the session from the kernel; the agent from the transcripts (the background output file, then the command line); `unknown` when no single transcript matches. It labels the card; it proves nothing | [11](#11-finding-who-asked) |
| B6 | The main session lists what it may answer with `notchling asks [--wait]` and answers with `notchling answer`; the daemon checks the session by the kernel and the main session by its transcript | [12](#12-who-may-answer) |
| B7 | `ask.open`, `ask.answer` and `ask.dismiss` are refused while `acting` is off; `ask.wait`, `ask.list` and `ask.mine` are not. Open questions keep waiting while it is off | [14](#14-ops-topic-command-line-events) |
| B8 | Limits: a question of at most 2,000 characters, 2 to 6 choices of at most 120 characters, an answer of at most 10 KB, 20 open questions per session, 100 in all | [9](#9-data-model) |
| B9 | One Inbox item per open question: kind `question`, actions `choice_1` to `choice_6`, Answer in Chat (`open`, Mac only) and Dismiss (`cancel`); the answer text is never in the item. After a restart the item is raised again with a fresh id and the withdrawn copy is dropped | [16.3](#163-inbox) |
| B10 | The phone answers through the Inbox only: choices and Dismiss; a question without choices shows Open on Mac. No protocol change | [16.4](#164-phone) |
| B11 | The notch shows the question on the asking session's row with its existing question card (choices and a typed answer) | [16.2](#162-notch) |
| B12 | Chat draws a card per open question above the reply box of the session it belongs to, reusing the question card's parts; a closed one stays as one line for 10 minutes | [16.1](#161-chat) |
| B13 | Workers in v1: the question goes through the existing inbox mirror to the home's Inbox; on a Worker, `ask` needs choices | [17](#17-workers) |
| B14 | Delegated Codex and Antigravity jobs use `ask --wait`; the delegate passes its deadline down; Codex's sandbox is not widened | [8](#8-who-asks-and-how-each-one-waits), [15](#15-the-skill-and-the-delegate) |
| B15 | An `ask` from a process in no Claude Code session (a Codex or Antigravity session Chat runs, a shell) is refused: "Notchling can't tell which session is asking." | [11](#11-finding-who-asked) |
| B16 | One new event kind, `session.ask`, with counts only; `Answerer` gains `main` | [14.5](#145-events) |
| B17 | A closed question is deleted 24 hours after it closed, its answer text with it; nothing open expires by age | [9](#9-data-model) |
| B18 | A new skill, `notchling-ask`, written from code for Claude Code, Codex and Antigravity, like the other `notchling-*` skills | [15](#15-the-skill-and-the-delegate) |
| B19 | A question from the main session itself gets the same card and the same wake; only the person answers it | [8](#8-who-asks-and-how-each-one-waits), [12](#12-who-may-answer) |
| B20 | The branch is read from the asker's folder's `.git/HEAD` (worktrees included); no `git` process | [11](#11-finding-who-asked) |
| B21 | `--main-may-answer` on a question whose asker is the main session or `unknown` is kept on the row and ignored: only the person answers it | [12](#12-who-may-answer) |
| B22 | An open or answered question with no waiter connected is `gone` when its `ask` process has ended (pid gone or its start time changed), checked on a 1 s tick | [13](#13-waiting-delivery-and-restarts) |

## Contents

1. [Why](#1-why)
2. [Goals, non-goals, anti-goals](#2-goals-non-goals-anti-goals)
3. [Constraints](#3-constraints)
4. [Approaches](#4-approaches)
5. [What exists today](#5-what-exists-today)
6. [Terms](#6-terms)
7. [The design as decided](#7-the-design-as-decided)
8. [Who asks, and how each one waits](#8-who-asks-and-how-each-one-waits)
9. [Data model](#9-data-model)
10. [States](#10-states)
11. [Finding who asked](#11-finding-who-asked)
12. [Who may answer](#12-who-may-answer)
13. [Waiting, delivery and restarts](#13-waiting-delivery-and-restarts)
14. [Ops, topic, command line, events](#14-ops-topic-command-line-events)
15. [The skill and the delegate](#15-the-skill-and-the-delegate)
16. [What the user sees](#16-what-the-user-sees)
17. [Workers](#17-workers)
18. [Tests, rule by rule](#18-tests-rule-by-rule)
19. [Risks](#19-risks)
20. [Where the code goes](#20-where-the-code-goes)
21. [Needs the maintainer](#needs-the-maintainer)

## 1. Why

You run several agents at once, each on its own branch. When one needs a decision today, it stops and reports to the main session, which relays the question to you. With several agents asking at once the questions pile up in the main chat and it is unclear which answer belongs to which agent. The main model spends tokens relaying, the agent that asked has ended and has to be started again with the answer, and agents on worker machines have no way to ask at all.

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. An agent asks with one command and gets your answer back, with no main-model turn spent relaying.
- G2. The card says who asks (the agent, its branch, its session) and how long it has waited, in Chat under the session, in the notch, and in the Inbox on the Mac and the phone.
- G3. A waiting agent spends no tokens: Claude Code agents wait in a background command whose exit wakes them; Codex and Antigravity jobs block inside their turn.
- G4. Only you answer, unless the asker allowed its own session's main session to answer.
- G5. A daemon restart or an app update never loses an open question or an answer you gave.
- G6. Several agents can ask at once; each answer reaches the agent that asked it.
- G7. Agents on a Worker can ask, and you answer from the home Mac's Inbox.

**Non-goals**

- A history of questions: the agent's transcript holds the command and the answer it printed.
- Typed answers on the phone; a Chat card for a session on another machine; typed answers to a Worker's question (all later; [17](#17-workers)).
- Asks from Codex and Antigravity sessions that Chat runs: you are already in that conversation.
- A shared decisions board for agents (the source dropped it), and phone push (dropped for v1).
- Waking the main session by typing a subagent's question into it.

**Anti-goals** (things this must never do)

- Let an agent answer a question meant for you.
- Let an agent of one session read or answer another session's questions.
- Write question or answer text to the Audit, Activity, events, `notchlingd.log`, `reply-screen.txt` or any log.
- Lose an open question or a given answer to a restart, or hand an answer to an agent that didn't ask it.
- Block a session's terminal, a Notchling thread or the inbox lock while a question waits.
- Type anything into a session.

## 3. Constraints

- The local API changes additively only; the phone protocol (`agentconsole.v1`) gets no new field or op in v1 (new words in two schema descriptions only, [16.4](#164-phone)).
- The daemon's switches are off at every start whatever any file says (`docs/notes/daemon.md`); only an app-tier call in that run turns `acting` on.
- Tiers (decision 0007): the app may do everything; the agent tier gets narrow ops. Files at rest are not a tier boundary: nothing an agent can write may turn an agent into the person. The kernel's facts about a caller (its process, parents, executable, open files) are; transcripts are not.
- No question or answer text in the Audit, Activity, events or logs; counts only (`LINT-LOG-NO-TEXT`). The text lives in the `asks` row, on the app-tier `asks` topic and in the Inbox item's detail (cut).
- A new `notchling.db` table needs a class in `store_class.rs`; `asks` is Node.
- A new event kind changes `crates/events/tests/golden/log-kinds.json` and `docs/log-kinds.md`.
- Files under 1,000 lines (`LINT-FILE-SIZE`), no comments in code, tests in `<module>_tests.rs`. `daemon_wiring.rs` is 967 lines and `acting_ops.rs` 888: Ask's wiring goes in a new file.
- Never hold a lock across I/O, a pipe or a socket call. An inbox responder only sets a slot and returns.
- Tests never touch real sessions, `~/.claude`, `~/.codex`, `~/.gemini`, Keychain, a real `claude`, `codex` or `agy`. The kernel facts sit behind the existing `Processes` trait (`notchlingd/src/vault_caller.rs`) with a fake.
- Changes to `AGENTS.md`, `docs/notes/` and `docs/policies/` wait for the maintainer.

## 4. Approaches

**A. Report to the main session (today).** The agent ends and returns the question as its result; the main model relays it; the answer goes back by a new subagent or a resume. Tokens spent twice, questions mixed in one chat, the agent's work interrupted, and nothing from a Worker.

**B. A card per question, answered straight to a waiting command (chosen).** `notchling ask` saves the question and waits; the card is drawn from the daemon's table; the answer ends the command. Claude Code wakes an agent whose background command exits (tested 2026-11-16), so a waiting agent costs nothing. One mechanism serves subagents, main sessions, delegated jobs and Workers.

**C. Claude Code's own question dialog (the AskUserQuestion tool and the hook).** Notchling already shows those as cards. But the dialog belongs to the session's terminal, one at a time, and only Claude Code has it: no Codex or Antigravity job, no Worker, and a parallel subagent's question waits behind the main session's dialog.

**D. A file the agent polls.** Each poll is a model step, so waiting costs tokens, and nothing wakes an agent that ended its turn.

## 5. What exists today

| Part | Where | What Ask does with it |
| --- | --- | --- |
| A caller's session from its parents | `notchlingd/src/hook_ops.rs` (`session_of`, `started_by_session`, `LINEAGE_DEPTH` 16), `hook.rs::parent_pid` | Finds the asking and the answering session |
| Kernel facts about a caller | `notchlingd/src/vault_caller.rs` (`Processes`: parent, name, cwd; `SystemProcesses`; a fake in its tests) | Gains start time, command line and the path of fd 1 |
| Waits for the person | `notchlingd/src/vault_wait.rs` (`Waiters`, 128-bit ids, memory only) | The model for the answer slot; Ask persists instead |
| The Inbox | `engine/src/inbox/`, `notchlingd/src/inbox_ops.rs` (`raise`, `resolve`, `decide`, `Responder`) | One item per open question |
| Inbox mirrors | `notchlingd/src/cluster_inbox.rs`, `inbox_mirrors.rs`, `inbox_ops_mirror.rs` (`node.inbox`, answers forwarded to the owner) | Carries a Worker's question to the home |
| The question card | `app/src/chat/prompts.rs` (`Prompt::Question(PendingQuestion)`), `chat/view/prompts.rs` (`question`, `answer_box`, `submit_row`), `app/src/notch/answer.rs` (`QuestionDraft`, typed answers) | Draws an Ask card |
| Subagent transcripts | `engine/src/transcript/subagents.rs` (`Subagent`: description, agent type, model, tool use id, parent, depth; `list_across_clears`) | The candidates for who asked, and the card's name |
| Background shell lines | `engine/src/transcript/shells.rs::launch` ("Command running in background with ID: … Output is being written to: …") | Ties a background `ask` to its transcript |
| Delegation | `engine/src/delegate/` (`codex_run.rs`, `agy_run.rs` declining every permission but edits with `--write`, `options::DEPTH_VARIABLE`) | Passes its deadline down; may allow `ask` in agy |
| Skills | `notchlingd/src/install_ops.rs::default_skills`, `engine/src/install/skills.rs` | Installs `notchling-ask` |
| A SQLite store per area | `engine/src/outbox/store.rs` (a mutex, one statement or `BEGIN IMMEDIATE` per call), `Agent::outbox_key()` | The same pattern and the same session key |
| Event kinds | `crates/events/src/kinds/agents.rs` (`session.outbox`), `kinds/choices.rs` (`Answerer`) | `session.ask` |

## 6. Terms

- **Question**: one `asks` row: a question, its choices, who asked, and its state.
- **Asker**: the agent that ran `ask`: the **main session**, a **subagent** of it, a **delegated job** (Codex or Antigravity under `notchling delegate`), or **unknown** (one of these, Notchling couldn't tell which).
- **Session**: the Claude Code session whose process the `ask` process descends from. A subagent's question belongs to its main session; so does a delegated job's.
- **Waiter**: an `ask.wait` call from the asking process that is open right now.
- **Card**: what you see for a question in Chat, the notch, the Inbox or the phone.
- **Background ask**: `ask` started with Bash `run_in_background: true`; the agent ends its turn and the command's exit wakes it.
- **Blocking ask**: `ask --wait`, run in the foreground of a turn.

## 7. The design as decided

From the approved source, restated; not open here.

```
Subagent                   Notchling                         You
notchling ask "Keep the old API?" ──▶ find which agent asked (its transcript)
                                     card in Chat, notch, Inbox ──▶
(waits, spending no tokens)                                    answer "Keep it"
            ◀── "Keep it"
(carries on)                         card: "Answered · Mac"
```

Three rules:

1. **Notchling works out who asked.** Subagents run inside the main session's process, so the connection alone can't tell them apart. Notchling finds the asker by looking for the `ask` command in each subagent's transcript. No token for the agent to remember.
2. **Only you answer decisions.** An open `notchling answer` would let an agent answer its own question, or one meant for you. A question is "you only" by default. The main session may answer only questions from its own subagents, only when the question allows it, and the card then says "Answered · main session".
3. **A restart doesn't kill questions.** The waiting `ask` reconnects after a daemon restart and keeps the same question id.

## 8. Who asks, and how each one waits

The source's open question is answered (wake test, 2026-11-16, run twice): a Claude Code background subagent that starts a command with `run_in_background` and ends its turn is woken when that command exits. Current Claude Code runs every subagent in the background (the Agent tool has no foreground subagent), so the source's "foreground subagent" row is gone.

| Asker | How it asks | How it waits | Who may answer |
| --- | --- | --- | --- |
| Background subagent | `ask …` with `run_in_background: true`, then ends its turn | The command's exit wakes it; it reads `Answer: …`. Its result reaches its caller only through SubagentHandback, called again after the answer: plain text at the end of a turn is not delivered | You; also the main session if it passed `--main-may-answer` |
| Main session (*B19*) | The same | The same wake (plan Task 1 confirms it for a main session) | You only |
| Codex or Antigravity job under `notchling delegate` (*B14*) | `ask --wait …` in the foreground | Blocks inside its turn; no background command wakes these tools. Stops 60 s before the delegate's deadline (exit 124) | You; also the main session with `--main-may-answer` |
| A job on a Worker (*B13*) | As above, with `--choice` required | As above | You, from the home Mac's Inbox, by a choice |
| Anything else (*B15*) | | Refused: "Notchling can't tell which session is asking." | |

What Task 1 records before the dependent tasks start (each a ruling in the ledger): the background `ask`'s fd 1 is the `.output` file its tool result names; the assistant line holding a Bash `tool_use` is in the transcript before the command runs; an idle main session wakes on a background command's exit; the parents of a command that Codex or agy runs under `notchling delegate` reach the Claude Code process within 16 steps (agy may run commands from a server it started earlier; if not, its asks are refused and the skill says so); whether a command in Codex's read-only and workspace-write sandboxes can reach the daemon's socket, and how long a command may block there; the shape of agy's permission request for a terminal command and how long one may block; whether a `claude -p` run waits for a background command.

## 9. Data model

*Builder's calls B1, B8, B17.* One table in `notchling.db`, created by schema version 5 in `engine/src/store.rs::migrate` (today 4), class **Node** in `store_class.rs::table_class`: a node's questions stay on it, and a Worker's reach the home only as an Inbox mirror. The daemon is its only reader and writer; the app sees rows through ops and the `asks` topic. An older daemon opening a version-5 file ignores the table.

```sql
CREATE TABLE IF NOT EXISTS asks (
    n INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    key TEXT NOT NULL,
    session TEXT NOT NULL,
    session_id TEXT,
    asker TEXT NOT NULL,
    agent_id TEXT,
    label TEXT,
    agent_type TEXT,
    model TEXT,
    tool_use_id TEXT,
    folder TEXT,
    branch TEXT,
    question TEXT NOT NULL,
    choices TEXT,
    main_may_answer INTEGER NOT NULL DEFAULT 0,
    mode TEXT NOT NULL,
    deadline_ms INTEGER,
    pid INTEGER NOT NULL,
    pid_start INTEGER,
    state TEXT NOT NULL,
    answer TEXT,
    choice INTEGER,
    by TEXT,
    inbox_id TEXT,
    created_ms INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    closed_ms INTEGER
);
CREATE INDEX IF NOT EXISTS asks_session ON asks(session, n);
CREATE INDEX IF NOT EXISTS asks_state ON asks(state);
```

| Column | Meaning |
| --- | --- |
| `id` | `q` + 32 lowercase hex characters from the OS's random source (as the vault's waits) |
| `key` | 32 hex characters the `ask` command made for itself; with `pid`, a retried open finds its question (*B3*) |
| `session`, `session_id` | The session key (`Agent::outbox_key()`: the job's short id, else the row id), and the Claude Code `sessionId` the parents gave; a question asked before the first scan listed the session keeps the `sessionId` as key until the next scan re-keys it |
| `asker` | `main`, `subagent`, `codex`, `antigravity`, `unknown` |
| `agent_id`, `label`, `agent_type`, `model`, `tool_use_id` | From [11](#11-finding-who-asked): the subagent's id, its description (or the session's name, or "Codex job"), type, model, and the `tool_use` that ran `ask` |
| `folder`, `branch` | The `ask` process's working folder from the kernel, and its branch (*B20*) |
| `question`, `choices` | The text (at most 2,000 characters) and a JSON array of 2 to 6 strings of at most 120 characters, or null |
| `main_may_answer` | The asker passed `--main-may-answer` |
| `mode`, `deadline_ms` | `background` or `wait`; a blocking ask's deadline |
| `pid`, `pid_start` | The asking process and its start time, from the kernel at open |
| `state` | [10](#10-states) |
| `answer`, `choice`, `by` | The answer text, the 1-based choice when a choice answered it, and `mac`, `phone` or `main` |
| `inbox_id` | The open Inbox item's id |
| `created_ms`, `updated_ms`, `closed_ms` | ms since 1970; `closed_ms` set on `delivered`, `dismissed`, `expired`, `gone` |

**Limits** (*B8*): `ask.open` refuses (nothing saved) an empty question, one over 2,000 characters, one choice or more than 6, a choice over 120 characters or empty, a 21st open question in a session ("20 questions from this session are already waiting.") and a 101st in all; `ask.answer` refuses an answer over 10 KB or empty.

**Retention** (*B17*): an `open` or `answered` row lives until it closes; nothing open expires by age. A closed row is deleted 24 hours after `closed_ms`, with its answer text, by the hourly sweep. There is no history: the agent's transcript holds the command and what it printed.

**Access.** `engine/src/ask/store.rs` keeps one `rusqlite::Connection` behind a mutex, opened with `store::open`; each call is one statement or one `BEGIN IMMEDIATE` transaction (insert with the limit counts), and nothing else runs while the mutex is held.

## 10. States

*Builder's call B2.*

| State | Means | Card | Moves to |
| --- | --- | --- | --- |
| `open` | Waiting for an answer | the question, choices, answer box, Dismiss | `answered` (an answer), `dismissed`, `expired` (a blocking ask's deadline), `gone` (the asker ended) |
| `answered` | An answer was taken; not yet handed to the asker | "Answered · Mac" | `delivered` (a waiter got it), `gone` (the asker ended first) |
| `delivered` | The asker got it | "Answered · Mac" / "· Phone" / "· main session" | deleted after 24 h |
| `dismissed` | You dismissed it; the asker is told at its next wait | "Dismissed" | deleted after 24 h |
| `expired` | A blocking ask's deadline passed | "No answer in time" | deleted after 24 h |
| `gone` | The asking process ended before an answer reached it | "Stopped waiting" | deleted after 24 h |

**First answer wins.** `AskOps` keeps an in-memory set of the questions an answer has been taken for, under one lock. Taking an answer (Chat, the notch, the Inbox's responder, the main session) checks and marks it there, which never blocks; then one `UPDATE asks SET state='answered', answer=?, choice=?, by=?, updated_ms=? WHERE id=? AND state='open'` persists it. A second answer finds the mark and is refused with who answered ("Already answered · Phone"). A daemon that stops between the mark and the write loses the mark and finds the question `open` again: it is shown again, never with an answer nobody gave. Dismiss takes the same mark.

## 11. Finding who asked

*Builder's calls B5, B15, B20.* All inputs about the caller come from the kernel for the peer of the socket, never from arguments: its pid (the socket's peer credentials, as `hook.request`), its parents, its command line (`KERN_PROCARGS2` on macOS, `/proc/<pid>/cmdline` on Linux), its working folder, the path of its fd 1 (`proc_pidfdinfo` with `PROC_PIDFDVNODEPATHINFO` on macOS, `/proc/<pid>/fd/1` on Linux) and its start time. They come through `vault_caller::Processes`, which gains `start_time`, `argv` and `stdout_path` (each `None` by default, so its fakes still build).

1. **The session.** Walk the parents (at most `LINEAGE_DEPTH`, 16) to the Claude Code process whose `sessions/<pid>.json`, in the main folder or a lane's, names a session (`ActingOps::session_of`, moved where both can use it). None: refused, `forbidden` "Notchling can't tell which session is asking." (*B15*). This is the fact the rules in [12](#12-who-may-answer) rest on.
2. **A delegated job.** A process on that walk whose executable is this `notchling` binary and whose command line is `notchling delegate <codex|agy> …` makes the asker `codex` or `antigravity`. That delegate process is then itself found by steps 3 to 6, so the card can say who delegated the job ("Codex job · delegated by «Review Task 3»").
3. **The transcripts.** The session's transcript (`transcript::locate` by its session id in that lane's projects folder) and its subagents' (`subagents::list_across_clears`, every depth), keeping files written in the last 10 minutes, at most 64, each read from its last 256 KB.
4. **Background match.** When fd 1 is a regular file whose name ends `.output`: the transcript that holds a Bash tool result whose `shells::launch` output path, made canonical, equals fd 1's canonical path is the asker's; its `tool_use_id` is kept.
5. **Command match.** Otherwise (or before that line is written): Bash `tool_use` lines at most 10 minutes older than the open whose command, split into simple commands at `&&`, `||`, `;`, `|` and newlines and each into shell words, holds one whose words after the program equal the `ask` process's command line after its program (the program itself matched by file name `notchling`). A command with `$(`, a backtick, `<<` or a `$` outside single quotes never matches. `tool_use` ids already held by another open question are skipped.
6. **The answer.** One transcript matches: the main transcript makes the asker `main` (its label the session's name), a subagent's makes it `subagent` (label its description, with its type and model). None, or more than one: `unknown` (card: "This session or one of its subagents").
7. **Timing.** The tool result and even the `tool_use` line may land just after the open. The search runs off the op thread, again on each file event in the transcript folders, for `FIND_MS` (10 s), then settles. The card shows the session at once and the agent when found (`ask.changed`).

**Branch** (*B20*): from the working folder up to the first `.git`; a folder `.git` or a file `.git` holding `gitdir: <path>` (a worktree); its `HEAD`: `ref: refs/heads/<name>` gives `<name>`, a hash gives its first 8 characters; no repository, no branch. No `git` process runs.

**Attribution is a label, not proof.** Transcripts are files any agent of the session can write. The session in step 1 comes from the kernel; who inside it asked is what the transcript says. [12](#12-who-may-answer) says what each fact may decide, and [Needs the maintainer](#needs-the-maintainer) M1 the trade-off.

## 12. Who may answer

*Builder's calls B6, B19, B21.* Pure, in `engine/src/ask/rules.rs`: `may_answer(row, who) -> Result<(), Refused>`.

- **The person**: an app-tier caller (Chat, the notch, the Mac's Inbox, the phone through the Inbox) may answer or dismiss any `open` question, with `acting` on (*B7*). `by` is `mac`, or `phone` when the Inbox's `decide` came from the bridge.
- **The main session**: an agent-tier `ask.answer` is taken only when all of these hold, else `forbidden` "Only the person answers this question.":
  1. the question is `open` and has `main_may_answer`;
  2. its asker is `subagent`, `codex` or `antigravity`, never `main` or `unknown` (*B21*);
  3. the caller's session, from the kernel ([11](#11-finding-who-asked) step 1), is the question's session;
  4. the caller is the main session: its `notchling answer` command is found ([11](#11-finding-who-asked) steps 4 and 5, waiting up to 5 s for the line) in the session's own transcript and in no subagent's;
  5. `acting` is on.
  It is recorded `by: main` and the card says "Answered · main session".
- **Nobody else.** An agent of another session fails rule 3; an agent asking its own question, or a sibling subagent, fails rule 4 unless it forges the main transcript (M1). A question from the main session (*B19*) fails rule 2: only you answer it.

**How the main session learns of one** (*B6*): `notchling asks` lists the open questions it may answer (`ask.mine`: the same rules 1 to 3; rule 4 is checked at the answer); `notchling asks --wait` blocks until there is one, so a main session that runs it in the background is woken when a subagent asks something it may answer. Notchling never types a question into the main session (non-goal; M5).

## 13. Waiting, delivery and restarts

*Builder's calls B3, B4, B22.*

- **Open.** `ask.open` saves the row, raises the Inbox item, publishes `ask.changed`, starts the search of [11](#11-finding-who-asked) and answers `{id, state: "open"}` at once. The command made a random `key` before its first call; an open retried after a lost reply, from the same process with the same `key`, returns the same question.
- **Wait.** The command calls `ask.wait {id, waitMs: 50000}` (its client timeout 60 s) in a loop. The daemon serves it only to the asking process: the caller's pid and start time must equal the row's (else `forbidden`). It answers at once when the row is `answered` (`{state: "answered", answer, choice?, by}`), `dismissed`, `expired` or `gone`, else when an answer comes, else `{state: "open"}` at 50 s. A reply carrying the answer moves the row to `delivered` (*B4*); the same process calling `ask.wait` again (its reply was lost) gets the same answer, so delivery never depends on the socket write.
- **Restart.** The command's socket call fails; it reconnects after 1, 2, 5 and 10 s, then every 10 s (a background ask without end; a blocking ask until its deadline), says hello again and calls `ask.wait` with the same id. `AskOps::load`, called between `Server::bind` and `server.spawn()` after `InboxOps::load`, keeps every `open` and `answered` row: an open one is raised in the Inbox again under a fresh id and its withdrawn copy is dropped (*B9*); an answered one is handed over at the next wait. An answer you gave while the command was reconnecting is kept the same way.
- **Gone** (*B22*): on a 1 s tick, for each `open` or `answered` row with no waiter connected: its process has ended (no such pid, or its start time differs) → `gone`, the Inbox item resolved Withdrawn, `ask.changed`. While a waiter is connected nothing is checked; a closed socket ends the wait and the tick takes over.
- **Expired**: a blocking ask carries `deadline_ms`; past it the row is `expired` and the Inbox item Withdrawn. The command stops at the same time with exit 124.
- **Dismissed**: Dismiss (card, Inbox, phone) sets `dismissed`; the next wait answers `{state: "dismissed"}` and the command exits 2.
- **Acting off.** Open questions keep waiting and waiters stay connected; answering waits for acting to be on.
- A question asked through a delegated job counts its wait inside the delegate's own limit; nothing else times out.

## 14. Ops, topic, command line, events

All additive. Names in a new block at the end of `wire.rs`'s `ops` and `topics`. Served by `AskOps` (`notchlingd/src/ask_ops.rs`, an `OpGroup`), built in a new `ask_wiring.rs` (*B7*: the acting gate as below).

### 14.1 Ops

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `ask.open` | agent | `{question, choices?: [..], mainMayAnswer?: bool, wait?: bool, deadlineMs?, key}` | `{id, state: "open"}`; behind `acting` |
| `ask.wait` | agent, the asking process only | `{id, waitMs?}` (at most 50,000) | `{state: "open"}`, `{state: "answered", answer, choice?, by}`, `{state: "dismissed"}`, `{state: "expired"}`, `{state: "gone"}` |
| `ask.mine` | agent | `{waitMs?}` (at most 50,000) | `[question…]`: the open questions the caller's session may answer by [12](#12-who-may-answer) rules 1 to 3, without answers; with `waitMs`, waits for the list to be non-empty |
| `ask.answer` | agent and app | `{id, choice?: n, text?}` (one of the two) | `{answered: true}`, or `{answered: false, state, by?}` when another answer won; behind `acting`; [12](#12-who-may-answer) decides |
| `ask.dismiss` | app | `{id}` | `{dismissed: true}` or `{dismissed: false, state}`; behind `acting` |
| `ask.list` | app | `{session?}` | `[row…]` in order, open and closed (closed for 24 h) |

Refusals, exact words (all `bad_request` unless noted): `Ask a question first.`; `That question is over 2,000 characters.`; `Give 2 to 6 choices.`; `A choice is empty or over 120 characters.`; `On a Worker, give choices (--choice): the answer comes from your Mac's Inbox.` (*B13*); `failed` `20 questions from this session are already waiting.`; `failed` `100 questions are already waiting.`; `forbidden` `Notchling can't tell which session is asking.`; `forbidden` `ACTING_OFF`; `failed` `Couldn't save the question.`; for `ask.wait` `forbidden` `Only the command that asked can wait for this answer.`; for `ask.answer` `forbidden` `Only the person answers this question.`, `bad_request` `Answer with a choice or text.`, `bad_request` `That answer is over 10 KB.`, `bad_request` `No such question.`

A row on the wire: `{id, session, sessionId?, asker, agentId?, label?, agentType?, model?, folder?, branch?, question, choices?, mainMayAnswer, mode, state, waiting, answer?, choice?, by?, createdMs, updatedMs, closedMs?}`. `waiting` says a waiter is connected. `answer` only on `ask.list` and the topic (app tier).

### 14.2 Topic

`asks`, app tier (`topic_tier`: `Tier::App`): `ask.changed {row}` on every insert or change, `ask.removed {id, session}` when a row is deleted. A client reads `ask.list` once and follows the topic.

### 14.3 Command line

```
notchling ask [--choice <text>]... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] [--socket <path>] -- <question>
notchling asks [--wait] [--json] [--socket <path>]
notchling answer <id> (--choice <n> | -- <text>) [--socket <path>]
```

`ask` prints on stderr `Asked (<id>). Waiting for the answer.`, then on stdout `Answer: <text>` (a choice prints its text) and `From: you (Mac)`, `From: you (phone)` or `From: the main session`, and exits 0. `--json` prints `{"id","state","answer","choice","by"}`. Dismissed: `No answer: the question was dismissed.`, exit 2. Refused (no daemon after 60 s of retries at the first open, acting off, no session, a limit, a Worker without choices): the reason on stderr, exit 3. No answer by the deadline: `No answer in time.`, exit 124. Anything else: exit 1. `--wait` blocks with a 900 s default timeout, or `--timeout`, never past `NOTCHLING_DELEGATE_DEADLINE_MS` − 60 s when that is set ([15](#15-the-skill-and-the-delegate)); without `--wait` the command waits without end (it runs in the background).

`asks` prints each question as `<id>  «label» (<asker>) · <branch> · <waited>`, the question, and its numbered choices; `--wait` waits for one. `answer` prints `Answered.` (exit 0), `Already answered · <by>.` (exit 2), or the refusal (exit 3).

### 14.4 The Inbox

`AskOps` raises and resolves items through `InboxOps` ([16.3](#163-inbox)); `engine/src/inbox/model.rs` gains `Kind::Question` and `ActionId::Choice1` to `Choice6` (`choice_1` … `choice_6`).

### 14.5 Events

*Builder's call B16.* `session.ask` (new, `crates/events/src/kinds/agents.rs`, Activity and DIAG, `Durability::Push`, 30 days): `session: Ref<Session>`, `then: AskThen` (`asked`, `found`, `answered`, `delivered`, `dismissed`, `expired`, `gone`, `refused`, `restored`), `asker: AskerKind` (`main`, `subagent`, `codex`, `antigravity`, `unknown`), `by: Answerer` (gains `main`), `chars: Count` (the question's length), `choices: Count`, `answer_chars: Count`, `waited: Millis`. No text. `op_kinds.rs` classifies every new op; none is quiet. A test plants a token in a question and an answer and asserts no event, Audit line, Activity row or log holds it.

## 15. The skill and the delegate

*Builder's calls B14, B18.*

**`notchling-ask`** (`engine/src/ask/skill.rs`, added to `install_ops::default_skills` for Claude Code, Codex and Antigravity; its text from code, its description fixed). It says, in steps, never in model names:

- Ask only for a decision you can't make or find out; one question per command; give 2 to 6 choices when the answer is one of a few (the phone answers only choices; on a Worker they are required); write a question that stands on its own (name the file, the branch, the trade-off), and never put a secret in it.
- **Claude Code**: run `'<binary>' ask [--choice '<option>']... -- '<question>'` with Bash `run_in_background: true`, then end your turn with one short line. Don't call SubagentHandback to wait: it ends your run. The command's exit wakes you; read its output: `Answer:` is the answer. When your work is done, deliver your report with SubagentHandback, even if you called it before: plain text at the end of a turn doesn't reach your caller.
- **Codex and Antigravity** (the tool note): run `'<binary>' ask --wait …` in the foreground with the longest timeout the tool allows; under `notchling delegate` it stops by itself before the delegate's limit (exit 124). Codex's note says what Task 1 found about the sandbox.
- Exit codes 0, 2 (decide yourself or report the question to your caller), 3 (ask your caller the way you would without Notchling), 124.
- `--main-may-answer` only when the main session knows enough to answer; for main sessions: `asks --wait` in the background, `answer <id>`, and answer only what you know.

**The delegate.** `codex_run.rs` and `agy_run.rs` give their child `NOTCHLING_DELEGATE_DEADLINE_MS` (ms since 1970, the delegate's own deadline). `ask --wait` stops 60 s before it and sends it as `deadlineMs`. An agy `session/request_permission` for a terminal command that is exactly this `notchling` binary (or `notchling`) followed by `ask --wait` and words, with no shell operator, substitution or redirection, is allowed once; every other request is declined as today. This waits for M3. Codex's sandbox is not changed (M4).

## 16. What the user sees

### 16.1 Chat

*Builder's call B12.* The selected session's open questions (its own and its subagents' and delegated jobs', matched by `session` key) are cards above the reply box, where the question card sits, oldest first, each `.part("Ask Card")`:

- A header line: «label», then the asker's type and model, the branch, and "waited 4 min" (from `createdMs`, redrawn each minute). Main: "This session asks"; `unknown`: "This session or one of its subagents".
- The question (selectable), the choices as buttons, a "Type an answer" box with Send, and Dismiss. Return sends; the reply box below is untouched.
- After an answer the card is one line for 10 minutes: "Answered · Mac: Keep it", "Answered · main session", "Dismissed", "No answer in time", "Stopped waiting".
- The Sidebar row shows the needs-you marker while any of its questions is open. A row from another node shows none in v1 (non-goal).
- Words in `locales/en/mac/chat.ftl`. The render hook `NOTCHLING_RENDER_ASK=<state>`.

### 16.2 Notch

*Builder's call B11.* The asking session's row shows "Question" as a needs-you state. Expanded, it shows the oldest open question with the existing question card (`QuestionDraft`: choices and a typed answer), "1 of 3" when more wait, and answers through `ask.answer`. The Inbox badge counts it with the rest.

### 16.3 Inbox

*Builder's call B9.* One item per open question, raised by `AskOps` with a responder once the search of [11](#11-finding-who-asked) settles (the agent found, or after `FIND_MS`), so its title names the agent:

- id `ask:<question id>:<ms>` (fresh per raise); kind `question`; title `«label» asks · <branch>` (cut to 80 characters; on a Worker `… on <node>`); detail the question, cut to 400 characters; source `{node?, session, job}`; no expiry.
- Actions: one per choice (`choice_1` …, label the choice cut to 60 characters, none primary), then Answer in Chat (`open`, primary when there are no choices; `open` holds `{ask: id, session}` and opens Chat on the card), then Dismiss (`cancel`).
- The responder only takes the answer mark ([10](#10-states)) and queues the write to `AskOps`; it returns whether the mark was taken, so the Inbox never shows an answer that wasn't the one taken. It refuses while `acting` is off.
- An answer from Chat, the notch or the main session resolves the item Answered with its choice's action (or none for text) and `answered_by` (`mac`, `phone`, `main`); `dismissed` resolves it with Dismiss; `expired` and `gone` withdraw it. The answer text is never in the item.
- The app's Inbox row words: "Answered · Mac: <choice>", "Answered in Chat", "Dismissed", and Answer in Chat opens Chat on the session and focuses its card.

### 16.4 Phone

*Builder's call B10.* No protocol change. The phone already lists Inbox items, shows any action by its label and sends its id (`apps/flutter/lib/features/inbox/inbox_screen.dart`), and shows `open` as Open on Mac. So it shows the question, answers with a choice or Dismiss, and a question without choices shows Open on Mac. `inbox.answer` from the phone needs the bridge and `acting` on, as today. The schema's descriptions of `InboxItem.kind` and `InboxAction.id` gain `question` and `choice_1` to `choice_6` (descriptions only; `tools/protocol/gen.py`). Typed answers on the phone are a later protocol addition.

## 17. Workers

*Builder's call B13.* **Decision: v1 includes asks from jobs on Workers, answered from the home Mac's Inbox by a choice.**

Why: the source held this back only "until the cluster can run work on workers"; cluster v1 lanes B to D have merged and Workers run Claude Code and Antigravity jobs. Everything the path needs already exists: each node runs `AskOps` on its own daemon, a Worker's Inbox items reach the home and the Can Be Master members through `node.inbox` (in memory, cut, the owner's qualified id), and an answer on a qualified id is forwarded to the owner by `InboxOps::decide`, whose responder takes it as on the Mac. So the Worker part adds no op, no policy row and no protocol field: one refusal and a node name on the card. What it can't do in v1 is take typed text, because an Inbox answer carries an action id only; so on a Worker `ask` requires choices, and the agent learns that at once.

- Who asked, the rules and the restart behavior are the Worker's own, as on the Mac. Its `acting` switch follows the home (`cluster_switches.rs`); off, `ask` is refused and the agent falls back.
- A Worker is a node whose own member record has the Worker role in the cluster state. A node in no cluster, or a Standby or client, is not.
- The plan's Worker task checks the existing chain end to end in the cluster testkit (a Worker's item on the home's `inbox.list` and the phone's, a choice forwarded back). If a link is missing it stops, and the Worker part moves to later; nothing on the Mac depends on it.
- Later: a Chat card under a remote session (a remote `asks` follow like `cluster_outbox.rs`), typed answers (text on `inbox.answer`, forwarded), a Worker's skills (installed the way the other `notchling-*` skills reach it; until then its agents simply don't know `ask`).

## 18. Tests, rule by rule

Engine tests use temp databases and transcript fixtures with real shapes and synthetic values (from Task 1's recordings). Daemon tests use a fake `Processes`, a temp data folder, a fake room and a fake Inbox responder path; the command-line test runs the built `notchling` over a copied fixture with its own daemon. None runs `claude`, `codex` or `agy`.

**Rule 1, who asked**
- A background ask: fd 1 is the `.output` file named in a subagent's tool result → that subagent, its description, type and model; the same file in the main transcript → `main`.
- A blocking ask: the command line matches one subagent's `tool_use` → that subagent; two subagents with the same command and one already claimed → the other; two unclaimed → `unknown`; none → `unknown`.
- `$(…)`, a backtick or `<<` in the command never matches; a command after `cd x &&` matches.
- No Claude Code session in the parents → refused, nothing saved.
- A delegated Codex job: `notchling delegate codex …` among the parents → `codex`, with who delegated it.
- The line written 3 s after the open → found by the file event, `found` emitted once.
- The branch from `.git/HEAD`, from a worktree's `gitdir:` file, a detached hash, no repository.

**Rule 2, only you answer**
- App tier answers any open question; agent tier is refused without `--main-may-answer`.
- With it: the main session of the same session answers (`by: main`); a subagent of that session (its `answer` command in its own transcript) is refused; an agent of another session is refused; a question from `main` or `unknown` is refused to every agent.
- `ask.wait` from another process, or the same pid with another start time, is refused.
- `ask.mine` lists only the caller's session's answerable questions, never their answers.
- Two answers at once (Chat and the Inbox on two threads, a thousand runs): exactly one taken, the other gets `answered: false` with the winner's `by`, the Inbox shows the winner.
- A crash between the mark and the write (a store that fails the write): the question is `open` after reload.

**Rule 3, a restart doesn't lose questions**
- Rows survive reopening the store; schema 5 on a version-4 file keeps every other table, and the class test passes with `asks`.
- `load`: an `open` row is raised in the Inbox again under a fresh id and its old copy dropped; an `answered` row is handed to the next wait.
- The command e2e: ask, stop the daemon, start it, answer, the same command prints the answer and exits 0 with the same id.
- A retried open with the same `key` and pid returns the same id; another pid with that `key` gets a new question.
- An answer given while no waiter is connected is delivered at the next wait.

**Waiting**
- A background ask answered after 2 s returns at once; at 50 s with no answer returns `open`.
- Dismiss → `dismissed`, exit 2; a blocking ask past its deadline → `expired`, exit 124, Inbox withdrawn.
- The asker's pid gone with no waiter → `gone` on the next tick; with a waiter connected, not checked.
- `NOTCHLING_DELEGATE_DEADLINE_MS` caps `--wait`.

**Limits and gates**
- Every refusal of [14.1](#141-ops) with its exact words; nothing saved.
- `ask.open`, `ask.answer`, `ask.dismiss` refused while acting is off; `ask.wait`, `ask.list`, `ask.mine` served.
- On a Worker an ask without choices is refused; with choices it is raised.

**No text anywhere**: one planted-token test over events, the Audit, Activity rows, `notchlingd.log` lines the daemon writes, and the Inbox item (the question may be in its detail, cut; the answer never).

**Inbox and phone**: the item's kind, title, detail cut, actions and their order; a choice from the phone (`decide` with `by: phone`) answers it; Answer in Chat is refused from the phone; an answer elsewhere resolves the item with the right words.

**Workers**: in the cluster testkit, a Worker's question reaches the home's `inbox.list` with a qualified id; a choice there is forwarded and the Worker's waiter gets it.

**App** (pure models tested everywhere, renders on a Mac): the card model maps every state and asker to words and buttons; the feed applies `ask.changed`/`ask.removed` in order and reseeds from `ask.list`; a session's cards include its subagents' questions; the notch's question card answers through `ask.answer`; `--render chat` with `NOTCHLING_RENDER_ASK` for each state; `bench_chat` counts unchanged with three open cards.

**Skill**: the text names `run_in_background`, ending the turn, reading `Answer:`, SubagentHandback after the answer, `--wait` for Codex and Antigravity, the exit codes; no model names.

**Live check** (the maintainer, plan's last task): [the plan](../plans/2026-11-16-ask.md#task-22-live-check-the-maintainer).

## 19. Risks

- **Attribution can be wrong or forged.** Two agents with the same command at the same second may be labelled `unknown`; any agent of a session can write lines into that session's transcripts. The card's name is a label. The kernel's session check holds; within one session the main-session rule can be forged by an agent of that session (M1).
- **An agent that never asks well.** Skill text can't force good questions. The card shows the agent and its branch so you can answer in context; Dismiss returns exit 2, and the skill says what to do then.
- **The wake depends on Claude Code.** Tested 2026-11-16 for background subagents; a change in Claude Code that stops waking agents on a background command's exit leaves them asleep with the answer printed. Task 1 records the shapes, the live check repeats it, and the `asks` table shows the question `delivered`, so you can see where it stopped.
- **Codex's sandbox may block the socket.** Then `ask` exits 3 there and the skill says so; the sandbox is not widened (M4).
- **agy's permission prompt.** Allowing one exact command is a new allow in the delegate (M3).
- **A long wait across a Claude Code restart.** The `ask` process dies with its session; the question goes `gone` and the card says Stopped waiting. A respawned session doesn't resume the wait.
- **The Inbox holds question text.** In a cluster it sits in `cluster.db` and the Standby sets, and a Worker's reaches the home (M2).
- **Schema number.** Another branch may take version 5 first; this one then takes the next number at merge.
- **Lane E** changes the home during a promote. A Worker's mirror follows the new home by its next `sync`; the forwarded answer goes to the owner, not the home.

## 20. Where the code goes

New:

- `crates/engine/src/ask/` (`mod.rs`, `model.rs`, `store.rs`, `rules.rs`, `find.rs`, `words.rs`, `branch.rs`, `inbox_item.rs`, `skill.rs`, and `*_tests.rs`), fixtures `crates/engine/tests/fixtures/ask/`
- `crates/notchlingd/src/ask_ops.rs`, `ask_rows.rs`, `ask_waiters.rs`, `ask_find.rs`, `ask_main.rs`, `ask_inbox.rs`, `ask_tick.rs`, `ask_wiring.rs`, `ask_cli.rs`, `caller_session.rs` (from `hook_ops.rs::session_of`), `process_facts.rs` (start time, command line, fd 1, executable), and their `*_tests.rs`; `crates/notchlingd/tests/ask_e2e.rs`
- `crates/app/src/chat/asks.rs` (the feed and the card model, beside `chat/outbox.rs`) and its tests, `crates/app/src/chat/view/ask_cards.rs`, `crates/app/src/notch/ask.rs`
- `crates/engine/src/delegate/agy_ask.rs` (M3's exact match)
- `docs/decisions/0022-ask.md`, `changes/ask.md`

Changed: `engine/src/store.rs`, `store_class.rs`, `inbox/model.rs`, `delegate/codex_run.rs`, `delegate/agy_run.rs`; `crates/events/src/kinds/agents.rs`, `kinds/choices.rs`; `crates/local-api/src/wire.rs`; `notchlingd/src/vault_caller.rs` (`Processes`), `hook_ops.rs`, `cli.rs`, `lib.rs`, `op_kinds.rs`, `daemon_wiring.rs` (one call into `ask_wiring`), `install_ops.rs`; `app/src/session_source.rs`, `actions.rs`, `chat/view/prompts.rs`, `chat/sidebar.rs`, `inbox/model.rs`, `notch/`; `locales/en/mac/{chat,notch,inbox}.ftl`; `protocol/agentconsole.v1.schema.json` (two descriptions); `docs/local-api-ops.md`, `docs/feature-map.md`, the crates' `AGENTS.md`, `docs/notes/inbox.md`, a new `docs/notes/ask.md` and its index line, `docs/policies/privacy.md` (M2).

## Needs the maintainer

Each needs your call before its task merges; the recommendation is what the plan builds unless you say otherwise.

1. **M1. How far the main session's answer is trusted.** The rule rests on two facts: the answering process is in the same session's process tree (from the kernel; an agent of another session can't fake it) and it is the main session, not a subagent (from the transcript, which any agent of that session can write). So one agent of a session could forge a line and answer a sibling's question that allowed the main session to answer. **Recommendation: accept for v1.** Only questions whose asker opted in, only from inside the same session, the card says "Answered · main session" and the event records `by: main`; nothing changes for questions meant for you, which no agent can answer.
2. **M2. Question text in the Inbox and across machines.** The Inbox item's detail holds the question (cut to 400 characters) so the Inbox and the phone can show it. The `inbox` table is class Cluster: in a cluster it lives in `cluster.db` and the Standby sets, and a Worker's question is mirrored to the home and the Can Be Master members. The answer text never goes in the Inbox. **Recommendation: yes**, with a line in `docs/policies/privacy.md` (a protected file) under the phone and the cluster. The alternative, an item without the question, makes the phone and the Inbox useless for it.
3. **M3. Letting an Antigravity delegate run `notchling ask`.** The delegate declines every agy permission request except edits with `--write`, so an agy job can't run `ask` today. **Recommendation: allow it once, only for a command that is exactly this `notchling` binary and `ask --wait` plus words, with no shell operator, substitution or redirection; decline everything else as today.** Without it, agy jobs can't ask.
4. **M4. Codex's sandbox.** If Task 1 finds that Codex's read-only sandbox blocks the daemon's socket, `ask` can't work in a read-only delegate. **Recommendation: don't widen the sandbox.** `ask` exits 3 there, the skill says to ask only in `--write` jobs or to report the question; widening it would give every read-only Codex turn the local API.
5. **M5. Waking the main session for a question it may answer.** Notchling could type the question into the main session through the Outbox. **Recommendation: no.** It would put agent-written text into a session's prompt and spend the main model's turn, which Ask exists to avoid; a main session that wants them runs `notchling asks --wait` in the background.
6. **M6. Workers in v1** ([17](#17-workers)). The builder's decision is to include them, choices only, through the existing inbox mirror. **Recommendation: include**; overrule it to keep Workers for later, which drops one task and one refusal.
7. **M7. A new skill in your tools.** `notchling-ask` goes into Claude Code's, Codex's and Antigravity's skills folders like the other `notchling-*` skills, so the providers see its text in each session's prompt. It holds no data, only the steps. **Recommendation: yes.**
