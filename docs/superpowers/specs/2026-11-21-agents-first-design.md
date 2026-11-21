# Agents first: every feature has an agent path

- **Date:** 2026-11-21
- **Status:** Draft for the maintainer's review. Nothing here is built. The calls only the maintainer can make are in [Needs the maintainer](#needs-the-maintainer), each with a recommendation.
- **Source:** goal 6 in [docs/goals.md](../../goals.md) and "Agents first" in the root `AGENTS.md` (branch `agent/goal-agents-first`, 803e1b3c), approved by the maintainer: "do all for agent friendly develop and use notchling". The audit of what an agent can do today and Fable's review of it (both kept with the job, not in git). The review's corrections, missing features, cross-cutting pieces, safety clashes and its 12-step order are taken as given; this design groups the 12 steps into phases and settles what the review left open.
- **Base:** `origin/agent/goal-agents-first` 803e1b3c (on `agent-run-oss` 5f993c28).
- **Touches:** decisions 0007 (tiers, switches, files at rest), 0019 (agent instructions per tool), 0020 (settings by class), 0022 (Ask), the typed event stream (XXXX), skills follow the app (XXXX), organizations (XXXX), cluster v1 nodes (XXXX). It needs a decision record of its own: [XXXX-agents-first](../../decisions/XXXX-agents-first.md), written with this design.

## In short

