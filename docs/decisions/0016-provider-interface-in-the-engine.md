# 0016: One provider interface per tool, in the engine, and model data from a catalog

## Context

Fable 5.2 came out and Chat's Model Picker still offered Fable 5.1. The Claude Code model list was written in four places (`crates/app/src/chat/claude_picker.rs`, `PICKER_MODELS` in `crates/notchlingd/src/control_ops.rs`, the Swift `ModelPicker`, `MODELS` in the phone bridge's router), and prices, effort levels and context windows were tables in code. Every new model meant a code change and a rebuild. On 2026-11-11 the maintainer said "it's not the good way to maintain the data information of provider" and "we should support all the providers".

[0015](0015-one-chat-interface-per-tool.md) put Chat's parts behind one interface per tool, `ChatTool`. That interface takes Chat's own view, so the notch, the phone bridge, search, `notchling delegate` and the daemon can't use it. Below it the daemon has one API per tool (`session.*` for Claude Code, `codex.*`, `agy.*`), and its clients branch on the tool again: about 170 `Tool::` matches outside `chat/view/tool/` when the design was written, 44 of them in `sidebar.rs`.

The design is [the tool provider interface spec](../superpowers/specs/2026-11-11-tool-provider-interface-design.md).

## Decision

- Each tool is one provider in the engine, made of small traits, one per concern: `Catalog` (models, efforts, context windows, prices), `Capabilities`, `Sessions`, `Turns`, `Prompts` and `Commands`. Claude Code, Codex and Antigravity each implement them, and one registry turns a tool into behavior, as `ProviderRegistry` does for accounts. Accounts and limits stay `AccountProvider` and `ProviderSource`.
- Model data is data. `crates/engine/data/catalog.json`, compiled in, holds every tool's prices and, for Claude Code, the current and older models, the effort levels and the context windows. A live source comes first (Anthropic's `GET /v1/models` for Claude Code, Codex's `models_cache.json`, the Model and Thinking Level options an `agy acp` session reports), then the last live list cached on disk, then the data file.
- A Claude model newer than every known one of its family takes the family's place among the current models and borrows the newest known one's price and window. Chat says so where it shows them.
- A `catalog.json` beside the daemon's socket overrides prices, windows and names, entry by entry. It never adds a model.
- A session starts only on a model the data file lists or a live source reports. A Claude id must also have the shape of one, `[a-z][a-z0-9.-]*` with an optional `[1m]`, since it goes on `claude`'s command line. The override file can't widen this.
- No login is renewed to read a model list. Claude Code's list is read with the main login's live token, read only, and only while the `accounts` switch is on.
- A model with no price counts its tokens and no dollars and is listed as unpriced, never a quiet $0. A price fixed later reprices the days already counted, for that model only: the cost cache keeps each model's price with what it counted, and at the next daemon start a model whose price changed is counted again from its transcripts still on disk. A day whose transcripts are gone (Claude Code deletes them after 30 days by default) is priced again from the tokens the cache kept, so a long prompt's dearer tier and fast mode, which those totals don't keep, are left out of it. A price added for a model nobody used changes nothing, and no day is dropped.
- One daemon API for every tool. The `session.*` ops widen to every tool, found from the session id; `catalog.get` and the `catalog.changed` event are new; `codex.*` and `agy.*` stay as aliases until no client uses them. The local API and the phone protocol change additively only.
- `ChatTool` stays as the presenter over the engine's answers and stops deciding. The app's other `Tool::` branches go, and a guard check, `GUARD-TOOL-BRANCH`, keeps new ones out.
- The work ships in five phases, each its own pull request into `agent-run-oss`, with the app working after each: 1. Catalog; 2. Capabilities and Sessions; 3. Turns; 4. Prompts and Commands; 5. Clean-up (the alias ops removed, the guard check added).

## Consequences

- A new model needs no code change: the live list brings it in, and the data file or the override gives it a price. A new tool is one implementation and a registration.
- Phase 1 is built. `notchling_engine::catalog` holds the data file, the override and the live cache (`live-models.json` beside the socket). The daemon's `CatalogService` serves `catalog.get` (agent tier) and is the one catalog that `cost.summary`, the `session.new --model` check and the phone bridge read. Chat's Claude Code Model Picker, Effort Picker and Context Meter read `catalog.get`, and the built-in list until the daemon answers. The phone gets a `models` list with `sessions.list` and may send `modelId` with `session.new`; the old short `model` still works.
- Reading Anthropic's list is new network and Keychain use from the daemon. A `catalog.get` for Claude Code starts a fetch when the cached list is a day old or missing; Chat asks at start, every hour and when a Claude Code picker menu opens. A failed fetch or a missing token keeps the cached list, and no new try starts for 15 minutes. Whether the endpoint takes a Claude Code login's token isn't checked against the real API yet; until it is, the cache and the data file are what show.
- More models can start a session than before: the built-in older ones (`claude-opus-4-6`), ids with a `[1m]` or date suffix, and what the live lists report. Before, only the picker's four ids could.
- Left for later:
  - `/model` sent through `session.command` isn't checked against the allowlist yet.
  - Codex's picker still reads Codex's cache in the app; it moves to `catalog.get` in phase 2, with the rest of the per-tool UI.
  - The Token Cost page doesn't show `unpriced` and `borrowed` yet; `cost.summary` returns them.
  - Claude Code's list is read with the main login's token only; a lane's own login isn't used.
  - The phone app doesn't read `models` or send `modelId` yet.
  - Phases 2 to 5 get their own plans.
- The Swift app, retired on 2026-11-11, keeps its own lists.

## Status

Proposed. It amends 0015: the interface per tool moves from Chat into the engine, and `ChatTool` becomes a presenter over it. The maintainer asked for this on 2026-11-11 and chose the live-login read, the borrowing rule and what a session may start on; accepting this record settles it.

## Date

2026-11-11
