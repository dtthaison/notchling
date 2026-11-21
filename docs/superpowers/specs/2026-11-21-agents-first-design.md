# Agents first: every feature has an agent path

- **Date:** 2026-11-21
- **Status:** Revision 2, after a builder check by Antigravity (gemini-4-argon). Nothing here is built. The maintainer decided the seven calls of revision 1 on 2026-11-21, each at the recommendation; they are marked *(decided)* in [Needs the maintainer](#needs-the-maintainer) and in the [decision record](../../decisions/XXXX-agents-first.md). The calls still open are in the same section.
- **Source:** goal 6 in [docs/goals.md](../../goals.md) and "Agents first" in the root `AGENTS.md` (branch `agent/goal-agents-first`, 803e1b3c), approved by the maintainer: "do all for agent friendly develop and use notchling". The audit of what an agent can do today and Fable's review of it (both kept with the job, not in git). The review's corrections, missing features, cross-cutting pieces, safety clashes and its 12-step order are taken as given; this design groups the 12 steps into phases and settles what the review left open.
- **Base:** `origin/agent/goal-agents-first` 803e1b3c (on `agent-run-oss` 5f993c28).
- **Touches:** decisions 0007 (tiers, switches, files at rest), 0019 (agent instructions per tool), 0020 (settings by class), 0022 (Ask), the typed event stream (XXXX), skills follow the app (XXXX), organizations (XXXX), cluster v1 nodes (XXXX). Its decision record: [XXXX-agents-first](../../decisions/XXXX-agents-first.md).
- **Revision 2** answers the builder check: Phase 1's open shapes (who may read a request, the wire shapes, the state machine of app-applied requests, stdout and stderr, how the server learns a call is pending, the op lists, idempotency and dry runs, the event fields, what "installed" means, the unaudited areas), five risks (above all, a secret request no longer turns the Vault on), and seven problems in the later phases (above all, an agent's own sessions no longer let it escalate, a phone no longer allows running code, and requests across machines have one owner).

## In short