1. Whatever a person can do in Notchling, an agent can do with a `notchling` command over an agent-tier op, with `--json`. Where the user must say yes, the command opens a **consent request**: a card in the Inbox (and in Chat under the asking session). The agent waits, or gets an id and checks later. It is never refused without a way to ask.
2. The card is now the only barrier between a misled agent and every guarded area. So it shows only facts the daemon worked out, shows the agent's own words as untrusted, allows once, has no "always allow", records what the yes covered, ends at a set time, and is rate limited per session ([Consent card rules](#5-consent-card-rules)).
3. A short list stays with the user, whoever asks: answering a permission prompt, removing hooks or the guard, deleting the Audit or starting a new chain, pairing a phone, an account of another organization, and a few more ([User-only actions](#6-user-only-actions)).
4. Every acting agent call is recorded, with who made it (tool, session, agent), and nothing turns that record off. Activity gets an Agents filter.
5. The switch rule changes in one way: an agent may *ask* for a switch to go on. Only the app, on the user's yes, turns it on, and every switch is still off at every start. Turning one off needs no yes.
6. One exit-code table and one JSON error shape for every command. `notchling help --json` lists every command, its ops, tier and consent; `notchling capabilities --json` says what this caller may do right now.
7. A guard check holds it: every op is reachable by a command, planned for a named phase, or on the user-only list with a reason; the planned list only shrinks; a new design names its agent path.
8. Nine phases, each merging alone, in order. Phase 1 builds the record, the consent request and the contract. Phases 2 to 9 add the features family by family.

## Phases

| Phase | Steps | What it adds | Consent |
| --- | --- | --- | --- |
| 1 | 1 to 3 | The agent call record and identity, one consent request (settings asks and secret requests fold into it), the CLI contract and discovery, the guard check | the mechanism |
| 2 | 4, 5 | Free reads (search, own Inbox items, accounts and limits redacted, organizations, switches, awake, versions, outbox, own activity, prompts, roles) and `notchling watch` | none |
| 3 | 6 | Switch requests | Mac only |
| 4 | 7 | Sessions: new, reply, stop, interrupt, command, dialog, reload, Chrome, Codex and Antigravity turns | free for own, request for others |
| 5 | 8 | Jobs: dispatch claude, codex, agy and shell; show, wait, tail, return, stop | request to dispatch, free for own jobs |
| 6 | 9 | Cluster admin: create, invite, members, failover, main, allowed secret names, remove, move home | Mac only |
| 7 | 10 | Accounts and lanes: switch, add, remove, move all | Mac only, same organization |
| 8 | 11 | Machines, Tailscale network, Set Up, Cost Profile apply, tool updates | Mac only, per file |
| 9 | 12 | Search data, hybrid search, and the skills for every command family | none |

Each phase merges alone: it ships its commands, ops, skill text, tests and docs together, and shrinks the planned list by what it delivered. A later phase never finishes half of an earlier one.

## Contents

1. [Why](#1-why)
2. [Goals and non-goals](#2-goals-and-non-goals)
3. [What exists today](#3-what-exists-today)
4. [Terms](#4-terms)
5. [Consent card rules](#5-consent-card-rules)
6. [User-only actions](#6-user-only-actions)
7. [Phase 1: the record, one consent request, the contract](#7-phase-1-the-record-one-consent-request-the-contract)
8. [Phase 2: free reads and `notchling watch`](#8-phase-2-free-reads-and-notchling-watch)
9. [Phase 3: switch requests](#9-phase-3-switch-requests)
10. [Phase 4: sessions](#10-phase-4-sessions)
11. [Phase 5: jobs](#11-phase-5-jobs)
12. [Phase 6: cluster admin](#12-phase-6-cluster-admin)
13. [Phase 7: accounts](#13-phase-7-accounts)
14. [Phase 8: machines, Set Up and Cost Profile apply](#14-phase-8-machines-set-up-and-cost-profile-apply)
15. [Phase 9: search data, hybrid search and skills](#15-phase-9-search-data-hybrid-search-and-skills)
16. [Not yet audited](#16-not-yet-audited)
17. [Agent path](#agent-path)
18. [Risks](#18-risks)
19. [Needs the maintainer](#needs-the-maintainer)

## 1. Why

Agents do more and more of the work on this Mac, and Notchling is where that work meets the user's accounts, machines, secrets and sessions. Today an agent can change a setting, use a secret, ask a question and look at the cluster, and little else: about 34 of the 58 features in the audit have no agent path. The rest says "do this in the app", so the agent stops and the user relays. Goal 6 makes that unfinished work.

The review found the pieces that every feature would otherwise rebuild: a record of what agents did, an identity for the caller, one way to ask for the user's yes, one way to report errors, and a way to discover what is there. It also found the danger: once every guarded area has a request path, a prompt-injected agent can reach all of them up to the card. The card has to carry that weight.

## 2. Goals and non-goals

Goals:

- An agent path for every feature: a command, an agent-tier op, `--json`, a skill line, and a test that drives it as an agent would.
- One consent request for everything that needs the user's yes, which works with `acting` off and with no app attached.
- The guard rails stay: no agent gets a way around the user's yes, and the user-only list stays user-only.
- An agent can find out what it may do, and why not, without reading the source.

Non-goals:

- Agents answering for the user. A yes comes from the person, on the Mac or, for the kinds marked so, a paired phone. No agent answers a consent card, the main session included.
- A standing grant. No "always allow", no "don't ask again", no allow-list an agent can grow.
- Vault approvals (a use of an "Ask me first" secret). They keep their own flow and their Always Allow ([0007](../../decisions/0007-local-api-tiers.md)'s note of 2026-11-13); they are a check the user set on a secret, not an agent's request for new power.
- Questions. Ask stays as it is ([0022](../../decisions/0022-ask.md)): a question is not a consent, and the main session may still answer the questions that opted in.
- An agent posting notifications. Ask covers telling the user something that needs an answer; free-form agent text in front of the user invites "press Allow on the next card".

## 3. What exists today

With the review's corrections:

- **Tiers.** `OpSpec` is `AGENT`, `APP`, `APP_QUIET` or `APPROVAL` (`crates/local-api/src/server.rs`). Agent-tier ops today: `sessions.list`, `settings.list/get/set/asked`, `settings.switches`, `settings.get_acting`, `catalog.get`, `models.check/update`, `cost.summary`, `cost.report`, the Cost Profile's reads (its preset and picks are Ask keys), `versions.get`, `cluster.status`, `cluster.join`, `cluster.dispatch` (delegate jobs only), `pages.list/find`, `search.query` (no command yet), `log.verify`, `agy.sessions`, `ask.open/wait/mine/answer`, `session.reload`, `session.question`, `vault.find/use/request`, the hook's `hook.request`, and Antigravity's sidecar and hook ops.
- **Records.** `op.called` covers app-tier calls only (`conn.rs::record_generic`). A successful agent call records nothing unless its handler does (`settings.agent`, `vault.*`, `session.ask`). Refusals and failures are recorded for every tier (`api.refused`, `op.failed`).
- **Identity.** At `hello`, an agent-tier connection gets a Claude Code session id by walking the peer's parents (`caller_session.rs`, 16 steps, an installed Claude binary). Nothing names a subagent, a Codex or Antigravity session, or a shell. Ask finds the agent inside a session from its transcripts (`ask_find.rs`), as a label.
- **Consent.** Three mechanisms: settings asks (Inbox, 300 s, refused at once with no app following the Inbox, at most 8), vault requests (need the vault switch on, so with it off an agent can't even ask), Ask (needs `acting` on and a Claude Code session). Three timeouts and no "open, get an id, check later".
- **Commands.** Exit codes differ per command: 3 is "not running" for `status`, "not allowed" for `settings`, "refused or no daemon" for `ask`, "denied" for `secret`. `--json` errors are prose on stderr. `secret` and `secret-request` have no `--json`. No `help --json`, no capabilities op.
- **Switches.** Six, off at every daemon start; only an app-tier call in that run turns one on, with the two audited `cluster` exceptions of [0007](../../decisions/0007-local-api-tiers.md)'s note of 2026-11-14. The app restores the user's choice when it attaches (`restored: true`, cause `restored_at_launch`).

## 4. Terms

- **Agent path.** A `notchling` command, the agent-tier op it calls, `--json` output, and a skill line that says when to use it.
- **Acting call.** An agent-tier call that changes something, runs something, sends something, uses a secret or opens a request. Reads are not acting calls.
- **Identity.** Who made a call, worked out by the daemon from the kernel: the tool, the session, and whether the daemon can vouch for it. The agent inside a session (main or which subagent) is a label from its transcript.
- **Verified session.** A Claude Code session whose process is an installed Claude binary and whose `sessions/<pid>.json` names it, or a session the daemon itself started (a Codex or Antigravity session Chat runs). Only a verified session owns anything: its requests, the sessions and jobs it started.
- **Consent request** (in short, a request). A saved request for the user's yes to one action, shown as a card. Its **kind** says what it may do; its **scope** is exactly what a yes covers.
- **User-only.** An action no agent may do or request. Its op refuses with reason `never`.

## 5. Consent card rules

Every consent card follows these rules, in every kind and every phase. A test per rule holds them ([7.4](#74-phase-1-tests)).

1. **Daemon-computed facts only.** The title, the action, the target, what changes (now and after), what it unlocks, who asks and from where are worked out by the daemon from its own state: the session list, the switch states, the settings key's spec, the vault's list, the cluster state, the plan of a file write. The agent passes only a kind, a target and typed arguments, which the daemon checks against the kind's spec. The words come from `.ftl` files, never from the agent.
2. **Agent text is shown as untrusted.** The only free text an agent supplies is its reason (`--reason`, at most 300 characters), and, where the action itself is text (a message to a session, a prompt, a shell command), that text. Both are shown in a box labelled "The agent says" or "The command", in plain text: no links, no Markdown, line breaks kept, nothing cut but the reason. Agent text never appears in a title, a button, the Inbox item, a notification, the phone or an event.
3. **Who asks, honestly.** The card names the tool and the session as the daemon knows them ("Claude Code · notchling · agent/agents-first"), marks a verified session as such, and shows the agent's label only as "as its transcript says". A caller in no session is "A program outside any agent session".
4. **Allow once.** The answers are Allow and Deny. Allow applies the action once, within the scope, and is used up. There is no "always allow", no "don't ask again", no "allow all".
5. **Scope recorded.** At open the daemon writes the scope: the kind, the target, the exact change, the session, the organization and the end time. Applying checks the action against the scope exactly. The scope, the answer, who gave it and the use are kept in the `consents` table and recorded as events.
6. **What you saw is what happens.** The daemon checks the facts again when the yes is applied. If they changed (the setting's value, the session ended, the file changed, the switch is already on), the request is withdrawn, not applied.
7. **An expiry.** Every request ends: 15 minutes by default, 1 minute to 12 hours with `--expires-in`. The card shows the time left. An answer after the end is refused, and the app disables Allow at the end.
8. **A per-session rate limit.** At most 3 requests waiting per session and 20 in all; at most 10 opened per session in a rolling hour; after a Deny, the same kind and target from that session is refused for 30 minutes. Callers in no session share one bucket: 1 waiting, 3 an hour. A retry with the same `--key` returns the same request and counts once.
9. **No reflex.** Allow is never the default button, Return doesn't press it, and it is enabled 1 second after the card is shown. Cards are answered one at a time.
10. **Only the person answers.** On the Mac, from the app (`ClientKind::App`, the process that said hello). A paired phone may Deny any request, and may Allow only the kinds marked "Mac or phone". No agent answers a consent card, the main session included: there is no agent-tier answer op.
11. **Power is applied by the app.** For the kinds that grant power (switches, cluster admin, accounts, machines, Set Up, Cost Profile, updates), the agent tier never gains the op that acts. Its op opens a request; the app performs the existing app-tier op, citing the request, when the user presses Allow.

## 6. User-only actions

These stay with the user. Their ops refuse every agent with `forbidden`, reason `never` and "Only you can do this, in Notchling.", and no consent kind exists for them. `help --json` lists them so an agent stops asking.

| Action | Ops | Why |
| --- | --- | --- |
| Answering a permission prompt | `session.permission`, `agy.permission`, the hook's decisions | The goals' never list. A prompt is the user's own check on an agent; an agent answering it removes the check. |
| Removing hooks or the guard | `install.remove`, the `costGuard` key, Notchling's hook entries | An agent would remove the check on itself. |
| Deleting the Audit or starting a new chain | `log.audit.delete`, `log.chain.start` | An agent would erase its own trail. |
| Pairing a phone, and revoking one | `bridge.pair.begin`, `bridge.pair.answer`, `bridge.revoke` | A paired phone answers cards: an agent could pair a device of its own and approve itself. Revoking cuts the user off their phone. |
| An account of another organization | any account op whose target is outside the caller's organization | The goals' never list. Refused before any card is raised. |
| Answering a consent card, an Inbox item or the user's question | `consent.answer`, `inbox.answer`, `ask.dismiss` | The card is the user's yes. |
| Reading a secret's value outside `secret --` | `vault.list`, `vault.save`, `vault.admin` | Values never reach an agent ([0007](../../decisions/0007-local-api-tiers.md)). |
| Promote, take back, continue here | `cluster.promote`, `cluster.take_back`, `cluster.continue_here`, `session.take_back` | The cluster design: never an agent. They change which machine drives the user's sessions. |
| The Never settings keys | logins, secrets, `codexNetwork`, the app's own records | [0020](../../decisions/0020-agents-change-settings-by-class.md). |
| Keeping a switch on at start | the Settings choice, if the maintainer adopts it | See the decision record's separate option. A request never asks for it. |

## 7. Phase 1: the record, one consent request, the contract

Phase 1 is steps 1 to 3 of the review. It adds no new feature for agents beyond the ones folded in; it builds what every later phase uses. It merges as one change, in the order 7.1, 7.2, 7.3, each with its tests.

### 7.1 Step 1: the agent call record and identity

**Op classes.** `OpSpec` gains `acts`. The `AGENT` constant goes; every agent-tier op is one of:

| Spec | Meaning | Recorded |
| --- | --- | --- |
| `AGENT_READ` | reads only | refusals and failures only, as today |
| `AGENT_ACTS` | changes, runs, sends, uses a secret, or opens a request | every call, by `agent.called` |
| `AGENT_HOOK` | Notchling's own hook and sidecar processes (`hook.request`, `agy.hook`, `agy.sidecar.take`, `agy.sidecar.report`) | by their own kinds, as today |

A test fails until every agent-tier op in `wire.rs` has one of them, as the quiet-op test does. Today's agent ops, classified:

- `AGENT_READ`: `sessions.list`, `settings.list`, `settings.get`, `settings.asked`, `settings.switches`, `settings.get_acting`, `catalog.get`, `models.check`, `cost.summary`, `cost.report`, `cost.profile.get`, `cost.setup.plan`, `cost.setup.check`, `versions.get`, `cluster.status`, `pages.list`, `pages.find`, `search.query`, `log.verify`, `agy.sessions`, `ask.wait`, `ask.mine`, `vault.find`, and the new `consent.get`, `consent.mine`, `daemon.capabilities`.
- `AGENT_ACTS`: `settings.set` (the Cost Profile's preset and picks are Ask keys, so they come through here), `models.update`, `cluster.join`, `cluster.dispatch`, `session.reload`, `session.question`, `ask.open`, `ask.answer`, `vault.use`, `vault.request`, and the new `consent.cancel`.
- `AGENT_HOOK`: `hook.request`, `agy.hook`, `agy.sidecar.take`, `agy.sidecar.report`.

**Identity.** At `hello`, the handler works out the caller's identity once (`Handler::identity_of(pid)`, replacing `session_of`), and the connection keeps it. CLI connections make one call, so one walk per command. `notchlingd/src/identity.rs` builds it over `caller_session.rs` and `process_facts.rs`:

| Field | From | Notes |
| --- | --- | --- |
| `tool` | the walk | `claude`, `codex`, `antigravity`, `shell` or `unknown`. A process on the walk that is this `notchling` running `delegate codex` or `delegate agy` makes it that tool, delegated. A process the daemon started for a Codex or Antigravity session makes it that tool. A Claude Code session found as today makes it `claude`. A tool found only by its binary's name is that tool but not verified. Nothing found: `shell`. |
| `session` | the walk | The Claude Code session id (as today), or the id of the Codex or Antigravity session the daemon started (from its own launch record: pid to session), or none. A delegated job's session is the delegating session. |
| `verified` | the walk | True only for a verified session (see [Terms](#4-terms)). |
| `folder` | the session | The session's folder from the daemon's session list, never the command's working folder. |
| `organization` | the folder | The organization whose folders hold the session's folder (longest match), else Personal. A caller with no session has none. |

Ownership follows only from a verified session: an unverified caller is treated as `shell` for every "own" rule, whatever its tool label says.

**Events.** In `crates/events/src/kinds/agents.rs`:

- `agent.called`: one line per acting agent call, written by the server (`conn.rs`), not by the handler, and also when the handler recorded its own kind. Fields: `op` (Op), `tool` (new choice `AgentTool`), `verified` (bool), `result` (new choice `AgentResult`: `ok`, `refused`, `failed`, `pending`), `code` (Option ErrorCode), `ms` (Millis), `consent` (Option of a new `Ref<Consent>`), `organization` (Option of a new `Ref<Organization>`). The envelope's actor carries the tier, the executable and the session, as today. Title "Agent Actions", `lock: Lock::Always("Every acting agent call is part of the audit trail.")`, sinks Audit and Activity, `Durability::CommitFromAgent`, `agents_read: Own`, `keep_days: 0`.
- `agent.named`: the label of the agent inside the session, found after the call. Fields: `asker` (AskerKind), `agent` (Option of a new `Ref<Agent>`, the transcript's agent id). It carries the call's `op_id`, passed to the finder's thread, so Activity joins the two. Only for calls from a verified Claude Code session; the finder is `ask_find`'s, run off the op thread, one at a time per session, at most 8 queued and the rest dropped as `unknown`. Same lock and sinks. The label's text is never in it: Activity reads the label from the transcript by the agent id, as Chat does for subagents.
- `agent.consent`: see [7.2](#72-step-2-one-consent-request).

Nothing turns these off: they are `Lock::Always`, no setting or switch names them, no `NOTCHLING_*` variable is read for them, and a test turns every log toggle off and still finds them. Reads by agents stay unrecorded one by one, as `settings` reads are today.

**Activity.** `activity.list` takes `kinds: ["agents"]`: every row whose actor tier is agent, whatever its kind. Settings > Activity gets an Agents filter; a row shows the tool, the session's name (from the session list), the agent's label (from the transcript, marked "as its transcript says"), the op's title and the result.

**Files.**

- `crates/local-api/src/server.rs`: `OpSpec { acts }`, the three agent constants, `Handler::identity_of` returning a small `AgentIdentity` (tool, session, verified, organization), which the scope's actor and the record use.
- `crates/local-api/src/conn.rs`: keep the identity from `hello`; record `agent.called` for every `AGENT_ACTS` call outside the `take_recorded()` branch, for ok, refused and failed alike.
- `crates/local-api/src/record.rs`: `Shared::agent_called`.
- `crates/events/src/kinds/agents.rs`, `choices.rs`, `field_ref.rs` (ref kinds `Agent`, `Consent`, `Organization`); the regenerated `tests/golden/log-kinds.json` and `docs/log-kinds.md`.
- `crates/notchlingd/src/identity.rs` and `identity_tests.rs`; `caller_session.rs` (the walk returns the chain it saw); `codex_ops.rs`, `agy_ops.rs` (expose pid to session for the processes they started); `daemon_wiring.rs` (`identity_of`); `agent_named.rs` (the finder follow-up).
- Every `*_ops.rs` with an agent op: `AGENT` becomes `AGENT_READ`, `AGENT_ACTS` or `AGENT_HOOK`.
- `crates/notchlingd/src/activity_ops.rs` and `crates/engine/src/activity/` (the `agents` filter); the app's Activity page and its `.ftl` words.

### 7.2 Step 2: one consent request

**What it is.** A row in a new `consents` table of `notchling.db` (schema 9 to 10, class Node: a node's requests stay on it and reach the cluster only as an Inbox item; the daemon is its only reader and writer; an older daemon ignores the table). A feature op that needs the user's yes from an agent opens one and replies at once with the request. The agent waits on it or checks later.

**Columns.** `id` (`c` and 32 hex characters, random), `kind`, `target`, `change` (JSON, the daemon's before and after), `facts` (JSON: the card's lines as message ids with typed arguments), `reason` (the agent's text, at most 300 characters), `session`, `tool`, `verified`, `folder`, `organization`, `pid`, `pid_start` (the asking process's start time), `key` (the caller's idempotency key), `state`, `answer_on` (`mac` or `mac_or_phone`), `applied_by` (`daemon` or `app`), `created_ms`, `expires_ms`, `answered_ms`, `by` (`mac`, `phone`), `used_ms`, `used_op_id`, `closed_ms`. A closed row is deleted 7 days after it closed, its reason with it; nothing waiting expires but by its own end time.

**States.** `waiting`, then `allowed` (taken, being applied), `applied`, `failed` (allowed, and applying failed), or `denied`, `expired`, `cancelled` (the agent withdrew it), `withdrawn` (the facts changed, the target is gone, or an app-applied yes wasn't applied within 60 seconds). The first answer wins: a mark in memory under one lock, then one `UPDATE … WHERE state = 'waiting'`, as Ask does.

**Kinds.** A kind is a `KindSpec` in `crates/engine/src/consent/kinds.rs`: its name, title, the typed arguments it takes and how they're checked, how the facts are computed and checked again, who answers (`answer_on`), who applies (`applied_by`), the Inbox kind it raises, and its default and longest expiry. Phase 1 has two, the ones it folds in; each later phase adds its own.

| Kind | Phase | Target | Answered on | Applied by |
| --- | --- | --- | --- | --- |
| `setting` | 1 | an Ask-class settings key and value | Mac or phone | the daemon, as settings asks today |
| `secret_request` | 1 | a Vault entry to add or update | Mac (the form takes the value); a phone may Deny | the app, by `vault.save {request: id}` |
| `switch` | 3 | one of the six switches, on | Mac | the app |
| `session` | 4 | start a session; act on a session the agent didn't start | Mac or phone | the daemon |
| `dispatch` | 5 | a job on a machine | Mac or phone; Mac only with secrets | the daemon |
| `job` | 5 | tail, return or stop another session's job | Mac or phone | the daemon |
| `cluster` | 6 | a cluster admin action | Mac | the app |
| `account` | 7 | an account or lane action, same organization | Mac | the app |
| `machine` | 8 | add, fix, update or remove a machine; the Tailscale network | Mac | the app |
| `install` | 8 | Set Up files, ticked one by one | Mac | the app |
| `cost_profile` | 8 | Cost Profile set, apply, undo, ticked one by one | Mac | the app |
| `update` | 8 | a tool update on this Mac or a worker | Mac | the app |

**Ops.**

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `consent.get` | agent, read | `{id, waitMs?}` | `Consent`; with `waitMs` (0 to 50,000) it waits until the state leaves `waiting` or the time ends |
| `consent.mine` | agent, read | `{states?: [..]}` | `{requests: [Consent]}`: the caller's session's requests, newest first |
| `consent.cancel` | agent, acts | `{id}` | `{cancelled: bool, state}` |
| `consent.list` | app, quiet | `{}` | `{requests: [Consent]}`, every request, waiting first |
| `consent.answer` | app | `{id, decision: "allow" \| "deny"}` | `{answered: bool, state, by?}`; Allow only for `applied_by: daemon` kinds |

There is no generic `consent.open`. A feature op opens the request itself, because it computes the facts: `settings.set` for an Ask key and `vault.request` in phase 1. Its reply holds `request: Consent`, beside the fields it has today. App-applied kinds are allowed by the app calling the real op with `consent: id`: the daemon checks that the request is `waiting`, that the op and its arguments equal the scope, that it hasn't ended, and that the caller is the app that said hello; then it records the answer (`by: mac`) and the use in one step and runs the op.

`consent.get` and `consent.cancel` are served to the asking session (the caller's verified session equals the row's) or, for a request from no verified session, to the asking process only (pid and start time equal the row's). Anyone else gets `forbidden` "Only the agent that asked can see this request."

`Consent` on the wire: `{id, kind, target, change, facts, state, answerOn, appliedBy, createdMs, expiresMs, answeredMs?, by?, usedMs?, session?, tool, verified, organization?, scope: {once: true, session?, until}, reason?}`. `reason` goes back only to the asker and the app.

**Limits and refusals.** The rate limits of [5](#5-consent-card-rules), rule 8, are checked at open, in `crates/engine/src/consent/rules.rs`, under the book's lock. Over a limit: `failed` with `data: {reason: "busy", retryAfterMs}`. In the cool-down after a Deny: the same, with the time left. A same-`key` retry from the same session returns the existing request. The checks for the action itself come first: an action on the user-only list is refused with reason `never` before any limit is counted; a target in another organization with `other_organization`.

**With `acting` off and no app.** A request opens with `acting` off (it may be the request to turn it on) and with no app attached: it is saved, the reply says `waiting`, and the card appears when the app attaches. Only the Mac's app and, for the kinds marked so, a phone with the bridge and `acting` on can answer.

**The Inbox item.** One per waiting request, raised at open: kind `consent` for the new kinds (a phone treats a kind it doesn't know as `notice`, so the protocol changes only in a description); the two folded kinds keep `setting_change` and `secret_request`. Id `consent:<id>:<ms>`, fresh on every raise. Title and detail are the daemon's facts in words (`<who> asks to <action>`, cut to 80 and 400 characters); the reason is never in the item. Actions: Deny (`deny`, from the Mac or a phone); Allow (`allow`), from the Mac, and from a phone only for `mac_or_phone` kinds; for app-applied kinds Allow is Mac-only and opens the app's card, as Answer in Chat does. The responder only marks; the consent loop applies, closes and withdraws. A request that ends resolves its item; an item that loaded Withdrawn after a restart is replaced by a fresh raise of the request, which is still waiting.

**Waiting and restarts.** A waiting request survives a restart: `ConsentOps::load` runs between bind and serving, after the Inbox's load, records `restored` and raises its item again. An allowed request not yet applied at a restart is withdrawn (a yes is for this run). A 1 s tick ends requests past their end as `expired`. The command long-polls `consent.get` (50 s a call) and reconnects with the same id after a restart, as `ask` does.

**Folding settings asks in.** `settings_asks.rs` goes; `settings.set` for an Ask key opens a `setting` request. Changes: with no app following the Inbox the ask is saved instead of refused with `needs_approval`; the default end is 15 minutes instead of 300 seconds; the limit is the per-session one instead of 8 in all; `settings.asked {id}` stays, as `consent.get` under its old name and shape. `settings.agent` keeps its results; `no_app` and `full` are no longer produced.

**Folding vault requests in.** `vault.request` opens a `secret_request` request instead of holding a wait. It no longer needs the vault switch on to ask: with the vault off, the card says so and its Allow turns the vault on (the app's own `settings.set_switch`, on the user's click) and then opens the form. The form's `vault.save {request: id}` applies it; the form's Cancel denies it. `vault.request.cancel` stays for the app; the agent withdraws with `consent.cancel`.

**Events.** One kind, `agent.consent`, like `session.ask`: `then` (`asked`, `refused`, `allowed`, `denied`, `expired`, `cancelled`, `withdrawn`, `applied`, `failed`, `restored`), `kind` (new choice `ConsentKind`), `by` (Answerer), `answer_on`, the target as the kind's own typed field (`switch`, `key`, `secret`, `session`, `machine`, `account`; never text), and the counts `reason_chars` and `waited` (ms). `Lock::Always`, Audit and Activity, `agents_read: Own`. Never the reason or a value.

**What the user sees.** A card under the asking session in Chat when the session is on this Mac (beside the Ask cards), the Inbox row everywhere, and the notch's badge. The card follows [5](#5-consent-card-rules). Words in `locales/en/mac/consent.ftl`.

**Files.**

- New: `crates/engine/src/consent/` (`mod.rs`, `model.rs`, `store.rs`, `rules.rs`, `kinds.rs`, `facts.rs`, `inbox_item.rs`, and `*_tests.rs`); `crates/notchlingd/src/consent_ops.rs`, `consent_wiring.rs`, `consent_tick.rs`, `consent_inbox.rs` and their tests; `crates/app/src/consent/` (the feed and the card model) and `crates/app/src/chat/view/consent_cards.rs`; `locales/en/mac/consent.ftl`.
- Changed: `crates/engine/src/store.rs` (schema 10), `store_class.rs`, `inbox/model.rs` (`Kind::Consent`); `crates/local-api/src/wire.rs` (the `consent.*` names); `notchlingd/src/settings_ops.rs`, `settings_cli.rs`; `vault_ops.rs`, `vault_inbox.rs`, `vault_cli.rs`; `daemon_wiring.rs` (one call into `consent_wiring`); `protocol/agentconsole.v1.schema.json` (the inbox kind's description adds `consent`); the app's `inbox/`, `vault/` sheet and `settings_live.rs`.

### 7.3 Step 3: the CLI contract and discovery

**One exit-code table.** Every command but the hooks and the sidecar (which always exit 0 and print nothing on trouble) uses these codes:

| Code | Name | Meaning | What the agent does |
| --- | --- | --- | --- |
| 0 | `ok` | Done: applied, answered, allowed and applied, or read | go on |
| 1 | `failed` | Notchling tried and it didn't work, or the request was withdrawn | read the error; maybe retry |
| 2 | `bad_request` | The command line or a value is wrong; nothing changed | fix the command |
| 3 | `not_running` | No daemon, no answer, or a socket Notchling doesn't trust | ask the user to open Notchling |
| 4 | `refused` | Not allowed: a switch is off, user-only, another session's, another organization's | follow `error.ask`, or stop when `never` |
| 5 | `denied` | The person said no, dismissed or cancelled | stop; don't ask again at once |
| 6 | `pending` | The request is saved and still waits; its id is in the output | check later with `notchling consent <id>` |
| 7 | `busy` | A limit was reached | wait `retryAfterMs`, then retry |
| 124 | `expired` | No answer in time; the request is closed | ask again only if still needed |

`secret <names> -- <command>` exits with the command's own code once the command ran. Its own refusals use the table; with `--json` it then prints one JSON error line on stderr, and no such line means the code is the command's.

**Codes that change.** One move, with the skills in the same change; the table is then fixed, and later only grows.

| Command | Today | From phase 1 |
| --- | --- | --- |
| `settings` | 1 refused, 2 trouble, 3 not allowed | 4 or 2, 3 or 1, and 5, 6 or 124 |
| `models`, `cost`, `cluster`, `session reload` | 1 refused, 2 trouble | 4 or 2, and 3 or 1 |
| `ask` | 1 gone or usage, 2 dismissed, 3 refused or no daemon, 124 | 1 gone, 2 usage, 5 dismissed, 4 refused, 3 no daemon, 124 |
| `answer` | 2 already answered, 3 refused or no daemon | 1 with reason `already_answered`, 4, 3 |
| `secret` | 2 usage or trouble, 3 said no | 2 or 3, 5 |
| `secret-request` | 1 said no, 2 trouble | 5, 3 or 1, and 6 or 124 |
| `secrets`, `pages`, `page`, `delegate`, `delegate-models` | 2 trouble | 3 or 1; usage 2 |
| `status`, `sessions` | 3 not running, 1 else | unchanged |

**JSON.** With `--json`, a command prints exactly one JSON value on stdout, success or failure (`watch` prints one per line). A success prints what it prints today, so readers of today's output keep working. A failure prints:

```json
{"error": {"exit": 4, "code": "forbidden", "reason": "switch_off", "message": "the cluster is off: this Mac links to no other machine until the maintainer turns this on", "op": "cluster.dispatch", "switch": "cluster", "ask": {"command": "notchling switch on cluster", "kind": "switch"}, "retryAfterMs": null, "request": null, "never": false}}
```

- `code` is the wire `ErrorCode`, or the command's own (`usage`, `not_running`, `denied`, `pending`, `expired`, `busy`).
- `reason` is one of a closed list: `usage`, `bad_value`, `unknown_key`, `not_running`, `untrusted_daemon`, `timeout`, `switch_off`, `app_only`, `not_yet`, `never`, `other_session`, `other_organization`, `no_session`, `needs_consent`, `busy`, `denied`, `cancelled`, `expired`, `withdrawn`, `already_answered`, `failed`, `internal`. The daemon sends it in the error's `data.reason` (additive: `ApiError` already carries `data`); the command fills it in for its own.
- `message` is the fixed text, never a value or a parse error that could quote one. The same text goes to stderr.
- `ask` says how to get it allowed: the command and the consent kind, or, for an op whose agent path is planned for a later phase, the reason `not_yet` with `{"command": "notchling ask -- …"}`, so the agent can still ask the user in words. A user-only op has `never: true` and no `ask`.
- `request` is the `Consent` when one was opened (exit 6, 5 or 124).

**Common flags.** On every acting command: `--json`; `--reason <text>`; `--wait` (the default: wait until the request ends or `--timeout`, default 900 s) or `--no-wait` (return at once, exit 6 with the id when a request opened); `--timeout <seconds>`; `--expires-in <seconds>`; `--key <text>` (idempotency: the same key from the same session within 24 hours returns the first result); `--dry-run` (print what would happen and what card it would raise, change nothing, exit 0). When a wait ends with the request still waiting, the command exits 6 and the request goes on; it isn't cancelled.

**New commands.**

```
notchling consent <id> [--wait] [--timeout <seconds>] [--json]   one request: its state, and its result once it ends
notchling consents [--json]                                     this session's requests, newest first
notchling consent cancel <id> [--json]                          withdraw a request this session opened
notchling help [--json]                                         every command; --json for agents
notchling capabilities [--json]                                 what this caller may do now, and why not
```

`secret` and `secret-request` gain `--json`. `settings set` and `secret-request` gain the common flags.

**`help --json`.** Static, from the registry below:

```json
{"notchling": "2.1.0", "api": 1,
 "exitCodes": [{"code": 0, "name": "ok", "meaning": "Done."}, …],
 "reasons": ["usage", …],
 "commands": [{"command": "settings set", "usage": "notchling settings set <key> <value> [--json] [--reason <text>] [--no-wait] …",
   "summary": "change a setting; an Ask key waits for your yes", "ops": [{"op": "settings.set", "tier": "agent", "acts": true}],
   "consent": "by_key_class", "json": true, "waits": true, "exits": [0, 1, 2, 3, 4, 5, 6, 7, 124], "skill": "notchling-settings"}, …],
 "planned": [{"feature": "Start a session", "ops": ["session.new"], "phase": 4}, …],
 "userOnly": [{"feature": "Answer a permission prompt", "ops": ["session.permission", "agy.permission"], "why": "never"}, …]}
```

`consent` is `none`, `request` (always opens one), `by_key_class`, `own_free` (free for what the caller owns, a request otherwise) or `never`. `notchling --help` prints the same registry as text, so the two never drift.

**`capabilities --json`.** A new op, `daemon.capabilities` (agent, read), in `notchlingd/src/capabilities_ops.rs`:

```json
{"api": 1, "daemon": "2.1.0",
 "caller": {"tier": "agent", "tool": "claude", "session": "…", "verified": true, "folder": "…", "organization": {"id": "…", "name": "Personal"}},
 "app": {"attached": true, "followsInbox": true},
 "switches": {"acting": false, "bridge": false, "accounts": false, "vault": true, "install": false, "cluster": false},
 "consent": {"waiting": 1, "openLeft": 2, "thisHourLeft": 9, "coolingDown": [{"kind": "setting", "target": "theme", "untilMs": 0}]},
 "ops": [{"op": "settings.set", "now": "open"}, {"op": "cluster.dispatch", "now": "needs_switch", "switch": "cluster"},
         {"op": "session.new", "now": "not_yet", "phase": 4}, {"op": "session.permission", "now": "never"}]}
```

`now` is `open`, `needs_consent`, `needs_switch`, `app_only`, `not_yet` or `never`, for this caller at this moment. No secret, email, account name, folder of another organization or Inbox text is in it.

**The registry.** `crates/notchlingd/src/commands.rs` holds one row per command: its words, usage, summary, flags, ops, consent, the exit codes it may return and its skill; and the two lists `PLANNED` (op, feature, phase) and `USER_ONLY` (op, feature, why: `never`, `app_ui`, `node`, `hook` or `phone`). `usage()`, `help --json`, `capabilities` and the docs come from it. A test checks that every `Command` variant has a row, that every row's usage parses, and that each row's ops have the tier and `acts` their `OpSpec` says. A golden test writes `crates/notchlingd/tests/golden/agent-paths.json` and `docs/agent-paths.md` (`NOTCHLING_UPDATE_GOLDEN=1`), as the log kinds do.

**The guard check.** `GUARD-AGENT-PATH`, `PYTHONPATH=tools/guard python3 -m guard agentpaths` (`tools/guard/guard/agentpaths.py`, tests in `tools/guard/tests/test_agentpaths.py`, a row and a bad example in `docs/rules.md`):

1. Every op in `wire.rs`'s `mod ops` is in `agent-paths.json` exactly once: as a command's op, in `planned` with a phase, or in `userOnly` with a reason from the closed list.
2. `guard gate` sends a change where `planned` grew to the maintainer, as `GUARD-PROVIDER-GAPS` does: a new op arrives with its command, or someone decides it is user-only.
3. A design under `docs/superpowers/specs/` dated 2026-11-21 or later has an `## Agent path` section that names at least one `notchling` command and one op, or says "No agent path:" with a reason from the same closed list.

**Files.**

- New: `crates/notchlingd/src/commands.rs`, `contract.rs` (the exit table, the reasons, the JSON error writer, the common flags, shared by every `*_cli.rs`), `help_cli.rs`, `consent_cli.rs`, `capabilities_ops.rs`, and their tests; `tests/golden/agent-paths.json`; `docs/agent-paths.md`; `tools/guard/guard/agentpaths.py` and its tests.
- Changed: `cli.rs` (`usage()` from the registry, the new commands), `clients.rs`, `ask_cli.rs`, `settings_cli.rs`, `models_cli.rs`, `cost_report_cli.rs`, `cost_setup_cli.rs`, `cluster_cli.rs`, `reload_cli.rs`, `pages_cli.rs`, `vault_cli.rs`, `delegate_cli.rs` (the table and the JSON errors); the refusals that know their reason add `data.reason` (`switches.rs`, `acting_ops.rs`, `conn.rs`'s tier refusal: `app_only`); `tools/guard/guard/__main__.py`; `docs/rules.md`.
- Skills: the `notchling-*` skills get one shared section, generated in `crates/engine/src/install/skills.rs`: the exit-code table, "run `notchling help --json`", "a request may wait; run it in the background and check with `notchling consent <id>`", "exit 4 with `never` means stop". `settings/skill.rs`, `vault/skill.rs`, `ask/skill.rs` and the delegate's skill change their exit codes.
- Docs: `docs/local-api-ops.md` (the new ops, the folded ones, the `data.reason` list), `docs/notes/daemon.md` (op classes, identity, the record), `docs/notes/inbox.md`, `docs/notes/data-and-logs.md` (the table), `docs/feature-map.md` (`src/consent/`), `docs/policies/privacy.md` (the reason stays on the Mac).

### 7.4 Phase 1 tests

Rule by rule. Fixtures only; no test touches the real `~/.claude`, Keychain or a real login.

- **Classes.** Every agent-tier op in `wire.rs` is `AGENT_READ`, `AGENT_ACTS` or `AGENT_HOOK`; no op that writes, runs or touches the vault is `AGENT_READ`.
- **The record.** An acting call records one `agent.called` when it succeeds, is refused, fails, or opens a request (`pending`); a read records none; an app call records none; a handler that marked its own kind still gets one; with every log toggle off it still lands; the Audit line carries the session and the executable.
- **Identity.** A Claude Code session with an installed binary is verified; a process merely named `claude`, a symlink, or a `sessions/<pid>.json` planted for a shell is not; a `notchling delegate codex` child is `codex` with the delegating session; a process under a Codex session the daemon started is `codex` with that session; a shell is `shell` with none; the organization comes from the session's folder, and running the command from another folder doesn't change it.
- **`agent.named`.** A background command's output file names the subagent; two matches are `unknown`; the event holds no label text; a flood of calls queues at most 8.
- **The request.** States and first answer wins (Chat, Inbox and phone at once); every limit (3 waiting, 10 an hour, 20 in all, the cool-down, the shared bucket, the same key); the end time; restart keeps a waiting request and raises a fresh item; an allowed, unapplied request is withdrawn at restart; the facts checked again (a setting changed meanwhile is withdrawn); a phone Deny works, a phone Allow of a Mac-only kind is not taken; no agent-tier op answers; opens with `acting` off and with no app attached; `consent.get` by another session is refused.
- **Untrusted text.** A planted token in `--reason` appears in the `consents` row, the asker's reply and the Mac card only: never in the Inbox item, an event, the Audit, Activity, the phone feed or a log.
- **The folds.** Today's settings ask tests pass with the new behavior (saved with no app; 15 minutes); a secret request with the vault off is saved, and the card's Allow turns the vault on and opens the form.
- **The contract.** Each command's exit codes, table-driven over a fake link; the JSON error on stdout for every reason; `secret` passes its command's code through; `help --json` golden; `capabilities` for a verified session, a shell and an app.
- **End to end** (`crates/notchlingd/tests/agent_e2e.rs`, over `tests/common` as `ask_e2e.rs` does): a fake Claude session runs `notchling settings set <ask key> <value> --json --no-wait`, gets exit 6 and a request id; an app client allows it through `inbox.answer`; the daemon restarts in between; `notchling consent <id> --json` exits 0 with `applied`; the Audit holds two `agent.called` lines and `agent.consent` `asked`, `restored`, `allowed`, `applied`.
- **Guard.** `test_agentpaths.py`: an op in no list fails; an op in two lists fails; a planned list that grew is flagged by `gate`; a dated spec without `## Agent path` fails.

Changing today's exit-code tests and the settings ask tests changes existing tests, so those files wait for the maintainer.

## 8. Phase 2: free reads and `notchling watch`

Steps 4 and 5: what an agent may read without asking, and a way to follow events instead of polling.

**Ops and commands.**

| Command | Op | Tier | Returns |
| --- | --- | --- | --- |
| `notchling search <words> [--kind --since] --json` | `search.query` | agent, read (exists) | pages and sessions, the caller's organization only |
| `notchling inbox [--json]` | `inbox.mine` | agent, read | the caller's own items in full (its questions, requests, secret requests); other items of its organization as kind, state and the daemon's title only; other organizations as a count |
| `notchling accounts [--json]` | `accounts.summary` | agent, read | per tool: id, the name the user gave or a masked email, organization, active, plan; the caller's organization only |
| `notchling limits [--json]` | `limits.summary` | agent, read | each account's limit state and reset time, same filter |
| `notchling orgs [--json]` | `organizations.mine` | agent, read | the caller's organization (id, name, color, folders) and how many others exist |
| `notchling switches [--json]` | `settings.switches` | agent, read (exists) | all six, `cluster` included (today it is missing from `settings.list`) |
| `notchling awake [on\|off] [--json]` | `awake.state`; `settings.set mode` | agent, read; free key | the power assertion's state; on and off change the free `mode` key |
| `notchling versions [--latest --workers] [--json]` | `versions.get`, `versions.latest`, `versions.workers` | agent, read | the checked versions, the newest upstream, each worker's |
| `notchling outbox [--json]` | `outbox.mine` | agent, read | the caller's session's queued messages in full; other sessions' as counts |
| `notchling activity [--json]` | `activity.mine` | agent, read | the caller's session's own events (`agents_read: Own`) |
| `notchling prompts [--json]` | `prompts.mine` | agent, read | the caller's session's pending prompts; never an answer |
| `notchling roles [--json]` | `catalog.roles` | agent, read | the role table |
| `notchling watch [--topic <t>]... [--until <state>] [--count <n>] [--json]` | `events.subscribe` | agent, scoped topics | one JSON line per event |

`watch` follows new **scoped topics**: `mine.consents`, `mine.asks`, `mine.jobs` (phase 5), `mine.sessions` (phase 4), and the public `settings.changed` and `settings.switch`. The hub delivers a scoped event only to subscribers whose verified session owns it; an unverified caller gets the public ones only. `watch` reconnects like `ask` and exits 0 on `--until` or `--count`, 124 at `--timeout`.

**Settings classes.** `hiddenAgents` moves from Never to Free (it hides a row in the notch); `newSessionChoice` from Never to Ask (it picks the account a new session uses). Ask accepts verified Codex and Antigravity sessions the daemon started, now that identity names them.

**Consent class.** None: every op here is a read or a Free key.

**Safety rule.** Reads are scoped to the caller's organization and redacted: no email in full, no token, no secret value, no folder or Inbox text of another organization or another session, no question or answer text but the caller's own. A caller with no verified session sees Personal's public facts only.

**Tests.** A planted email, token, folder and question of another organization and another session appear in no reply and no watched event; each read for a verified session, a shell and another organization's session; scoped topics reach only the owner; `watch --until applied` ends when the request is applied; the free and changed settings classes.

## 9. Phase 3: switch requests

Step 6, on phase 1's request. The decision record covers the rule.

**Ops and commands.**

| Command | Op | Tier | Consent |
| --- | --- | --- | --- |
| `notchling switch on <acting\|bridge\|accounts\|vault\|install\|cluster> [common flags]` | `switch.request {switch, on: true}` | agent, acts | `switch`, Mac only, applied by the app |
| `notchling switch off <name> [--json]` | `switch.off {switch}` | agent, acts | none |

The app applies an allowed request with its own op and the request's id: `settings.set_acting {on, consent}`, `bridge.set {on, consent}` or `settings.set_switch {switch, on, consent}`. The event is `switch.changed` with the new cause `agent_request`. `switch off` is free and recorded; the app shows a notice, "«agent» turned off the cluster", with Turn On.

**Consent class.** Mac only: a phone may Deny, never Allow. The card says what the switch unlocks (fixed words per switch), that it is off again at the next start, and who asks.

**Safety rule.** No tier turns a switch on through a request by itself: the daemon only marks the request, and the switch goes on only by the app's call with the id, from the process that said hello. Every switch is still off at every start; a yes lasts for this run only and is withdrawn at a restart if unused. A request for a switch that is already on is refused (`bad_request`, "It is already on."). On a Worker, the request reaches the home's Inbox only while its cluster link is on, and the home applies only the switches the cluster may set (`acting`, `bridge`, `install`) through the member record; `vault` and `accounts` are answered only by the node's own app.

**Tests.** Request, Allow, the app's call with the id, the switch on, `agent.called` and `agent.consent` recorded; an agent-tier `settings.set_switch` with a valid id is still `forbidden`; a phone Allow is not taken; a restart leaves every switch off and withdraws the unused yes; the id can't be used twice or for another switch; `switch off` from an agent works and raises the notice.

## 10. Phase 4: sessions

Step 7.

**Ops and commands.**

| Command | Ops | Consent |
| --- | --- | --- |
| `notchling session new --folder <f> [--tool claude\|codex\|agy] [--model --effort] [-- <first message>]` | `session.new`, `folder.trust`, `codex.*`, `agy.open` | `session`, Mac or phone |
| `notchling session reply <id> -- <text>` | `session.reply`, `session.send`, `codex.send`, `agy.send` | free for own, `session` for others |
| `notchling session answer <id> --choice <n>` | `session.answer` (a session's question, not a permission) | free for own, `session` for others |
| `notchling session stop\|interrupt\|remove <id>` | `session.stop`, `session.interrupt`, `session.remove`, `codex.stop`, `codex.delete`, `agy.cancel` | free for own, `session` for others |
| `notchling session command <id> -- </command>` | `session.command` | free for own, `session` for others |
| `notchling session dialog <id> [--pick <n> \| --dismiss]` | `session.dialog`, `session.dialog_pick`, `session.dialog_dismiss` | free for own, `session` for others |
| `notchling session reload <id>` | `session.reload` (exists for self) | free for own, `session` for others |
| `notchling session chrome <id> on\|off` | `session.chrome` | free for own, `session` for others |
| `notchling session options <id> --set <k>=<v>` | `agy.set_option` | free for own, `session` for others |
| `notchling session show <id> [--json]` | `agy.state`, `agy.conversation`, `session.shell_output`, `session.held` | read; own in full, others of the organization redacted |

**Own.** The daemon records who started a session: `session.new` from a verified session stores `started_by`. A session the caller's verified session started is its own; so is itself.

**Consent class.** `session`, Mac or phone. The card shows the target session's name, folder and organization; for a reply or a first message, the text in full in the untrusted box; for a new session, the folder, tool, model and account it will use, and "Trusts this folder" when it isn't trusted yet.

**Safety rule.** `acting` must be on: off is `refused` with `ask: notchling switch on acting`. The target session must be in the caller's organization; another organization's is `never`. Permission prompts are user-only, whoever owns the session. `session.take_back` and `session.move` are not here: take back is user-only, move belongs to accounts (phase 7). At most 30 free actions an hour from one session on one own session, then `busy`.

**Tests.** Own versus others for each op; another organization refused before any card; `session.permission` and `agy.permission` refused with `never` from every caller; a reply's text is in the card and the session, never an event; acting off gives the right `ask`; `started_by` survives a restart; the hourly limit.

## 11. Phase 5: jobs

Step 8.

**Ops and commands.**

| Command | Ops | Consent |
| --- | --- | --- |
| `notchling dispatch <machine> --tool claude\|codex\|agy\|shell --folder <f> [--model --effort --secrets a,b --key k] -- <prompt\|command>` | `cluster.dispatch` (agent for every tool) | `dispatch`, Mac or phone; Mac only with `--secrets` |
| `notchling jobs [--json]` | `jobs.mine` (new) | read |
| `notchling job <id> [--wait] [--json]` | `jobs.get` (new) | read: own in full, others' state only |
| `notchling job tail <id>` | `node.tail`, `cluster.follow` | free for own, `job` for others |
| `notchling job return <id>` | `cluster.return` | free for own, `job` for others |
| `notchling job stop <id>` | `shell.stop` | free for own, `job` for others |

`notchling delegate` registers its local job with the daemon (`delegate.begin` and `delegate.end`, agent, acts), so it is recorded and listed in `jobs`. Today's delegate rule stays: a read-only `agy` delegate to a machine needs no card.

**Own.** The dispatch record keeps the dispatching session.

**Consent class.** `dispatch`: the card shows the machine (name, organization, reachable or not), the folder, the tool, model and effort, the secret names, and the prompt or command in full in the untrusted box. The `--key` makes a retried dispatch return the first job.

**Safety rule.** `cluster` must be on (`ask: notchling switch on cluster`). The machine and the secrets must be in the caller's organization. Secrets need the names allowed for the cluster (`cluster.allow.set`, phase 6). An agent on a member calls its own daemon, which forwards the dispatch and its request to the primary with the caller's identity; the request reaches the home's Inbox through the mirror.

**Tests.** A dispatch per tool through the card; the key; own versus others for tail, return and stop; another organization's machine or secret refused; `--secrets` Mac only; a member's dispatch forwarded with its identity; `delegate` recorded and listed.

## 12. Phase 6: cluster admin

Step 9.

**Ops and commands.** `notchling cluster create <name>`, `invite --node <id> --name <n>`, `member <node> --role <r>`, `failover on|off`, `main <node>`, `allow <names>`, `remove <node>`, `move-home <node>`: one agent op, `cluster.admin.request {action, args}` (agent, acts), which opens a `cluster` request; on Allow the app calls today's app-tier op (`cluster.create`, `cluster.join.token`, `cluster.invite`, `cluster.member.set`, `cluster.failover.set`, `cluster.main.set`, `cluster.allow.set`, `cluster.remove`, `cluster.move_home`, `cluster.move.now`) with the id. `cluster join --token-stdin` stays as it is.

**Consent class.** `cluster`, Mac only. The card shows the cluster, the machine (name, address, organization), the role or setting before and after, and for `allow` each secret name.

**Safety rule.** An invite's token goes only to the asking process, in the reply of its `consent.get`, never in an event, the Inbox or the card; it expires as today. Promote, take back and continue here stay user-only. `cluster.side_copy.hide` is the app's view only (`app_ui`).

**Tests.** Each action through the card and the app's call; the token reaches only the asker; promote refused with `never`; a phone Allow not taken.

## 13. Phase 7: accounts

Step 10.

**Ops and commands.** `notchling accounts switch <id>`, `add <claude|codex|agy>`, `remove <id>`, `move-all <lane>`, and `notchling lanes list|create|remove|switch`: one agent op, `accounts.request {action, args}` (agent, acts), opening an `account` request; on Allow the app calls today's ops (`accounts.switch`, `providers.switch`, `accounts.add.begin`, `providers.add.begin`, `accounts.remove`, `providers.remove`, `accounts.move_all`, `lanes.*`, `session.move`). `lanes list` is a read.

**Consent class.** `account`, Mac only. The card shows the tool, the account by the name the user gave (or a masked email), its organization, and what changes ("New sessions use …", "Moves 3 sessions to …").

**Safety rule.** The organization check comes first: a target outside the caller's organization is refused with `never` before any card. A caller with no verified session has no organization and is refused with `no_session` ("Run this from a session in the organization's folder."). The `accounts` switch must be on. Adding an account opens the sign-in in the app; the agent gets the new account's id, never a URL with a code or a token. The rule against renewing a login in use stays as the switch path has it.

**Tests.** Cross-organization refused before a card, for every action; no session refused; the sign-in result carries no secret; a switch through the harness only, never a real login; a token in use never renewed (the existing harness).

## 14. Phase 8: machines, Set Up and Cost Profile apply

Step 11. Read-only plans first, then writes by request, ticked file by file.

**Machines.** The Machines engine (`crates/engine/src/machines/`) runs in the app process today: the Machines window and the `notchling-machines` binary. The daemon serves it first (`machines_ops.rs`): `machines.list`, `machines.check`, `machines.plan` (agent, read) behind `notchling machines list|check|plan --json`. Then `machines.request {action: add|fix|update|remove, args}` (agent, acts) opens a `machine` request; the card shows the host, the user, the organization, and each step and what it copies, from the plan. The admin password is typed on the Mac or taken from the Vault with its own approval; ssh keys stay where they are. `tailscale.switch` becomes `notchling network switch <tailnet>`, a `machine` request; the card says the other network drops.

**Set Up.** `install.plan` (agent, read) behind `notchling install plan --json`; `notchling install apply` opens an `install` request listing each file with its diff summary. The user ticks each file; the scope records the ticked files and their content hashes; the app's `install.apply` writes only those, and only if each file still has the hash shown. `install.remove` stays user-only. `agy.sidecar.setup` is an `install` request too, with its one config key.

**Cost Profile.** `notchling cost setup --apply`, `cost undo` and `cost profile set` from the agent tier open a `cost_profile` request with the same per-file ticks. An agent's `--consent a,b` only fills the dry run; it is never consent. `costGuard` stays user-only.

**Updates.** `notchling versions update [--worker <node>]` opens an `update` request for `versions.update_local` or a worker's update; `notchling check-updates` uses the Ask key `checkForUpdates`.

**Consent class.** All Mac only, applied by the app.

**Safety rule.** Per-file consent: a yes covers exactly the files ticked, at the hash shown. The `install` switch must be on for Set Up. Machines in another organization are refused with `never`.

**Tests.** A plan as a read changes nothing; a file changed between the card and the apply is not written; an unticked file is not written; `install.remove` and `costGuard` refused with `never`; the admin password never reaches the agent; the machine engine's fakes only.

## 15. Phase 9: search data, hybrid search and skills

Step 12.

**Search data.** The index covers what [the hybrid search design](2026-11-13-hybrid-search-design.md) names, scoped by organization, and `notchling search` gains `--kind` and `--since`. Then hybrid search per that design.

**Skills.** Every command family ends up with skill text: a new phase ships its family's lines with its commands (phases 2 to 8 each carry theirs), and phase 9 checks the whole set: one short `notchling` index section shared by every skill (from phase 1), and family skills only where the steps aren't obvious from `help --json` (cluster, sessions, jobs, accounts, machines). A test drives each skill's example command the way an agent would.

**Consent class.** None.

**Safety rule.** Search returns the caller's organization only; text of other sessions follows phase 2's redaction.

**Tests.** The hybrid search design's tests, plus: no result from another organization; every skill example runs against the fixture daemon and exits as its skill says.

## 16. Not yet audited

The review stopped before these: Morning, the overnight worker, reply receipts, Plan-Only Main Chat, the phone app's actions, and the app's menus, sheets and `locales/en`. Phase 1's builder walks them first and puts each user action in the registry: as a command, in `PLANNED` with the phase whose family it belongs to, or in `USER_ONLY` with a reason. The guard check then holds them like every op. An action that has no op at all (a menu that only changes the app's view) is `app_ui`.

## Agent path

The design's own path, as `GUARD-AGENT-PATH` asks of designs from today on.

| Feature | Command | Op | Tier |
| --- | --- | --- | --- |
| Check a request | `notchling consent <id> [--wait] --json` | `consent.get` | agent, read |
| List this session's requests | `notchling consents --json` | `consent.mine` | agent, read |
| Withdraw a request | `notchling consent cancel <id> --json` | `consent.cancel` | agent, acts |
| What may I do now | `notchling capabilities --json` | `daemon.capabilities` | agent, read |
| Every command | `notchling help --json` | none (static) | none |
| Change an Ask setting | `notchling settings set <key> <value> --json` | `settings.set` | agent, acts, `setting` request |
| Ask for a secret | `notchling secret-request <name> --json` | `vault.request` | agent, acts, `secret_request` request |

## 18. Risks

- **The card is the only barrier.** A prompt-injected agent can now reach every guarded area up to the card, and a user who allows by reflex lets it through. The rules of [5](#5-consent-card-rules) narrow it: facts the agent can't write, its words marked untrusted, once, a delay on Allow, an end time, a rate limit, Mac-only Allow for power. What is left is a user who reads "turn on the vault" and allows it anyway.
- **A forged phone.** `devices.json` is a file an agent can write, so a forged device could answer: the risk the maintainer accepted for vault approvals. Power kinds are Mac only, and a phone may always Deny, so the forged phone can at most allow a setting, a secret request's form (which still needs the Mac), a session action or a dispatch without secrets.
- **Labels can be forged inside a session.** Any agent of a session can write its transcripts, so the agent label can lie. Ownership rests on the kernel's session, never on the label.
- **The organization of a session is its folder.** An agent can't change it mid-session, but a session started in the wrong folder gets that folder's organization. Starting a session is itself a request.
- **The exit codes move once.** A script or an agent with old skill text reads 3 as "not allowed". The skills change in the same merge, and the skills follow the app.
- **Volume.** An agent in a loop writes an `agent.called` line per acting call. The Audit's 500 MB cap and the per-session limits bound it; a busy session shows in Activity's Agents filter.
- **Stale cards.** Requests saved with no app attached pile up while the user is away. The end time and the second check of the facts keep a late yes from doing something other than what the card said.
- **Schema number.** Another branch may take version 10 first; this one then takes the next at merge.

## Needs the maintainer

Each needs your call before its phase merges; the recommendation is what the plan builds unless you say otherwise. The record [XXXX-agents-first](../../decisions/XXXX-agents-first.md) carries the first three.

1. **An agent may request a switch on, Mac only; the app turns it on; still off at every start.** Recommendation: **yes**.
2. **Keep the cluster on at start, with the consent in a Keychain item only the signed app reads.** Recommendation: **no, not now**; the reasons are in the record. It is a separate choice from 1.
3. **Every acting agent call is recorded, and nothing turns it off.** Recommendation: **yes**.
4. **An agent may turn any switch off without asking** (recorded, and the app shows a notice with Turn On). Recommendation: **yes**: off takes power away.
5. **One exit-code table, moving today's codes once** (settings 3 becomes 5, `ask`'s dismissed 2 becomes 5, and the others in [7.3](#73-step-3-the-cli-contract-and-discovery)), with the skills in the same change. Recommendation: **yes**.
6. **A phone may Deny any request but Allow only "Mac or phone" kinds** (settings, sessions, dispatch without secrets). Recommendation: **yes**.
7. **A request opened with no app attached is saved and waits** (15 minutes by default, 12 hours at most) instead of being refused. Recommendation: **yes**.
8. **Settings asks and vault requests fold into the one request; vault approvals keep their own flow and Always Allow.** Recommendation: **yes**.
9. **The organization comes from the session's folder; a caller in no session has none and can't request organization-scoped actions.** Recommendation: **yes**.
10. **`hiddenAgents` becomes Free and `newSessionChoice` Ask.** Recommendation: **yes**.
11. **Phase 1 changes existing tests** (exit codes, settings asks), which the zone sends to you. Recommendation: **yes**, reviewed as one change.
