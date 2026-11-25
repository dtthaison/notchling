# One provider interface per tool, in the engine

Date: 2026-11-11. Status: draft for the maintainer's approval. Changes decision 0015 (new record 0016).

## Why

Every model release meant a code change and a rebuild: Fable 5.2 came out and the Model Picker still
offered Fable 5.1, because the Claude Code model list is written in four places (`crates/app/src/chat/claude_picker.rs`,
`crates/notchlingd/src/control_ops.rs` `PICKER_MODELS`, the Swift `ModelPicker`, the phone's
`bridge/router.rs` `MODELS`), and prices, effort levels and context windows are tables in code too.
The maintainer's words: "it's not the good way to maintain the data information of provider", and "we
should support all the providers".

Decision 0015 already put Chat's parts behind one interface per tool (`ChatTool`, `crates/app/src/chat/view/tool/`).
That was the right pattern in the wrong layer:

- `ChatTool` takes Chat's own view (`&ChatView`, GPUI's `Context`), so the notch, the phone bridge,
  search, `notchling delegate` and the daemon can't use it.
- Below it the daemon has one API per tool: `session.*` for Claude Code, `codex.send|stop|delete`,
  `agy.sessions|state|conversation|open|send|cancel|set_option|permission`. Every client branches on
  the tool again: about 170 `Tool::` matches outside `chat/view/tool/` (sidebar.rs 44, data.rs 13,
  prompts.rs 12, closed.rs 12, search/wire.rs 10, ...).
- Data nobody keeps current lives in code: models, efforts, context windows, prices.

## Goals

1. One provider interface per tool **in the engine**, split into small parts, implemented for Claude
   Code, Codex and Antigravity. A new tool (Grok was named) is one implementation plus a registration.
2. Model data is never a code change: models, efforts and context windows come from the tool or the
   provider where they publish them; prices come from one data file.
3. One daemon API for every tool; clients (Chat, the notch, the phone bridge, search, delegate) ask
   by session id and never branch on the tool for behavior.
4. `ChatTool` becomes a thin presenter over the engine interface; the remaining `Tool::` branches in
   the app go away, held there by a guard check.

## Non-goals

- The Swift app (retired on 2026-11-11). Its lists stay as they are.
- Changing how a pick reaches a tool (`/model` and `/effort` typed into Claude Code, Codex's `model`
  and `effort` on `codex.send`, Antigravity's `session/set_config_option`).
- New features a tool lacks today (images for Codex, Stop from the phone). The interface says "can't"
  for them; adding them is separate work.
- Claude Desktop: accounts only, already behind `AccountProvider`; it runs no sessions here.

## Anti-goals (a failure even if it works)

- A big-bang rewrite. Each phase below ships alone, with the app working after it.
- A cost shown as $0 because a price is missing. Unknown is shown as unknown.
- A part missing for one tool. Every part answers for every tool, with "can't, because ..." where it can't.
- Renewing or refreshing any login to read a model list (refresh tokens rotate; AGENTS.md).
- Reading or writing the user's real `~/.claude`, `~/.codex`, `~/.gemini` in tests.

## The design

### Patterns

- **Strategy** behind a **registry**: one trait per concern, one implementation per tool, one place
  (`ToolRegistry`) that turns a `Tool` into behavior. Same as `AccountProvider`/`ProviderRegistry`.
- **Interface segregation**: small traits rather than one wide one, so a tool implements what applies
  and a client depends on only what it uses.
- **Adapter**: each implementation maps its tool's own API (Claude Code's jobs and `claude attach`,
  `codex exec --json`, `agy acp`) onto the shared types.
- **Layered lookup** for data: live source, then the last value cached on disk, then a built-in value.
- **Capabilities**: each tool declares what it can do; screens disable a part from that, with the reason.

### The engine interface

A new module, `crates/engine/src/agents/` (name is the builder's call; `providers` is taken by
accounts and `tools/` holds the runners). The concerns:

| Trait | Answers | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- | --- |
| `Catalog` | models (id, name, current or older, context window), efforts per model and the default, price | Anthropic's model list + data file | `models_cache.json` (has `context_window`, `supported_reasoning_levels`, `default_reasoning_level`) | `agy acp` `configOptions` (Model, Thinking Level, Mode), last seen cached |
| `Capabilities` | what the tool can do: queue while working, stop, images, slash commands, questions, permissions, move to another account, new session, delete | all but some | per today's behavior | per today's behavior |
| `Sessions` | list, new, close/delete, which account a session uses | jobs under every lane's config folder | rollouts | conversation summaries db copy |
| `Turns` | send, stop, take back a queued message, working or not | `ReplySender` path | `codex exec` runs | `session/prompt`, `session/cancel` |
| `Prompts` | the question or permission waiting, answer it | hook, transcript, screen read | none (exec can't ask) | `session/request_permission` |
| `Commands` | slash commands and how each runs (as a message, in a terminal, refused) | `slash.rs` | `tools/codex/commands.rs` | `available_commands_update` |
| Accounts, Limits | already `AccountProvider` and `ProviderSource`; the registry links each tool to its provider id | | | |

Conversations already share one shape (`transcript::model`); each tool's reader stays as it is and the
`Sessions` implementation calls it.

Every trait method is blocking and pure where it can be, with the I/O behind the existing seams
(`HttpClient`, `ToolSpawner`, `Spawner`, file roots passed in), so every implementation is tested
against fixtures.

### Model data

`ModelInfo { id, name, tier: Current | Older, context_window: Option<u64>, efforts: Vec<String>,
default_effort: Option<String>, price: Option<Price> }`. `Price { input, output, cache_read }` in
dollars per million tokens; cache writes stay 1.25x input (5 min) and 2x (1 h).

Where each field comes from, in order (first that answers wins):

| Field | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| Models | 1. Anthropic's model list (`GET /v1/models`) read with Claude Code's own live login from Keychain (`Claude Code-credentials`, or the lane's item for a lane's session; read only, never refreshed or written, the maintainer's choice, since Notchling's saved copy is usually expired), through `/usr/bin/security` like the other Keychain reads; 2. the last list cached on disk; 3. the data file's list | 1. `models_cache.json` (Codex refreshes it); 2. data file | 1. the open `agy acp`'s `configOptions`; 2. last seen, cached on disk; 3. data file |
| Current vs older | data file names the current set per tool; a model the live list reports that is newer than every model the data file knows in its family (Fable, Mythos, Opus, Sonnet, Haiku) becomes current in that family's place and borrows its efforts, window and price until the data file names it (the maintainer's choice); the rest is older | Codex's own `visibility: "list"` and order | as agy lists them |
| Efforts | data file, per model family (`low`..`max`; Haiku 4.5 none) unless the model list reports them | `supported_reasoning_levels`, `default_reasoning_level` | Thinking Level option values |
| Context window | data file per model **as Claude Code uses it** (Claude Code's `/context` says 400k for Opus 5.5 while the API allows 1M, so the API's `max_input_tokens` is not the right number); none when unknown | `context_window` from the cache | none unless agy reports one |
| Price | data file | data file | data file |

**The data file** (`crates/engine/data/catalog.json`, compiled in with `include_str!`) holds per tool:
the current model set, effort levels per family, Claude Code's context windows, and prices (the
current `cost/prices.rs` and `cost/other.rs` tables move here). A copy in the data folder
(`<data root>/notchling/catalog.json`) overrides it field by field when present, so a price or a new
model can be fixed without a rebuild. Prefix matching keeps today's rules: longest prefix first for
Claude (`fable-5-2` before `fable`); whole ids minus a date suffix for others.

**Repricing**: a price fixed in the data file also fixes costs already counted (my call). The cost
cache (`cost/cache.rs`) keeps tokens per model per day, with 5-minute and 1-hour cache writes apart,
and prices them when read; its version is bumped, forcing one full re-read.

**Borrowed values are visible too**: a price or window taken from the family shows as such in the
cost views and the Context Meter's tooltip.

