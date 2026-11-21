# Agent paths

Written by `NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-notchlingd commands`; do not edit. Every op of the local API is a command's op, planned for a phase of the agents-first design, user-only or internal. `notchling help --json` says the same to an agent.

## Commands

| Command | Ops | Consent | Exits |
| --- | --- | --- | --- |
| `notchling status [--json] [--socket <path>]` | `daemon.status` (agent) | none | 0 1 2 3 4 |
| `notchling sessions [--json] [--socket <path>]` | `sessions.list` (agent) | none | 0 1 2 3 4 |
| `notchling providers [--json]` | none | none | 0 1 2 |
| `notchling pages [words] [--json] [--socket <path>]` | `pages.list` (agent) | none | 0 1 2 3 4 |
| `notchling page <link|id|words> [--json] [--socket <path>]` | `pages.find` (agent) | none | 0 1 2 3 4 |
| `notchling delegate <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>` | none | none | 0 1 2 3 4 124 |
| `notchling delegate-models [codex|agy] [--for <role>] [--json]` | none | none | 0 1 2 |
| `notchling models [check|update] [--json] [--no-skills] [--force]` | `models.check` (agent), `models.update` (agent, acts) | none | 0 1 2 3 4 |
| `notchling cost report [--days N] [--plan api|subscription|auto] [--json]` | `cost.report` (agent) | none | 0 1 2 3 4 |
| `notchling cost setup [--apply] [--consent a,b] [--json]` | `cost.setup.plan` (agent), `cost.setup.apply` (app, acts) | none | 0 1 2 3 4 |
| `notchling cost check [--json]` | `cost.setup.check` (agent) | none | 0 1 2 3 4 |
| `notchling cost undo <claude|codex|agy> [--json]` | `cost.setup.undo` (app, acts) | none | 0 1 2 3 4 |
| `notchling secrets [words] [--json] [--socket <path>]` | `vault.find` (agent) | none | 0 1 2 3 4 |
| `notchling secret [--json] <name>[,<name>…] -- <command> [args…]` | `vault.use` (agent, acts) | by_entry | 0 125 126 127 |
| `notchling secret-request <name> [--kind token|login --url --note --how --expires YYYY-MM-DD] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]` | `vault.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling settings list [--json]` | `settings.list` (agent) | none | 0 1 2 3 4 |
| `notchling settings get <key> [--json]` | `settings.get` (agent) | none | 0 1 2 3 4 |
| `notchling settings set <key> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--dry-run] [--json]` | `settings.set` (agent, acts), `settings.asked` (agent, acts) | by_key_class | 0 1 2 3 4 5 6 7 124 |
| `notchling consent <id> [--wait] [--timeout <seconds>] [--json]` | `consent.get` (agent) | none | 0 1 2 3 4 5 6 124 |
| `notchling consents [--json]` | `consent.mine` (agent) | none | 0 1 2 3 4 |
| `notchling consent cancel <id> [--json]` | `consent.cancel` (agent, acts) | none | 0 1 2 3 4 |
| `notchling inbox [--json] [--socket <path>]` | `inbox.mine` (agent) | none | 0 1 2 3 4 |
| `notchling accounts [--json] [--socket <path>]` | `accounts.summary` (agent) | none | 0 1 2 3 4 |
| `notchling limits [--tool <claude|codex|agy> [--pool <name>]] [--json] [--socket <path>]` | `limits.summary` (agent) | none | 0 1 2 3 4 |
| `notchling orgs [--json] [--socket <path>]` | `organizations.mine` (agent) | none | 0 1 2 3 4 |
| `notchling search <words> [--kind pages|sessions] [--since <seconds>] [--json] [--socket <path>]` | `search.query` (agent) | none | 0 1 2 3 4 |
| `notchling switches [--json] [--socket <path>]` | `settings.switches` (agent) | none | 0 1 2 3 4 |
| `notchling awake [on|off] [--json] [--socket <path>]` | `awake.state` (agent) | none | 0 1 2 3 4 |
| `notchling versions [--latest] [--workers] [--json] [--socket <path>]` | `versions.get` (agent), `versions.latest` (agent), `versions.workers` (agent) | none | 0 1 2 3 4 |
| `notchling roles [--json] [--socket <path>]` | `catalog.roles` (agent) | none | 0 1 2 3 4 |
| `notchling session reload <id|self> [--after-turn] [--json]` | `session.reload` (agent, acts) | own_free | 0 1 2 3 4 |
| `notchling cluster status [--json]` | `cluster.status` (agent) | none | 0 1 2 3 4 |
| `notchling cluster join --token-stdin [--json]` | `cluster.join` (agent, acts) | none | 0 1 2 3 4 |
| `notchling cluster reach <addr> <port>` | none | none | 0 1 2 |
| `notchling ask [--choice <text>]... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] -- <question>` | `ask.open` (agent, acts), `ask.wait` (agent) | none | 0 1 2 3 4 5 124 |
| `notchling asks [--wait] [--json]` | `ask.mine` (agent) | none | 0 1 2 3 4 124 |
| `notchling answer <id> (--choice <n> | -- <text>) [--json]` | `ask.answer` (agent, acts) | own_free | 0 1 2 3 4 |
| `notchling help [--json]` | none | none | 0 2 |
| `notchling capabilities [--json] [--socket <path>]` | `daemon.capabilities` (agent) | none | 0 1 2 3 4 |

