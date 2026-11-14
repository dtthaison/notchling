# Provider matrix

Generated from crates/engine/src/contract by the provider_matrix test; NOTCHLING_UPDATE_GOLDEN=1 rewrites it.

| Tool | Native | Fallback | Not applicable | Gap | Unwired |
| --- | --- | --- | --- | --- | --- |
| Claude Code | 32 | 2 | 1 | 4 | 34 |
| Codex | 18 | 1 | 6 | 14 | 19 |
| Antigravity | 19 | 0 | 4 | 16 | 19 |

## Sessions

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `session.list` | N: jobs/ and sessions/ in every lane's config folder · unwired | N: Rollouts under CODEX_HOME/sessions · unwired | N: The summaries copy, nesting_depth 0 · unwired |
| `session.status` | N: Job state, transcript and screen · unwired | N: Rollout tail (turn_open) and Notchling's own runs · unwired | N: Its agy acp, else the holder's log · unwired |
| `session.new` | N: A claude job · unwired | N: First codex exec turn, shown as a draft row · unwired | N: ACP session/new, shown as a draft row · unwired |
| `session.stop` | N: claude stop · unwired | NA: Nothing runs between Codex turns; Stop interrupts the turn | N: Closes its agy acp · unwired |
| `session.remove` | N: claude rm · unwired | N: codex delete · unwired | GAP: phase 5: hide-row; agy has no delete |
| `session.move` | N: Lanes: Move and Move All · unwired | GAP: phase 5: a lane needs its own CODEX_HOME | NA: One Antigravity login per Mac; a switch moves every session |
| `folder.trust` | N: Trust Folder · unwired | NA: codex exec asks no trust question; the sandbox is set per turn | N: trustedWorkspaces through agy_trust · unwired |

## Conversation

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `conversation.read` | N: The session's transcript · unwired | N: The rollout reader · unwired | N: ACP session/load, else the log · unwired |
| `conversation.follow` | N: File watch on the transcript · unwired | N: File watch on the rollout, --json for its own runs · unwired | N: ACP updates, else the log · unwired |
| `conversation.context` | N: Transcript usage; the window comes from the catalog · unwired | N: model_context_window in the rollout · unwired | N: Tokens in the log; the window is the catalog's · unwired |

## Subagents

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `subagent.list` | N: subagents/agent-*.jsonl and their .meta.json · unwired | GAP: phase 1: child rollouts by parent_thread_id | GAP: phase 1: summaries rows by parent_conversation_id |
| `subagent.status` | N: task-notification, finished, handed back, 10 min quiet · unwired | GAP: phase 1: the child's task_complete or turn_aborted | GAP: phase 1: summaries status and killed, send_message |
| `subagent.glance` | N: glance::peek over the subagent's transcript tail · unwired | GAP: phase 1: the child's token_count and newest call (meanwhile delegate-jobs) | GAP: phase 1: the child log's tokens and newest step |
| `subagent.open` | N: The subagent's transcript · unwired | GAP: phase 1: the rollout from the child's own turn | GAP: phase 1: the child's log |
| `subagent.stop` | NA: No outside stop for one subagent; Stop interrupts the whole turn | NA: Only the model's interrupt_agent stops a Codex subagent | NA: The Agent Manager's kill is inside agy; Notchling can't reach it |

## Signals

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `signal.turn_finished` | N: Job state goes from Working to Completed · unwired | N: task_complete and turn_aborted in the rollout · unwired | N: ACP stopReason; log PLANNER_RESPONSE DONE · unwired |
| `signal.subagent_finished` | N: task-notification in the parent transcript · unwired | GAP: phase 2: the child's task_complete | GAP: phase 2: the child goes idle after send_message |
| `signal.needs_you` | N: Hook, transcript and screen · unwired | NA: codex exec never asks | N: ACP request_permission · unwired |

## Turns

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `turn.send` | N: Typed through claude attach · unwired | N: codex exec resume · unwired | N: ACP session/prompt · unwired |
| `turn.hold` | F (held-queue): Claude Code's own queue is lost on restart and injected mid-turn · unwired | F (held-queue): codex exec takes one message per turn; Notchling holds the rest · unwired | GAP: phase 4: held-queue; the box is off while it works |
| `turn.take_back` | F (held-queue): The held message is Notchling's, so Notchling edits it · unwired | GAP: phase 4: held-queue | GAP: phase 4: held-queue |
| `turn.interrupt` | N: Esc · unwired | N: SIGINT · unwired | N: ACP session/cancel · unwired |
| `turn.options` | N: /model and /effort · unwired | N: Model and effort per turn · unwired | N: ACP session/set_config_option · unwired |
| `prompt.question` | N: AskUserQuestion hook · unwired | NA: codex exec can't ask a question | NA: ACP carries permission requests only |
| `prompt.permission` | N: PermissionRequest hook · unwired | NA: Codex turns run in a sandbox and never ask | N: ACP request_permission · unwired |
| `command.slash` | N: slash.rs lists and runs them · unwired | GAP: phase 5: Codex's slash commands in Chat | GAP: phase 5: available_commands_update in Chat |

## Accounts and usage

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `account.switch` | N: Saved logins in the Keychain and the config folder · unwired | N: Saved logins in auth.json · unwired | N: The Keychain item and its mirror file · unwired |
| `account.lanes` | N: CLAUDE_CONFIG_DIR per account · unwired | GAP: phase 5: CODEX_HOME per account | NA: One Antigravity login per Mac |
| `usage.limits` | N: Plan limits from the provider's usage source · unwired | N: Plan limits from the provider's usage source · unwired | N: The local language server · unwired |
| `usage.cost` | N: Transcript usage priced from the catalog · unwired | N: Rollout token counts priced from the catalog · unwired | N: Log token counts priced from the catalog · unwired |
| `catalog.models` | N: Anthropic's list in data/catalog.json · unwired | N: models_cache.json · unwired | GAP: phase 5: context windows in catalog.json |

## Delegation

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `delegate.target` | GAP: phase 3: notchling delegate claude | N: codex exec · unwired | N: agy acp · unwired |
| `delegate.caller` | N: CLAUDE_CODE_SESSION_ID · unwired | GAP: phase 3: CODEX_THREAD_ID | GAP: phase 3: NOTCHLING_SESSION, then the process tree |

## Setup

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `setup.rules` | GAP: design: 0019 | GAP: design: 0019 | GAP: design: 0019 |
| `setup.skills` | N: install::skills writes the notchling-* skills · unwired | N: install::skills writes the notchling-* skills · unwired | N: install::skills writes the notchling-* skills · unwired |
| `setup.hooks` | N: install::hooks writes Notchling's five hooks · unwired | GAP: design: 0019 (planned not applicable) | GAP: design: 0019 (planned not applicable) |

## Versions

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `version.detect` | GAP: design: version-sync | GAP: design: version-sync | GAP: design: version-sync |
| `version.update` | GAP: design: version-sync | GAP: design: version-sync | GAP: design: version-sync |

## Search

| Capability | Claude Code | Codex | Antigravity |
| --- | --- | --- | --- |
| `search.text` | N: Transcript text in Search's sessions corpus · unwired | N: Rollout text in Search's sessions corpus · unwired | GAP: phase 5: text from its logs (meanwhile title-index) |
