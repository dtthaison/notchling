# One provider contract: every capability, for every tool

Date: 2026-11-14 (asked for on 2026-11-13). Status: draft for the maintainer's approval. It widens
decision [0016](../../decisions/0016-provider-interface-in-the-engine.md) (one provider interface per
tool, in the engine) from six traits to every behavior Notchling has, and adds the declaration,
the fallbacks and the checks that keep it whole. Accepting it takes a new decision record (the next
free number; 0021 unless another lands first). Base: `merge/settings-batch`; "today" in this document
means the code there.

## In short

1. Everything Notchling does with a session is one **capability** from a fixed list (39 of them,
   [section 5](#5-the-capabilities)), each with one engine **port** and one shared flow. Chat, the
   notch, the phone, Search, the Inbox and the command line ask the port, never the tool.
2. Each **provider** (Claude Code, Codex, Antigravity; a new tool later) declares every capability as
   **Native** (the tool records or offers it, Notchling reads or drives that), **Fallback** (a named
   Notchling flow does it) or **NotApplicable** (with the reason the UI shows). A fourth state,
   **Gap**, names today's holes; it lives in a baseline that only shrinks, and a new provider starts
   with none. A Native capability whose port isn't routed through the registry yet is listed in
   `unwired.json`, which also only shrinks; there is no fifth state.
3. The declaration is the routing: the registry calls the provider's port for Native, Notchling's flow
   for Fallback, and returns the reason for NotApplicable. A port method that a provider declares
   anything but Native must answer `Unsupported`, so nothing is half there.
4. Three checks hold it: each provider's declaration is an exhaustive `match` with wildcards denied
   (a new capability or a new tool does not build until every cell is answered); one conformance
   suite every provider runs over its own fixtures; and `docs/provider-matrix.md`, generated from the
   registry and compared as a golden file.
5. Subagents of every tool are one row type: `Subagent.origin = {tool, via: native | delegated}`, from
   four sources (Claude Code's `subagents/` folder, Codex child rollouts, Antigravity child
   conversations, Notchling's delegation records), merged once in the engine.
6. One finished signal: the daemon's `SignalWatch` emits `turn.finished`, `subagent.finished` and
   `delegate.finished` (one payload) for every tool's sessions, listed by the daemon itself. They feed
   the Details Panel, the notch's peek, the Inbox, the phone (its Hub reads `signals.recent`, and its
   subagent list is filled for the first time) and keep awake.
7. `notchling delegate --background` returns a job id and `notchling delegate --wait <id>` blocks until
   the job ends and prints what a foreground run prints. Any agent of any tool can use them. A job's
   parent is the innermost id in its environment: `NOTCHLING_SESSION` (a delegated job, an `agy acp`
   Notchling runs), then `CODEX_THREAD_ID` (Codex sets it in every shell command; the name is in the
   0.166.0 binary), then `CLAUDE_CODE_SESSION_ID`, then the caller's process tree. Deeper jobs and
   children show as a tree under their parent.
8. Research: Codex writes each subagent as its own rollout whose first `session_meta` names
   `parent_thread_id`, and copies the parent's history into it after a second `session_meta`. Today
   Notchling lists every such child as a top-level Codex session **under the parent's id** (a bug found
   here). Antigravity makes each subagent its own conversation with `parent_conversation_id` and
   `nesting_depth` in its summaries row; Notchling already hides those, but never shows them.
9. Six phases, each shippable on its own: the contract and the matrix (no behavior change); subagents
   in one shape with the Codex and Antigravity readers; signals; delegate everywhere; the held queue
   for every tool; the rest of 0016's ports and the gaps.
10. No open questions. The user decided the three this draft asked: Tell Me When Work Finishes
    (default Long Turns and Jobs), gaps that only shrink, and Claude Code as a delegate target with the
    guidance for delegating in Notchling's delegate skill ([section 14](#14-decided-by-the-user)).

## Calls settled after review

A builder read the first draft; the coordinator settled these on 2026-11-14. Each is worked into the
sections it touches, where it is marked "call N".

1. **Base and the designs not landed** (coordinator's call). The base is `merge/settings-batch`.
   `setup.rules`, `setup.skills`, `setup.hooks`, `version.detect` and `version.update` stay in the list
   (39 capabilities) and are declared Gap for every tool, with the reason "design: 0019" or "design:
   version-sync", and listed in `gaps.json`. They leave it when those designs land. Gaps today: 38.
2. **Native before its port exists** (coordinator's call). Phase 0 declares the truth and changes no
   behavior. `contract/unwired.json` (shrinks only, like `gaps.json`) lists each Native capability whose
   port isn't routed through the registry yet. The conformance suite runs only wired capabilities and
   fails when a capability is neither wired nor in `unwired.json`. `Support` keeps its four states.
3. **Parent precedence: innermost wins** (coordinator's call). `NOTCHLING_SESSION`, then
   `CODEX_THREAD_ID`, then `CLAUDE_CODE_SESSION_ID`, then the process tree. `notchling delegate` removes
   inherited `CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID` and `NOTCHLING_SESSION` from its job's
   environment and sets `NOTCHLING_SESSION` to its own job's id. Deeper rows show nested under their
   parent in the top session's Details Panel, a tree collapsed past depth 1 and paged as today, so
   `Registry::subagents` recurses.
4. **Codex children come from an index** (coordinator's call). The daemon keeps a persistent child
   index, its own small SQLite under `daemon/`, keyed by rollout path with size and mtime, filled
   incrementally from the watcher. The first backfill reads at most 30 days of day folders, newest
   first. No per-parent unbounded scan.
5. **The daemon lists every tool's sessions in phase 2** (coordinator's call). A slice of the Sessions
   port comes forward: the daemon reuses `bridge::foreign` read-only, whether or not the bridge switch
   is on or an app is attached. The phone Hub moves onto `signals.recent`, so finish events have one
   source. A terminal-run Antigravity session's `turn.finished` comes from its summaries row's status
   change, else from the quiet rule.
6. **A finished-agents reader in the daemon** (coordinator's call). The daemon gets its own cached
   reader of a parent transcript's `<task-notification>` lines (a small tail read, cached by size and
   mtime, like `glance`), used by `SignalWatch` and the phone. Chat keeps its `ChatData` path.
7. **The held queue joins, for every tool** (coordinator's call), as the user asked: "one queue; while
   it works, combine everything sent into that one". Codex's outbox changes to join; its outbox tests
   change and are flagged for the maintainer. An Antigravity session another `agy` holds stays
   disabled with its reason, and the held queue takes nothing for it.
8. **The wire name is `antigravity`** (coordinator's call), as `catalog::Tool` serializes it. `agy` is
   only the command's name. Readers of delegation records and ids accept `agy` too; writers emit
   `antigravity`. Row ids become `antigravity:<conversation>`.

## Contents

- [Calls settled after review](#calls-settled-after-review)

1. [Why](#1-why)
2. [Goals, non-goals, anti-goals](#2-goals-non-goals-anti-goals)
3. [Terms](#3-terms)
4. [Research: how Codex and Antigravity run subagents](#4-research-how-codex-and-antigravity-run-subagents)
5. [The capabilities](#5-the-capabilities)
6. [The contract in the engine](#6-the-contract-in-the-engine)
7. [The matrix today](#7-the-matrix-today)
8. [Subagents in one shape](#8-subagents-in-one-shape)
9. [Signals: one finished event](#9-signals-one-finished-event)
10. [Delegate: target, caller, wait](#10-delegate-target-caller-wait)
11. [How "every capability" is enforced](#11-how-every-capability-is-enforced)
12. [Phases](#12-phases)
13. [Left to the builder, and to check while building](#13-left-to-the-builder-and-to-check-while-building)
14. [Decided by the user](#14-decided-by-the-user)
15. [Sources](#15-sources)

## 1. Why

The maintainer, 2026-11-13: every behavior a provider can have is one shared flow; adding a provider
means implementing every capability; where the provider has no native support, Notchling's own flow
stands in (the held queue, `notchling delegate` jobs shown as subagents, Notchling's own finished
notification). Nothing silently missing.

What is uneven today, found in the code on `merge/settings-batch`:

- **Subagents show for Claude Code sessions only.** `ChatData::subagents` (`crates/app/src/chat/data.rs`)
  answers rows only for `(Some(path), Tool::Claude)` and `Vec::new()` otherwise. A Codex or
  Antigravity session's Details Panel lists nothing, not even jobs it handed off with `notchling delegate`.
- **Codex's own subagents are top-level sessions.** `tools/codex/list.rs` makes a row of every rollout,
  and `transcript/codex::meta` keeps the last `session_meta` in the file. A child rollout holds two
  (its own, then a copy of the parent's), so the child is listed with the parent's id, folder and first
  prompt: a second `codex:<parent>` row ([4.1](#41-codex)).
- **A delegation knows its parent only from Claude Code.** `delegate::record::PARENT_VARIABLE` is
  `CLAUDE_CODE_SESSION_ID`, and no record is written without it. A job a Codex or Antigravity session
  starts has no parent and no `--wait`.
- **The phone never gets subagents.** `bridge::model::Session::subagents` is `Vec::new()` and nothing
  fills it; `transcript::subagents::console_list` has no caller. No tool's subagents reach the phone.
- **Antigravity rows have no tokens or doing line.** `subagents::peek` answers `None` for
  `Origin::Antigravity`, delegated jobs included.
- **Send while working** is three behaviors: Claude Code's message is held by Notchling
  (`reply::held`), Codex's waits in `CodexRuns`' outbox, Antigravity's box is disabled ("busy").
- **Search** indexes Claude Code and Codex text, Antigravity titles only (`search/index/read.rs`).
- **`notchling sessions`** lists Claude Code only: `sessions.list` reads the Monitor; Codex and
  Antigravity rows are built separately by the app and by the bridge's `foreign_rows`.
- **Eight enums name the tools** (`catalog::Tool`, `transcript::subagents::Origin`,
  `search::SessionTool`, `delegate::options::Tool`, `bridge::foreign::Tool`, the app's `chat::data::Tool`,
  `tools::requests::RowTool`, `sheets::agents::Tool`), two of them with only two of the three, and
  about 350 non-test lines across `app`, `engine` and `notchlingd` name a variant; 36 app files outside
  `chat/view/tool/` do.

Decision 0015 put Chat's composer behind `ChatTool`. Decision 0016 moved the interface into the
engine as six traits (`Catalog`, built; `Capabilities`, `Sessions`, `Turns`, `Prompts`, `Commands`, not
yet) and left "whether `Capabilities` is a struct of flags or a method per part" to the builder. This
design answers that question with a declaration per capability, widens the list to what 0016 left
out (subagents, signals, delegation, setup, versions, search, the phone) and says how it is enforced.

## 2. Goals, non-goals, anti-goals

**Goals**

1. Every behavior is a capability with one port and one shared flow; surfaces ask the registry by
   session id.
2. Every provider answers every capability: Native, Fallback (named) or NotApplicable (with a reason).
3. A new provider is its ports plus one registration. It does not build until every capability is
   declared, and does not merge until the conformance suite passes on its fixtures.
4. Subagents of every tool, native or delegated, are one row type with one finished signal that
   reaches Chat, the notch, the Inbox and the phone.
5. Any agent can start a job in the background and wait for it.

**Non-goals**

- Turning on Codex's multi-agent feature or Antigravity's subagents, or changing either tool's config.
  Notchling shows what they do.
- Hooks in Codex or Antigravity (decision 0019 decided none).
- Stopping one native subagent from outside where the tool offers no way to.
- The Swift app (retired) and Claude Desktop (accounts only).

**Anti-goals** (a failure even if it works)

- A capability missing for one tool with no declaration saying so.
- A fallback that looks native. Where the difference matters the UI says whose it is ("Held by
  Notchling until the turn ends").
- A big-bang rewrite. Each phase ships alone, with the app working after it.
- A model compute or a draw that reads a file (crates/AGENTS.md): subagent sources run in the loader
  and the daemon, cached by size and mtime.
- Opening a tool's own database for writing. Codex's `state_5.sqlite` is not needed; Antigravity's
  summaries db is read through Notchling's copy, as now.
- A signal fired twice for one finish, or fired for what was already finished when the daemon started.

## 3. Terms

| Term | Means |
| --- | --- |
| Provider | A tool that runs sessions: Claude Code, Codex, Antigravity CLI. One `Tool` value each |
| Capability | One behavior from the list in section 5, with an id like `subagent.status` |
| Port | The engine trait (or trait method) behind a capability |
| Native | The tool records or offers it, and Notchling reads or drives that record or API |
| Fallback | Notchling's own flow does it, named (`held-queue`, `delegate-jobs`, ...), because the tool has nothing to read or drive |
| NotApplicable | It can't make sense for this tool; the declaration carries the reason the UI shows |
| Gap | Should be Native or Fallback and isn't built yet; may name a flow that stands in meanwhile; temporary, listed in `gaps.json` |
| Wired | A Native or Fallback cell whose answer goes through the registry's route. One that doesn't yet is listed in `unwired.json`, which only shrinks; it is not a state of `Support` (call 2) |
| Surface | A consumer that is the same for every provider: Chat, the notch, the phone, Search, the Inbox, logs, keep awake, the `notchling` command line |
| Always flow | A Notchling flow that runs for every provider on top of its native answer: delegated jobs as subagents, `SignalWatch`, the activity log |

## 4. Research: how Codex and Antigravity run subagents

Done 2026-11-13 and 14 with Codex CLI 0.166.0 and Antigravity CLI 1.5.8 on the maintainer's Mac. Local
files were read for their shape only (record types, keys, table and column names); no content, prompt
or id is copied here. Two real runs per tool were allowed: Codex used one, Antigravity two, each in a
scratch folder with the cheapest model and a prompt that spawns one subagent. Every fact below says how
it is known: **run** (seen in a real run), **files** (seen in existing local files), **source** (read in
the tool's source or binary), **docs**.

### 4.1 Codex

| Question | Answer | Known by |
| --- | --- | --- |
| How it spawns | Model tools. Version 2, namespace `collaboration`: `spawn_agent`, `send_message`, `followup_task`, `wait_agent`, `list_agents`, `interrupt_agent`. Version 1 ("collab"): `spawn_agent`, `send_input`, `wait_agent`, `close_agent`, `resume_agent`. Flags `multi_agent` (v1, default on, deprecated) and `multi_agent_v2` (listed off), yet every subagent on this Mac is v2 with no flag passed. `/subagents` in the TUI switches between a session's subagents; `/agents` is a separate command center | source, run, files |
| Limits | Depth 1 by default (`agents.max_depth`); 6 threads in v1, 4 concurrent in v2 (`agents.max_concurrent_threads_per_session`). Roles `default`, `worker`, `explorer`, plus TOML roles in `~/.codex/agents/` | source |
| Where a subagent lives | Its own rollout, `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. Not in `session_index.jsonl` | run, files |
| Parent link | The child's **first** `session_meta` payload has `parent_thread_id`, `thread_source: "subagent"`, `agent_nickname`, `agent_path` (`/root/<name>`), `multi_agent_version`, and `source: {subagent: {thread_spawn: {parent_thread_id, depth, agent_path, agent_nickname, agent_role}}}`. A **second** `session_meta` follows, the parent's, and after it the parent's history up to the spawn (a forked copy; in the run it held a `task_started` and a `turn_aborted`). The child's own turn starts at the next `task_started` | run, source |
| Also on disk | `state_5.sqlite`: `threads` (`source`, `thread_source`, `agent_nickname`, `agent_role`, `agent_path`, `tokens_used`, `rollout_path`) and `thread_spawn_edges(parent_thread_id, child_thread_id, status Open/Closed)`. The run's child did not appear there when checked, and the schema is at migration 62: not a source to depend on | files, source |
| Status | `AgentStatus`: PendingInit, Running, Interrupted, Completed, Errored, Shutdown, NotFound, derived from events (TurnStarted, TurnComplete, TurnAborted, ShutdownComplete). In the child's rollout: `event_msg` `task_started`, then `task_complete` (`last_agent_message`, `error`, `duration_ms`) or `turn_aborted` (`reason`) | source, run |
| How the parent hears | V2 queues a message from the child to the parent that starts no turn: in the parent's rollout an `inter_agent_communication_metadata` record, then `response_item` `agent_message`. The parent's rollout also gets `event_msg` `item_completed` with item `SubAgentActivity` (kind `started` with model and effort, `interacted`, `interrupted`, `completed`, and `agent_thread_id`), and `CollabAgentToolCall` for `wait_agent`. V1 injected `<subagent_notification>` into the parent | run, source |
| Tokens, doing line | The child's `event_msg` `token_count` (`info.last_token_usage`, `model_context_window`) and `token_usage_record`: the same shapes Notchling's Codex glance already reads. Doing line: the newest `function_call` or reasoning item | run, source |
| Live from outside | `codex exec --json` showed only the parent's thread, a `collab_tool_call` for `wait` with empty receivers, and the reply: no child id. `app-server` v2 has `CollabAgentToolCall` and `SubAgentActivity` items with thread ids. Notchling, which runs Codex turns with `exec --json`, learns of children from the files | run, source |
| Session id in shell commands | `CODEX_THREAD_ID` is set in every command the model runs (`protocol/src/shell_environment.rs`); the name is in the installed 0.166.0 binary | source, binary |

What it means here:

- **Bug.** A child rollout becomes a top-level Codex row (`tools/codex/list.rs::rollouts` and
  `session`), and `transcript/codex::meta` keeps the last `session_meta`, which is the parent's. Fix:
  the first `session_meta` decides; a rollout with `parent_thread_id` (or `thread_source: "subagent"`)
  is a subagent and leaves the Sidebar and the notch, as delegated rows do.
- **Every reader of a child skips the forked block**: the conversation (its own turn starts at the
  first `task_started` after the second `session_meta`), the status tail, Search, and the cost reader.
  The run's forked block held no `token_count`, but a parent with earlier turns would carry theirs,
  and the cost tracker keys tokens by file, so they would be counted twice (to check in phase 1).
- Children are found through the daemon's own child index (call 4, [8.2](#82-the-four-sources)): a
  head read of each rollout as the watcher sees it change, kept by size and mtime, not through
  `state_5.sqlite`, whose schema is Codex's to change.

### 4.2 Antigravity

| Question | Answer | Known by |
| --- | --- | --- |
| How it spawns | Model tool `invoke_subagent` (role, prompt, `workspace`: `inherit`, `branch` (a git worktree) or `share`; in the run also `TypeName` and `Model`), `define_subagent` for a transient one, and `manage_subagents` (seen with action `list`; other actions unknown, and the docs don't name it). `/agents` opens the Agent Manager: subagents by prompt, with state `running`, `done`, `error`, `killed`, the step and the model; `k` kills one. `/browser` and `/swarm` workers are subagents too | docs, run |
| Limits | Nesting depth 10; a child inherits the parent's permissions and sandbox, not its history. Children run in the same `agy` process (no second process seen; not checked by process inspection) | docs, run |
| Where a subagent lives | Its own conversation: own id, `brain/<id>/.system_generated/logs/transcript.jsonl` (same line shape as the parent's), `conversations/<id>.db` (one SQLite file per conversation in `~/.gemini/antigravity-cli/`), and its own `conversation_summaries` row | run, files |
| Parent link | Summaries columns `parent_conversation_id`, `nesting_depth` (1 for a child), `agent_name` (the type: `self` for a generic child; `image-generator`, `research` seen); title and preview empty. The role is only in the parent's `invoke_subagent` call | run, files |
| Status | Summaries `status`: `CASCADE_RUN_STATUS_IDLE` for every child seen (done or idle), `..._RUNNING` while running (not seen for a child in runs this short), plus a `killed` column. `manage_subagents list` said `running` for a child that had already reported, so it lags | run, files |
| How the parent hears | The child calls its `send_message` tool to the parent and goes idle; the parent gets a `SYSTEM_MESSAGE` step `[Message] ... sender=<child id> ...` at its next step (57 s later in the run, the parent being busy). Notchling's `delegate::progress` already waits on this shape for delegated agy jobs (`.system_generated/subagents/*.json` with `conversationId`, `messages/*.json` with `sender`) | run, source (Notchling) |
| Tokens, doing line | The child transcript's `PLANNER_RESPONSE` steps carry `input_tokens`, `cache_read_tokens`, `output_tokens`; the newest step's `tool_calls[].name` or `content` is the doing line | run |
| Live from outside | `agy --print --output-format stream-json` sends `step_update` events of type `subagent` (ACTIVE, then DONE) with `subagent_info.subagents[]` (`type_name`, `role`, `conversation_id`, `log_uri`, `workspace_uris`). Over ACP, whether children appear is untested (an old ACP log shows a subagent call inside an ACP session) | run, changelog |
| Session id in shell commands | None found: no `ANTIGRAVITY_*`, `AGY_*` or `GEMINI_*` name that holds an id; the id reaches MCP servers as `_meta["antigravity.google/root_conversation_id"]` | binary names, changelog |

What it means here:

- `tools/agy/summaries.rs` reads `WHERE nesting_depth = 0`, so children are not top-level rows: no
  bug, but nothing shows them. A second query by `parent_conversation_id` over the same copy lists them.
- The cost reader walks `brain/*` and counts children as their own folders, which is right: a child
  spends its own tokens.
- One ACP experiment is owed before Chat's own Antigravity sessions rely on live child updates; the
  files are enough for list, status, glance and open.

### 4.3 Claude Code, for comparison

The Agent tool writes `<transcript>/subagents/agent-<id>.jsonl` and `agent-<id>.meta.json`
(`description`, `agentType`, `model`, `effort`, `toolUseId`, `requestShape`, `spawnDepth`, `finished`).
The parent learns of a finish through `<task-notification>` (`task-id`, `status`) in its transcript;
Notchling also reads "Report delivered to your caller" in a child's tail and calls a child quiet after
10 minutes without writes. Claude Code also has `Stop` and `SubagentStop` hooks. This design does not
add them: the files already carry the finish, and decision 0019 keeps Notchling's hooks to its five.

## 5. The capabilities

One list for every provider. The id is what the registry, the golden matrix and the conformance tests
use. "Port" is the engine trait method behind it (section 6).

| Group | Id | What it is | Port |
| --- | --- | --- | --- |
| Sessions | `session.list` | Find the tool's sessions: id, folder, title, account, times | `Sessions::list` |
| | `session.status` | Working, needs you, idle, ended | `Sessions::status` |
| | `session.new` | Start one in a folder | `Control::new` |
| | `session.stop` | End its process or job | `Control::stop` |
| | `session.remove` | Delete it | `Control::remove` |
| | `session.move` | Run it under another account | `Control::move_to` |
| | `folder.trust` | The tool's trust step for a folder | `Control::trust` |
| Conversation | `conversation.read` | Its messages as one `Conversation` (Chat, phone) | `Conversations::read` |
| | `conversation.follow` | A running turn as it grows | `Conversations::follow` |
| | `conversation.context` | Tokens in context and the window (Context Meter) | `Conversations::context` |
| Subagents | `subagent.list` | Its subagents: id, description, type, model, effort, depth, times | `Subagents::list` |
| | `subagent.status` | Running, completed, failed, stopped, quiet | `Subagents::list` (status field) |
| | `subagent.glance` | A running one's doing line and context tokens | `Subagents::glance` |
| | `subagent.open` | Its conversation | `Subagents::open` |
| | `subagent.stop` | Stop one subagent from outside | `Subagents::stop` |
| Signals | `signal.turn_finished` | A turn ended, with its outcome | `Signals::turn` |
| | `signal.subagent_finished` | A subagent ended, with its outcome | `Signals::subagent` |
| | `signal.needs_you` | A question or permission waits | `Prompts::pending` |
| Turns | `turn.send` | Send a message, with images | `Turns::send` |
| | `turn.hold` | A message sent while it works waits for the turn's end | `Turns::hold` |
| | `turn.take_back` | Edit or take back that waiting message | `Turns::take_back` |
| | `turn.interrupt` | Stop the current turn | `Turns::interrupt` |
| | `turn.options` | Model, effort, mode for the next turn | `Turns::set_option` |
| | `prompt.question` | Answer a question | `Prompts::answer` |
| | `prompt.permission` | Answer a permission | `Prompts::permit` |
| | `command.slash` | List and run slash commands | `Commands::list`, `run` |
| Accounts and usage | `account.switch` | List, add, switch logins | `AccountProvider` (exists) |
| | `account.lanes` | Two accounts' sessions at once | `Lanes` |
| | `usage.limits` | Plan limits | `ProviderSource` (exists) |
| | `usage.cost` | Tokens and dollars per day and model | `Cost::parse` |
| | `catalog.models` | Models, efforts, windows, prices | `CatalogSource` (0016) |
| Delegation | `delegate.target` | `notchling delegate <tool>` runs a job on it | `DelegateTarget` |
| | `delegate.caller` | Its session can call `notchling delegate` and own the job | `Caller::session_of` |
| Setup | `setup.rules` | Always-on instructions (decision 0019) | `instructions::Target` |
| | `setup.skills` | Skills (0019) | `instructions::Target` |
| | `setup.hooks` | Notchling's hooks (0019) | `instructions::Target` |
| Versions | `version.detect` | The installed version (version-sync design) | `Versions::installed` |
| | `version.update` | Update, or install an exact version | `Versions::install` |
| Search | `search.text` | Its sessions' text for Search's sessions corpus | `SessionText` |

Not on the list, because they are **surfaces** built from the list, the same for every provider: the
notch's rows and peek, keep awake, the Inbox, the activity log, the phone's list, chat, replies,
subagents and alerts, `notchling sessions`, `notchling delegate --wait`. A surface works for a provider
as soon as the capabilities it reads do; [section 7.2](#72-surfaces) says where a surface still
branches on the tool or skips a capability.

## 6. The contract in the engine

### 6.1 Where it lives

`crates/engine/src/contract/` (0016 suggested `agents/`; one module either way, the name is the
builder's): `mod.rs` (`Tool`, `Capability`, `Support`, `Flow`), `registry.rs`, `ports.rs`, `gaps.json`,
`unwired.json`, one folder per provider (`claude/`, `codex/`, `antigravity/`), each with a `support.rs`
(the declaration) and the port implementations, which call today's readers and runners
(`transcript::*`, `tools::codex`, `tools::agy`, `control`, `delegate`). `catalog::Tool` becomes
`contract::Tool`, re-exported where it is used; the other seven enums go in phases 1 to 6.

**Names on the wire** (call 8). `Tool` serializes as `claude`, `codex` and `antigravity`, as
`catalog::Tool` does today. `agy` is only the command's name (`notchling delegate agy` keeps working).
Every reader of a tool name or id accepts `agy` as well: delegation records (`"tool": "agy"`, file
names `agy-<session>.json`), row ids (`agy:`, `agy-new:`, `agy-perm:`), search keys. Every writer emits
`antigravity`: row ids become `antigravity:<conversation>`, records `"tool": "antigravity"`. Keys the
Mac has stored under an old id (drafts and settings in `notchling.db`, hidden rows, `search.db` keys)
are read under both until a one-time rewrite at the daemon's start moves them; the phone protocol's
`Session.tool` description lists `antigravity` (the phone app stores the value but doesn't branch on
it).

### 6.2 The types

```rust
pub enum Tool { Claude, Codex, Antigravity }   // "claude", "codex", "antigravity"; reads "agy" too

pub enum Capability { SessionList, SessionStatus, /* ... the 39 of section 5 */ SearchText }

impl Capability {
    pub const ALL: [Capability; 39];
    pub fn id(self) -> &'static str;          // "subagent.status"
    pub fn title(self) -> &'static str;       // "Subagent status"
}

pub enum Support {
    Native { how: &'static str },
    Fallback { flow: Flow, why: &'static str },
    NotApplicable { why: &'static str },
    Gap { plan: &'static str, meanwhile: Option<Flow> },
}

pub enum Flow {
    HeldQueue, HideRow, DelegateJobs, ProcessTree, CatalogData, VersionProbe, VersionCache, TitleIndex,
}
```

`how`, `why` and `plan` are one short line each, shown in the matrix and, for NotApplicable and Gap,
as the disabled control's tooltip (decision 0015's rule: a part is never missing, it says why).

### 6.3 The provider and its ports

```rust
pub trait Provider: Send + Sync {
    fn tool(&self) -> Tool;
    fn support(&self, capability: Capability) -> Support;   // exhaustive, no wildcard arm
    fn sessions(&self) -> &dyn Sessions;
    fn conversations(&self) -> &dyn Conversations;
    fn subagents(&self) -> &dyn Subagents;
    fn signals(&self) -> &dyn Signals;
    fn turns(&self) -> &dyn Turns;
    fn prompts(&self) -> &dyn Prompts;
    fn commands(&self) -> &dyn Commands;
    fn control(&self) -> &dyn Control;
    fn usage(&self) -> Usage<'_>;            // its AccountProvider id, ProviderSource, cost parser, lanes
    fn catalog(&self) -> &dyn CatalogSource;
    fn delegate(&self) -> &dyn Delegate;     // target runner and caller lookup
    fn setup(&self) -> &dyn instructions::Target;
    fn versions(&self) -> &dyn Versions;
    fn text(&self) -> &dyn SessionText;
}

pub trait Subagents: Send + Sync {
    fn list(&self, parent: &SessionRef, roots: &Roots, now: Timestamp) -> Result<Vec<Subagent>, Unsupported> {
        Err(Unsupported::not_native(Capability::SubagentList))
    }
    fn glance(&self, subagent: &Subagent) -> Result<Glance, Unsupported> { /* same default */ }
    fn open(&self, subagent: &Subagent) -> Result<Arc<Conversation>, Unsupported> { /* same */ }
    fn stop(&self, subagent: &Subagent) -> Result<(), Unsupported> { /* same */ }
}
```

Every port method has a default that answers `Unsupported`, so a provider writes only the methods it
declares Native. Methods are blocking, take their roots as parameters (tests never reach `~/.claude`,
`~/.codex`, `~/.gemini`) and keep effects behind today's seams (`Launcher`, `ToolSpawner`, `Spawner`,
`Pty`, `HttpClient`, `Keychain`), so every implementation runs over fixtures and fakes.

### 6.4 The registry routes by the declaration

```rust
impl Registry {
    pub fn support(&self, tool: Tool, c: Capability) -> Support;

    pub fn subagents(&self, top: &SessionRef, roots: &Roots, now: Timestamp) -> Vec<Subagent> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.under(top, 1, roots, now, &mut seen, &mut out);
        out
    }

    fn under(&self, parent: &SessionRef, depth: u8, roots: &Roots, now: Timestamp,
             seen: &mut HashSet<String>, out: &mut Vec<Subagent>) {
        if depth > MAX_DEPTH {
            return;
        }
        let p = self.of(parent.tool);
        let mut rows = match p.support(Capability::SubagentList) {
            Support::Native { .. } => p.subagents().list(parent, roots, now).unwrap_or_default(),
            Support::Fallback { flow, .. } | Support::Gap { meanwhile: Some(flow), .. } =>
                self.flows.subagents(flow, parent, roots, now),
            _ => Vec::new(),
        };
        rows.extend(self.flows.delegate_jobs.subagents(parent, now));   // an Always flow
        rows.sort_by(|a, b| a.started.total_cmp(&b.started));
        for mut row in rows {
            if !seen.insert(row.id.clone()) {
                continue;
            }
            row.depth = depth;
            let next = row.as_parent();      // a native child or a job can have children of its own
            out.push(row);
            self.under(&next, depth + 1, roots, now, seen, out);
        }
    }
}
```

It recurses (call 3): a Codex child, an Antigravity child or a delegated job is itself a parent, of
native children and of jobs it delegated. `MAX_DEPTH` is 10 (Antigravity's nesting limit) and `seen`
stops a loop. Listing the tree is cheap (index lookups, a summaries query, record reads); what costs a
read, `glance`, runs only for rows on screen (8.3).

So a provider can't declare Native and skip the code (the conformance suite calls it), can't declare
Fallback and also half-answer (its port must say `Unsupported`), and a surface can't forget a provider
(it never names one).

### 6.5 Which process runs what

The registry is built in both processes, over the same roots. The app's loader and Search use the
reading ports (`session.list` for rows it reads itself, `conversation.*`, `subagent.list|glance|open`,
`search.text`), off the main thread, cached by size and mtime (decision 0004: the app reads transcripts
itself). The daemon uses the acting ports behind the `session.*` ops (0016's one API, routed by id), and
runs `SignalWatch` (section 9). The phone bridge and `notchling sessions` read through the daemon.

Three readers live in the daemon only:

- **The Codex child index** (call 4, [8.2](#82-the-four-sources)). The app's loader asks for a
  parent's children with a quiet, read-only, app-tier op (`codex.children {thread}`). With no daemon (a
  fixture root, a `--render`) it reads the parent's start day folder and the next one, two folders at
  most, so fixtures still show children.
- **Every tool's session list** (call 5, from phase 2): `bridge::foreign`'s rows, read-only, whether or
  not the bridge switch is on or an app is attached, beside the Monitor's Claude Code rows.
- **The finished-agents reader** (call 6): a parent transcript's `<task-notification>` statuses, for
  `SignalWatch` and the phone. Chat keeps reading them from the conversation it already parsed
  (`ChatData`).

### 6.6 Notchling's flows

| Flow | What it does | Where | Used today |
| --- | --- | --- | --- |
| `held-queue` | One held message per session while it works; everything sent meanwhile joins it; sent when the turn ends; take back edits it. For every tool (call 7) | `engine::reply::held`, `held_ops.rs` | Claude Code (since 8e73a97: its own queue is lost on restart and injected mid-turn); Codex and Antigravity in phase 4 |
| `hide-row` | A row the tool can't delete is hidden in Notchling; the tool's data stays | the app's hidden list | (Antigravity's Remove, phase 5) |
| `delegate-jobs` | `notchling delegate` records as subagents of their parent; status, stop, wait | `engine::delegate::record` | Always, for every provider |
| `process-tree` | Find a caller's session from its ancestor processes | new, `contract::caller` | (Antigravity's `delegate.caller`, phase 4) |
| `catalog-data` | Windows, prices and names from Notchling's data file | `engine::catalog` | prices for every tool |
| `version-probe` | `agy --version` at the slow cadence, or ACP `initialize`'s agent info | version-sync design | (Antigravity) |
| `version-cache` | Notchling keeps the agy binaries it checked, to install an exact version | version-sync design, section 8 | (Antigravity) |
| `title-index` | Search finds a session by its title only | `search::index` | Antigravity, until its text reader lands |

## 7. The matrix today

### 7.1 Providers

**N** Native, **F** Fallback (flow), **NA** NotApplicable (reason), **GAP** (with what stands in). "Today"
means the merged code on `merge/settings-batch`. The setup and version rows are Gap for every tool
until decision 0019 and the version-sync design land (call 1); each cell says what that design plans.
Every N and F cell starts in `unwired.json` in phase 0 (call 2) and leaves it in the phase that routes
it.

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `session.list` | N: `jobs/`, `sessions/` in every lane's config folder | N: rollouts. **Bug:** child rollouts listed as sessions under the parent's id | N: summaries copy, `nesting_depth = 0` |
| `session.status` | N: job state, transcript, screen | N: rollout tail (`turn_open`), Notchling's runs | N: its `agy acp`, else the holder's log (`elsewhere`) |
| `session.new` | N: a `claude` job | N: first `codex exec` (draft row) | N: ACP `session/new` (draft row) |
| `session.stop` | N: `claude stop` | NA: nothing runs between turns | N: closes its `agy acp` |
| `session.remove` | N: `claude rm` | N: `codex delete` | GAP: no delete known; `hide-row` meanwhile |
| `session.move` | N: lanes, Move, Move All | GAP: a lane needs its own `CODEX_HOME` | NA: one Antigravity login per Mac; a switch moves every session |
| `folder.trust` | N: Trust Folder | NA: `codex exec` asks no trust question; the sandbox is set per turn | N: `trustedWorkspaces` (`agy_trust`); only delegate calls it so far (a surface gap) |
| `conversation.read` | N | N (a child's conversation is `subagent.open`) | N: ACP `session/load`, else the log |
| `conversation.follow` | N: file watch | N: file watch, `--json` for its own runs | N: ACP updates, else the log |
| `conversation.context` | N, window from the catalog | N: `model_context_window` | N: tokens; the window is `catalog.models`' |
| `subagent.list` | N: `subagents/agent-*.jsonl` and `.meta.json` | GAP: child rollouts by `parent_thread_id` | GAP: summaries rows by `parent_conversation_id` |
| `subagent.status` | N: `<task-notification>`, `finished`, "handed back", 10 min quiet | GAP: child `task_complete`/`turn_aborted`, parent `SubAgentActivity` | GAP: summaries `status`/`killed`, `send_message`, parent `[Message]` |
| `subagent.glance` | N: `glance::peek` | N for delegated rows (`codex::peek`); GAP for native children (same reader) | GAP: `peek` answers none; the child log has tokens and steps |
| `subagent.open` | N | GAP: rollout reader from the child's own turn | GAP: `elsewhere::from_lines` over the child's log |
| `subagent.stop` | NA: no outside stop for one subagent; Stop interrupts the turn | NA: only the model's `interrupt_agent`; `exec` has no outside call | NA: the Agent Manager's kill is inside agy (recheck over ACP) |
| `signal.turn_finished` | N: job Working to Completed | N: `task_complete`, `turn_aborted` | N: ACP `stopReason`; log `PLANNER_RESPONSE` `DONE` |
| `signal.subagent_finished` | N: `<task-notification>` | GAP: child `task_complete`, parent `SubAgentActivity` `completed` | GAP: child idle after `send_message`, parent `[Message]` |
| `signal.needs_you` | N: hook, transcript, screen | NA: `codex exec` never asks | N: ACP `request_permission` |
| `turn.send` | N: typed through `claude attach` | N: `codex exec resume` | N: `session/prompt` |
| `turn.hold` | F `held-queue` | F `held-queue` (today `CodexRuns`' own outbox, one turn per message; joins in phase 4, call 7) | GAP: box disabled while busy; `held-queue` in phase 4, but a session another `agy` holds stays disabled (call 7) |
| `turn.take_back` | F `held-queue` | GAP: `held-queue` | GAP: `held-queue` |
| `turn.interrupt` | N: Esc | N: SIGINT | N: `session/cancel` |
| `turn.options` | N: `/model`, `/effort` | N: model and effort per turn | N: `session/set_config_option` |
| `prompt.question` | N: AskUserQuestion hook | NA: `codex exec` can't ask | NA: ACP carries permission requests only (recheck) |
| `prompt.permission` | N: PermissionRequest hook | NA: turns run in a sandbox and never ask | N: `request_permission` |
| `command.slash` | N: `slash.rs` | GAP: `tools/codex/commands.rs` exists, Chat shows none | GAP: `available_commands_update` not shown |
| `account.switch` | N | N | N |
| `account.lanes` | N: `CLAUDE_CONFIG_DIR` | GAP: `CODEX_HOME` per account | NA: one login per Mac |
| `usage.limits` | N | N | N: the local language server |
| `usage.cost` | N | N; check the forked block isn't counted twice | N |
| `catalog.models` | N: Anthropic's list, data file | N: `models_cache.json` (the app still reads it directly) | N: `configOptions`, cached; GAP: no context windows (`catalog-data` once the data file has them) |
| `delegate.target` | GAP: no `notchling delegate claude` yet; decided (14.3), phase 3 | N: `codex exec` | N: `agy acp` |
| `delegate.caller` | N: `CLAUDE_CODE_SESSION_ID` | GAP: `CODEX_THREAD_ID` (phase 3) | GAP: `NOTCHLING_SESSION` on the runner's `agy acp`, `process-tree` for a terminal run (phase 3) |
| `setup.rules` | GAP (design: 0019): `~/.claude/rules/` | GAP (design: 0019): `~/.codex/AGENTS.md` | GAP (design: 0019): a plugin's `rules/` |
| `setup.skills` | GAP (design: 0019): built today in `install::skills` | GAP (design: 0019): built today in `install::skills` | GAP (design: 0019): built today in `install::skills` |
| `setup.hooks` | GAP (design: 0019): five hooks, built today in `install::hooks` | GAP (design: 0019): 0019 plans NA, no hooks for Codex | GAP (design: 0019): 0019 plans NA, the same |
| `version.detect` | GAP (design: version-sync): `versions/<v>` link | GAP (design: version-sync): `codex-package.json` | GAP (design: version-sync): `version-probe` |
| `version.update` | GAP (design: version-sync): `claude update`, pins | GAP (design: version-sync): installer `--release`, npm | GAP (design: version-sync): `version-cache`, agy installs latest only |
| `search.text` | N | N (children are indexed as sessions today: the `session.list` bug) | GAP: `title-index` meanwhile |

Gaps today: Claude Code 6, Codex 15, Antigravity 17 (38; 15 of them are the setup and version rows
waiting on their designs). One bug: Codex's `session.list` makes a session of every child rollout,
under the parent's id, and the same rows reach Search and the notch.

### 7.2 Surfaces

Where a surface still misses a capability for some tool, or branches on the tool instead of the registry.

| Surface | Today | After |
| --- | --- | --- |
| Chat: Sidebar, conversation | Every tool; rows built per tool in the app | `session.list`, `conversation.*` through the registry |
| Chat: Details Panel subagents | Claude Code sessions only | Every session: native and delegated rows, as a tree (call 3) |
| Chat: composer | `ChatTool` per tool (0015) | `ChatTool` presents the registry's answers (0016) |
| Notch | Rows and a peek when a turn ends, every tool | Plus `subagent.finished` and `delegate.finished` (14.1) |
| Phone: list, chat | Every tool (`foreign_rows`) | The daemon's own list of every tool's sessions (call 5) |
| Phone: subagents | None for any tool (never filled) | Every tool, with `tool` and `via` |
| Phone: alerts | Turn finished and needs you, from the phone's own diff | Pushed by the Hub from `signals.recent`, subagents included (call 5); the phone's own diff stays for an older Mac |
| Search | Claude Code, Codex text; Antigravity titles | Every tool's `search.text`; children under their parent |
| Inbox | No session items | `delegate.finished` for background jobs (14.1) |
| System banners | None for a finish | Tell Me When Work Finishes: Off, Long Turns and Jobs (the default), Everything (14.1) |
| Keep awake | Working sessions | Plus running subagents and jobs (a background job with an idle parent let the Mac sleep) |
| `notchling sessions` | Claude Code only | Every tool, from phase 2 (call 5) |
| Activity log (logging design) | `tool.turn` for Codex and Antigravity, `session.*` kinds for Claude Code | One `turn.finished` and one `subagent.finished` kind, `tool` a field |

## 8. Subagents in one shape

### 8.1 The row

```rust
pub struct Subagent {
    pub id: String,              // "claude:<agent id>", "codex:<thread>", "antigravity:<conversation>", "job:<delegation id>"
    pub session: String,         // the top session's row id
    pub parent: String,          // the direct parent: the session, a child or a job (call 3)
    pub origin: Origin,
    pub depth: u8,               // 1 for a child of the session, 2 for a child of that, ...
    pub description: String,
    pub agent_type: String,      // Claude's agentType, Codex's role or nickname, agy's agent_name, the job's tool
    pub model: Option<String>,
    pub effort: Option<String>,
    pub started: Timestamp,
    pub updated: Timestamp,
    pub status: Status,          // Running, Completed, Failed, Stopped, Quiet (unchanged)
    pub path: String,            // what `open` and `glance` read; empty when nothing can be read
    pub background: bool,        // Claude only, kept
    pub tool_use_id: Option<String>,
}

pub struct Origin { pub tool: Tool, pub via: Via }
pub enum Via { Native, Delegated }
```

Today's `Origin` (Claude, Codex, Antigravity) mixes which tool runs the child with how it started, so
`subagent_rows.rs` calls every non-Claude row "handed over". With Codex's own children that would be
wrong. `via == Delegated` takes that place, and `origin.tool` picks the mark.

### 8.2 The four sources

| Source | Finds | Status | Glance | Open |
| --- | --- | --- | --- | --- |
| Claude Code | `<transcript>/subagents/agent-*.jsonl` + meta (`subagents::list`, the `/clear` predecessors as today) | as today in Chat; in the daemon, the finished-agents reader (call 6) | `conversation::peek_subagent` | `read_subagent_shared` |
| Codex | The daemon's child index (call 4): rollouts whose **first** `session_meta` has `parent_thread_id` = the parent's thread. The app asks `codex.children`; with no daemon it reads the parent's start day folder and the next | last of `task_started` / `task_complete` / `turn_aborted` after the forked block: Running, Completed (`error` null), Failed (`error` set), Stopped (`turn_aborted`); no write for 10 min while Running is Quiet | `codex::peek` (token count, newest call), after the forked block | the Codex reader from the child's own `task_started` |
| Antigravity | Notchling's summaries copy: `WHERE parent_conversation_id = ?` (`agent_name`, `status`, `killed`, `nesting_depth`, `last_modified_time`); a child's own children by the same query on its id (call 3) | `killed` is Stopped; `RUNNING` is Running; `IDLE` after a `send_message` in its log, or a parent `[Message]` from it, is Completed; `IDLE` with neither is Quiet; an `ERROR_MESSAGE` last step is Failed | the child's `logs/transcript.jsonl` tail: last `PLANNER_RESPONSE` tokens, newest tool call | `elsewhere::from_lines` over that log |
| `delegate-jobs` | Records whose parent is this row: the session (and its `/clear` or continued predecessors), a native child, or a job | `record.state` as today | by the job's tool, through that tool's glance | by the job's tool |

**The Codex child index** (call 4). The daemon's own SQLite, `<data>/daemon/codex-children.db`, one
table: `rollouts(path PRIMARY KEY, size, mtime, thread, parent, depth, nickname, role, started)`, with
an index on `parent`. It is filled from `platform::watch` batches under `~/.codex/sessions`: a rollout
whose size or mtime changed has its head read again (the first `session_meta` line, at most 64 KB), and
an unchanged one is never read. At its first start the daemon backfills the day folders of the last 30
days, newest first, in the background; a session older than that shows no native children until its
rollout changes. A day folder read again sweeps the rows of files that are gone. It is a cache: deleting
it costs one backfill. Top-level rollouts are rows too (`parent` empty), so the daemon's session list
hides children with a lookup. There is never a scan per parent.

Description: Claude's meta `description`; Codex's `agent_nickname` and role, else the first line of the
child's first task; Antigravity's role from the parent's `invoke_subagent` call when the loader has the
parent's log, else `agent_name`; a job's prompt line, as today.

### 8.3 One merge, used everywhere

`Registry::subagents` (6.4) replaces `ChatData::subagents` and the app's `delegated::subagents`. Chat's
loader calls it off the main thread and hands rows to the Details Panel as now (`Loaded::subagent_rows`,
`SubagentRows` keyed by size and mtime: the draw path reads nothing). The daemon calls it for
`SignalWatch` and to fill the phone's `Session.subagents`.

The rows are a tree in the top session's Details Panel (call 3). Depth-1 rows are paged as today
(`Page`: every running one, else the 3 newest, then Load N More). A row with children of its own shows
their count and starts collapsed; opening it shows them under it, indented, paged the same way. A
collapsed row's children are listed (that is cheap) but never glanced or drawn, so a row not shown is
still not read. A child of a child is a Codex or Antigravity subagent's own subagent, a job a subagent
delegated, or a job's job. The phone gets `parent` and `depth` and indents as it can.

### 8.4 Children leave the session lists

A Codex child rollout and an Antigravity child conversation are not sessions in the Sidebar, the notch
or `notchling sessions`; they show under their parent. Search still finds them, as a subagent hit that
opens the parent with the row selected, as delegated rows work today. The daemon's list hides them by
the index's `parent` (Codex) and `nesting_depth` (Antigravity, as now); the app's own Codex rows use the
first `session_meta` it already reads.

## 9. Signals: one finished event

### 9.1 The events

Topic `signals` (new), app tier. One payload for all three; `delegate.finished` is the subagent payload
of a job, sent also when the job has no parent session.

```json
{"event": "subagent.finished", "data": {"seq": 812,
  "session": "codex:...", "parent": "codex:...", "subagent": "codex:...", "depth": 1,
  "tool": "codex", "via": "native", "description": "Check the parser", "status": "completed",
  "started": 1763100000.1, "ended": 1763100042.7, "tokens": 18230}}

{"event": "delegate.finished", "data": {"seq": 813, "job": "antigravity-4121-1763100000123",
  "session": "claude:...", "parent": "claude:...", "tool": "antigravity", "via": "delegated",
  "status": "failed", "...": "..."}}

{"event": "turn.finished", "data": {"seq": 814, "session": "antigravity:...", "tool": "antigravity",
  "outcome": "done", "started": 1763100000.0, "ended": 1763100075.2}}
```

`outcome` is `done`, `failed`, `stopped` or `needs_you`. `seq` grows for the daemon's run. A new op,
`signals.recent {after?}` (app tier), returns the events after a `seq` from a ring of the last 500, so a
client that connects later sees what it missed, and the phone Hub reads it with a cursor per connection
(call 5).

### 9.2 `SignalWatch` in the daemon

- **Pure part** in the engine (`contract::signals`): `step(before: &Seen, now: &Snapshot) -> Vec<Signal>`.
  A snapshot is the sessions' status, the subagents' status for the sessions it looks at, and the
  delegation records.
- **What it looks at**: every session the daemon lists (call 5): Claude Code's from the Monitor, Codex's
  and Antigravity's from `bridge::foreign`'s rows, read-only, whatever the bridge switch says and with
  or without an app attached. The subagents of a session only while the session works or has had a
  running subagent in the last 10 minutes, plus every delegation record not ended. A session idle for
  an hour costs nothing.
- **Where statuses come from**: Claude Code's subagents from the daemon's finished-agents reader
  (call 6), which reads only what a parent transcript gained since the last read (size and mtime; the
  first read is the last 1 MB) and keeps each `<task-notification>`'s id and status; Codex's from the
  child index and each child's tail; Antigravity's from the summaries copy; jobs from their records.
- **An Antigravity session run in a terminal** (no `agy acp` of Notchling's): `turn.finished` when its
  summaries row's `status` goes from `RUNNING` to `IDLE`. Where a row never shows `RUNNING` (the short
  runs in 4.2 never did), the quiet rule: its log's last step is a `PLANNER_RESPONSE` with status
  `DONE` and the log has not grown for 30 s (the builder tunes the window).
- **When**: on `platform::watch` batches under the folders the Monitor already watches, the Codex day
  folders and the Antigravity summaries db's folder and the delegations folder, and every 5 s, as the
  Monitor. All reads are the heads and tails the readers cache.
- **Rules**: the first snapshot after a start sets the baseline and fires nothing. A finish fires once
  per (session, subagent) or (session, turn start) per daemon run. Quiet is not finished. A subagent
  first seen already finished fires only if it started after the baseline. A turn needs to have been
  seen Working.
- Held messages already wait for the turn's end; `held_ops` takes `turn.finished` as its trigger
  instead of polling rows, with the 20 s grace kept.

### 9.3 Who listens

| Surface | Does |
| --- | --- |
| Details Panel | On a `subagent.finished` for the selected session, asks the loader for a load; the row's status comes from that load, never from the event |
| Notch | Peeks the parent, after the existing order: a question, then needs you, then a turn finished, then a subagent or job finished. A subagent of a session that still works doesn't peek (14.1) |
| Inbox | A `delegate.finished` of a `--background` job raises a done item ("Codex job finished · Check the parser"), Open goes to the parent's row. New `Kind::JobFinished` (additive), no responder, expires by the clock |
| Phone | The Hub reads `signals.recent` with a cursor per connection and pushes each finish as a new, additive message, so finish events have one source (call 5); a phone keeps its own diff for an older Mac. `Session.subagents` is filled from the daemon's registry. Schema first (`add-protocol-field`): optional `tool`, `via`, `outcome`, `parent` on `Subagent`. `SubagentState` keeps `running` and `done`: a new value would break older phones |
| Keep awake | Counts running subagents and jobs as work |
| Activity log | `turn.finished` and `subagent.finished` kinds (Toggle, on; Activity and Diagnostics) with `session`, `tool`, `via`, `status`, `duration_ms`, `tokens`, no text; the logging design's `tool.turn` becomes `turn.finished` |
| System banner | The setting **Tell Me When Work Finishes** (14.1; key `notifyFinished` in `settings/keys.rs`, allowed `off`, `long`, `all`, default `long`), in Settings > General: Off; Long Turns and Jobs, a `turn.finished` that ran over 60 s and a `--background` job's `delegate.finished`; Everything, every finish. Shown only when Chat isn't in front, and never for a subagent of a session that still works, in any mode. Through the app's one notification worker (`system::Detached`), once per event `seq`. Off stops banners only: the notch, the Inbox item and the phone stay |

## 10. Delegate: target, caller, wait

### 10.1 Targets

Codex (`codex exec`) and Antigravity (`agy acp`), as now, and Claude Code from phase 3 (the user's
decision, 14.3): `notchling delegate claude [--model <m>] [--effort <e>] ...` runs `claude -p` with
`--output-format stream-json` on the main login, never a lane's (the job's environment drops
`CLAUDE_CONFIG_DIR` with the ids 10.2 removes). The default is `sonnet` at `medium` effort; both are
selectable, and `delegate-models` lists Claude Code's models and efforts from the catalog. Without
`--write` the job gets read-only tools, with it edits in `--folder`; the flags come from the installed
`claude --help` when it is built. `notchling hook` already passes on a `claude -p` session, so a job
never asks the Mac. Each job is a turn on the user's Claude account and counts in its limits; the skill
says so (10.4). Until phase 3 the matrix cell stays a Gap.

### 10.2 Callers: who is the parent

The innermost id wins (call 3), and the record says which one it was (`parentVia`):

1. `NOTCHLING_SESSION`: set by `notchling delegate` on its job's process to the job's own id
   (`job:<id>`), and by the agy runner on each `agy acp` to that session's id (the runner keeps one
   `agy acp` per open session, `Runner`'s `live` map). Notchling sets it only where the tool exports no
   id of its own: on a Claude Code or Codex process it would hide that tool's inner id (a Codex child's
   own thread, a Claude Code session's new id after `/clear`).
2. `CODEX_THREAD_ID`: a Codex session or one of its children. Codex sets it in every command its model
   runs.
3. `CLAUDE_CODE_SESSION_ID`: a Claude Code session (today's only way).
4. `process-tree`: walk the caller's ancestors to the first `claude`, `codex` or `agy` process and map it
   to a session: Claude Code's `sessions/<pid>.json`, the Codex rollout that process holds open, the
   Antigravity brain folder it holds (the `lsof` reading `elsewhere` already does). This is how an
   Antigravity session the user runs in a terminal becomes a parent.
5. None: the job is still recorded, has no parent, shows in `notchling delegate --jobs` and the
   Activity log, and can be waited on.

`notchling delegate` removes inherited `CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID` and
`NOTCHLING_SESSION` from its job's environment and sets `NOTCHLING_SESSION=job:<id>`, so a job that
delegates again names its own job, never the session above it. One case stays one level off: a
Codex child inside a delegated Codex job sees the job's `NOTCHLING_SESSION` before its own
`CODEX_THREAD_ID`, so its jobs show under the job; the environment can't say which was set last.

A record is now written for every job, not only under Claude Code. Version 2 adds `job`, `parentTool`,
`parentVia`, `background` and `result` (the output file's name), and writes the tool as `antigravity`
(file `antigravity-<session>.json`); `agy` is still read (call 8). Version 1 records still read, as
Claude Code parents.

### 10.3 Background and wait

| Command | Does |
| --- | --- |
| `notchling delegate <tool> --background ...` | Starts the same job detached (`setsid`, output to `<data>/delegations/<job>.out`, 0600), prints `{"job": "...", "wait": "notchling delegate --wait <job>"}` and exits 0 |
| `notchling delegate --wait <job> [--timeout <s>] [--json]` | Waits on the delegations folder (`platform::watch`) and the job's pid, then prints what a foreground run prints. Exit 0 done, 1 failed, 2 stopped, 3 unknown job, 124 timeout |
| `notchling delegate --status <job> [--json]` | The record and, when ended, the outcome |
| `notchling delegate --jobs [--all] [--json]` | The caller's session's jobs (by the chain above), or all |
| `notchling delegate --stop <job>` | SIGINT, then SIGTERM, then SIGKILL, only if the pid still runs `notchling delegate` with that job (pids are reused): `subagent.stop` for jobs |

Like `notchling delegate` today, these need no daemon and no `acting` switch, and the depth limit
(`NOTCHLING_DELEGATE_DEPTH`, 2) stays. Results and records are the user's own files, 0600, kept 30 days
as records are. The `notchling-delegate` skill says when to use them (10.4).

### 10.4 The delegate skill

The guidance for delegating lives in Notchling's `notchling-delegate` skill, not only in code (the
user's decision, 14.3). Its text is `delegate::skill::TEMPLATE` with a note per tool (`SkillTool`), and
`install::skills` writes a copy for each tool. Phase 3, the phase that adds the Claude target, rewrites
it to say:

- **Which tool for what**: Claude for review and the hardest work; Codex or Antigravity for images, and
  for a second opinion from a different model (a session never asks its own tool for one); any of them
  for work that can run beside the caller's.
- **Model and effort**: `delegate-models` lists them; a cheap model at low effort for lookups and
  mechanical work, a stronger one at high effort for review, the strongest at its highest effort only for
  the hardest work; the Claude target's default is `sonnet` at `medium`.
- **Cost**: each Claude delegation costs a turn on the user's Claude account and counts in its limits;
  Codex and Antigravity jobs spend those accounts the same way.
- **Waiting**: a short job in the foreground; a long one with `--background`, keep working, then
  `--wait <job>`; `--status <job>` to look without waiting, `--jobs` to list them.
- **Results**: a job's end is a `delegate.finished` signal. It shows under the caller's session in the
  Details Panel, peeks the notch, raises an Inbox item for a background job, and `--wait` prints the
  result.

The user asked for the Claude Code and Codex copies, and an Antigravity one when 0019's per-tool
instructions land. On the base, `install_ops.rs` already writes an Antigravity copy from the same
template, so it gets the new text in phase 3 too. The text must stay in sync with decision 0019's
instructions per tool: when 0019 lands, this skill is one of its per-tool data files and every copy is
written from there, so the skill and the instructions never say different things. The two sentences
today's tests pin ("Nested delegation stops at two levels." and "It sees the folder, not your
context.") stay, so the change adds tests for the new lines and edits none.

## 11. How "every capability" is enforced

### 11.1 It does not build without a declaration

Each provider's `support.rs` is one `match capability { ... }` with an arm per capability, under
`#![deny(clippy::wildcard_enum_match_arm)]`. A new capability is a compile error in every provider
until each answers it; a new `Tool` has no `Provider` until every arm is written. The engine builds on
Linux, so CI's clippy sees it.

### 11.2 Registry tests

`contract/registry_tests.rs`, for every tool and capability: `why` and `plan` are not empty and fit a
tooltip (120 characters); a `Fallback` flow covers that capability (`Flow::covers`); every Gap is in
`contract/gaps.json` and every line there is still a Gap (the file shrinks with the work); a new tool's
gaps are empty (14.2). The same for `contract/unwired.json` (call 2): every line names a Native
or Fallback cell, and a new tool starts with none. A Gap whose reason is "design: 0019" or "design:
version-sync" is the only kind a test lets name a design rather than a phase (call 1).

### 11.3 One conformance suite

`crates/engine/tests/conformance/`: the same tests for every provider, one test per (tool, capability),
generated by a macro:

```rust
conformance!(claude, Tool::Claude, "claude");
conformance!(codex, Tool::Codex, "codex");
conformance!(antigravity, Tool::Antigravity, "antigravity");
```

Each provider has a fixture root, `crates/engine/tests/fixtures/conformance/<tool>/`, with the same
cast: an idle session, a working session, a session with one running and one finished subagent, and a
`before/` and `after/` pair for each signal. The fixtures are made up (no real transcript); Codex's
child rollout carries the forked block. What each test checks follows the declaration:

| Declared | The test |
| --- | --- |
| Native | The port answers over the fixture, with the shared expectations: `subagent.list` gives two rows, statuses Running and Completed, `origin.tool` this tool, `via` Native, ids unique, `parent` set; `subagent.glance` gives tokens and a doing line for the running one; `signal.subagent_finished` fires once from `before` to `after` and not again on a second step; and so on per capability |
| Fallback | The port answers `Unsupported`; the registry's answer is the flow's, checked with the flow's fakes |
| NotApplicable | The port answers `Unsupported` with the declared reason; the registry returns it |
| Gap | As NotApplicable, plus the flow that stands in, if any; the test output counts it |

Only wired cells run their Native or Fallback test (call 2). A Native or Fallback cell in
`unwired.json` is skipped and counted in the test output, so phase 0 can declare today's truth before
the ports route it. The suite fails when a Native or Fallback cell is neither wired (the registry
routes it and the conformance test exists) nor in `unwired.json`, and when a cell in `unwired.json`
already passes its test, so the file is cut in the pull request that wires it.

Acting ports run over fakes only: `FakeLauncher`, `FakeSpawner` (Codex, ACP), the pty fakes,
`MemoryKeychain`, `ScriptedHttp`. No test starts a real tool.

### 11.4 The matrix is generated, and gaps only shrink

`docs/provider-matrix.md` is written by the engine's `provider_matrix` test (`NOTCHLING_UPDATE_GOLDEN=1`
rewrites it, as other goldens) and compared exactly, so the doc can't drift from the code.
`notchling providers [--json]` prints the same table, and Settings > Agents shows each tool's
Fallback, NotApplicable and Gap rows under a **What Notchling Can Do** part, so nothing is missing
without the user being able to see it. A guard rule, `GUARD-PROVIDER-GAPS`, sends a pull request whose
`gaps.json` or `unwired.json` grows to the maintainer, as `GUARD-BASELINE` does for the lint baseline.
The setup and version rows leave `gaps.json` in the pull requests that land decision 0019 and the
version-sync design (call 1).

### 11.5 Surfaces don't branch

0016's `GUARD-TOOL-BRANCH` (no new `Tool::` match outside the contract's providers, the registry, the
account providers and the app's row-id parsing; new or changed lines only) is the other half: a surface
that can't name a tool can't leave one out.

### 11.6 The add-provider skill

`.claude/skills/add-provider/SKILL.md` still describes the Swift app. Rewritten for the contract:

1. Add the `Tool` value and its id prefix; the build now fails in every `support.rs` and in the
   conformance macro.
2. Write `contract/<tool>/support.rs`: every capability, Native, Fallback or NotApplicable, with its
   line. No Gap for a new tool.
3. Implement the ports for each Native capability over the tool's own files and APIs, effects behind
   the existing seams.
4. Make `tests/fixtures/conformance/<tool>/` with the shared cast, and add the macro line.
5. Accounts and limits: an `AccountProvider` and a `ProviderSource` (the login rules in
   `crates/engine/AGENTS.md`); catalog: a `CatalogSource` and its rows in `catalog.json`.
6. Setup: an `instructions::Target` (0019) and the tool's lines in the delegate skill (10.4);
   versions: a reader and an installer (version-sync).
7. Run the checks, regenerate `docs/provider-matrix.md`, add the tool's facts to the engine's
   AGENTS.md "Things that bit before" and its row to `docs/feature-map.md`.

## 12. Phases

Each phase is its own pull request into the integration branch, with the app working after it.

| Phase | What | Tests | Zone |
| --- | --- | --- | --- |
| 0. Contract and matrix | `contract/` types, the three `support.rs` with today's truth, the registry (routing only what exists), `gaps.json` (38, the setup and version rows as "design: …" gaps, call 1), `unwired.json` (every Native and Fallback cell not routed yet, call 2), the golden `docs/provider-matrix.md`, `notchling providers`, `catalog::Tool` moved, the add-provider skill rewritten, decision record. Declares the truth only: no behavior change | registry tests, golden; the conformance suite runs, with every unwired cell skipped and counted | engine and `.claude/`: maintainer |
| 1. Subagents in one shape | `Origin {tool, via}`, `session`, `parent`, `depth`; `Registry::subagents` recursing (call 3) replaces `ChatData::subagents` and `delegated::subagents`; the Details Panel's tree, collapsed past depth 1; Codex: first `session_meta` decides, the daemon's child index and `codex.children` (call 4), child reader skipping the forked block (conversation, status, glance, Search, cost); Antigravity: children query on the copy, status mapping, glance and open from the child log; the wire name `antigravity`, read-both and the one-time key rewrite (call 8); Details Panel for Codex and Antigravity sessions | conformance for `subagent.*` and `session.list` over synthetic rollouts (with the forked block) and a summaries db built in the test; the index over a temp folder (backfill limit, a changed file read once, an unchanged one never, a gone one swept); the tree's paging; ids and records read under `agy` and `antigravity`; `bench_subagents` still reads no subagent file in a steady state | `transcript/`, `search/`, app: auto; the index and its op, `tools/codex`, `tools/agy`, `cost`, the key rewrite: maintainer (`CAP-FILEWRITE`) |
| 2. Signals | `contract::signals` (pure), `SignalWatch` and `signals.recent {after}` in the daemon; the daemon lists every tool's sessions through `bridge::foreign`, read-only, whatever the bridge switch and with no app (call 5); the daemon's finished-agents reader (call 6); a terminal-run Antigravity session's `turn.finished` from its summaries status, else the quiet rule; `held_ops` on `turn.finished`, the notch's peek, Inbox `JobFinished`, the Tell Me When Work Finishes setting and its banners (14.1); the phone Hub pushes from `signals.recent` and fills `subagents` and the schema fields; keep awake, the log kinds | pure step tests (baseline, once only, quiet is not finished, first seen finished, the quiet rule's window, the banner rule per mode: 60 s, Chat in front, a working parent's subagent never); the finished-agents reader over a growing fixture (reads only what was added); socket tests over a fixture root with the bridge off and no app; the Hub's cursor; a Flutter test for the alert | daemon, bridge, protocol, the setting's key: maintainer; app and phone features: auto |
| 3. Delegate everywhere | Record v2 (`"tool": "antigravity"`), the caller chain, innermost first (`NOTCHLING_SESSION`, `CODEX_THREAD_ID`, `CLAUDE_CODE_SESSION_ID`, `process-tree`), the scrub of inherited ids and `NOTCHLING_SESSION=job:<id>` on each job, the agy runner's `NOTCHLING_SESSION` (call 3), `--background`, `--wait`, `--status`, `--jobs`, `--stop`; the Claude target, `notchling delegate claude` on the main login, `sonnet` at `medium` by default (14.3); the delegate skill's new text in every copy (10.4) | option and record tests; the chain over `FakeProcs` with every combination of the three variables; a job's environment holds only its own id; e2e over a fixture root with fake tools in `<root>/.local/bin`: a job that ends, fails, times out, loses its pid, delegates again; the Claude target over a fake `claude` (main login, default model and effort, read-only without `--write`); the skill's new lines, the pinned ones unchanged | engine `delegate`, `notchlingd`: maintainer (`CAP-PROCESS`, `CAP-FILEWRITE`) |
| 4. Held queue for every tool | One held message per session for every tool, everything sent meanwhile joined into it (call 7): Codex's outbox becomes `held-queue` (one turn per message becomes one joined turn), Antigravity's box no longer refuses while busy, take back for all, `session.reply` routed by id. An Antigravity session another `agy` holds keeps sending off, with its reason, and the held queue takes nothing for it | `held_tests` per tool's rows; `codex_acting_tests`, `agy_ops_tests` with `FakeSpawner`. Codex's outbox tests change from one turn per message to a joined turn: flagged for the maintainer (`WEAK-TEST-EDIT`) | sending: maintainer |
| 5. 0016's other ports | Sessions (the rest of the port; its listing slice came in phase 2, call 5), Turns, Prompts, Commands as ports (0016 phases 2 to 4); slash commands for Codex and Antigravity in Chat; Antigravity's Remove as `hide-row`; Antigravity's `search.text` from its logs; Gemini windows in `catalog.json`; Codex `CODEX_HOME` lanes if wanted | conformance for each | mixed; one PR per item |
| 6. Clean-up | `codex.*`/`agy.*` alias ops removed, `GUARD-TOOL-BRANCH` and `GUARD-PROVIDER-GAPS` on, the other tool enums gone, folder notes | guard rules with bad examples | `tools/guard`, docs: maintainer |

## 13. Left to the builder, and to check while building

**Left to the builder:** module and type names; the fixture layout; whether `SignalWatch` shares the
Monitor's watcher; cache sizes; the clippy lint set that denies wildcards; how the registry is passed
to the daemon's ops and the app's loader; the order of work inside a phase; whether `Support` lines
live in code or in a data file next to `catalog.json`; the Codex child index's columns and how fast its
backfill reads (call 4); the quiet rule's window (call 5, 30 s to start); how the finished-agents
reader bounds its first read (call 6, 1 MB to start); how the Details Panel draws a nested row (call 3,
within the kit's tokens and today's row sizes).

**To check while building:**

- Codex: whether a child's forked block can carry the parent's `token_count` lines (cost double count);
  v1 shapes (`<subagent_notification>`, `Collab*` events) on a Mac where v1 is still in use; how a
  failed child looks on disk; why v2 is on here although its flag is listed off.
- Antigravity: whether a child's summaries row ever says `RUNNING`, and whether a top session's does
  while a terminal run works (call 5 picks the quiet rule where it doesn't); `manage_subagents`' other actions;
  one ACP run to see whether children show in `session/update`; whether the summaries db's `-readonly`
  open failure after a run (the copy route is unaffected) matters anywhere else.
- Claude Code: nothing new; the `SubagentStop` hook stays unused.

## 14. Decided by the user

Open questions: none. The user answered the three the first draft asked (2026-11-14); each is now the
user's decision, worked into the sections it touches.

1. **Finished notifications** (user's decision). A Settings choice, **Tell Me When Work Finishes**: Off,
   Long Turns and Jobs, Everything; the default is Long Turns and Jobs: a banner for a turn that ran over
   60 s and for a background job, only when Chat isn't in front. A subagent of a session that still
   works never notifies (its parent reads the result itself; six Codex workers would be six banners).
   See 9.3.
2. **Gaps only shrink** (user's decision). `gaps.json` lists today's gaps and may only shrink; a new
   provider starts with zero gaps. The user's answer counted the first draft's 24. Call 1 made the setup
   and version rows Gap for every tool, which adds 14 (Antigravity's `version.detect` was one of the
   24), so the file starts with 38; the 15 "design: …" cells leave it when decision 0019 and the
   version-sync design land. See 11.2 and 11.4.
3. **Claude Code is a delegate target** (user's decision). `notchling delegate claude` runs `claude -p`
   on the main login, `sonnet` at `medium` effort by default, model and effort selectable. The guidance
   for delegating goes into Notchling's delegate skill, not only into code: which tool for what, how to
   pick model and effort, that each Claude delegation costs a turn on the user's Claude account, how to
   wait (`--wait`, `--background`, `--status`), and that results come back as a finished signal. The
   target and the skill text come in phase 3, and the text stays in sync with decision 0019's per-tool
   instructions. See 10.1 and 10.4.

## 15. Sources

Codex (openai/codex `main`, newer than the installed 0.166.0; latest release 0.169.0 when read):

- Subagents docs: https://developers.openai.com/codex/subagents
- v2 tools: https://github.com/openai/codex/tree/main/codex-rs/core/src/tools/handlers/multi_agents_v2
- v1 tools: https://github.com/openai/codex/tree/main/codex-rs/core/src/tools/handlers/multi_agents
- `SessionSource`, `SubAgentSource`, `AgentStatus`: https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- Status from events: https://github.com/openai/codex/blob/main/codex-rs/core/src/agent/status.rs
- The parent hears a finish: https://github.com/openai/codex/blob/main/codex-rs/core/src/agent/control/completion.rs
- v1 notification: https://github.com/openai/codex/blob/main/codex-rs/core/src/context/subagent_notification.rs
- Feature flags: https://github.com/openai/codex/blob/main/codex-rs/features/src/lib.rs
- Limits: https://github.com/openai/codex/blob/main/codex-rs/core/src/config/mod.rs
- State db: https://github.com/openai/codex/tree/main/codex-rs/state
- app-server items: https://github.com/openai/codex/blob/main/codex-rs/app-server-protocol/src/protocol/v2/item.rs
- `CODEX_THREAD_ID`: https://github.com/openai/codex/blob/main/codex-rs/protocol/src/shell_environment.rs
- Slash commands: https://github.com/openai/codex/blob/main/codex-rs/tui/src/slash_command.rs

Antigravity (closed source):

- Subagents: https://antigravity.google/docs/subagents.md
- `/agents`: https://www.antigravity.google/docs/cli/commands/agents
- `agy changelog` (local, 1.5.8): `subagent_info` in the structured stream, `root_conversation_id` in MCP
  `_meta`, a `model` per subagent in the status line
- Notchling's own reading of agy's subagent files: `crates/engine/src/delegate/progress.rs`

Verified how: Codex, one real `codex exec --json` run (two rollouts, the parent link, the forked block,
`SubAgentActivity`, the inter-agent message), existing local rollouts and the state db's schema, and
the binary for `CODEX_THREAD_ID`. Antigravity, two real `agy --print --output-format stream-json`
runs (the child conversation, its summaries row, `subagent_info`, `send_message` and the parent's
`[Message]` step; both runs ended at the print timeout after the child reported, the first after model
capacity errors), and existing summaries rows and schema. Docs only: Codex's limits and v1 shapes,
Antigravity's lifecycle names, `/agents` columns and the depth limit.
