# 0015: Chat's parts are shared; each tool is one adapter behind one interface

## Context

Chat shows Claude Code, Codex and Antigravity sessions, and more tools will come (Grok was named). On 2026-11-11 the maintainer asked that every part of Chat's UI be the same component for every tool, "the same behavior, only the implementation behind an interface or API per provider". Until then the views asked which tool a row was wherever they did something: 38 branches in 18 files. A part a branch forgot was simply missing for one tool, which is how Antigravity went without a Context Meter, an Account Menu and a Limit Ring until 2026-11-11.

The engine already worked this way for accounts (`AccountProvider`), for limits (`ProviderSource`) and for conversations (each tool's reader maps onto one `Conversation`).

## Decision

- Chat's views draw each part once, for every tool. They ask a `ChatTool` (`crates/app/src/chat/view/tool/mod.rs`) what to show and what to send: the reply box's text and refusal, sending, the outbox, Stop, the pickers (`chat::pickers::Menu`), a pick, the account button and its click, and the Limit Ring's limits.
- Each tool is one file in `chat/view/tool/` that maps its own API onto those answers. `tool::of` is the only place a `Tool` becomes behavior.
- A tool that can't do something still answers: a refusal with the reason, or a disabled menu with a tooltip. A part is never missing for one tool. `docs/chat-composer-parity.md` lists where each tool stands.
- New parts are added to the interface, so every tool has to answer them before the app builds.

## Consequences

- A new tool is a new `Tool` case, one adapter file and its arm in `tool::of`, plus an `AccountProvider` and a `ProviderSource` in the engine if it has logins and limits.
- Branches on `Tool` remain outside the composer, account button and Limit Ring (the slash menu, prompts, the row menu, the sidebar's marks, the Antigravity conversation fetch). They move behind the interface as their parts become shared; until then they are the parity doc's gaps.
- A guard check that refuses a new `Tool::` branch outside `chat/view/tool/` and the data layer would keep it this way. It isn't written yet.

## Status

Proposed. The maintainer asked for this design on 2026-11-11; accepting this record settles it.

## Date

2026-11-11