**What a session may start on** (the maintainer's choice): `PICKER_MODELS` in `control_ops.rs` is a
security allowlist, not only a list to show (`flags_arg`, `bridge_backend.rs`, `/model` through
`session.command`). It becomes: the models built into the app's data file, plus those the live
sources report (Anthropic's model list, Codex's cache, the open `agy acp`). The override file in the
data folder can change names, prices and windows but never adds an allowed model.

**Unknown is visible**: a model with no price adds tokens but no dollars, and the cost views say
"some models have no price" with the ids; the daemon logs each unpriced id once. A test fails when a
model in the built-in current set has no price.

**Refresh**: the Claude list once a day and on demand (the picker opening when the cache is older than
a day), off the main thread; Codex re-reads its cache file when its mtime changes; Antigravity updates
whenever a session loads.

### One daemon API

The existing `session.*` ops keep their names and meanings and are widened to every tool
(builder's check; my call, so no op changes meaning and the local API stays additive):

| Op | Means, for every tool | Claude Code today |
| --- | --- | --- |
| `session.reply {id, text}` | send a message | unchanged |
| `session.interrupt {id}` | stop the current turn | unchanged (Esc through `claude attach`) |
| `session.stop {id}` | end the session's process or job | unchanged (`claude stop`); Codex: nothing runs between turns, answers ok; Antigravity: closes its `agy acp` |
| `session.remove {id}` | delete the session | unchanged (`claude rm`); Codex: `codex delete` |
| `session.take_back`, `session.answer`, `session.permission` | as today, for any tool that has them | unchanged |
| `session.new {tool?, folder, ...}` | start a session; `tool` defaults to Claude Code, so older callers are unaffected | unchanged |
| `session.set_option {id, option, value}` (new) | a picker choice (model, effort, mode) | sends `/model` or `/effort` as today |
| `catalog.get {tool}` (new), `catalog.changed` (new event) | the tool's models, efforts, windows, prices | |

The daemon finds the tool from the session id (`codex:...`, `agy:...`, `codex-new:...`, `agy-new:...`,
otherwise Claude Code). The `codex.*` and `agy.*` ops stay as aliases that call the same code until
every client has moved; then they are removed in their own change.

Acting stays as it is: off at every start until the app turns it on (decision 0007's note), the same
tiers and gates apply to the new ops as to the ops they replace.

The phone protocol (`protocol/agentconsole.v1.schema.json`) gets, additively, a `models` list (full
ids and names, for starting a Claude Code session) and a new optional `modelId` on `session.new`
beside the old `model` enum, which stays for older phones (my call: widening the enum would make an
older Mac refuse new ids). Switching models in Codex and Antigravity sessions from the phone is not
in this work. Schema first, then `tools/protocol/gen.py`, per the `add-protocol-field` skill.

### The app

`ChatTool` keeps drawing and stops deciding: pickers come from `catalog.get`, the Context Meter's
window from the catalog, sends and stops through `session.*`. The notch, search, the sidebar's marks,
prompts, the row menu, the slash menu and New Session move to the capabilities and the generic ops.
`format::context_window` and `claude_picker::MODELS`/`OLDER_MODELS` are deleted.

A guard check (`GUARD-TOOL-BRANCH`, in `tools/guard`, Python standard library) refuses a new
`Tool::Claude|Codex|Antigravity` match outside the engine's implementations, the registry and the
app's data layer, failing only on new or changed lines like the other lint rules.

## Phases (each its own pull request into `agent-run-oss`)

1. **Catalog.** Engine `Catalog` for all three tools, the data file with prices moved in, the cache,
   `catalog.get`. Replace `claude_picker` lists, `PICKER_MODELS`, `format::context_window`,
   `cost/prices.rs`, `cost/other.rs`, `delegate/agy_pick.rs` `LEVELS`. Phone `models` field.
   Decision 0016 written with it.
2. **Capabilities and Sessions.** Traits, registry, `session.new|close` routed by id; sidebar, closed
   rows, search and the notch use them.
3. **Turns.** `session.send|stop|take_back` for every tool; `codex.*`/`agy.*` become aliases;
   `ChatTool::send/stop/queues` read from the engine.
4. **Prompts and Commands.** Questions, permissions and slash commands behind the interface; the
   prompt cards and slash menu stop branching.
5. **Clean-up.** Remove the alias ops once no client uses them, add `GUARD-TOOL-BRANCH`, update
   `docs/chat-composer-parity.md` and the folder notes.

## Constraints

- Project rules (AGENTS.md, docs/rules.md): tests never touch Keychain, real logins, or the real tool
  folders; fixtures for every source (a synthetic `models_cache.json`, a recorded `/v1/models` body
  with no account data, an ACP recording such as `Fixtures/acp-agy.jsonl`).
- Reading Anthropic's model list is a new network and Keychain use: it waits for the maintainer's
  approval (`maintainer-gate`, `CAP-*`). It never refreshes a token; a 401 just falls back to the cache.
- Sending and process launching are protected areas; phases 3 and 4 wait for the maintainer to merge.
- The phone protocol and the local API change additively only.
- Docs and folder notes are kept true in the same change (`legacy/macos/Sources/AGENTS.md` is not
  touched, the Swift app is retired; Rust notes live in the crates and `ARCHITECTURE.md`).

## Left to the builder

Module and type names; a new tool (Grok) is a new `Tool` enum case, its id prefix and its
implementation, not open string ids (my call: three closed enums exist and that is fine); whether `Capabilities` is a struct of flags or a method per part; the cache
file layout; how the registry is passed to the daemon's ops; the order of work inside a phase.

## Open, to check while building

- Whether `GET /v1/models` accepts a Claude Code OAuth token (with the `oauth-2025-04-20` beta header).
  If it doesn't, Claude's live source is skipped and the data file plus Claude Code's own
  `additionalModelOptionsCache` (in `~/.claude.json`, read for that key only) are the sources.
