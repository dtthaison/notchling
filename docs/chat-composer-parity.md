# Chat's reply box: one design for every tool

The reply box (part "Composer") and the row under it look and behave the same for a Claude Code,
a Codex and an Antigravity session. Where a tool can't do something, the control is still there,
disabled, with a tooltip that says why; it is never just missing. New parts are added for every
tool at once, or the table below says which tool waits and why.

Written 2026-11-11, from the code in `crates/app/src/chat/view/` (`composer.rs`, `tools.rs`,
`slash_menu.rs`, `accounts.rs`, `limit_ring.rs`).

The parts ask each session's tool through one interface, `ChatTool` in
`crates/app/src/chat/view/tool/` (decision [0015](decisions/0015-one-chat-interface-per-tool.md)):
a new tool is one adapter file there.

A Claude Code session's Model and Effort pickers and its Context Meter's window come from the daemon's
model catalog (`catalog.get`, decision [0016](decisions/0016-provider-interface-in-the-engine.md)),
and the built-in list until the daemon answers. Codex's pickers still read Codex's own
`models_cache.json` and Antigravity's its session's options; Codex moves to `catalog.get` in
phase 2 of 0016.

## The parts, left to right

Under the box: **Model** · **Effort** · status · **Context Meter** · **Account Menu** · **Limit Ring**.
In the box: **+** (attach), the text, **Send** or **Stop**. Over the box: the **Slash Menu**, and the
**Queued** bubbles above it.

## Where each tool stands

| Part | Claude Code | Codex | Antigravity | To do |
| --- | --- | --- | --- | --- |
| Reply box, Return sends | yes | yes | yes | |
| + attach files, images, ⌘U | yes | yes | yes | |
| Send while a turn works | queued (Claude Code's queue) | queued (Notchling's outbox, next turn) | refused: the box is disabled ("busy") | Antigravity: queue in the outbox like Codex, send at the turn's end |
| Edit a queued message | yes (take back) | no | no | Codex and Antigravity: take back from the outbox |
| Stop / Esc | yes (interrupt) | yes (SIGINT) | yes (`session/cancel`) | |
| Model picker | `/model`, rows from the catalog | next turn's model, rows from Codex's cache | its Model option | done 2026-11-11: one Model Picker for every tool (`chat::pickers`); Antigravity's Mode is a third picker of the same kind. Codex: rows from the catalog (phase 2 of 0016) |
| Effort picker | `/effort`, the levels of the model in use from the catalog | next turn's effort | Thinking Level | done 2026-11-11 |
| Slash Menu | yes (Claude Code's commands) | none in Chat | none in Chat | Codex: its commands and `~/.codex/prompts` (the Swift app had them). Antigravity: the commands its `available_commands_update` sends |
| Context Meter | yes, with % of the window from the catalog (it says when the window is another model's) | yes when the rollout says | yes since 2026-11-11 (from its log before a turn ends), tokens only | a context window per Gemini model, so it shows a % |
| Account Menu | menu: this session's account, Move All, Add | button: opens Accounts on Codex | button: opens Accounts on Antigravity (2026-11-11) | the same menu for all: list the tool's accounts with a tick, Switch, Add Account… |
| Limit Ring | the account's 5h ring, card | the active login's Codex group | the model's group, card with every group (2026-11-11) | |
| Questions and permissions | cards in the conversation | none (exec can't ask) | permission card | Codex: say in the card area that this turn can't ask, once |
| Read-only / exited states | yes, with a tooltip | no state | no state | an ended Codex/Antigravity session that can't resume says so the same way |

## Order of the work

1. Antigravity sends while a turn works: queue it (the box isn't disabled any more), with Queued
   bubbles and Edit, the same as Codex; then Edit for Codex too.
2. Model and Effort as two buttons for every tool, the same part names (`Model Picker`,
   `Effort Picker`).
3. Slash Menu for Codex and Antigravity.
4. One Account Menu for every tool.
5. Context windows for the Gemini and GPT models, so every meter shows a %.

Each step keeps the part names (`.part("…")`) the same across tools, so "the Model Picker" names one
thing whatever the session.