## Planned

| Op | Feature | Phase |
| --- | --- | --- |
| `events.subscribe` | Follow events | 2 |
| `inbox.list` | Read the Inbox | 2 |
| `accounts.list` | Read accounts | 2 |
| `providers.list` | Read providers | 2 |
| `providers.usage` | Read provider usage | 2 |
| `limits.status` | Read limits | 2 |
| `limits.usage` | Read limits | 2 |
| `limits.set_mode` | Change the limit mode | 2 |
| `cost.summary` | Read the cost summary | 2 |
| `organizations.list` | Read the organization | 2 |
| `settings.get_acting` | Read the switches | 2 |
| `versions.check` | Check tool versions | 2 |
| `outbox.list` | Read queued messages | 2 |
| `activity.list` | Read the session's activity | 2 |
| `prompts.list` | Read pending prompts | 2 |
| `catalog.get` | Read the model catalog | 2 |
| `log.verify` | Verify the log chain | 2 |
| `settings.set_acting` | Switch Act on Sessions on | 3 |
| `bridge.set` | Switch the Phone Bridge on | 3 |
| `settings.set_switch` | Switch a daemon switch on | 3 |
| `session.new` | Start a session | 4 |
| `folder.trust` | Trust a folder for a session | 4 |
| `session.reply` | Reply to a session | 4 |
| `session.send` | Queue a reply to a session | 4 |
| `session.answer` | Answer a session's question | 4 |
| `session.interrupt` | Interrupt a session | 4 |
| `session.stop` | Stop a session | 4 |
| `session.remove` | Remove a session | 4 |
| `session.command` | Run a slash command in a session | 4 |
| `session.dialog` | Read a session's dialog | 4 |
| `session.dialog_pick` | Pick in a session's dialog | 4 |
| `session.dialog_dismiss` | Dismiss a session's dialog | 4 |
| `session.chrome` | Switch a session's Chrome | 4 |
| `session.shell_output` | Read a session's shell output | 4 |
| `session.reload.state` | Read a reload's state | 4 |
| `session.held.discard` | Discard a held message | 4 |
| `codex.send` | Send to a Codex session | 4 |
| `codex.stop` | Stop a Codex session | 4 |
| `codex.delete` | Delete a Codex session | 4 |
| `codex.children` | Read a Codex session's children | 4 |
| `agy.sessions` | List Antigravity sessions | 4 |
| `agy.state` | Read an Antigravity session | 4 |
| `agy.conversation` | Read an Antigravity conversation | 4 |
| `agy.open` | Open an Antigravity session | 4 |
| `agy.send` | Send to an Antigravity session | 4 |
| `agy.cancel` | Cancel an Antigravity turn | 4 |
| `agy.set_option` | Set an Antigravity option | 4 |
| `cluster.dispatch` | Dispatch a job to a machine | 5 |
| `cluster.follow` | Follow a job | 5 |
| `cluster.return` | Return a job's result | 5 |
| `shell.stop` | Stop a shell job | 5 |
| `cluster.create` | Create a cluster | 6 |
| `cluster.join.token` | Make a join token | 6 |
| `cluster.invite` | Invite a machine | 6 |
| `cluster.member.set` | Change a member | 6 |
| `cluster.remove` | Remove a member | 6 |
| `cluster.main.set` | Choose the primary | 6 |
| `cluster.move_home` | Move the home | 6 |
| `cluster.move.now` | Move the home now | 6 |
| `cluster.failover.set` | Set failover | 6 |
| `cluster.allow.set` | Allow secrets for the cluster | 6 |
| `accounts.switch` | Switch an account | 7 |
| `accounts.add.begin` | Add an account | 7 |
| `accounts.remove` | Remove an account | 7 |
| `accounts.move_all` | Move every session to a lane | 7 |
| `lanes.list` | Read lanes | 7 |
| `lanes.create` | Create a lane | 7 |
| `lanes.remove` | Remove a lane | 7 |
| `lanes.switch` | Switch a lane | 7 |
| `providers.switch` | Switch a provider | 7 |
| `providers.add.begin` | Add a provider | 7 |
| `providers.remove` | Remove a provider | 7 |
| `session.move` | Move a session to another account | 7 |
| `install.plan` | Plan Set Up | 8 |
| `install.apply` | Apply Set Up | 8 |
| `tailscale.switch` | Switch the tailnet | 8 |
| `versions.update_local` | Update a tool | 8 |
| `cost.profile.get` | Read the Cost Profile | 8 |
| `cost.profile.set` | Change the Cost Profile | 8 |
| `agy.sidecar.setup` | Set up the Antigravity sidecar | 8 |

