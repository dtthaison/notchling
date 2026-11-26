# Agent paths

Written by `NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-notchlingd commands`; do not edit. Every op of the local API is a command's op, planned for a phase of the agents-first design, user-only or internal. `notchling help --json` says the same to an agent.

## Commands

| Command | Ops | Consent | Exits |
| --- | --- | --- | --- |
| `notchling status [--json] [--socket <path>]` | `daemon.status` (agent) | none | 0 1 2 3 4 |
| `notchling sessions [--json] [--socket <path>]` | `sessions.list` (agent) | none | 0 1 2 3 4 |
| `notchling sessions history [--limit <n>] [--before <cursor>] [--json] [--socket <path>]` | `sessions.history` (agent) | none | 0 1 2 3 4 |
| `notchling providers [--json]` | none | none | 0 1 2 |
| `notchling pages [words] [--json] [--socket <path>]` | `pages.list` (agent) | none | 0 1 2 3 4 |
| `notchling page <link|id|words> [--json] [--socket <path>]` | `pages.find` (agent) | none | 0 1 2 3 4 |
| `notchling delegate <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>` | `delegate.begin` (agent, acts), `delegate.end` (agent, acts) | none | 0 1 2 3 4 124 |
| `notchling delegate-models [codex|agy] [--for <role>] [--json]` | none | none | 0 1 2 |
| `notchling models [check|update] [--json] [--no-skills] [--force]` | `models.check` (agent), `models.update` (agent, acts) | none | 0 1 2 3 4 |
| `notchling cost report [--days N] [--plan api|subscription|auto] [--json]` | `cost.report` (agent) | none | 0 1 2 3 4 |
| `notchling cost setup [--apply] [--consent a,b] [--json]` | `cost.setup.plan` (agent), `cost.setup.apply` (app, acts) | none | 0 1 2 3 4 |
| `notchling cost check [--json]` | `cost.setup.check` (agent) | none | 0 1 2 3 4 |
| `notchling cost undo <claude|codex|agy> [--json]` | `cost.setup.undo` (app, acts) | none | 0 1 2 3 4 |
| `notchling secrets [words] [--json] [--socket <path>]` | `vault.find` (agent) | none | 0 1 2 3 4 |
| `notchling secret [--json] <name>[,<name>…] -- <command> [args…]` | `vault.use` (agent, acts) | by_entry | 0 125 126 127 |
| `notchling secret-request <name> [--kind token|login --url --note --how --expires YYYY-MM-DD] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `vault.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling settings list [--json]` | `settings.list` (agent) | none | 0 1 2 3 4 |
| `notchling settings get <key> [--json]` | `settings.get` (agent) | none | 0 1 2 3 4 |
| `notchling settings set <key> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `settings.set` (agent, acts), `settings.asked` (agent, acts) | by_key_class | 0 1 2 3 4 5 6 7 124 |
| `notchling switch on <acting|bridge|accounts|vault|install|cluster|machines> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `switch.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling switch off <acting|bridge|accounts|vault|install|cluster|machines> [--json]` | `switch.off` (agent, acts) | none | 0 1 2 3 4 |
| `notchling consent <id> [--wait] [--timeout <seconds>] [--json]` | `consent.get` (agent) | none | 0 1 2 3 4 5 6 124 |
| `notchling consents [--json]` | `consent.mine` (agent) | none | 0 1 2 3 4 |
| `notchling watch [--topic <topic>]... [--until <state>] [--count <n>] [--timeout <seconds>] [--json]` | `events.subscribe` (agent) | none | 0 1 2 3 4 124 |
| `notchling outbox [--json]` | `outbox.mine` (agent) | none | 0 1 2 3 4 |
| `notchling receipts [--json]` | none | none | 0 1 2 3 4 |
| `notchling activity [--kind <event>]... [--since <ms>] [--limit <n>] [--json]` | `activity.mine` (agent) | none | 0 1 2 3 4 |
| `notchling prompts [--json]` | `prompts.mine` (agent) | none | 0 1 2 3 4 |
| `notchling consent cancel <id> [--json]` | `consent.cancel` (agent, acts) | none | 0 1 2 3 4 |
| `notchling inbox [--json] [--socket <path>]` | `inbox.mine` (agent) | none | 0 1 2 3 4 |
| `notchling shells [--stuck] [--json] [--socket <path>]` | `shells.mine` (agent), `shells.stuck` (agent) | none | 0 1 2 3 4 |
| `notchling accounts [--json] [--socket <path>]` | `accounts.summary` (agent) | none | 0 1 2 3 4 |
| `notchling limits [--tool <claude|codex|agy> [--pool <name>]] [--json] [--socket <path>]` | `limits.summary` (agent) | none | 0 1 2 3 4 |
| `notchling orgs [--json] [--socket <path>]` | `organizations.mine` (agent) | none | 0 1 2 3 4 |
| `notchling search <words> [--kind pages|sessions] [--since <30m|24h|7d|date|seconds>] [--tool claude|codex|agy] [--project <folder>] [--account <value>] [--owner <value>] [--section <name>] [--state <name>] [--when today|<n>d|YYYY-MM] [--mode text|hybrid|semantic] [--limit <n>] [--json] [--socket <path>]` | `search.query` (agent) | none | 0 1 2 3 4 |
| `notchling switches [--json] [--socket <path>]` | `settings.switches` (agent) | none | 0 1 2 3 4 |
| `notchling awake [on|off] [--json] [--socket <path>]` | `awake.state` (agent) | none | 0 1 2 3 4 |
| `notchling versions [--latest] [--workers] [--json] [--socket <path>]` | `versions.get` (agent), `versions.latest` (agent), `versions.workers` (agent) | none | 0 1 2 3 4 |
| `notchling roles [--json] [--socket <path>]` | `catalog.roles` (agent) | none | 0 1 2 3 4 |
| `notchling chat open-file <path> [--session <id>] [--json] [--socket <path>]` | `chat.open_file` (agent, acts) | none | 0 1 2 3 4 |
| `notchling shells stop <id> [--json] [--socket <path>]` | `shells.stop` (agent, acts) | none | 0 1 2 3 4 |
| `notchling session reload <id|self> [--after-turn] [--json]` | `session.reload` (agent, acts) | own_free | 0 1 2 3 4 |
| `notchling session new <folder> [--model <name>] [--message <text>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.start` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling session reply <id> <text> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.say` (agent, acts) | own_free | 0 1 2 3 4 5 6 7 124 |
| `notchling session stop|interrupt <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.halt` (agent, acts) | own_free | 0 1 2 3 4 5 6 7 124 |
| `notchling session remove <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.discard` (agent, acts) | own_free | 0 1 2 3 4 5 6 7 124 |
| `notchling session answer <id> --answers <json> [--question <json>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.choose` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling session command <id> <command> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.run` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling session dialog <id> --pick <1-9>|--dismiss [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.pick` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling session chrome <id> on|off [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.browse` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling session options <id> <option> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `session.tune` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling cluster status [--json]` | `cluster.status` (agent) | none | 0 1 2 3 4 |
| `notchling cluster join --token-stdin [--json]` | `cluster.join` (agent, acts) | none | 0 1 2 3 4 |
| `notchling cluster reach <addr> <port>` | none | none | 0 1 2 |
| `notchling ask [--choice <text>]... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] -- <question>` | `ask.open` (agent, acts), `ask.wait` (agent) | none | 0 1 2 3 4 5 124 |
| `notchling asks [--wait] [--json]` | `ask.mine` (agent) | none | 0 1 2 3 4 124 |
| `notchling answer <id> (--choice <n> | -- <text>) [--json]` | `ask.answer` (agent, acts) | own_free | 0 1 2 3 4 |
| `notchling help [--json]` | none | none | 0 2 |
| `notchling capabilities [--json] [--socket <path>]` | `daemon.capabilities` (agent) | none | 0 1 2 3 4 |
| `notchling dispatch <machine> --tool <claude|codex|agy|shell> [--folder <path>] [--model <name>] [--effort <level>] [--text <prompt>] [--command <line>] [--secret <name>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `cluster.dispatch` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling jobs [--json] [--socket <path>]` | `jobs.mine` (agent) | none | 0 1 2 3 4 |
| `notchling job <id> [--wait] [--timeout <seconds>] [--json] [--socket <path>]` | `jobs.get` (agent) | none | 0 1 2 3 4 124 |
| `notchling job tail <id> [--from <offset>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `jobs.tail` (agent, acts) | own_free | 0 1 2 3 4 5 6 7 124 |
| `notchling job return <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `jobs.return` (agent, acts) | own_free | 0 1 2 3 4 5 6 7 124 |
| `notchling job stop <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `jobs.stop` (agent, acts) | own_free | 0 1 2 3 4 5 6 7 124 |
| `notchling cluster create <name> | notchling cluster token | notchling cluster invite --node <id> --name <name> [--role master|worker] [--acting on|off] | notchling cluster member <node> [--role master|worker] [--delegation on|off] | notchling cluster failover on|off|30m|1h | notchling cluster main <session> [on|off] | notchling cluster allow <node> <secret>... | notchling cluster remove <node> | notchling cluster move-home <node> | notchling cluster move-now; each takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `cluster.admin.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling accounts switch <account> | notchling accounts add claude|codex|agy | notchling accounts remove <account> | notchling accounts move-all <account>|main | notchling accounts move <session> <lane>|main | notchling lanes create <account> | notchling lanes remove <lane> | notchling lanes switch <lane> <account>; each takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `accounts.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling lanes list [--json] [--socket <path>]` | `lanes.summary` (agent) | none | 0 1 2 3 4 |
| `notchling facets <sessions|pages> <tool|project|account|section|owner|kind|state> [--prefix <text>] [--limit <n>] [--json] [--socket <path>]` | `search.facets` (agent) | none | 0 1 2 3 4 |
| `notchling semantic [--json] [--socket <path>]` | `embed.status` (agent) | none | 0 1 2 3 4 |
| `notchling semantic download <model> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `embed.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling machines list [--json] [--socket <path>]` | `machines.list` (agent) | none | 0 1 2 3 4 |
| `notchling machines check [<machine>] [--json] [--socket <path>]` | `machines.check` (agent) | none | 0 1 2 3 4 |
| `notchling machines plan <machine> [<step>] [--json] [--socket <path>]` | `machines.plan` (agent) | none | 0 1 2 3 4 |
| `notchling machines peers [--json] [--socket <path>]` | `machines.peers` (agent) | none | 0 1 2 3 4 |
| `notchling network list [--json] [--socket <path>]` | `network.list` (agent) | none | 0 1 2 3 4 |
| `notchling machines add <user@host> [--name <text>] | notchling machines import <peer> --user <user> | notchling machines addresses <machine> <address>... | notchling machines remove <machine> | notchling machines fix <machine> <step> | notchling machines setup <machine> | notchling machines running <machine> on|off | notchling network switch <tailnet>; each takes [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `machines.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling install plan [--what <a,b>] [--json] [--socket <path>]` | `install.plan` (agent) | none | 0 1 2 3 4 |
| `notchling install request [--part <id>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `install.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling cost profile set|setup|undo [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `cost.request` (agent, acts), `cost.profile.get` (agent) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling versions update <tool> [--to <version>] [--worker <machine>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | `versions.request` (agent, acts) | request | 0 1 2 3 4 5 6 7 124 |
| `notchling check-updates [on|off] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]` | none | by_key_class | 0 1 2 3 4 5 6 7 124 |

## Planned

| Op | Feature | Phase |
| --- | --- | --- |
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
| `catalog.get` | Read the model catalog | 2 |
| `log.verify` | Verify the log chain | 2 |
| `session.shell_output` | Read a session's shell output | 4 |
| `session.reload.state` | Read a reload's state | 4 |
| `session.held.discard` | Discard a held message | 4 |
| `codex.children` | Read a Codex session's children | 4 |
| `agy.sessions` | List Antigravity sessions | 4 |
| `agy.state` | Read an Antigravity session | 4 |
| `agy.conversation` | Read an Antigravity conversation | 4 |
| `agy.open` | Open an Antigravity session | 4 |

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
| `settings.set_acting` | app_ui |
| `bridge.set` | app_ui |
| `settings.set_switch` | app_ui |
| `session.new` | app_ui |
| `session.answer` | app_ui |
| `session.command` | app_ui |
| `session.dialog` | app_ui |
| `session.dialog_pick` | app_ui |
| `session.dialog_dismiss` | app_ui |
| `session.chrome` | app_ui |
| `embed.refresh` | app_ui |
| `embed.settings` | app_ui |
| `embed.remove` | app_ui |
| `embed.fetch` | app_ui |
| `embed.fetch.cancel` | app_ui |
| `agy.set_option` | app_ui |
| `folder.trust` | app_ui |
| `session.reply` | app_ui |
| `session.send` | app_ui |
| `session.interrupt` | app_ui |
| `session.stop` | app_ui |
| `session.remove` | app_ui |
| `codex.send` | app_ui |
| `codex.stop` | app_ui |
| `codex.delete` | app_ui |
| `agy.send` | app_ui |
| `agy.cancel` | app_ui |
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
| `outbox.list` | app_ui |
| `activity.list` | app_ui |
| `prompts.list` | app_ui |
| `outbox.edit` | app_ui |
| `outbox.remove` | app_ui |
| `outbox.hold` | app_ui |
| `outbox.resend` | app_ui |
| `outbox.discard` | app_ui |
| `cluster.side_copy.hide` | app_ui |
| `cluster.create` | app_ui |
| `cluster.join.token` | app_ui |
| `cluster.invite` | app_ui |
| `cluster.member.set` | app_ui |
| `cluster.remove` | app_ui |
| `cluster.main.set` | app_ui |
| `cluster.move_home` | app_ui |
| `cluster.move.now` | app_ui |
| `cluster.failover.set` | app_ui |
| `cluster.allow.set` | app_ui |
| `cluster.follow` | app_ui |
| `cluster.return` | app_ui |
| `accounts.switch` | app_ui |
| `accounts.add.begin` | app_ui |
| `accounts.remove` | app_ui |
| `accounts.move_all` | app_ui |
| `lanes.list` | app_ui |
| `lanes.create` | app_ui |
| `lanes.remove` | app_ui |
| `lanes.switch` | app_ui |
| `providers.switch` | app_ui |
| `providers.add.begin` | app_ui |
| `providers.remove` | app_ui |
| `session.move` | app_ui |
| `shell.stop` | app_ui |
| `machines.apply` | app_ui |
| `install.apply` | app_ui |
| `cost.profile.set` | app_ui |
| `versions.update_local` | app_ui |
| `tailscale.switch` | app_ui |
| `agy.sidecar.setup` | app_ui |
| `organizations.folders.preview` | app_ui |