1. Whatever a person can do in Notchling, an agent can do with a `notchling` command over an agent-tier op, with `--json`. Where the user must say yes, the command opens a **consent request**: a card in the Inbox (and in Chat under the asking session). The agent waits, or gets an id and checks later. It is never refused without a way to ask.
2. The card is now the only barrier between a misled agent and every guarded area. So it shows only facts the daemon worked out, shows the agent's own words as untrusted, allows once, has no "always allow", records what the yes covered, ends at a set time, and is rate limited per session ([Consent card rules](#5-consent-card-rules)).
3. A short list stays with the user, whoever asks: answering a permission prompt, removing hooks or the guard, deleting the Audit or starting a new chain, pairing a phone, an account of another organization, and a few more ([User-only actions](#6-user-only-actions)).
4. Every acting agent call is recorded, with who made it (tool, session, agent), and nothing turns that record off. Activity gets an Agents filter.
5. The switch rule changes in one way: an agent may *ask* for a switch to go on. Only the app, on the user's yes, turns it on, and every switch is still off at every start. Turning one off needs no yes.
6. One exit-code table and one JSON error shape for every command. `notchling help --json` lists every command, its ops, tier and consent; `notchling capabilities --json` says what this caller may do right now.
7. A guard check holds it: every op is reachable by a command, planned for a named phase, user-only, or internal with a reason; the planned list only shrinks; a new design names its agent path.
8. Nine phases, each merging alone, in order. Phase 1 builds the record, the consent request and the contract. Phases 2 to 9 add the features family by family.

## Phases

| Phase | Steps | What it adds | Consent |
| --- | --- | --- | --- |
| 1 | 1 to 3 | The agent call record and identity, one consent request (settings asks and secret requests fold into it), the CLI contract and discovery, the guard check | the mechanism |
| 2 | 4, 5 | Free reads (search, own Inbox items, accounts and limits redacted, organizations, switches, awake, versions, outbox, own activity, prompts, roles) and `notchling watch` | none |
| 3 | 6 | Switch requests | Mac only |
| 4 | 7 | Sessions: new, reply, stop, interrupt, command, dialog, reload, Chrome, Codex and Antigravity turns | Mac only, except stopping; messages to a session the agent started are free |
| 5 | 8 | Jobs: dispatch claude, codex, agy and shell; show, wait, tail, return, stop | Mac only to dispatch; free for own jobs |
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

- Agents answering for the user. A yes comes from the person, on the Mac or, for the few kinds marked so, a paired phone. No agent answers a consent card, the main session included.
- A standing grant. No "always allow", no "don't ask again", no allow-list an agent can grow.
- Vault approvals (a use of an "Ask me first" secret). They keep their own flow and their Always Allow ([0007](../../decisions/0007-local-api-tiers.md)'s note of 2026-11-13); they are a check the user set on a secret, not an agent's request for new power.
- Questions. Ask stays as it is ([0022](../../decisions/0022-ask.md)): a question is not a consent, and the main session may still answer the questions that opted in.
- An agent posting notifications. Ask covers telling the user something that needs an answer; free-form agent text in front of the user invites "press Allow on the next card".
- A hard barrier against an agent with a shell. Such an agent can already run `claude -p` or `codex exec` itself, or write a file at rest ([0007](../../decisions/0007-local-api-tiers.md)'s note of 2026-11-08). The request keeps Notchling from being the path that acts without the user, and puts what it does where the user sees it. Where Notchling holds real power (switches, logins, the vault, other machines), the app tier and the switches stay the boundary.

## 3. What exists today

With the review's corrections:

- **Tiers.** `OpSpec` is `AGENT`, `APP`, `APP_QUIET` or `APPROVAL` (`crates/local-api/src/server.rs`). Agent-tier ops today: `sessions.list`, `settings.list/get/set/asked`, `settings.switches`, `settings.get_acting`, `catalog.get`, `models.check/update`, `cost.summary`, `cost.report`, the Cost Profile's reads (its preset and picks are Ask keys), `versions.get`, `cluster.status`, `cluster.join`, `cluster.dispatch` (delegate jobs only), `pages.list/find`, `search.query` (no command yet), `log.verify`, `agy.sessions`, `ask.open/wait/mine/answer`, `session.reload`, `session.question`, `vault.find/use/request`, the hook's `hook.request`, and Antigravity's sidecar and hook ops.
- **Records.** `op.called` covers app-tier calls only (`conn.rs::record_generic`). A successful agent call records nothing unless its handler does (`settings.agent`, `vault.*`, `session.ask`). Refusals and failures are recorded for every tier (`api.refused`, `op.failed`). A handler says it recorded its own kind through the scope's thread-local flag (`notchling_events::scope::mark_recorded`, read by `take_recorded`).
- **Identity.** At `hello`, an agent-tier connection gets a Claude Code session id by walking the peer's parents (`caller_session.rs`, 16 steps). `is_claude_binary` takes the process's executable from the kernel, resolves it, and matches it against the resolved install files (`~/.local/bin/claude`, `/usr/local/bin/claude`, `/opt/homebrew/bin/claude`) or the install folders (`~/.local/share/claude`, `~/.claude/local`). Nothing names a subagent, a Codex or Antigravity session, or a shell. Ask finds the agent inside a session from its transcripts (`ask_find.rs`), as a label.
- **Consent.** Three mechanisms: settings asks (Inbox, 300 s, refused at once with no app following the Inbox, at most 8), vault requests (need the vault switch on, so with it off an agent can't even ask), Ask (needs `acting` on and a Claude Code session). Three timeouts and no "open, get an id, check later". `settings set` prints its waiting note on stderr even with `--json`.
- **Commands.** Exit codes differ per command: 3 is "not running" for `status`, "not allowed" for `settings`, "refused or no daemon" for `ask`, "denied" for `secret`. `--json` errors are prose on stderr. `secret` and `secret-request` have no `--json`. No `help --json`, no capabilities op.
- **Switches.** Six, off at every daemon start; only an app-tier call in that run turns one on, with the two audited `cluster` exceptions of [0007](../../decisions/0007-local-api-tiers.md)'s note of 2026-11-14. The app restores the user's choice when it attaches (`restored: true`, cause `restored_at_launch`). A cluster member's `acting`, bridge and `install` follow the home's `desired` for it (`cluster.member.set {node, desired}`) while no app is attached there.
- **The phone.** It shows any Inbox kind, an unknown one as a notice, with every action by its label; an action `open` shows as a disabled "Open on Mac" (`apps/flutter/lib/features/inbox/inbox_screen.dart`). The protocol's `InboxItem.kind` is a free string, described, not an enum.

## 4. Terms

- **Agent path.** A `notchling` command, the agent-tier op it calls, `--json` output, and a skill line that says when to use it.
- **Acting call.** An agent-tier call that changes something, runs something, sends something, uses a secret or opens a request. Reads are not acting calls.
- **Identity.** Who made a call, worked out by the daemon from the kernel: the tool, the session, and whether the daemon can vouch for it. The agent inside a session (main or which subagent) is a label from its transcript.
- **Verified session.** A Claude Code session whose process is an installed Claude Code ([7.1](#71-step-1-the-agent-call-record-and-identity)) and whose `sessions/<pid>.json` names it, or a session the daemon itself started (a Codex or Antigravity session Chat runs). Only a verified session owns anything, and only a verified session has an organization.
- **Consent request** (in short, a request). A saved request for the user's yes to one action, shown as a card. Its **kind** says what it may do; its **scope** is exactly what a yes covers.
- **Owner node.** The machine whose daemon holds a request's row: the one the asking agent runs on.
- **User-only.** An action no agent may do or request. Its op refuses with reason `never`.

## 5. Consent card rules

Every consent card follows these rules, in every kind and every phase. A test per rule holds them ([7.4](#74-phase-1-tests)).

1. **Daemon-computed facts only.** The title, the action, the target, what changes (now and after), what it unlocks, who asks and from where are worked out by the daemon from its own state: the session list, the switch states, the settings key's spec, the vault's list, the cluster state, the plan of a file write. The agent passes only a kind, a target and typed arguments, which the daemon checks against the kind's spec. The words come from `.ftl` files, never from the agent.
2. **Agent text is shown as untrusted.** The free text an agent supplies is its reason (`--reason`, at most 300 characters); where the action itself is text (a message to a session, a prompt, a shell command), that text; and a secret request's prefilled form fields (`--url`, `--note`, `--how`), which the user edits in the form. They are shown in a box labelled "The agent says" or "The command", in plain text: no links, no Markdown, line breaks kept, nothing cut but the reason. Agent text never appears in a title, a button, the Inbox item, a notification, the phone or an event. The reason is deleted when the request closes.
3. **Who asks, honestly.** The card names the tool and the session as the daemon knows them ("Claude Code · notchling · agent/agents-first"), marks a verified session as such, and shows the agent's label only as "as its transcript says". A caller in no verified session is "A program outside any agent session", with the app it runs under ("in Terminal").
4. **Allow once.** The answers are Allow and Deny. Allow applies the action once, within the scope, and is used up. There is no "always allow", no "don't ask again", no "allow all". A "Deny all waiting" exists, because a denial grants nothing.
5. **Scope recorded.** At open the daemon writes the scope: the kind, the target, the exact change, the session, the organization and the end time. Applying checks the action against the scope exactly. A kind with parts (the files of a Set Up or a Cost Profile write) lets the user narrow the scope on the card, never widen it: the answer records the parts allowed, and the apply must name a subset of them. The scope, the answer, who gave it and the use are kept in the `consents` table and recorded as events.
6. **What you saw is what happens.** The daemon checks the facts again when the yes is applied. If they changed (the setting's value, the session ended, the file changed, the switch is already on), the request is withdrawn, not applied.
7. **An expiry.** Every request ends: 15 minutes by default, 1 minute to 12 hours with `--expires-in`. An end later than 1 hour is open only to a verified session, one such request per session and 5 in all. The card shows the time left. An answer after the end is refused, and the app disables Allow at the end.
8. **A per-session rate limit.** At most 3 requests waiting per verified session and 20 in all; at most 10 opened per session in a rolling hour; after a Deny, the same kind and target from that session is refused for 30 minutes. A caller in no verified session counts against the app it runs under (its root app: the last process on the walk before `launchd`, such as Terminal, an editor or a launchd job): 1 waiting, 3 an hour, an end of at most 15 minutes. A retry with the same `--key` returns the same request and counts once.
9. **No reflex.** Allow is never the default button, Return doesn't press it, and it is enabled 1 second after the card is shown. Cards are answered one at a time.
10. **Only the person answers.** On the Mac, from the app (`ClientKind::App`, the process that said hello). A paired phone may Deny any request, and may Allow only a settings ask or an action that only stops something (stopping or interrupting a session or a job). Everything that runs code, drives a session, reaches another machine or grants power is allowed on the Mac only, because a paired device is a file an agent can write. No agent answers a consent card, the main session included: there is no agent-tier answer op.
11. **Power is applied by the app.** For the kinds that grant power (switches, cluster admin, accounts, machines, Set Up, Cost Profile, updates) and for secret requests, the agent tier never gains the op that acts. Its op opens a request; the app performs the existing app-tier op, citing the request, when the user presses Allow.
12. **One owner.** A request lives on its owner node. Its Inbox item reaches the home and the Can Be Master members through the existing inbox mirror, and an answer given there is forwarded to the owner, carrying who gave it; the owner checks that against the kind's rule. A node that acts on another node's request checks the answer in its own copy of the mirrored item, never on the asker's word.

## 6. User-only actions

These stay with the user. Their ops refuse every agent with `forbidden`, reason `never` and "Only you can do this, in Notchling.", and no consent kind exists for them. `help --json` lists them so an agent stops asking.

| Action | Ops | Why |
| --- | --- | --- |
| Answering a permission prompt | `session.permission`, `agy.permission`, the hook's decisions | The goals' never list. A prompt is the user's own check on an agent; an agent answering it removes the check. |
| Loosening a session's permissions | a permission mode that skips prompts, `/permissions` rules, a start with prompts skipped | The same check, removed ahead of time. |
| Removing hooks or the guard | `install.remove`, the `costGuard` key, Notchling's hook entries, turning Plan-Only Main Chat off | An agent would remove the check on itself. |
| Deleting the Audit or starting a new chain | `log.audit.delete`, `log.chain.start` | An agent would erase its own trail. |
| Pairing a phone, and revoking one | `bridge.pair.begin`, `bridge.pair.answer`, `bridge.revoke` | A paired phone answers cards: an agent could pair a device of its own and approve itself. Revoking cuts the user off their phone. |
| An account of another organization | any account op whose target is outside the caller's organization | The goals' never list. Refused before any card is raised. |
| Answering a consent card, an Inbox item or the user's question | `consent.answer`, `consent.opened`, `inbox.answer`, `ask.dismiss` | The card is the user's yes. |
| Reading a secret's value outside `secret --` | `vault.list`, `vault.save`, `vault.admin` | Values never reach an agent ([0007](../../decisions/0007-local-api-tiers.md)). |
| Promote, take back, continue here | `cluster.promote`, `cluster.take_back`, `cluster.continue_here`, `session.take_back` | The cluster design: never an agent. They change which machine drives the user's sessions. |
| The Never settings keys | logins, secrets, `codexNetwork`, the app's own records | [0020](../../decisions/0020-agents-change-settings-by-class.md). |
| Keeping a switch on at start | none | Not built: the maintainer decided against it on 2026-11-21. |

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
| `tool` | the walk | `claude`, `codex`, `antigravity`, `shell` or `unknown`. A process on the walk that is this `notchling` running `delegate codex` or `delegate agy` makes it that tool, delegated. A process the daemon started for a Codex or Antigravity session makes it that tool. A Claude Code session found as today makes it `claude`. A tool found only by its binary's name is that tool, unverified. Nothing found: `shell`. |
| `session` | the walk | The Claude Code session id (as today), or the id of the Codex or Antigravity session the daemon started (from its own launch record: pid and start time to session), or none. A delegated job's session is the delegating session. |
| `verified` | the walk | True only for a verified session, as below. |
| `root` | the walk | The caller's root app: the last process before `launchd`, by executable and start time. The rate limit of a caller in no verified session counts against it. |
| `folder` | the session | The session's folder from the daemon's session list, never the command's working folder. |
| `organization` | the folder | For a verified session only: the organization whose folders hold the session's folder (longest match), else Personal. Anyone else has none. |
| `cmd_hash`, `out_hash` | the process | Salted hashes of the asking process's command line after its program, and of its stdout's canonical path when that is a regular file: the evidence `agent.named` resolves from ([below](#agent-named)). |

**What "installed" means.** A Claude Code process is verified when all of these hold, each read from the kernel or resolved on disk at the call: the process's executable (from its audit token) resolves to a file; that file lies under one of the install folders, resolved: `~/.local/share/claude`, `~/.claude/local`, and Homebrew's `Caskroom/claude-code` under `/opt/homebrew` and `/usr/local`; and the session's `sessions/<pid>.json` names that process. Matching an install *file* (`~/.local/bin/claude` and the two others) is no longer enough on its own: those are links the user's account can re-point, so a link re-pointed at `/bin/zsh` would make every shell look like Claude Code. A Claude Code installed with npm runs as `node` and is not verified; it is treated as a shell for ownership, and its card says so. The test fixtures cover a re-pointed link, a copy of the binary elsewhere, a process merely named `claude`, and a planted `sessions/<pid>.json`.

Ownership follows only from a verified session: an unverified caller is treated as `shell` for every "own" rule, whatever its tool label says.

**Events.** In `crates/events/src/kinds/agents.rs`:

- `agent.called`: one line per acting agent call, written by the server (`conn.rs`), not by the handler, and also when the handler recorded its own kind. Fields: `op` (Op), `tool` (new choice `AgentTool`), `verified` (bool), `result` (new choice `AgentResult`: `ok`, `refused`, `failed`, `pending`, `dry_run`), `code` (Option ErrorCode), `ms` (Millis), `consent` (Option of a new `Ref<Consent>`, always hashed, as `Item` is), `organization` (Option of a new `Ref<Organization>`), `cmd_hash` and `out_hash` (Option Hex). The envelope's actor carries the tier, the executable and the session, as today. Title "Agent Actions", `lock: Lock::Always("Every acting agent call is part of the audit trail.")`, sinks Audit and Activity, `Durability::CommitFromAgent`, `agents_read: Own`, `keep_days: 0`.
- <a id="agent-named"></a>`agent.named`: the label of the agent inside the session. Fields: `asker` (AskerKind), `agent` (Option of a new `Ref<Agent>`, the transcript's agent id). It carries the call's `op_id`, so Activity joins the two. No queue can drop it: the evidence (`cmd_hash`, `out_hash`, the session) is already in the call's own line, so a burst of calls can't hide one. The daemon emits `agent.named` when `ask_find`'s finder settles, off the op thread, coalescing calls of one session with the same evidence into one run; and when Activity shows a row whose label isn't known yet, the daemon resolves it from the transcript then. A transcript gone by then gives `unknown`. Same lock and sinks. The label's text is never in it: Activity reads the label from the transcript by the agent id, as Chat does for subagents.
- `agent.consent`: see [7.2](#72-step-2-one-consent-request).

**How the server learns a call is pending.** `conn.rs` sees only the handler's `Result<Value, ApiError>`, so the handler tells it through the scope, as `mark_recorded` does today. `notchling_events::scope` gains `note_consent(id: &str, state: ConsentNote)` (`ConsentNote::Pending` when the call opened or returned a waiting request, `Used` when an app call applied one) and `take_consent() -> Option<(String, ConsentNote)>`, a thread-local the call's thread clears. After the handler returns, `conn.rs` takes it: `Pending` makes `result: pending`, either one fills `consent`. A call with `dryRun: true` is recorded with `result: dry_run` (it changes nothing, but it is how an agent learns what a card would say, so the record keeps it). `conn.rs` never reads the reply's JSON to find this out.

Nothing turns these off: they are `Lock::Always`, no setting or switch names them, no `NOTCHLING_*` variable is read for them, and a test turns every log toggle off and still finds them. Reads by agents stay unrecorded one by one, as `settings` reads are today.

**Activity.** `activity.list` takes `kinds: ["agents"]`: every row whose actor tier is agent, whatever its kind. Settings > Activity gets an Agents filter; a row shows the tool, the session's name (from the session list), the agent's label (from the transcript, marked "as its transcript says"), the op's title and the result.

**Files.**

- `crates/local-api/src/server.rs`: `OpSpec { acts }`, the three agent constants, `Handler::identity_of` returning a small `AgentIdentity` (tool, session, verified, root, organization, the two hashes), which the scope's actor and the record use; `Handler::agent_path(op)` ([7.3](#73-step-3-the-cli-contract-and-discovery)).
- `crates/local-api/src/conn.rs`: keep the identity from `hello`; record `agent.called` for every `AGENT_ACTS` call outside the `take_recorded()` branch, for ok, refused, failed, pending and dry runs alike; the tier refusal's reason from `agent_path`.
- `crates/local-api/src/record.rs`: `Shared::agent_called`.
- `crates/events/src/scope.rs` (`note_consent`, `take_consent`), `kinds/agents.rs`, `choices.rs`, `field_ref.rs` (ref kinds `Agent`, `Consent` always hashed, `Organization`, `Job`); the regenerated `tests/golden/log-kinds.json` and `docs/log-kinds.md`.
- `crates/notchlingd/src/identity.rs` and `identity_tests.rs`; `caller_session.rs` (the walk returns the chain it saw; `is_claude_binary` takes the install-folder rule); `codex_ops.rs`, `agy_ops.rs` (pid and start time to session for the processes they started); `daemon_wiring.rs` (`identity_of`); `agent_named.rs` (the finder runs and the lazy lookup).
- Every `*_ops.rs` with an agent op: `AGENT` becomes `AGENT_READ`, `AGENT_ACTS` or `AGENT_HOOK`.
- `crates/notchlingd/src/activity_ops.rs` and `crates/engine/src/activity/` (the `agents` filter, the label lookup); the app's Activity page and its `.ftl` words.

### 7.2 Step 2: one consent request

**What it is.** A row in a new `consents` table of `notchling.db` (schema 9 to 10, class Node: a request stays on its owner node and reaches the cluster only as an Inbox item; the daemon is its only reader and writer; an older daemon ignores the table). A feature op that needs the user's yes from an agent opens one and replies at once with the request. The agent waits on it or checks later.

**Columns.** `id` (`c` and 32 hex characters, 128 random bits from the OS), `kind`, `target`, `change` (JSON, the daemon's before and after), `facts` (JSON: the card's lines as message ids with typed arguments), `parts` (JSON or null: the parts a narrowable kind offers, each with its id and content hash), `parts_allowed` (the subset the user kept), `reason` (the agent's text, at most 300 characters, set to null when the request closes), `session`, `tool`, `verified`, `root`, `folder`, `organization`, `pid`, `pid_start` (the asking process's start time), `idem_key` (the caller's `--key`), `state`, `answer_on` (`mac` or `mac_or_phone`), `applied_by` (`daemon` or `app`), `created_ms`, `expires_ms`, `answered_ms`, `by` (`mac`, `phone`), `used_ms`, `used_op_id`, `result` (JSON or null: what the action returned, when it holds no secret), `closed_ms`. A closed row is deleted 7 days after it closed; nothing waiting expires but by its own end time.

**States.**

| State | Means | Next |
| --- | --- | --- |
| `waiting` | open, not answered | `opened`, `applying`, `denied`, `expired`, `cancelled`, `withdrawn` |
| `opened` | app-applied only: the user pressed Allow and the app's sheet for it is open (a secret request's form) | `applying`, `applied`, `denied`, `expired`, `withdrawn` |
| `applying` | the yes is taken and the action runs: a daemon-applied kind between the answer and the write, or an app-applied action that ends later (an account's sign-in, a Worker's switch through the home) | `applied`, `failed`, `withdrawn` |
| `applied` | done; `result` holds what it returned | closed |
| `failed` | the yes was taken and the action failed | closed |
| `denied` | the person said no (Deny, or Cancel in the app's sheet) | closed |
| `expired` | the end time passed first | closed |
| `cancelled` | the asking agent withdrew it | closed |
| `withdrawn` | the facts changed, the target is gone, a restart came while `applying`, or a cross-node apply didn't land in 5 minutes | closed |

The first answer wins: a mark in memory under one lock, then one `UPDATE … WHERE state IN ('waiting', 'opened')`, as Ask does. `opened` moves the end time to at least 10 minutes after the sheet opened, once, so a user filling the form isn't cut off. At a restart, `waiting` stays `waiting`, `opened` goes back to `waiting` (the sheet is gone), and `applying` is `withdrawn` (a yes is for this run).

**Kinds.** A kind is a `KindSpec` in `crates/engine/src/consent/kinds.rs`: its name, title, the typed arguments it takes and how they're checked, how the facts are computed and checked again, whether it has parts, who answers (`answer_on`), who applies (`applied_by`), what its result is (`none`, `stored`, or `handover`: a secret, below), the Inbox kind it raises, and its default and longest expiry. Phase 1 has two, the ones it folds in; each later phase adds its own.

| Kind | Phase | Target | Phone may Allow | Applied by | Result |
| --- | --- | --- | --- | --- | --- |
| `setting` | 1 | an Ask-class settings key and value | yes | the daemon, as settings asks today | stored: today's applied reply |
| `secret_request` | 1 | a Vault entry to add or update | no | the app, by `vault.save {consentId}` | stored: the entry, never its value |
| `switch` | 3 | one of the six switches, on | no | the app | none |
| `session` | 4 | start a session, or drive one the agent doesn't own | no | the daemon | stored: the session id |
| `session_stop` | 4 | stop or interrupt a session the agent doesn't own | yes | the daemon | none |
| `dispatch` | 5 | a job on a machine | no | the daemon | stored: the job id |
| `job` | 5 | tail or return another session's job | no | the daemon | none |
| `job_stop` | 5 | stop another session's job | yes | the daemon | none |
| `cluster` | 6 | a cluster admin action | no | the app | handover for an invite's token, else none |
| `account` | 7 | an account or lane action, same organization | no | the app | stored: the account id |
| `machine` | 8 | add, fix, update or remove a machine; the Tailscale network | no | the app | none |
| `install` | 8 | Set Up files, with parts | no | the app | stored: the files written |
| `cost_profile` | 8 | Cost Profile set, apply, undo, with parts | no | the app | stored: the files written |
| `update` | 8 | a tool update on this Mac or a worker | no | the app | none |

A **handover** result is a secret the asker needs (an invite's join token). It is kept in the daemon's memory only, never in the row, an event, the Inbox or the card; handed once to a `consent.get` from the asking verified session; and dropped after the handover, at the request's end, or at a restart. A kind with a handover result refuses a caller in no verified session (`no_session`).

**Who may read or withdraw a request.** `consent.get` and `consent.cancel` are served:

- for a request from a verified session, to a caller in the same verified session (any process of it, so a later command, a reconnect after a restart, or the main session for its subagent's request);
- for a request from no verified session, to any caller that presents the id: the 128-bit id is the capability, the agent tier is this user only, and `consent.mine` lists nothing for such callers, so the id is known only to whoever received it. This holds across restarts.

Anyone else gets `forbidden`, reason `other_session`, "Only the agent that asked can see this request." A handover result goes only to the verified-session case.

**Ops.**

| Op | Tier | Args | Reply |
| --- | --- | --- | --- |
| `consent.get` | agent, read | `{id, waitMs?}` | `Consent`; with `waitMs` (0 to 50,000) it waits until the state is closed or the time ends |
| `consent.mine` | agent, read | `{states?: [..]}` | `{requests: [Consent]}`: the caller's verified session's requests, newest first; empty for anyone else |
| `consent.cancel` | agent, acts | `{id}` | `{cancelled: bool, state}` |
| `consent.list` | app, quiet | `{}` | `{requests: [Consent]}`, every request on this node and the mirrored ones it may answer, waiting first |
| `consent.answer` | app | `{consentId, decision: "allow" \| "deny", parts?: [id]}` | `{answered: bool, state, by?}`. Allow only for `applied_by: daemon` kinds; Deny for every kind, and the app's sheet Cancel uses it |
| `consent.opened` | app | `{consentId}` | `{state}`: the app's sheet for an app-applied request is open |

`inbox.answer` on a request's item and `consent.answer` drive the same transition: both call `ConsentOps::decide(id, decision, parts, by)`, the item's responder only marks, and the first answer wins.

A feature op opens the request itself, because it computes the facts: `settings.set` for an Ask key and `vault.request` in phase 1. There is no generic open op. App-applied kinds are applied by the app calling the real op with `consentId`: the daemon checks that the request is `waiting` or `opened` (or `applying` for the second half of a long action), that the op and its arguments equal the scope (or name a subset of `parts_allowed`), that it hasn't ended, and that the caller is the app that said hello; then it records the answer (`by: mac`) and the use in one step and runs the op. `consentId` is the one name on every app op; `vault.save` still reads today's `request` from an older app, for one release.

**Wire shapes.** The agent-side options go in one object, `consent`, beside the op's own arguments, and `dryRun` beside it:

```json
{"op": "settings.set", "args": {"key": "chatShortcut", "value": "cmd+shift+k", "wait": 0,
  "dryRun": false, "consent": {"reason": "The user asked for a shorter shortcut.", "expiresInMs": 900000, "key": "a1b2c3"}}}
```

`consent.key` is the idempotency key (named inside `consent` so it can't clash with `settings.set`'s own `key`). A Free key or an app-tier call ignores `consent`.

`settings.set` replies:

- a Free key, or the app tier: `{key, value, state: "applied", changed}`, as today;
- an Ask key from the agent tier: today's `Ask`, `{id, key, value, state, message}`, with `id` the request's id and `state` the request's (`waiting`, `applying`, `applied`, `denied`, `expired`, `withdrawn`, `failed`), and `value` the value asked for, plus `request: Consent`;
- `settings.asked {id, wait?}` stays, and replies the same `Ask` with `request`; it is `consent.get` in today's shape.

`vault.request` takes `{name, kind?, url?, note?, how?, expires?, reason?, dryRun?, consent?: {expiresInMs?, key?}}` (its `reason` is the request's reason) and replies at once `{state: "waiting", request: Consent}`; there is no `entry` until the user saves. Once applied, `Consent.result` is `{entry: Entry}`, today's reply.

`Consent`: `{id, kind, target, change, facts, parts?, partsAllowed?, state, answerOn, appliedBy, createdMs, expiresMs, answeredMs?, by?, usedMs?, session?, tool, verified, organization?, scope: {once: true, session?, until}, result?, handover?, reason?}`. `reason` goes back only to the asker and the app, while the request is open. `handover` is present once, as above.

A dry run (`dryRun: true`) changes nothing and opens nothing, and replies:

```json
{"dryRun": true, "op": "settings.set", "would": "ask",
 "change": {"key": "chatShortcut", "before": "cmd+k", "after": "cmd+shift+k"},
 "request": {"kind": "setting", "facts": [{"line": "consent-setting-change", "args": {"key": "chatShortcut"}}], "answerOn": "mac_or_phone", "expiresInMs": 900000},
 "error": null}
```

`would` is `apply` (a Free key: `request` is null), `ask` (an Ask key or a secret request), or `refuse` (a Never key, a switch, a bad value: `request` is null and `error` holds the JSON error object of [7.3](#73-step-3-the-cli-contract-and-discovery)).

**Idempotency.** `consent.key` (the `--key` flag) is scoped to the verified session, or for anyone else to the root app. Within 24 hours:

- the same key with the same op and arguments returns the first request, whatever its state now: a request that was denied comes back denied, so a fresh ask needs a fresh key;
- the same key with another op, target or value is `bad_request`, reason `key_reused`;
- a call that opens no request (a Free key) ignores the key. `help --json` marks the commands where `--key` counts (`idempotent: true`): in phase 1 `settings set` and `secret-request`, later the ones that start a session or a job.

**Limits and refusals.** The rate limits and expiry limits of [5](#5-consent-card-rules), rules 7 and 8, are checked at open, in `crates/engine/src/consent/rules.rs`, under the book's lock. Over a limit: `failed` with `data: {reason: "busy", retryAfterMs}`. In the cool-down after a Deny: the same, with the time left. The checks for the action itself come first: an action on the user-only list is refused with reason `never` before any limit is counted; a target in another organization with `other_organization`. When the 20 waiting fill, the app shows one Inbox notice with Deny All Waiting.

**With `acting` off and no app.** A request opens with `acting` off (it may be the request to turn it on) and with no app attached: it is saved, the reply says `waiting`, and the card appears when the app attaches. Only the Mac's app and, for the kinds marked so, a phone with the bridge and `acting` on can answer.

**The Inbox item.** One per waiting request, raised at open: kind `consent` for the new kinds; the two folded kinds keep `setting_change` and `secret_request`. The protocol's `kind` is a free string and a phone shows an unknown one as a notice with its actions by label, so the protocol changes only in the kind's description. Id `consent:<id>:<ms>`, fresh on every raise. Title and detail are the daemon's facts in words (`<who> asks to <action>`, cut to 80 and 400 characters); the reason is never in the item. Actions:

- a kind a phone may allow: `allow` and `deny`;
- any other kind: `deny` and `open` ("Open on Mac"); there is no `allow` action in the item, so no phone, old or new, shows an Allow it can't use. The Mac's app draws its own Allow on the card from the consent feed (`consent.list`), not from the item's actions.

The responder only marks; the consent loop applies, closes and withdraws. A request that ends resolves its item; an item that loaded Withdrawn after a restart is replaced by a fresh raise of the request, which is still waiting.

**Waiting and restarts.** A waiting request survives a restart: `ConsentOps::load` runs between bind and serving, after the Inbox's load, records `restored` and raises its item again. A 1 s tick ends requests past their end as `expired`. The command long-polls `consent.get` (or `settings.asked` for `settings set`), 50 s a call, and reconnects with the same id after a restart, as `ask` does.

**Folding settings asks in.** `settings_asks.rs` goes; `settings.set` for an Ask key opens a `setting` request. Changes: with no app following the Inbox the ask is saved instead of refused with `needs_approval`; the default end is 15 minutes instead of 300 seconds; the limit is the per-session one instead of 8 in all. Events: `settings.agent` keeps its line at open (`asked`, committed before, as today) and at the end (`allowed`, `denied`, `expired`, `withdrawn`, `failed`); `no_app` and `full` are no longer produced. So an Ask key gives `agent.called` (`pending`), `settings.agent` and `agent.consent`, each with its own job: the call, the setting's history, the request's life.

**Folding vault requests in.** `vault.request` opens a `secret_request` request instead of holding a wait. It opens with the vault off too, so an agent can always ask. A secret request never turns the Vault on: with the vault off, the card says "The Vault is off", its Allow is disabled, and it shows Open Vault Settings. Turning the Vault on is the user's own act in Settings, with its own confirm, or a `switch` request of its own (phase 3) whose card says it opens every saved secret to agents. Once the Vault is on, the card's Allow opens the form (`consent.opened`); the form's Save sends `vault.save {consentId}`, which applies it; its Cancel sends `consent.answer {consentId, decision: "deny"}`. `vault.request.cancel` stays for an older app and does the same. The agent withdraws with `consent.cancel`. The prefilled fields (`url`, `note`, `how`) are agent text the user edits; the form marks them as the agent's.

**Events.** One kind, `agent.consent`, like `session.ask`. Fields, all of them from phase 1 so the golden doesn't churn as phases add kinds:

- `consent` (`Ref<Consent>`, always hashed), so the request's lines join up;
- `then`: `asked`, `refused`, `opened`, `allowed`, `denied`, `expired`, `cancelled`, `withdrawn`, `applied`, `failed`, `restored`, `handed_over`;
- `kind` (new choice `ConsentKind`, every kind of the table above), `by` (Answerer), `answer_on`;
- the target, as typed fields, each optional: `switch` (SwitchName), `key` (SettingKey), `secret` (`Ref<Secret>`), `session` (`Ref<Session>`, the target session, apart from the envelope's asking session), `machine` (`Ref<Machine>`), `account` (`Ref<Account>`), `lane` (`Ref<Lane>`), `job` (`Ref<Job>`), `action` (new choice `ConsentAction`: the cluster, account, machine, install, cost and update actions), `tool` (AgentTool);
- the counts `reason_chars`, `parts`, `parts_allowed` and `waited` (ms).

`Lock::Always`, Audit and Activity, `agents_read: Own`. Never the reason, a value, a token or a path.

**What the user sees.** A card under the asking session in Chat when the session is on this Mac (beside the Ask cards), the Inbox row everywhere, and the notch's badge. The card follows [5](#5-consent-card-rules). Words in `locales/en/mac/consent.ftl`.

**Across machines.** The row lives on the owner node. The Inbox item reaches the home and the Can Be Master members through the existing mirror; an answer given there goes to the owner through `InboxOps::decide`, as a Worker's Ask answer does, carrying `by`; the owner checks `by` against the kind's rule (a phone's Allow of a Mac-only kind is not taken). `consent.get`, `consent.mine` and `consent.cancel` stay local, because the row is local. Phase 1 needs nothing more, since both of its kinds act on the owner node. Phases 3 and 5 add the two cases where another node acts ([9](#9-phase-3-switch-requests), [11](#11-phase-5-jobs)). An app-applied kind whose owner node has no app and that the cluster can't apply for it is refused there at open, reason `no_app`, "Open Notchling on «node» to allow this."

**Files.**

- New: `crates/engine/src/consent/` (`mod.rs`, `model.rs`, `store.rs`, `rules.rs`, `kinds.rs`, `facts.rs`, `inbox_item.rs`, `idem.rs`, and `*_tests.rs`); `crates/notchlingd/src/consent_ops.rs`, `consent_wiring.rs`, `consent_tick.rs`, `consent_inbox.rs`, `consent_handover.rs` and their tests; `crates/app/src/consent/` (the feed and the card model) and `crates/app/src/chat/view/consent_cards.rs`; `locales/en/mac/consent.ftl`.
- Changed: `crates/engine/src/store.rs` (schema 10), `store_class.rs`, `inbox/model.rs` (`Kind::Consent`); `crates/local-api/src/wire.rs` (the `consent.*` names); `notchlingd/src/settings_ops.rs`, `settings_cli.rs`; `vault_ops.rs`, `vault_inbox.rs`, `vault_cli.rs`; `daemon_wiring.rs` (one call into `consent_wiring`); `protocol/agentconsole.v1.schema.json` (the inbox kind's description adds `consent`); the app's `inbox/`, `vault/` sheet and `settings_live.rs`.

### 7.3 Step 3: the CLI contract and discovery

**One exit-code table.** Every command but the hooks and the sidecar (which always exit 0 and print nothing on trouble) uses these codes:

| Code | Name | Meaning | What the agent does |
| --- | --- | --- | --- |
| 0 | `ok` | Done: applied, answered, allowed and applied, or read | go on |
| 1 | `failed` | Notchling tried and it didn't work, or the request was withdrawn | read the error; maybe retry |
| 2 | `bad_request` | The command line or a value is wrong; nothing changed | fix the command |
| 3 | `not_running` | No daemon, no answer, a socket Notchling doesn't trust, or a daemon older than this command | ask the user to open Notchling |
| 4 | `refused` | Not allowed: a switch is off, user-only, another session's, another organization's | follow `error.ask`, or stop when `never` |
| 5 | `denied` | The person said no, dismissed or cancelled | stop; don't ask again at once |
| 6 | `pending` | The request is saved and still waits; its id is in the output | check later with `notchling consent <id>` |
| 7 | `busy` | A limit was reached | wait `retryAfterMs`, then retry |
| 124 | `expired` | No answer in time; the request is closed | ask again only if still needed |
| 125 | `stopped` | `secret` only: Notchling stopped before running the command | read stderr |

**`secret` runs a command,** so its codes can't be the table's. It exits with the command's own code once the command ran; 125 when Notchling stopped before running it (any reason: refused, denied, no daemon, a bad name), 126 when the command can't be run and 127 when it isn't found, as `env` and `timeout` do. `--json` goes before the names, `notchling secret [--json] <names> -- <command>`, and adds one JSON error line on stderr when it exits 125, the error object below with `exit: 125`. Without `--json` the stderr line is the message. Nothing else Notchling writes goes to stderr once the command started.

**Codes that change.** One move, with the skills in the same change *(decided)*; the table is then fixed, and later only grows.

| Command | Today | From phase 1 |
| --- | --- | --- |
| `settings` | 1 refused, 2 trouble, 3 not allowed | 4 or 2, 3 or 1, and 5, 6 or 124 |
| `models`, `cost`, `cluster`, `session reload` | 1 refused, 2 trouble | 4 or 2, and 3 or 1 |
| `ask` | 1 gone or usage, 2 dismissed, 3 refused or no daemon, 124 | 1 gone, 2 usage, 5 dismissed, 4 refused, 3 no daemon, 124 |
| `answer` | 2 already answered, 3 refused or no daemon | 1 with reason `already_answered`, 4, 3 |
| `secret` | 2 usage or trouble, 3 said no | 125 for every stop of its own; 126, 127 |
| `secret-request` | 1 said no, 2 trouble | 5, 3 or 1, and 6 or 124 |
| `secrets`, `pages`, `page`, `delegate`, `delegate-models` | 2 trouble | 3 or 1; usage 2 |
| `status`, `sessions` | 3 not running, 1 else | unchanged |

Every non-zero exit without `--json` ends its stderr line with the code's name ("… (denied)"), so an agent holding old skill text still reads the meaning from the words.

**stdout and stderr.**

- With `--json`, stdout holds exactly one JSON value, success or failure (`watch` writes one per line), and Notchling writes nothing to stderr: no waiting note, no message copy. A caller that captures `2>&1` still gets one value.
- Without `--json`, results go to stdout, and errors, waiting notes and progress to stderr.
- `secret` is the one exception, as above.

A success prints what it prints today, so readers of today's output keep working. A failure prints:

```json
{"error": {"exit": 4, "code": "forbidden", "reason": "switch_off", "message": "the cluster is off: this Mac links to no other machine until the maintainer turns this on", "op": "cluster.dispatch", "switch": "cluster", "ask": {"command": "notchling switch on cluster", "kind": "switch"}, "retryAfterMs": null, "request": null, "never": false, "phase": null}}
```

- `code` is the wire `ErrorCode`, or the command's own (`usage`, `not_running`, `denied`, `pending`, `expired`, `busy`, `stopped`).
- `reason` is one of a closed list: `usage`, `bad_value`, `unknown_key`, `key_reused`, `not_running`, `untrusted_daemon`, `old_daemon`, `timeout`, `switch_off`, `app_only`, `no_app`, `not_yet`, `never`, `internal_op`, `other_session`, `other_organization`, `no_session`, `needs_consent`, `busy`, `denied`, `cancelled`, `expired`, `withdrawn`, `already_answered`, `failed`, `internal`. The daemon sends it in the error's `data.reason` (additive: `ApiError` already carries `data`); the command fills in only its own (`usage`, `not_running`, `untrusted_daemon`, `old_daemon`, `timeout`).
- `message` is the fixed text, never a value or a parse error that could quote one.
- `ask` says how to get it allowed: the command and the consent kind. For an op whose agent path is planned for a later phase, the reason is `not_yet` with its `phase` and `ask: {"command": "notchling ask -- …"}`, so the agent can still ask the user in words. A user-only op has `never: true` and no `ask`.
- `request` is the `Consent` when one was opened (exit 6, 5 or 124).

**An updated Notchling with the old daemon still running.** The command and the daemon ship in one bundle, but a daemon keeps running across the swap until the app restarts it. The command reads the daemon's version from `hello`; a newer command that gets `unknown_op` from an older daemon exits 3, reason `old_daemon`, "Notchling was updated. Open it to finish, then try again." A newer daemon with an older app keeps publishing `vault.request.pending` on the vault topic and reads `vault.save {request}` for one release, so a secret request still works until the app restarts.

**Common flags.** On every acting command: `--json`; `--reason <text>`; `--wait` (the default: wait until the request ends or `--timeout`, default 900 s) or `--no-wait` (return at once, exit 6 with the id when a request opened); `--timeout <seconds>`; `--expires-in <seconds>`; `--key <text>`; `--dry-run` (print the dry run's reply, exit 0, or 2 or 3 when it can't be worked out). When a wait ends with the request still waiting, the command exits 6 and the request goes on; it isn't cancelled.

**New commands.**

```
notchling consent <id> [--wait] [--timeout <seconds>] [--json]   one request: its state, and its result once it ends
notchling consents [--json]                                     this session's requests, newest first
notchling consent cancel <id> [--json]                          withdraw a request this session opened
notchling help [--json]                                         every command; --json for agents
notchling capabilities [--json]                                 what this caller may do now, and why not
```

`notchling consent <id>` exits by the request's state: `applied` 0; `waiting`, `opened`, `applying` 6; `denied` and `cancelled` 5; `expired` 124; `failed` and `withdrawn` 1. `secret` and `secret-request` gain `--json`. `settings set` and `secret-request` gain the common flags. `settings set --json` prints the final `Ask` reply, as today but with `request`; `secret-request --json` prints `{entry, request}` once applied.

**The registry.** `crates/notchlingd/src/commands.rs` holds one row per command (its words, usage, summary, flags, ops, consent, whether `--key` counts, the exit codes it may return and its skill) and three lists, which together with the commands hold every op exactly once:

| List | Holds | `data.reason` for an agent | `capabilities` `now` |
| --- | --- | --- | --- |
| `PLANNED` | an op whose agent path comes in a named phase: op, feature, phase | `not_yet`, with `phase` | `not_yet` |
| `USER_ONLY` | an op of [6](#6-user-only-actions): op, feature, why | `never` | `never` |
| `INTERNAL` | an op only Notchling's own processes call: op, why (`app_ui`: the app's own view, such as `git.*` polls or `cluster.side_copy.hide`; `hook`: the hook and sidecar ops; `node`: ops cluster nodes call) | `app_only` for `app_ui`, `internal_op` for `hook` and `node` | `app_only` or `internal` |

`Handler::agent_path(op)` answers from the registry, so the daemon, not the command, says why: `conn.rs`'s tier refusal of an app-tier op from an agent puts `{reason, phase?, ask?, never?}` in the error's `data`. An `AGENT_HOOK` op is not refused by tier (an agent may call it, and gets nothing useful); `capabilities` lists it as `internal` and `help` doesn't list it.

`usage()`, `help --json`, `capabilities` and the docs come from the registry. A test checks that every `Command` variant has a row, that every row's usage parses, and that each row's ops have the tier and `acts` their `OpSpec` says. A golden test writes `crates/notchlingd/tests/golden/agent-paths.json` and `docs/agent-paths.md` (`NOTCHLING_UPDATE_GOLDEN=1`), as the log kinds do.

**`help --json`.** Static, from the registry:

```json
{"notchling": "2.1.0", "api": 1,
 "exitCodes": [{"code": 0, "name": "ok", "meaning": "Done."}, …],
 "reasons": ["usage", …],
 "commands": [{"command": "settings set", "usage": "notchling settings set <key> <value> [--json] [--reason <text>] [--no-wait] …",
   "summary": "change a setting; an Ask key waits for your yes", "ops": [{"op": "settings.set", "tier": "agent", "acts": true}],
   "consent": "by_key_class", "json": true, "waits": true, "idempotent": true, "exits": [0, 1, 2, 3, 4, 5, 6, 7, 124], "skill": "notchling-settings"}, …],
 "planned": [{"feature": "Start a session", "ops": ["session.new"], "phase": 4}, …],
 "userOnly": [{"feature": "Answer a permission prompt", "ops": ["session.permission", "agy.permission"]}, …]}
```

`consent` is `none`, `request` (always opens one), `by_key_class`, `own_free` (free for what the caller owns, a request otherwise) or `never`. `notchling --help` prints the same registry as text, so the two never drift.

**`capabilities --json`.** A new op, `daemon.capabilities` (agent, read), in `notchlingd/src/capabilities_ops.rs`:

```json
{"api": 1, "daemon": "2.1.0",
 "caller": {"tier": "agent", "tool": "claude", "session": "…", "verified": true, "folder": "…", "organization": {"id": "…", "name": "Personal"}},
 "app": {"attached": true, "followsInbox": true},
 "switches": {"acting": false, "bridge": false, "accounts": false, "vault": true, "install": false, "cluster": false},
 "consent": {"waiting": 1, "openLeft": 2, "thisHourLeft": 9, "longLeft": 1, "coolingDown": [{"kind": "setting", "target": "theme", "untilMs": 0}]},
 "ops": [{"op": "settings.set", "now": "open"}, {"op": "cluster.dispatch", "now": "needs_switch", "switch": "cluster"},
         {"op": "session.new", "now": "not_yet", "phase": 4}, {"op": "session.permission", "now": "never"}]}
```

`now` is `open`, `needs_consent`, `needs_switch`, `app_only`, `not_yet`, `never` or `internal`, for this caller at this moment. No secret, email, account name, folder of another organization or Inbox text is in it; a caller in no verified session gets no `folder` and no `organization`.

**The guard check.** `GUARD-AGENT-PATH`, `PYTHONPATH=tools/guard python3 -m guard agentpaths` (`tools/guard/guard/agentpaths.py`, tests in `tools/guard/tests/test_agentpaths.py`, a row and a bad example in `docs/rules.md`):

1. Every op in `wire.rs`'s `mod ops` is in `agent-paths.json` exactly once: as a command's op, or in `planned`, `userOnly` or `internal`, with the fields that list needs.
2. `guard gate` sends a change where `planned`, `userOnly` or `internal` grew to the maintainer, as `GUARD-PROVIDER-GAPS` does: a new op arrives with its command, or the maintainer decides it is user-only or internal. The check fails when `wire.rs` has no `mod ops` constants, so an empty read never passes.
3. Every file under `docs/superpowers/specs/` (subfolders too) is named `YYYY-MM-DD-name.md`, and one dated 2026-11-21 or later has an `## Agent path` section with a line or table row that names a `notchling` command and an op (a file name such as `settings.json` is not an op), or a line that starts "No agent path:" with a reason from the `INTERNAL` reasons.

**Files.**

- New: `crates/notchlingd/src/commands.rs`, `contract.rs` (the exit table, the reasons, the JSON error writer, the stdout and stderr rule, the common flags, the old-daemon check, shared by every `*_cli.rs`), `help_cli.rs`, `consent_cli.rs`, `capabilities_ops.rs`, and their tests; `tests/golden/agent-paths.json`; `docs/agent-paths.md`; `tools/guard/guard/agentpaths.py` and its tests.
- Changed: `cli.rs` (`usage()` from the registry, the new commands), `clients.rs`, `ask_cli.rs`, `settings_cli.rs`, `models_cli.rs`, `cost_report_cli.rs`, `cost_setup_cli.rs`, `cluster_cli.rs`, `reload_cli.rs`, `pages_cli.rs`, `vault_cli.rs`, `delegate_cli.rs` (the table, the JSON errors, nothing on stderr with `--json`); the refusals that know their reason add `data.reason` (`switches.rs`, `acting_ops.rs`, `conn.rs`'s tier refusal); `tools/guard/guard/__main__.py`; `docs/rules.md`.
- Skills: the `notchling-*` skills get one shared section, generated in `crates/engine/src/install/skills.rs`: the exit-code table, "run `notchling help --json`", "a request may wait; run it in the background and check with `notchling consent <id>`", "exit 4 with `never` means stop", "read the name at the end of the error line". `settings/skill.rs`, `vault/skill.rs`, `ask/skill.rs` and the delegate's skill change their exit codes.
- Docs: `docs/local-api-ops.md` (the new ops, the folded ones, the `data.reason` list), `docs/notes/daemon.md` (op classes, identity, the record), `docs/notes/inbox.md`, `docs/notes/data-and-logs.md` (the table), `docs/feature-map.md` (`src/consent/`), `docs/policies/privacy.md` (the reason stays on the Mac and is deleted at close).

### 7.4 Phase 1 tests

Rule by rule. Fixtures only; no test touches the real `~/.claude`, Keychain or a real login.

- **Classes.** Every agent-tier op in `wire.rs` is `AGENT_READ`, `AGENT_ACTS` or `AGENT_HOOK`; no op that writes, runs or touches the vault is `AGENT_READ`.
- **The record.** An acting call records one `agent.called` when it succeeds, is refused, fails, opens a request (`pending`, with the hashed request ref) or is a dry run (`dry_run`); a read records none; an app call records none; a handler that marked its own kind still gets one; with every log toggle off it still lands; the Audit line carries the session and the executable; the server finds `pending` through the scope note, and a handler reply that merely looks like a request without the note is recorded `ok`.
- **Identity.** An install under the install folders is verified; a re-pointed `~/.local/bin/claude`, a copy of the binary elsewhere, a process merely named `claude`, an npm install running as `node`, and a `sessions/<pid>.json` planted for a shell are not; a `notchling delegate codex` child is `codex` with the delegating session; a process under a Codex session the daemon started is `codex` with that session; a shell is `shell` with none and its root app; the organization comes from the session's folder, running the command from another folder doesn't change it, and an unverified caller has none.
- **`agent.named`.** A background command's output file names the subagent; two matches are `unknown`; the event holds no label text; a burst of 100 calls from one session still resolves each call's label (from its own evidence), and the lazy lookup gives the same answer as the eager one.
- **The request.** Every state change of the table, and no other; first answer wins (Chat, Inbox and phone at once, through `consent.answer` and `inbox.answer`); every limit (3 waiting, 10 an hour, 20 in all, the long-end limits, the cool-down, the root-app bucket, Deny All Waiting); the end time, and `opened` moving it once; restart keeps a waiting request and raises a fresh item, sends `opened` back to `waiting` and withdraws `applying`; the facts checked again (a setting changed meanwhile is withdrawn); a phone Deny works, a phone Allow of a Mac-only kind is not taken, and a Mac-only item carries no `allow` action; no agent-tier op answers; opens with `acting` off and with no app attached.
- **Who may read.** The same verified session reads and withdraws from a new process and after a restart; another verified session is refused; for an unverified request the id alone works, `consent.mine` is empty, and a handover kind is refused.
- **Idempotency and dry runs.** The same key returns the same request in every state; a reused key with another value is `key_reused`; a Free key ignores the key; a dry run of a Free, an Ask and a Never key and of a secret request gives the shapes above and changes nothing.
- **Untrusted text.** A planted token in `--reason` appears in the `consents` row, the asker's reply and the Mac card only, and is gone from the row once the request closes: never in the Inbox item, an event, the Audit, Activity, the phone feed or a log.
- **The folds.** Today's settings ask tests pass with the new behavior (saved with no app; 15 minutes); a secret request with the vault off is saved, its card's Allow stays disabled, the Vault stays off, and once the user turns it on in Settings the form opens and Save applies it; Cancel denies it and leaves the Vault as it was.
- **The contract.** Each command's exit codes, table-driven over a fake link; the JSON error on stdout and nothing on stderr with `--json`, for every reason; `secret` passes its command's code through and exits 125 for its own stops, with the JSON line on stderr only then; `old_daemon` against a fake older daemon; `help --json` golden; `capabilities` for a verified session, a shell and an app; an app-tier op from an agent carries the registry's reason.
- **End to end** (`crates/notchlingd/tests/agent_e2e.rs`, over `tests/common` as `ask_e2e.rs` does, with a fake Claude Code under a fixture install folder): a verified session runs `notchling settings set <ask key> <value> --json --no-wait`, gets exit 6 and a request id; an app client allows it through `inbox.answer`; the daemon restarts in between; `notchling consent <id> --json` from a new process of the session exits 0 with `applied`; the Audit holds two `agent.called` lines and `agent.consent` `asked`, `restored`, `allowed`, `applied`.
- **Guard.** `test_agentpaths.py`: an op in no list fails; an op in two lists fails; a planned or internal list that grew is flagged by `gate`; a dated spec without `## Agent path` fails.

Phase 1 changes today's exit-code tests and the settings ask tests in its branch, so the suite passes; those files go to the maintainer with the change ([Needs the maintainer](#needs-the-maintainer), 11).

## 8. Phase 2: free reads and `notchling watch`

Steps 4 and 5: what an agent may read without asking, and a way to follow events instead of polling.

**Ops and commands.**

| Command | Op | Tier | Returns |
| --- | --- | --- | --- |
| `notchling search <words> [--kind --since] --json` | `search.query` | agent, read (exists) | pages and sessions of the caller's organization only |
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
| `notchling receipts [--json]` | `outbox.mine` with receipts | agent, read | the delivery receipts of the caller's session's messages |
| `notchling roles [--json]` | `catalog.roles` | agent, read | the role table |
| `notchling watch [--topic <t>]... [--until <state>] [--count <n>] [--json]` | `events.subscribe` | agent, scoped topics | one JSON line per event |

`watch` follows new **scoped topics**: `mine.consents`, `mine.asks`, `mine.jobs` (phase 5), `mine.sessions` (phase 4), and the public `settings.changed` and `settings.switch`. The hub delivers a scoped event only to subscribers whose verified session owns it; an unverified caller gets the public ones only. `watch` reconnects like `ask` and exits 0 on `--until` or `--count`, 124 at `--timeout`.

**Settings classes.** `hiddenAgents` moves from Never to Free (it hides a row in the notch); `newSessionChoice` from Never to Ask (it picks the account a new session uses). Ask accepts verified Codex and Antigravity sessions the daemon started, now that identity names them.

**Consent class.** None: every op here is a read or a Free key.

**Safety rule.** Reads are scoped to the caller's organization and redacted: no email in full, no token, no secret value, no folder or Inbox text of another organization or another session, no question or answer text but the caller's own. A caller in no verified session has no organization, so it reads nothing organization-scoped: `search`, `inbox` (beyond the requests whose ids it holds), `accounts`, `limits`, `orgs`, `outbox`, `activity`, `prompts` and `receipts` answer empty with `reason: no_session` in the reply. It is never mapped to Personal. It may read the switches, awake, versions, roles and the public topics.

**Tests.** A planted email, token, folder and question of another organization and another session appear in no reply and no watched event; each read for a verified session, a shell and another organization's session; a shell gets no Personal data; scoped topics reach only the owner; `watch --until applied` ends when the request is applied; the free and changed settings classes.

## 9. Phase 3: switch requests

Step 6, on phase 1's request. The decision record covers the rule *(decided)*.

**Ops and commands.**

| Command | Op | Tier | Consent |
| --- | --- | --- | --- |
| `notchling switch on <acting\|bridge\|accounts\|vault\|install\|cluster> [common flags]` | `switch.request {switch, on: true}` | agent, acts | `switch`, Mac only, applied by the app |
| `notchling switch off <name> [--json]` | `switch.off {switch}` | agent, acts | none *(decided)* |

The app applies an allowed request with its own op and the request's id: `settings.set_acting {on, consentId}`, `bridge.set {on, consentId}` or `settings.set_switch {switch, on, consentId}`, sent at once when the user presses Allow, so answer and use are one step. The event is `switch.changed` with the new cause `agent_request`. `switch off` is free and recorded; the app shows a notice, "«agent» turned off the cluster", with Turn On. The card for `vault` says it opens every saved secret to agents that may use them, for this run.

**Consent class.** Mac only: a phone may Deny, never Allow. The card says what the switch unlocks (fixed words per switch), that it is off again at the next start, and who asks.

**Safety rule.** No tier turns a switch on through a request by itself: the daemon only checks the request, and the switch goes on only by the app's call with the id, from the process that said hello. Every switch is still off at every start; a yes lasts for this run only. A request for a switch that is already on is refused (`bad_request`, "It is already on.").

**On a Worker.** The request lives on the Worker (its owner node) and its item reaches the home through the mirror, but only while the Worker's cluster link is on; with it off, the request can be answered only by the Worker's own app (none on a headless Worker: refused at open, `no_app`). The home can apply only what the cluster may set: `acting`, the bridge and `install`. On Allow, the home's app calls `cluster.member.set {node, desired, consentId}`, the existing path for a member's switches; the forwarded answer moves the Worker's row to `applying`; the Worker applies `desired` as it does today, from `Members`/`HelloOk` while no app is attached, and marks the row `applied` when the switch goes on, or `withdrawn` after 5 minutes. `vault` and `accounts` are never applied through the cluster: a Worker's request for them is refused at open with `no_app` unless the Worker has an app attached. The Worker trusts the forwarded `by` because the home is its pinned peer.

**Tests.** Request, Allow, the app's call with the id, the switch on, `agent.called` and `agent.consent` recorded; an agent-tier `settings.set_switch` with a valid id is still `forbidden`; a phone Allow is not taken; a restart leaves every switch off and withdraws an `applying` yes; the id can't be used twice or for another switch; `switch off` from an agent works and raises the notice; a Worker's `acting` request answered at the home lands through `desired`; a Worker's `vault` request is refused with `no_app`; a forwarded phone Allow is not taken on the Worker.

## 10. Phase 4: sessions

Step 7.

**What an agent owns.** The daemon records who started a session: `session.new` from a verified session stores `started_by` (the session id), in the daemon's own launch record. A session the caller's verified session started is its *child*. Owning a child lets the agent talk to it and end it, nothing more. Its own session is not a child: an agent acting on itself gets nothing free but `session reload self`, which exists today.

**Ops and commands.**

| Command | Ops | On a child | On itself | On another session |
| --- | --- | --- | --- | --- |
| `notchling session new --folder <f> [--tool claude\|codex\|agy] [--model --effort] [-- <first message>]` | `session.new`, `folder.trust`, `codex.*`, `agy.open` | n/a | n/a | `session`, Mac only |
| `notchling session reply <id> -- <text>` | `session.reply`, `session.send`, `codex.send`, `agy.send` | free | n/a | `session`, Mac only |
| `notchling session stop\|interrupt <id>` | `session.stop`, `session.interrupt`, `codex.stop`, `agy.cancel` | free | free | `session_stop`, Mac or phone |
| `notchling session remove <id>` | `session.remove`, `codex.delete` | free once it has stopped | no | `session`, Mac only |
| `notchling session answer <id> --choice <n>` | `session.answer` (a session's question, never a permission) | `session`, Mac only | no | `session`, Mac only |
| `notchling session command <id> -- </command>` | `session.command` | `session`, Mac only | `session`, Mac only | `session`, Mac only |
| `notchling session dialog <id> [--pick <n> \| --dismiss]` | `session.dialog`, `session.dialog_pick`, `session.dialog_dismiss` | `session`, Mac only | `session`, Mac only | `session`, Mac only |
| `notchling session chrome <id> on\|off` | `session.chrome` | `session`, Mac only | `session`, Mac only | `session`, Mac only |
| `notchling session options <id> --set <k>=<v>` | `agy.set_option` | `session`, Mac only | `session`, Mac only | `session`, Mac only |
| `notchling session reload <id>` | `session.reload` | free | free (exists) | `session`, Mac only |
| `notchling session show <id> [--json]` | `agy.state`, `agy.conversation`, `session.shell_output`, `session.held`, `session.reload.state` | read | read | read, redacted, same organization |

**Why a child gets only messages and stop.** The `session.new` card shows the folder, tool, model, account, the first message in full, "Trusts this folder" when it isn't trusted yet, and "«agent» may send it messages and stop it until it ends". That is what the user allows. Its messages are prompts, and the child still asks the user before every tool it isn't allowed to use, which no agent can answer. Slash commands, dialog picks, Chrome, a tool's options and question answers can change what the child may do without asking (a permission mode, a trust dialog, a browser), so they take a card every time, on a child and on itself alike. A child always starts in the default permission mode; a start or a command that skips permission prompts is user-only.

**Consent class.** `session`, Mac only: starting a session or driving one runs code with the user's account. `session_stop`, Mac or phone: it only stops something. The card shows the target session's name, folder and organization, and for a message or a command, the text in full in the untrusted box.

**Safety rule.** `acting` must be on: off is `refused` with `ask: notchling switch on acting`. The target session must be in the caller's organization; another organization's is `never`. Permission prompts and loosening permissions are user-only, whoever owns the session. `session.take_back` is user-only, and `session.move` belongs to accounts (phase 7). A child gets at most 30 free messages an hour from its parent, then `busy`. A child's own children are not its grandparent's: ownership doesn't chain.

**Tests.** Child, itself and another session for each op of the table; `session command`, `dialog`, `chrome` and `options` take a card on a child and on itself; another organization refused before any card; `session.permission` and `agy.permission` refused with `never` from every caller; a start that skips prompts refused with `never`; a phone Allow of `session` not taken, of `session_stop` taken; a reply's text is in the card and the session, never an event; acting off gives the right `ask`; `started_by` survives a restart; the hourly limit; a grandchild is not owned.

## 11. Phase 5: jobs

Step 8.

**Ops and commands.**

| Command | Ops | Consent |
| --- | --- | --- |
| `notchling dispatch <machine> --tool claude\|codex\|agy\|shell --folder <f> [--model --effort --secrets a,b --key k] -- <prompt\|command>` | `cluster.dispatch` (agent for every tool) | `dispatch`, Mac only |
| `notchling jobs [--json]` | `jobs.mine` (new) | read |
| `notchling job <id> [--wait] [--json]` | `jobs.get` (new) | read: own in full, others' state only |
| `notchling job tail <id>` | `node.tail`, `cluster.follow` | free for own, `job` (Mac only) for others |
| `notchling job return <id>` | `cluster.return` | free for own, `job` (Mac only) for others |
| `notchling job stop <id>` | `shell.stop` | free for own, `job_stop` (Mac or phone) for others |

Today's read-only `agy` delegate to a machine stays as decided: no card.

**The local delegate stays as it is.** `notchling delegate codex|agy` runs the tool on this Mac, read-only unless `--write`, which edits only the folder given. It takes no card, because an agent with a shell can run `codex exec` itself in that folder: the delegate grants nothing it couldn't do. It registers its job with the daemon (`delegate.begin` and `delegate.end`, agent, acts), so it is recorded and listed in `jobs`. A dispatch differs: it reaches another machine, which the agent can't reach by itself, so it takes a card.

**Own.** The dispatch record keeps the dispatching session.

**Consent class.** `dispatch`, Mac only, for every tool, with or without secrets: it runs code on another machine, and a paired phone is a file an agent can write. The card shows the machine (name, organization, reachable or not), the folder, the tool, model and effort, the secret names, and the prompt or command in full in the untrusted box. The `--key` makes a retried dispatch return the first job.

**Across machines.** The request lives on the node the agent runs on. Its item reaches the home through the mirror, and the answer comes back to it. Once allowed, that node sends the dispatch to the primary with the request's id and the caller's identity. The primary runs it only when its own copy of the mirrored Inbox item says Allowed by the Mac for that id, and the dispatch equals the scope the item names (the scope's hash is in the item's source, which the mirror carries); it never takes the member's word. If the primary has no copy yet, the dispatch waits for the mirror up to 60 seconds, then fails, and the request goes `failed`. `consent <id>` keeps working on the asking node, because the row is there.

**Safety rule.** `cluster` must be on (`ask: notchling switch on cluster`). The machine and the secrets must be in the caller's organization. Secrets need the names allowed for the cluster (`cluster.allow.set`, phase 6).

**Tests.** A dispatch per tool through the card; a phone Allow not taken for any tool; the key; own versus others for tail, return and stop; another organization's machine or secret refused; a member's dispatch run only when the primary's mirrored item says Allowed by the Mac, refused on a forged forward; `delegate` recorded and listed, with no card.

## 12. Phase 6: cluster admin

Step 9.

**Ops and commands.** `notchling cluster create <name>`, `invite --node <id> --name <n>`, `member <node> --role <r>`, `failover on|off`, `main <node>`, `allow <names>`, `remove <node>`, `move-home <node>`: one agent op, `cluster.admin.request {action, args}` (agent, acts), which opens a `cluster` request; on Allow the app calls today's app-tier op (`cluster.create`, `cluster.join.token`, `cluster.invite`, `cluster.member.set`, `cluster.failover.set`, `cluster.main.set`, `cluster.allow.set`, `cluster.remove`, `cluster.move_home`, `cluster.move.now`) with the id. `cluster join --token-stdin` stays as it is.

**Consent class.** `cluster`, Mac only. The card shows the cluster, the machine (name, address, organization), the role or setting before and after, and for `allow` each secret name.

**Safety rule.** An invite's join token is a handover result: when the app's `cluster.invite {consentId}` returns it, the daemon keeps it in memory only and gives it once to the asking verified session's next `consent.get`; `notchling cluster invite --no-wait` followed later by `notchling consent <id>` from the same session gets it. It is never in the row, an event, the Inbox or the card, and it is lost at a restart (the token expires in 10 minutes anyway; the agent asks again). A caller in no verified session can't request an invite (`no_session`). Promote, take back and continue here stay user-only. `cluster.side_copy.hide` is internal (`app_ui`).

**Tests.** Each action through the card and the app's call; the token reaches only the asking session, once, and is in no row, event or log; an invite from a shell refused; promote refused with `never`; a phone Allow not taken.

## 13. Phase 7: accounts

Step 10.

**Ops and commands.** `notchling accounts switch <id>`, `add <claude|codex|agy>`, `remove <id>`, `move-all <lane>`, and `notchling lanes list|create|remove|switch`: one agent op, `accounts.request {action, args}` (agent, acts), opening an `account` request; on Allow the app calls today's ops (`accounts.switch`, `providers.switch`, `accounts.add.begin`, `providers.add.begin`, `accounts.remove`, `providers.remove`, `accounts.move_all`, `lanes.*`, `session.move`). `lanes list` is a read.

**Consent class.** `account`, Mac only. The card shows the tool, the account by the name the user gave (or a masked email), its organization, and what changes ("New sessions use …", "Moves 3 sessions to …").

**Adding an account takes time.** The app's `accounts.add.begin {consentId}` only opens the sign-in in the browser, so the request goes `applying`. The daemon follows the sign-in's end on its `accounts.add.ended` event: an account added makes it `applied` with `result: {account: id}`; a cancelled or failed sign-in makes it `failed`; no end within the sign-in's own limit makes it `withdrawn`. The agent never sees a URL with a code or a token.

**Safety rule.** The organization check comes first: a target outside the caller's organization is refused with `never` before any card. A caller in no verified session has no organization and is refused with `no_session` ("Run this from a session in the organization's folder."). The `accounts` switch must be on. The rule against renewing a login in use stays as the switch path has it.

**Tests.** Cross-organization refused before a card, for every action; no session refused; the sign-in's three endings; the result carries no secret; a switch through the harness only, never a real login; a token in use never renewed (the existing harness).

## 14. Phase 8: machines, Set Up and Cost Profile apply

Step 11. Read-only plans first, then writes by request, ticked file by file.

**Machines.** The Machines engine (`crates/engine/src/machines/`) runs in the app process today: the Machines window and the `notchling-machines` binary. The daemon serves it first (`machines_ops.rs`): `machines.list`, `machines.check`, `machines.plan` (agent, read) behind `notchling machines list|check|plan --json`. Then `machines.request {action: add|fix|update|remove, args}` (agent, acts) opens a `machine` request; the card shows the host, the user, the organization, and each step and what it copies, from the plan. The admin password is typed on the Mac or taken from the Vault with its own approval; ssh keys stay where they are. `tailscale.switch` becomes `notchling network switch <tailnet>`, a `machine` request; the card says the other network drops.

**Set Up.** `install.plan` (agent, read) behind `notchling install plan --json`; `notchling install apply` opens an `install` request whose parts are the plan's files, each with its content hash and diff summary. The user unticks any file on the card; the answer records the files kept (`parts_allowed`); the app's `install.apply {consentId, parts}` must name a subset of them, and writes only those, and only if each still has the hash shown. `install.remove` stays user-only. `agy.sidecar.setup` is an `install` request too, with its one config key as its one part.

**Cost Profile.** `notchling cost setup --apply`, `cost undo` and `cost profile set` from the agent tier open a `cost_profile` request with the same parts. An agent's `--consent a,b` only fills the dry run; it is never consent. `costGuard` stays user-only.

**Updates.** `notchling versions update [--worker <node>]` opens an `update` request for `versions.update_local` or a worker's update; `notchling check-updates` uses the Ask key `checkForUpdates`.

**Morning.** Its checks (`crates/engine/src/machines/morning.rs`) are reads in this phase's `machines check`; anything it fixes is a `machine` request.

**Consent class.** All Mac only, applied by the app.

**Safety rule.** Per-file consent: a yes covers exactly the files kept, at the hash shown. The `install` switch must be on for Set Up. Machines in another organization are refused with `never`.

**Tests.** A plan as a read changes nothing; a file changed between the card and the apply is not written; an unticked file is not written; an apply naming a file outside `parts_allowed` is refused; `install.remove` and `costGuard` refused with `never`; the admin password never reaches the agent; the machine engine's fakes only.

## 15. Phase 9: search data, hybrid search and skills

Step 12.

**Search data.** The index covers what [the hybrid search design](2026-11-13-hybrid-search-design.md) names, scoped by organization, and `notchling search` gains `--kind` and `--since`. Then hybrid search per that design.

**Skills.** Every command family ends up with skill text: a new phase ships its family's lines with its commands (phases 2 to 8 each carry theirs), and phase 9 checks the whole set: one short `notchling` index section shared by every skill (from phase 1), and family skills only where the steps aren't obvious from `help --json` (cluster, sessions, jobs, accounts, machines). A test drives each skill's example command the way an agent would.

**Consent class.** None.

**Safety rule.** Search returns the caller's organization only, and nothing to a caller in no verified session; text of other sessions follows phase 2's redaction.

**Tests.** The hybrid search design's tests, plus: no result from another organization or for a shell; every skill example runs against the fixture daemon and exits as its skill says.

## 16. Not yet audited

The review stopped before these. Phase 1's builder walks each and puts every user action in the registry, as a command, in `PLANNED` with the phase whose family it belongs to, in `USER_ONLY`, or in `INTERNAL`. The guard check then holds them like every op. Where this design already knows the answer:

| Area | Where it goes |
| --- | --- |
| Plan-Only Main Chat | Turning it on is a `setting` request (it narrows what the main session may do); turning it off is user-only (it is a check on the agent). Not in this tree yet; the rule holds when it lands. |
| Reply receipts | Reads of the caller's own messages' receipts in phase 2 (`notchling receipts`); nothing to act on. |
| Morning | Its checks are reads, its fixes `machine` requests, in phase 8. |
| The phone app's actions | Each is an Inbox answer or a session action the Mac already has an op for; they follow those ops' rows. Pairing stays user-only. |
| The app's menus, sheets and `locales/en` | Each action that changes something maps to its op's row; one that only changes the app's view is `INTERNAL` (`app_ui`). |
| The overnight worker | Open: the builder couldn't find it in the tree. Phase 1's walk classifies it; until then it is not in the registry, and the maintainer names where it lives ([Needs the maintainer](#needs-the-maintainer), 14). |

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

- **The card is the only barrier.** A prompt-injected agent can now reach every guarded area up to the card, and a user who allows by reflex lets it through. The rules of [5](#5-consent-card-rules) narrow it: facts the agent can't write, its words marked untrusted, once, a delay on Allow, an end time, a rate limit, Mac-only Allow for everything that runs code or grants power. What is left is a user who reads "turn on the vault" and allows it anyway.
- **A forged phone.** `devices.json` is a file an agent can write, so a forged device could answer: the risk the maintainer accepted for vault approvals. A phone may Allow only a settings ask or a stop, so a forged phone can change an Ask setting or stop a session or a job, and nothing that runs code, reaches a machine or grants power.
- **Labels can be forged inside a session.** Any agent of a session can write its transcripts, so the agent label can lie. Ownership rests on the kernel's session, never on the label. The evidence for the label is recorded with each call, so a burst of calls can't erase it.
- **A child session is driven without a card.** After one `session.new` card, the parent may send its child messages and stop it. The card says so, the child still asks the user before its tools, and everything that could loosen the child takes a card each time.
- **"Installed" rests on folders the user's account can write.** An agent with a shell can write under `~/.local/share/claude` too. Verification is the daemon's best word on who asks, as decision 0007 says of files at rest; it decides ownership and labels, never a power the app tier holds.
- **The organization of a session is its folder.** An agent can't change it mid-session, but a session started in the wrong folder gets that folder's organization. Starting a session is itself a Mac-only request.
- **The exit codes move once.** A session started before the update keeps its old skill text until it restarts, and a user's script may test `$? -eq 2` after `ask`. The skills change in the same merge and follow the app; every error line names its code, so the words still say what happened; `secret` no longer mixes its own codes with its command's. Scripts outside Notchling need a look; the release notes list the moves.
- **An update with the daemon still running.** A newer command against an older daemon says so (`old_daemon`) instead of failing oddly; a newer daemon keeps the old app's secret-request path for one release.
- **Volume.** An agent in a loop writes an `agent.called` line per acting call. The Audit's 500 MB cap and the per-session limits bound it; a busy session shows in Activity's Agents filter.
- **Stale cards.** Requests saved with no app attached pile up while the user is away. The end time, the long-end limits, Deny All Waiting and the second check of the facts keep a late yes from doing something other than what the card said.
- **Schema number.** Another branch may take version 10 first; this one then takes the next at merge.

## Needs the maintainer

The record [XXXX-agents-first](../../decisions/XXXX-agents-first.md) carries 1 to 7.

Decided on 2026-11-21, each at the recommendation:

1. *(decided)* **An agent may request a switch on, Mac only; the app turns it on; still off at every start.** Yes.
2. *(decided)* **Keep the cluster on at start, with the consent in a Keychain item only the signed app reads.** No, not now.
3. *(decided)* **Every acting agent call is recorded, and nothing turns it off.** Yes.
4. *(decided)* **An agent may turn any switch off without asking** (recorded, and the app shows a notice with Turn On). Yes. Question 12 asks about one exception and leaves this as it is.
5. *(decided)* **One exit-code table, moving today's codes once**, with the skills in the same change. Yes. Revision 2 adds code 125 for `secret`'s own stops.
6. *(decided)* **A phone may Deny any request but Allow only the kinds marked for it.** Yes. Revision 2 narrows the marked kinds; see question 13.
7. *(decided)* **A request opened with no app attached is saved and waits** (15 minutes by default, 12 hours at most) instead of being refused. Yes.

Still open; the recommendation is what the plan builds unless you say otherwise:

8. **Settings asks and vault requests fold into the one request; vault approvals keep their own flow and Always Allow.** Recommendation: **yes**.
9. **The organization comes from the session's folder; a caller in no verified session has none, reads nothing organization-scoped, and can't request organization-scoped actions.** Recommendation: **yes**.
10. **`hiddenAgents` becomes Free and `newSessionChoice` Ask.** Recommendation: **yes**.
11. **Phase 1 changes existing tests** (exit codes, settings asks), which the zone sends to you. Recommendation: **yes**, reviewed as one change.
12. **Is turning `cluster` off a request, not free, on a node with no app attached?** There no app can turn it back on until the daemon restarts, so an agent could strand a headless Worker until someone reaches it. The bridge needs no exception: with it off, no phone can give a yes, and the Mac shows Turn On. Recommendation: **yes, for `cluster` on a node with no app attached only**; everywhere else off stays free, as decided.
13. **A phone may Allow only settings asks and stops, until pairing has an anchor an agent can't write.** Revision 1 also let a phone allow sessions and dispatch without secrets; the builder check showed that a forged device could then run code on every machine in the organization. Recommendation: **yes**.
14. **Where is the overnight worker?** The builder couldn't find it in the tree, so it can't be classified yet. Name it, or let phase 1's walk find it.