## User only

| Op | Feature | Why |
| --- | --- | --- |
| `session.permission` | Answer a permission prompt | A prompt is the user's own check on an agent. |
| `agy.permission` | Answer a permission prompt | A prompt is the user's own check on an agent. |
| `install.remove` | Remove hooks and skills | An agent would remove the check on itself. |
| `log.audit.delete` | Delete the Audit | An agent would erase its own trail. |
| `log.chain.start` | Start a new log chain | An agent would erase its own trail. |
| `bridge.pair.begin` | Pair a phone | A paired phone answers cards: an agent could approve itself. |
| `bridge.pair.answer` | Pair a phone | A paired phone answers cards: an agent could approve itself. |
| `bridge.revoke` | Revoke a phone | A paired phone answers cards: an agent could approve itself. |
| `inbox.answer` | Answer an Inbox item | The card is the user's yes. |
| `consent.answer` | Answer a consent card | The card is the user's yes. |
| `ask.dismiss` | Dismiss a question | The card is the user's yes. |
| `vault.approval.answer` | Answer a secret approval | The card is the user's yes. |
| `vault.list` | List the Vault | Values never reach an agent. |
| `vault.save` | Save a secret | Values never reach an agent. |
| `vault.admin` | Use a secret on another machine | Values never reach an agent. |
| `vault.remove` | Remove a secret | Values never reach an agent. |
| `cluster.promote` | Promote a machine | It changes which machine drives the user's sessions. |
| `cluster.take_back` | Take a machine back | It changes which machine drives the user's sessions. |
| `cluster.continue_here` | Continue a session here | It changes which machine drives the user's sessions. |
| `session.take_back` | Take a session back | It changes which machine drives the user's sessions. |
| `organizations.create` | Create an organization | It changes what an agent may see. |
| `organizations.rename` | Rename an organization | It changes what an agent may see. |
| `organizations.color.set` | Colour an organization | It changes what an agent may see. |
| `organizations.folders.set` | Set an organization's folders | It changes what an agent may see. |
| `organizations.delete` | Delete an organization | It changes what an agent may see. |

## Internal

| Op | Why |
| --- | --- |
| `hello` | protocol |
| `ask.list` | app_ui |
| `consent.list` | app_ui |
| `consent.opened` | app_ui |
| `hook.request` | hook |
| `session.question` | hook |
| `agy.sidecar.take` | hook |
| `agy.sidecar.report` | hook |
| `agy.sidecar.status` | hook |
| `agy.hook` | hook |
| `vault.request.cancel` | app_ui |
| `vault.pending` | app_ui |
| `inbox.seen` | app_ui |
| `bridge.status` | app_ui |
| `bridge.devices` | app_ui |
| `bridge.tunnel.status` | app_ui |
| `accounts.add.finish` | app_ui |
| `accounts.result` | app_ui |
| `providers.add.cancel` | app_ui |
| `git.status` | app_ui |
| `git.diff` | app_ui |
| `git.branch_diff_stat` | app_ui |
| `git.branch_diff_file` | app_ui |
| `git.branches` | app_ui |
| `git.worktrees` | app_ui |
| `git.branch_list` | app_ui |
| `outbox.edit` | app_ui |
| `outbox.remove` | app_ui |
| `outbox.hold` | app_ui |
| `outbox.resend` | app_ui |
| `outbox.discard` | app_ui |
| `cluster.side_copy.hide` | app_ui |
| `organizations.folders.preview` | app_ui |
