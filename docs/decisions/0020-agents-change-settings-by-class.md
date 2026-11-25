# 0020: Agents read every setting and change one by its class

## Context

[0007](0007-local-api-tiers.md) made settings app tier only: "An agent that can run a shell can use the command line, but can't … change settings."

People now ask the agent in a chat to change Notchling: "switch Notchling to light mode", "what's my chat history size?", "list the settings". With settings app tier only, the agent can't answer, or it edits `notchling.db` behind the app's back, with no check, no audit and no live update.

Facts that bound the change:

- An agent with a shell can already write `notchling.db`. Files at rest are not a tier boundary ([0007](0007-local-api-tiers.md)'s note of 2026-11-08), so a checked path through the daemon takes nothing away; what it must not do is turn on what only the app tier may.
- The five switches (`acting`, the bridge, `accounts`, `vault`, `install`) are that boundary. They are off at every daemon start, and only an app-tier call in that run turns one on.
- The Inbox already takes a decision from the user on the Mac or a paired phone, first answer wins, with a responder for the waiting producer (vault approvals, [0007](0007-local-api-tiers.md)'s note of 2026-11-13).
- [Goals](../goals.md): agents never loosen their own guard rails, by any path.

## Decision

- The agent tier may read every setting, `settings.list` and `settings.get`, and change one with `settings.set`, by the key's class. The class is data on the key's line in `crates/engine/src/settings/keys.rs`, with a title, a description and the reason for the class:
  - **Free**: set at once. A preference that gives an agent nothing: appearance, the notch's looks, keeping the Mac awake, how much Chat history loads.
  - **Ask**: raised as an Inbox item; the user allows or denies it on the Mac or a paired phone, and the first answer wins. No answer within 5 minutes, or no app open to ask, is a no. For anything that changes what runs, hooks, shortcuts, other apps, account switching, and model and delegation defaults.
  - **Never**: refused. The five switches, logins, secrets, sandbox permissions (`codexNetwork`: an agent could open the network to its own delegated Codex jobs) and the app's own records. An agent never sees these values; it does see the switches' state.
- A key with no class is Never, and a test fails until someone gives it one.
- The switches stay as [0007](0007-local-api-tiers.md) set them: no tier changes one through `settings.set`, and only the app's `settings.set_acting`, `bridge.set` and `settings.set_switch` do.
- Values are checked against the key's type, choices and range. Every `settings.set` (set, asked, allowed, denied, expired, refused) goes to `audit.jsonl`.
- Every change through the daemon goes out as `settings.changed` on the `settings` topic, and the app applies it at once.
- Agents learn it from a `notchling-settings` skill, installed for Claude Code and Codex like the other `notchling-*` skills, and the `notchling settings list|get|set` command.

This amends 0007's line "Account switching, vault values, Machines fixes and settings are app tier only" and its consequence "can't … change settings": settings in the Free and Ask classes become agent tier under these rules; the switches and the Never keys stay app tier.

## Consequences

- An agent can do what the user asks of it in a chat, and the change shows in the app at once. What it changed is in the audit log.
- An agent misled by what it reads (a prompt injection) can change Free keys without asking. They are chosen so the worst case is a display or memory preference the user sees and changes back.
- An Ask item can be allowed by reflex. The item says what changes, to what, and what it is now; anything that grants power is Never, not Ask.
- A paired phone can allow an Ask item, with the same known risk as for vault approvals: the paired-devices list is a file an agent can write.
- The Settings sheet keeps writing the store itself and applying its own change, so it works with no daemon; its writes are not on the `settings` topic. Only changes made through the daemon are.
- Keys added later appear in `settings.list` as soon as they exist, as Never until classified. Expected: the Appearance setting Free, a global search shortcut Ask.
- Done in `crates/notchlingd/src/settings_ops.rs`, `settings_asks.rs`, `settings_cli.rs`, `crates/engine/src/settings/agents.rs` and `skill.rs`, and in the app in `src/mac/settings_live.rs`. Wire shapes: [local-api-ops.md](../local-api-ops.md).

## Status

Proposed. Amends [0007](0007-local-api-tiers.md). The maintainer settles it by merging the change that brings it.

## Date

2026-11-13
