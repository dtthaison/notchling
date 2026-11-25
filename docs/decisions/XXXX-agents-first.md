# XXXX: Agents first: an agent path for every feature, the user's yes by request, every acting call recorded

## Context

Goal 6 ([goals](../goals.md)), approved on 2026-11-21: whatever a person can do in Notchling, an agent can do with a `notchling` command and an agent-tier op, with `--json`. Where the user must agree, the agent asks on a card and waits; it is never refused without a way to ask, and the guard rails stay.

Facts that bound the change:

- [0007](0007-local-api-tiers.md): two tiers, the app and the agent. Six switches (`acting`, the bridge, `accounts`, `vault`, `install`, `cluster`) are off at every daemon start, and only an app-tier call in that run turns one on, with two audited `cluster` exceptions for headless nodes (its note of 2026-11-14). Nothing an agent can write turns on what only the app tier may (its note of 2026-11-08).
- [0020](0020-agents-change-settings-by-class.md): agents change settings by class, and never the switches.
- Today a successful agent-tier call is recorded only when its handler records it (`settings.agent`, the vault's kinds, `session.ask`); `op.called` covers app-tier calls only. Refusals and failures are recorded for every tier.
- Three ways to ask for a yes exist (settings asks, vault requests, Ask), with three timeouts. A settings ask is refused when no app follows the Inbox; a vault request needs the vault switch on, so with it off an agent can't ask for a secret at all; Ask needs `acting` on.
- The review of the goal's audit: once every guarded area has a request path, a prompt-injected agent can reach each of them up to the card. The card becomes the only barrier.

The design: [2026-11-21-agents-first-design.md](../superpowers/specs/2026-11-21-agents-first-design.md).

## Decision

1. **An agent path for every feature.** Every op is reachable by a `notchling` command, planned for a named phase, user-only, or internal (only Notchling's own processes call it) with a reason. A registry in `crates/notchlingd/src/commands.rs` says which; `notchling help --json` and `notchling capabilities --json` print it; the daemon's refusals carry its reason; the guard check `GUARD-AGENT-PATH` holds it, and the planned and internal lists only shrink without the maintainer.
2. **One consent request.** A feature op that needs the user's yes opens a saved request (the `consents` table of `notchling.db`, on the node the agent runs on) and replies at once; the agent waits or checks later by id. It opens with `acting` off, and with no app attached it is saved and waits *(decided)*. Settings asks and vault requests fold into it; vault approvals and Ask keep their own flows. The card follows the design's consent card rules: facts the daemon computed, the agent's text shown as untrusted and deleted at close, Allow once, no "always allow", the scope recorded and checked at use (a kind with files may be narrowed by the user, never widened), the facts checked again before applying, an end time (15 minutes by default, 12 hours at most, more than 1 hour only for a verified session), and a rate limit per session or, for a caller in no session, per the app it runs under. Only the person answers: the Mac's app, or a paired phone for the kinds marked so, which are settings asks and actions that only stop something; a phone may always Deny *(decided)*. No agent answers a consent card, the main session included.
3. **Power is applied by the app.** For the kinds that grant power (switches, cluster admin, accounts, machines, Set Up, Cost Profile, updates) and for secret requests, the agent tier gets an op that opens a request, never the op that acts. On the user's Allow, the app calls its existing app-tier op with the request's id; the daemon checks the id against the scope and uses it up. A secret request never turns the Vault on: with the vault off its card can't be allowed until the user turns the Vault on, by their own act or a switch request of its own.
4. **The switch rule.** An agent may *request* a switch on, through an Inbox consent card that only the Mac's app can allow. Only the app, on the user's yes, turns it on, by its own `settings.set_acting`, `bridge.set` or `settings.set_switch` call citing the request (on a Worker, the home's `cluster.member.set` for the switches the cluster may set). No tier turns a switch on from a request by itself. Every switch is still off at every daemon start: a yes lasts for that run, and one not yet used at a restart is withdrawn *(decided)*. An agent may turn a switch off without asking; that is recorded and the app shows a notice *(decided)*. Whether `cluster` off on a node with no app attached should need a request is asked separately in the design; this point stands until the maintainer answers.
5. **Every acting agent call is recorded** *(decided)*. The server writes one `agent.called` line for every agent-tier call that changes, runs, sends, uses a secret, opens a request or dry-runs one, whether it succeeds, is refused, fails or waits, and whether or not its handler records a kind of its own. It names the tool, the session and whether the daemon vouches for it, and keeps the evidence for the agent's label; the agent inside the session follows as `agent.named`, a label from the transcript. Both are `Lock::Always`: no setting, switch, log toggle or environment variable turns them off. Reads are not recorded one by one.
6. **Identity from the kernel.** The daemon works out the caller once per connection: the tool (Claude Code, Codex, Antigravity, a shell), the session, and whether it is a verified session (a Claude Code whose executable lies under an install folder and whose session file names it, or a session the daemon itself started). Only a verified session owns anything (its requests, the sessions and jobs it started), and only a verified session has an organization: the one of the session's folder, never the command's working folder. Anyone else reads nothing organization-scoped and is never taken for Personal.
7. **User-only.** These have no request and refuse every agent with reason `never`: answering a permission prompt or loosening a session's permissions; removing hooks or the guard, or turning Plan-Only Main Chat off; deleting the Audit or starting a new chain; pairing or revoking a phone; an account of another organization; answering a consent card, an Inbox item or the user's question; reading a secret's value outside `secret --`; promote, take back and continue here; the Never settings keys.
8. **One command contract.** One exit-code table for every command (0 ok, 1 failed, 2 bad request, 3 not running, 4 refused, 5 denied, 6 pending, 7 busy, 124 expired, and 125 for `secret` when it stopped before running its command), one JSON error shape on stdout with a reason from a closed list and nothing on stderr with `--json`, and common flags on every acting command (`--reason`, `--wait` or `--no-wait`, `--timeout`, `--expires-in`, `--key`, `--dry-run`). Today's codes move to it once, with the skills in the same change *(decided)*.

This amends [0007](0007-local-api-tiers.md): "Only an app-tier call in that run turns one on" stays, and an agent may now request it; "Privileged calls are audit-logged" now covers every acting agent call. It amends [0020](0020-agents-change-settings-by-class.md): the switches stay out of `settings.set`, and a switch request is a separate op; an Ask key no longer fails when no app is attached, it waits.

## A separate choice: keep the cluster on at start

Decided apart from the switch rule above: **no, not now** *(decided 2026-11-21)*. Nothing of it is built; the reasoning below is kept so it can be picked up again.

**The option.** A Settings choice, Keep Cluster On at Start. On the user's click the app writes a Keychain item whose access list holds only the signed app and the signed daemon in `/Applications/Notchling.app`. At daemon start, when the item is there and matches this node and its cluster, the daemon turns `cluster` on before any app attaches, audited with a cause of its own.

**What it would buy.** A member Mac that restarts without its app open rejoins its cluster, so dispatches to it keep working. Today the app restores the switch when it attaches (`restored_at_launch`), and a headless member already rejoins at start (0007's note of 2026-11-14). The gap is a member joined from the app whose app isn't running.

**Why not now** (the recommendation the maintainer took):

- It would be the first stored consent that turns a switch on at start on a Mac that has an app: "nothing at rest turns on what only the app tier may" would hold only as far as the Keychain item does, beyond the narrow headless exception.
- The Keychain item is not an anchor an agent can't reach here. Builds on this Mac are signed with the Apple Development identity by scripts agents run (`NOTCHLING_SIGN_IDENTITY`), so an agent could sign a binary that meets the same signing requirement. Ad hoc builds, like the 2.0 release, lose access to the item on every update, and a Keychain read that waits for a prompt on the daemon's start path is the bug [data-and-logs.md](../notes/data-and-logs.md) records.
- The cost of a forged item is high: with `cluster` on and no app attached, the home may set `acting`, the bridge and `install` on this Mac (0007's note of 2026-11-14), so the cluster's machines would have the run of it at boot without the user.
- The gap has fixes that add no anchor: open Notchling at login, so the app restores the switch as it attaches, or join the machine headless.

Revisit when the app tier checks a code signature made with a key no agent on this Mac can use (0007's Phase 6 follow-up). If it is picked up again, it should be only `cluster`, never the other five; set only by the user's click in Settings, never through a request; bound to this node, its cluster and the home's pinned certificate; ended after 30 days and at any change to the cluster; read off the start path so it never holds the socket; audited at every start; and shown in Settings with Turn Off.

## Consequences

- An agent can do what the user asks of it in a chat, end to end, and stops only where the user's yes is needed or the action is user-only. It can find out which with `capabilities` instead of trying.
- A misled agent can raise a card for anything that has a request. The card's rules narrow the risk; what remains is a user who allows a clear card by reflex. A phone may allow only settings asks and stops, so the forged-phone risk accepted in 0007's note of 2026-11-13 can't run code, reach a machine or grant power.
- The Activity store and the Audit grow by one line per acting agent call. The Audit's caps and the per-session limits bound it; Activity gets an Agents filter.
- Scripts and old skill text that read today's exit codes read some of them wrongly after the move. The skills follow the app and change in the same merge, and every error line names its code.
- The switches keep their strength: off at every start, on only by the app's call in that run.
- Built in nine phases ([the design](../superpowers/specs/2026-11-21-agents-first-design.md#phases)); phase 1 brings points 1, 2, 5, 6 and 8, phase 3 point 4, and the later phases the request kinds of point 3.

## Status

Accepted. The maintainer decided on 2026-11-21, each at the design's recommendation: the switch request and off at every start (point 4), turning a switch off without asking (point 4), every acting call recorded with no way to turn it off (point 5), one exit-code table with today's codes moved once (point 8), a phone allowing only the kinds marked for it (point 2), a request saved when no app is attached (point 2), and not keeping the cluster on at start (the separate choice). The rest, points 1, 3, 6 and 7 and the card's details in point 2, are the designer's calls, settled when phase 1 merges. Amends [0007](0007-local-api-tiers.md) and [0020](0020-agents-change-settings-by-class.md).

## Date

2026-11-21
